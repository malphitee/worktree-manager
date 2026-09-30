//! 全局配置：定位、旧格式迁移、规范化与校验、原子保存、项目解析。
//! 配置文件不存在时返回默认值且**不创建文件**；只有用户保存后才写盘。

use std::path::{Path, PathBuf};

use serde::Deserialize;

use crate::atomic_json;
use crate::error::AppError;
use crate::git::{self, GitRunner};
use crate::models::{
    AppConfig, ProjectConfig, ProjectResolution, ProjectType, SCHEMA_VERSION, SharedDirectoryRule,
};
use crate::path_utils;
use crate::platform;
use crate::validation;

/// `platform::config_dir()/config.json`
pub fn config_path() -> PathBuf {
    platform::config_dir().join("config.json")
}

/// 旧格式（含 `fdCommonSource` / `goCommonSource` / 规则 `enabled` 字段）。
/// `schema_version` 与 `go_common_source` 只做「接受但不使用」：版本一律按当前值写出，
/// `goCommonSource` 按 docs/data-model.md §1 明确丢弃。
#[allow(dead_code)]
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct RawConfig {
    #[serde(default)]
    schema_version: Option<u32>,
    #[serde(default)]
    workspace_root: Option<String>,
    #[serde(default)]
    shared_directories: Option<Vec<RawRule>>,
    #[serde(default)]
    projects: Option<Vec<ProjectConfig>>,
    #[serde(default)]
    recent_iterations: Option<Vec<String>>,
    #[serde(default)]
    fd_common_source: Option<String>,
    #[serde(default)]
    go_common_source: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct RawRule {
    #[serde(default)]
    source_path: Option<String>,
    #[serde(default)]
    target_directory: Option<String>,
    #[serde(default)]
    enabled: Option<bool>,
}

/// 读取配置：不存在 → 默认值（不创建文件）；损坏 → `io` 错误；旧格式 → 内存迁移
pub fn load() -> Result<AppConfig, AppError> {
    let path = config_path();
    if !path.is_file() {
        return Ok(AppConfig::default());
    }
    let text = std::fs::read_to_string(&path)
        .map_err(|error| AppError::Io(format!("读取配置失败 {}：{error}", path.display())))?;
    let value: serde_json::Value = serde_json::from_str(&text)
        .map_err(|error| AppError::Io(format!("解析配置失败 {}：{error}", path.display())))?;
    let migrated = migrate_legacy(value)?;
    Ok(light_normalize(migrated))
}

/// 旧格式迁移（只在内存中进行，用户保存时才写回新格式）：
/// - `fdCommonSource` → 追加一条 `{ sourcePath, targetDirectory: "fd-common" }`
/// - `goCommonSource` 直接丢弃
/// - 旧规则 `enabled === false` 丢弃，`enabled === true` 或缺省保留
pub fn migrate_legacy(value: serde_json::Value) -> Result<AppConfig, AppError> {
    let raw: RawConfig = serde_json::from_value(value)
        .map_err(|error| AppError::Io(format!("配置解析失败：{error}")))?;

    let mut shared_directories: Vec<SharedDirectoryRule> = Vec::new();
    if let Some(rules) = raw.shared_directories {
        for rule in rules {
            if rule.enabled == Some(false) {
                continue;
            }
            let (Some(source_path), Some(target_directory)) = (rule.source_path, rule.target_directory)
            else {
                continue;
            };
            shared_directories.push(SharedDirectoryRule {
                source_path,
                target_directory,
            });
        }
    }
    if let Some(fd_common) = raw.fd_common_source {
        let source_path = fd_common.trim().to_string();
        let already_present = shared_directories
            .iter()
            .any(|rule| rule.target_directory.eq_ignore_ascii_case("fd-common"));
        if !source_path.is_empty() && !already_present {
            shared_directories.push(SharedDirectoryRule {
                source_path,
                target_directory: "fd-common".to_string(),
            });
        }
    }

    let config = AppConfig {
        schema_version: SCHEMA_VERSION,
        workspace_root: raw.workspace_root,
        shared_directories,
        projects: raw
            .projects
            .unwrap_or_default()
            .into_iter()
            .filter(|project| !project.id.trim().is_empty())
            .collect(),
        recent_iterations: raw.recent_iterations.unwrap_or_default(),
    };
    Ok(light_normalize(config))
}

/// 轻规范化（读路径使用）：不校验存在性，只做去空白、去重、限长
pub fn light_normalize(mut config: AppConfig) -> AppConfig {
    config.schema_version = SCHEMA_VERSION;
    config.workspace_root = config
        .workspace_root
        .map(|root| root.trim().to_string())
        .filter(|root| !root.is_empty());
    config.recent_iterations = normalize_recent_iterations(config.recent_iterations);
    config
}

fn normalize_recent_iterations(values: Vec<String>) -> Vec<String> {
    let mut result: Vec<String> = Vec::new();
    for value in values {
        let trimmed = value.trim().to_string();
        if trimmed.is_empty() || validation::validate_iteration(&trimmed).is_err() {
            continue;
        }
        if result.iter().any(|existing| existing == &trimmed) {
            continue;
        }
        result.push(trimmed);
        if result.len() == 10 {
            break;
        }
    }
    result
}

/// 保存前的规范化与校验（返回规范化后的配置；不通过则不写盘）
pub fn normalize_and_validate(config: AppConfig) -> Result<AppConfig, AppError> {
    let mut normalized = light_normalize(config);

    if let Some(root) = normalized.workspace_root.clone() {
        validation::validate_absolute_path(&root)?;
        // 允许尚不存在：解析最近已存在祖先，确认可定位
        path_utils::resolve_allow_missing(Path::new(&root))?;
    }

    let mut target_directories: Vec<String> = Vec::new();
    let mut rules: Vec<SharedDirectoryRule> = Vec::new();
    for rule in normalized.shared_directories {
        validation::validate_directory_name(&rule.target_directory)?;
        let key = rule.target_directory.to_lowercase();
        if target_directories.contains(&key) {
            return Err(AppError::Validation(format!(
                "公共目录目标名重复：{}",
                rule.target_directory
            )));
        }
        target_directories.push(key);
        let canonical = path_utils::canonicalize_existing(Path::new(&rule.source_path))?;
        if !canonical.is_dir() {
            return Err(AppError::Validation(format!(
                "公共目录源路径不是目录：{}",
                rule.source_path
            )));
        }
        rules.push(SharedDirectoryRule {
            source_path: canonical.to_string_lossy().to_string(),
            target_directory: rule.target_directory,
        });
    }
    normalized.shared_directories = rules;

    let mut project_ids: Vec<String> = Vec::new();
    let mut projects: Vec<ProjectConfig> = Vec::new();
    for project in normalized.projects {
        let mut conflicts: Vec<String> = target_directories.clone();
        conflicts.extend(project_ids.iter().cloned());
        validation::validate_project_id(&project.id, &conflicts)?;
        project_ids.push(project.id.to_lowercase());

        let canonical = path_utils::canonicalize_existing(Path::new(&project.repository_path))?;
        let repository_root = resolve_repository_root(&canonical.to_string_lossy())?;
        if !platform::paths_equal(&repository_root, &canonical) {
            return Err(AppError::Validation(format!(
                "不是 Git 仓库根目录：{}（仓库根为 {}）",
                project.repository_path,
                repository_root.display()
            )));
        }
        projects.push(ProjectConfig {
            id: project.id,
            repository_path: canonical.to_string_lossy().to_string(),
            project_type: detect_project_type(&canonical),
            vendor_available: vendor_available(&canonical),
        });
    }
    normalized.projects = projects;
    Ok(normalized)
}

/// 校验通过后原子写入 `config.json`
pub fn save(config: AppConfig) -> Result<AppConfig, AppError> {
    let normalized = normalize_and_validate(config)?;
    atomic_json::write_json_atomic(&config_path(), &normalized)?;
    Ok(normalized)
}

/// 把迭代号前插进 `recentIterations`（去重、限 10）并原子写盘
pub fn remember_iteration(mut config: AppConfig, iteration: &str) -> Result<AppConfig, AppError> {
    let mut recent: Vec<String> = vec![iteration.trim().to_string()];
    recent.extend(config.recent_iterations.iter().cloned());
    config.recent_iterations = normalize_recent_iterations(recent);
    atomic_json::write_json_atomic(&config_path(), &config)?;
    Ok(config)
}

/// 项目类型判定：`composer.json` → php；`go.mod` → go；否则 other；不可读 → unknown
pub fn detect_project_type(root: &Path) -> ProjectType {
    let readable = std::fs::read_dir(root).is_ok();
    if root.join("composer.json").is_file() {
        return ProjectType::Php;
    }
    if root.join("go.mod").is_file() {
        return ProjectType::Go;
    }
    if readable {
        ProjectType::Other
    } else {
        ProjectType::Unknown
    }
}

/// 源仓库是否具备 vendor 目录
pub fn vendor_available(root: &Path) -> bool {
    root.join("vendor").is_dir()
}

/// 解析用户选择的路径为项目信息（逐条容错，不整体失败）
pub fn resolve_projects(paths: &[String]) -> Vec<ProjectResolution> {
    paths.iter().map(|path| resolve_project(path)).collect()
}

fn resolve_project(path: &str) -> ProjectResolution {
    let unresolved = |message: String| ProjectResolution {
        input_path: path.to_string(),
        repository_path: None,
        suggested_id: None,
        project_type: ProjectType::Unknown,
        vendor_available: false,
        error: Some(message),
    };

    let root = match resolve_repository_root(path) {
        Ok(root) => root,
        Err(error) => return unresolved(error.message()),
    };
    let project_type = detect_project_type(&root);
    let suggested_id = root
        .file_name()
        .map(|name| name.to_string_lossy().to_string())
        .filter(|name| validation::validate_project_id(name, &[]).is_ok());
    ProjectResolution {
        input_path: path.to_string(),
        repository_path: Some(root.to_string_lossy().to_string()),
        suggested_id,
        project_type,
        vendor_available: project_type == ProjectType::Php && vendor_available(&root),
        error: None,
    }
}

/// `git rev-parse --show-toplevel`（工作目录＝该路径）并 canonicalize
fn resolve_repository_root(path: &str) -> Result<PathBuf, AppError> {
    let command = GitRunner::cmd_rev_parse_toplevel(Path::new(path));
    let output = git::run(&command).map_err(|_| AppError::Validation("不是 Git 仓库".to_string()))?;
    let trimmed = output.trim();
    if trimmed.is_empty() {
        return Err(AppError::Validation("不是 Git 仓库".to_string()));
    }
    path_utils::canonicalize_existing(Path::new(trimmed))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::SCHEMA_VERSION;
    use serde_json::json;

    #[test]
    fn migrates_fd_common_source_and_drops_go_common_source() {
        let value = json!({
            "schemaVersion": 1,
            "workspaceRoot": "/Users/me/Work/workspace",
            "fdCommonSource": "/Users/me/Work/fd-common",
            "goCommonSource": "/Users/me/Work/go-common",
            "sharedDirectories": [
                { "sourcePath": "/Users/me/Work/keep", "targetDirectory": "keep", "enabled": true },
                { "sourcePath": "/Users/me/Work/drop", "targetDirectory": "drop", "enabled": false }
            ],
            "projects": [
                { "id": "api3", "repositoryPath": "/repo/api3", "projectType": "php", "vendorAvailable": true },
                { "id": "", "repositoryPath": "/repo/empty", "projectType": "other", "vendorAvailable": false }
            ],
            "recentIterations": ["7.3.0"]
        });

        let config = migrate_legacy(value).unwrap();
        let targets: Vec<&str> = config
            .shared_directories
            .iter()
            .map(|rule| rule.target_directory.as_str())
            .collect();
        assert_eq!(targets, vec!["keep", "fd-common"]);
        assert_eq!(
            config.shared_directories[1].source_path,
            "/Users/me/Work/fd-common"
        );
        // goCommonSource 已丢弃（不在任何字段中出现）
        assert!(!serde_json::to_string(&config).unwrap().contains("go-common"));
        // 空 id 项目被丢弃
        assert_eq!(config.projects.len(), 1);
        assert_eq!(config.projects[0].id, "api3");
        assert_eq!(config.schema_version, SCHEMA_VERSION);
    }

    #[test]
    fn workspace_root_empty_string_becomes_none() {
        let value = json!({ "schemaVersion": 1, "workspaceRoot": "   " });
        let config = migrate_legacy(value).unwrap();
        assert_eq!(config.workspace_root, None);
    }

    #[test]
    fn recent_iterations_are_deduplicated_and_limited_to_ten() {
        let mut values: Vec<String> = (0..12).map(|index| format!("rel-{index}")).collect();
        values.push("rel-1".to_string());
        values.push("bad/iter".to_string());
        let value = json!({ "schemaVersion": 1, "recentIterations": values });
        let config = migrate_legacy(value).unwrap();
        assert_eq!(config.recent_iterations.len(), 10);
        assert_eq!(config.recent_iterations[0], "rel-0");
        assert!(!config.recent_iterations.iter().any(|item| item == "bad/iter"));
    }

    #[test]
    fn normalize_rejects_invalid_root_and_duplicate_targets() {
        let temp = tempfile::tempdir().unwrap();
        let source = temp.path().join("common");
        std::fs::create_dir_all(&source).unwrap();

        let relative_root = AppConfig {
            workspace_root: Some("relative/path".to_string()),
            ..Default::default()
        };
        assert!(normalize_and_validate(relative_root).is_err());

        let duplicate_targets = AppConfig {
            workspace_root: Some(temp.path().to_string_lossy().to_string()),
            shared_directories: vec![
                SharedDirectoryRule {
                    source_path: source.to_string_lossy().to_string(),
                    target_directory: "fd-common".to_string(),
                },
                SharedDirectoryRule {
                    source_path: source.to_string_lossy().to_string(),
                    target_directory: "FD-Common".to_string(),
                },
            ],
            ..Default::default()
        };
        assert!(normalize_and_validate(duplicate_targets).is_err());
    }

    #[test]
    fn normalize_rejects_missing_shared_source_and_project_conflicts() {
        let temp = tempfile::tempdir().unwrap();
        let config = AppConfig {
            workspace_root: Some(temp.path().to_string_lossy().to_string()),
            shared_directories: vec![SharedDirectoryRule {
                source_path: temp.path().join("missing").to_string_lossy().to_string(),
                target_directory: "fd-common".to_string(),
            }],
            ..Default::default()
        };
        assert!(normalize_and_validate(config).is_err());
    }

    #[test]
    fn resolve_projects_reports_non_repo_and_detects_php_with_vendor() {
        let temp = tempfile::tempdir().unwrap();

        // 非仓库目录
        let plain = temp.path().join("plain");
        std::fs::create_dir_all(&plain).unwrap();
        let resolutions = resolve_projects(&[plain.to_string_lossy().to_string()]);
        assert_eq!(resolutions.len(), 1);
        assert_eq!(resolutions[0].error.as_deref(), Some("不是 Git 仓库"));
        assert!(resolutions[0].repository_path.is_none());

        // 真实仓库（本机无 git 时打印原因并跳过）
        let repo = temp.path().join("api3");
        std::fs::create_dir_all(&repo).unwrap();
        let status = std::process::Command::new("git")
            .args(["init", "--quiet"])
            .current_dir(&repo)
            .status();
        let Ok(status) = status else {
            eprintln!("跳过：本机没有可执行的 git");
            return;
        };
        if !status.success() {
            eprintln!("跳过：git init 失败（退出码 {:?}）", status.code());
            return;
        }
        std::fs::write(repo.join("composer.json"), "{}").unwrap();
        std::fs::create_dir_all(repo.join("vendor")).unwrap();

        let resolutions = resolve_projects(&[repo.to_string_lossy().to_string()]);
        let resolution = &resolutions[0];
        assert_eq!(resolution.error, None);
        assert_eq!(
            resolution.repository_path.as_deref(),
            Some(repo.canonicalize().unwrap().to_string_lossy().as_ref())
        );
        assert_eq!(resolution.suggested_id.as_deref(), Some("api3"));
        assert_eq!(resolution.project_type, ProjectType::Php);
        assert!(resolution.vendor_available);
    }
}
