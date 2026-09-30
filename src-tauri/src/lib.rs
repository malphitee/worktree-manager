//! Tauri 装配层（architecture.md §3）：Command 适配函数只做
//! 反序列化 → 获取操作锁（若需要）→ 调服务 → 错误转换 → 发进度事件；不写业务规则。

pub mod archive;
pub mod atomic_json;
pub mod base_ref;
pub mod config;
pub mod copy;
pub mod error;
pub mod git;
pub mod head_state;
pub mod manifest;
pub mod merge_check;
pub mod models;
pub mod path_utils;
pub mod platform;
pub mod relocate;
pub mod removal;
pub mod validation;
pub mod vendor;
pub mod workspace;

use std::path::Path;
use std::sync::Mutex;

use tauri::{AppHandle, Emitter, State};

use crate::error::AppError;
use crate::models::{
    AppConfig, ArchiveAssessment, ArchiveOutcome, ArchiveRequest, CreateBatchResult, CreateRequest,
    MergeCheckResult, MoveOutcome, MoveProjectRequest, ProjectResolution, RemoteBranches,
    RemovalAssessment, RemoveRequest, WorkspaceGroup,
};

// ============================ 全局操作锁（§6.2） ============================

/// 全局单个非阻塞锁：`try` 语义，已占用立即返回 `busy`，不排队、不取消
#[derive(Default)]
pub struct OperationState {
    current: Mutex<Option<&'static str>>,
}

impl OperationState {
    pub fn acquire(&self, name: &'static str) -> Result<OperationGuard<'_>, AppError> {
        let mut current = self.current.lock().expect("操作锁中毒");
        match *current {
            Some(running) => Err(AppError::Busy(running.to_string())),
            None => {
                *current = Some(name);
                Ok(OperationGuard { state: self })
            }
        }
    }
}

/// 持锁守卫：离开作用域即释放
pub struct OperationGuard<'a> {
    state: &'a OperationState,
}

impl Drop for OperationGuard<'_> {
    fn drop(&mut self) {
        if let Ok(mut current) = self.state.current.lock() {
            *current = None;
        }
    }
}

/// 把阻塞任务放到 `spawn_blocking` 执行（Git 子进程不在 UI 线程）
async fn run_blocking<T, F>(task: F) -> Result<T, AppError>
where
    T: Send + 'static,
    F: FnOnce() -> Result<T, AppError> + Send + 'static,
{
    tauri::async_runtime::spawn_blocking(task)
        .await
        .map_err(|error| AppError::Io(format!("后台任务执行失败：{error}")))?
}

// ============================ 配置（不持锁） ============================

#[tauri::command]
async fn get_config() -> Result<AppConfig, AppError> {
    run_blocking(config::load).await
}

#[tauri::command]
async fn save_config(config_value: AppConfig) -> Result<AppConfig, AppError> {
    run_blocking(move || config::save(config_value)).await
}

#[tauri::command]
async fn resolve_projects(paths: Vec<String>) -> Result<Vec<ProjectResolution>, AppError> {
    run_blocking(move || Ok(config::resolve_projects(&paths))).await
}

// ============================ 列表（仅复核持锁） ============================

#[tauri::command]
async fn list_workspaces(
    reconcile: bool,
    state: State<'_, OperationState>,
) -> Result<Vec<WorkspaceGroup>, AppError> {
    if reconcile {
        let _guard = state.acquire("复核状态")?;
        run_blocking(move || {
            let config = config::load()?;
            manifest::list_groups(&config, true)
        })
        .await
    } else {
        run_blocking(move || {
            let config = config::load()?;
            manifest::list_groups(&config, false)
        })
        .await
    }
}

// ============================ 创建（持锁 + 进度事件） ============================

#[tauri::command]
async fn create_workspaces(
    request: CreateRequest,
    app: AppHandle,
    state: State<'_, OperationState>,
) -> Result<CreateBatchResult, AppError> {
    let _guard = state.acquire("创建工作区")?;
    run_blocking(move || {
        let config = config::load()?;
        workspace::create_batch(&config, request, |progress| {
            // 进度事件失败不影响创建流程
            let _ = app.emit("create-progress", progress);
        })
    })
    .await
}

// ============================ 合并检查（持锁 + 事件） ============================

#[tauri::command]
async fn check_merge_status(
    app: AppHandle,
    state: State<'_, OperationState>,
    iteration: String,
    project_id: Option<String>,
    worktree_path: Option<String>,
) -> Result<MergeCheckResult, AppError> {
    let _guard = state.acquire("检查合并")?;
    run_blocking(move || {
        let config = config::load()?;
        merge_check::check(
            &config,
            &iteration,
            merge_check::MergeFilter {
                project_id,
                worktree_path,
            },
            |progress| {
                let _ = app.emit("merge-check-progress", progress);
            },
        )
    })
    .await
}

// ============================ 移除（两种，均持锁） ============================

#[tauri::command]
async fn assess_removal(
    iteration: String,
    project_id: String,
    worktree_path: String,
) -> Result<RemovalAssessment, AppError> {
    run_blocking(move || {
        let config = config::load()?;
        removal::assess_managed(&config, &iteration, &project_id, &worktree_path)
    })
    .await
}

#[tauri::command]
async fn remove_worktree(
    request: RemoveRequest,
    state: State<'_, OperationState>,
) -> Result<(), AppError> {
    let _guard = state.acquire("移除 worktree")?;
    run_blocking(move || {
        let config = config::load()?;
        removal::remove_managed(&config, &request)
    })
    .await
}

#[tauri::command]
async fn assess_discovered_removal(
    iteration: String,
    worktree_path: String,
    source_path: String,
) -> Result<RemovalAssessment, AppError> {
    run_blocking(move || {
        let config = config::load()?;
        removal::assess_discovered(&config, &iteration, &worktree_path, &source_path)
    })
    .await
}

#[tauri::command]
async fn remove_discovered_worktree(
    iteration: String,
    project_id: String,
    worktree_path: String,
    source_path: String,
    confirmation: String,
    remove_copied_vendor: bool,
    state: State<'_, OperationState>,
) -> Result<(), AppError> {
    let _guard = state.acquire("移除 worktree")?;
    run_blocking(move || {
        // projectId 仅用于前端关联行；仍做单层安全名校验
        if !project_id.is_empty() {
            validation::validate_directory_name(&project_id)?;
        }
        let config = config::load()?;
        removal::remove_discovered(
            &config,
            &iteration,
            &worktree_path,
            &source_path,
            &confirmation,
            remove_copied_vendor,
        )
    })
    .await
}

// ==================== 基分支候选（不持锁）/ 排序与移动（持锁） ====================

#[tauri::command]
async fn list_remote_branches(
    project_id: String,
    include_remote: bool,
) -> Result<RemoteBranches, AppError> {
    run_blocking(move || {
        let config = config::load()?;
        base_ref::list_remote_branches(&config, &project_id, include_remote)
    })
    .await
}

#[tauri::command]
async fn reorder_project(
    state: State<'_, OperationState>,
    iteration: String,
    worktree_path: String,
    before_worktree_path: Option<String>,
) -> Result<(), AppError> {
    let _guard = state.acquire("调整顺序")?;
    run_blocking(move || {
        let config = config::load()?;
        relocate::reorder(
            &config,
            &iteration,
            &worktree_path,
            before_worktree_path.as_deref(),
        )
    })
    .await
}

#[tauri::command]
async fn move_project(
    state: State<'_, OperationState>,
    request: MoveProjectRequest,
) -> Result<MoveOutcome, AppError> {
    let _guard = state.acquire("移动项目")?;
    run_blocking(move || {
        let config = config::load()?;
        relocate::move_project(&config, &request)
    })
    .await
}

// ============================ 打开目录（不持锁） ============================

/// 解析「打开目录」的目标：迭代目录本身或 worktree 路径，必须位于工作区根目录之内。
/// 独立于命令函数以便单测覆盖越界拒绝（前端提交的路径不可信）。
fn resolve_open_target(
    config: &AppConfig,
    iteration: &str,
    worktree_path: Option<&str>,
) -> Result<std::path::PathBuf, AppError> {
    validation::validate_iteration(iteration)?;
    let root = config
        .workspace_root
        .as_deref()
        .ok_or_else(|| AppError::Validation("请先在设置中填写工作区根目录".to_string()))?;
    let root = path_utils::canonicalize_existing(&path_utils::resolve_allow_missing(Path::new(root))?)?;
    let iteration_dir =
        path_utils::canonicalize_existing(&path_utils::join_segments(&root, &[iteration])?)?;
    let target = match worktree_path {
        Some(value) => path_utils::canonicalize_existing(Path::new(value))?,
        None => iteration_dir.clone(),
    };
    path_utils::ensure_within(&iteration_dir, &target)?;
    Ok(target)
}

#[tauri::command]
async fn open_iteration(iteration: String) -> Result<(), AppError> {
    run_blocking(move || {
        let config = config::load()?;
        let target = resolve_open_target(&config, &iteration, None)?;
        platform::open_in_file_manager(&target)
    })
    .await
}

#[tauri::command]
async fn open_project(
    iteration: String,
    project_id: String,
    worktree_path: String,
) -> Result<(), AppError> {
    run_blocking(move || {
        if !project_id.is_empty() {
            validation::validate_directory_name(&project_id)?;
        }
        let config = config::load()?;
        let target = resolve_open_target(&config, &iteration, Some(&worktree_path))?;
        platform::open_in_file_manager(&target)
    })
    .await
}

// ================================== 启动 ==================================

pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .manage(OperationState::default())
        .invoke_handler(tauri::generate_handler![
            get_config,
            save_config,
            resolve_projects,
            list_workspaces,
            create_workspaces,
            check_merge_status,
            assess_archive,
            archive_iteration,
            set_iteration_note,
            set_iteration_hidden,
            list_remote_branches,
            reorder_project,
            move_project,
            assess_removal,
            remove_worktree,
            assess_discovered_removal,
            remove_discovered_worktree,
            open_iteration,
            open_project,
        ])
        .run(tauri::generate_context!())
        .expect("启动 Tauri 应用失败");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn operation_lock_is_non_blocking_and_reports_current_operation() {
        let state = OperationState::default();
        let guard = state.acquire("创建工作区").unwrap();
        let error = match state.acquire("检查合并") {
            Ok(_) => panic!("锁被占用时应返回 busy"),
            Err(error) => error,
        };
        assert_eq!(error.code(), crate::models::ErrorCode::Busy);
        assert_eq!(error.message(), "已有操作执行中：创建工作区");
        drop(guard);
        assert!(state.acquire("检查合并").is_ok());
    }
}

#[cfg(test)]
mod open_target_tests {
    use super::*;

    /// 验收 §4：`open_*` 的后端路径校验（越界即 validation，迭代号非法即 validation）
    #[test]
    fn open_target_is_restricted_to_iteration_directory() {
        let temp = tempfile::tempdir().unwrap();
        let workspace = temp.path().join("ws");
        let iteration_dir = workspace.join("7.3.0");
        std::fs::create_dir_all(iteration_dir.join("api3")).unwrap();
        let outside = temp.path().join("outside");
        std::fs::create_dir_all(&outside).unwrap();

        let config = AppConfig {
            workspace_root: Some(workspace.to_string_lossy().to_string()),
            ..Default::default()
        };

        assert!(resolve_open_target(&config, "7.3.0", None).is_ok());
        assert!(resolve_open_target(
            &config,
            "7.3.0",
            Some(&iteration_dir.join("api3").to_string_lossy())
        )
        .is_ok());

        let error = resolve_open_target(&config, "7.3.0", Some(&outside.to_string_lossy()))
            .unwrap_err();
        assert_eq!(error.code(), crate::models::ErrorCode::Validation);
        assert!(error.message().contains("路径越界"));

        assert!(resolve_open_target(&config, "../etc", None).is_err());
    }
}

// ============================ 归档 / 备注 / 隐藏（均持锁） ============================

/// 迭代目录（必须存在）；不存在 → `notFound`
fn existing_iteration_directory(config: &AppConfig, iteration: &str) -> Result<std::path::PathBuf, AppError> {
    validation::validate_iteration(iteration)?;
    let root = config
        .workspace_root
        .as_deref()
        .ok_or_else(|| AppError::Validation("请先在设置中填写工作区根目录".to_string()))?;
    let root = path_utils::canonicalize_existing(&path_utils::resolve_allow_missing(Path::new(root))?)?;
    let joined = path_utils::join_segments(&root, &[iteration])?;
    let directory = joined
        .canonicalize()
        .map_err(|_| AppError::NotFound(format!("迭代目录不存在：{}", joined.display())))?;
    path_utils::ensure_within(&root, &directory)?;
    Ok(directory)
}

#[tauri::command]
async fn assess_archive(
    app: AppHandle,
    state: State<'_, OperationState>,
    iteration: String,
) -> Result<ArchiveAssessment, AppError> {
    let _guard = state.acquire("评估归档")?;
    run_blocking(move || {
        let config = config::load()?;
        archive::assess(&config, &iteration, |progress| {
            let _ = app.emit("archive-progress", progress);
        })
    })
    .await
}

#[tauri::command]
async fn archive_iteration(
    app: AppHandle,
    state: State<'_, OperationState>,
    request: ArchiveRequest,
) -> Result<ArchiveOutcome, AppError> {
    let _guard = state.acquire("归档迭代")?;
    run_blocking(move || {
        let config = config::load()?;
        archive::archive(&config, &request, |progress| {
            let _ = app.emit("archive-progress", progress);
        })
    })
    .await
}

#[tauri::command]
async fn set_iteration_note(
    state: State<'_, OperationState>,
    iteration: String,
    note: Option<String>,
) -> Result<Option<String>, AppError> {
    let _guard = state.acquire("修改备注")?;
    run_blocking(move || {
        let config = config::load()?;
        let directory = existing_iteration_directory(&config, &iteration)?;
        let normalized = workspace::normalize_note(note.as_deref().unwrap_or(""))?;
        manifest::update_note(&directory, normalized)
    })
    .await
}

#[tauri::command]
async fn set_iteration_hidden(
    state: State<'_, OperationState>,
    iteration: String,
    hidden: bool,
) -> Result<Option<String>, AppError> {
    let _guard = state.acquire("隐藏迭代")?;
    run_blocking(move || {
        let config = config::load()?;
        let directory = existing_iteration_directory(&config, &iteration)?;
        manifest::set_hidden(&directory, hidden)
    })
    .await
}
