//! 路径可信化工具（architecture.md §4.2）：canonicalize、允许缺失的解析、边界验证、逐段 join、目录名顺延。

use std::ffi::{OsStr, OsString};
use std::path::{Component, Path, PathBuf};

use crate::error::AppError;
use crate::platform;

/// 规范化一个必须已存在的路径
pub fn canonicalize_existing(path: &Path) -> Result<PathBuf, AppError> {
    path.canonicalize().map_err(|error| {
        AppError::Validation(format!("路径不存在或无法访问：{}（{error}）", path.display()))
    })
}

/// 允许目标缺失：找到最近已存在的祖先并 canonicalize，再把剩余段依次接回
pub fn resolve_allow_missing(path: &Path) -> Result<PathBuf, AppError> {
    let mut current = path.to_path_buf();
    let mut tail: Vec<OsString> = Vec::new();
    loop {
        if current.exists() {
            let mut resolved = canonicalize_existing(&current)?;
            for segment in tail.iter().rev() {
                resolved.push(segment);
            }
            return Ok(resolved);
        }
        match (current.file_name(), current.parent()) {
            (Some(name), Some(parent)) => {
                tail.push(name.to_os_string());
                current = parent.to_path_buf();
            }
            _ => {
                return Err(AppError::Validation(format!(
                    "无法解析路径：{}",
                    path.display()
                )))
            }
        }
    }
}

/// 候选路径（必须存在）规范化后必须仍位于 root（规范化）之内
pub fn ensure_within(root: &Path, candidate: &Path) -> Result<(), AppError> {
    let root_canonical = canonicalize_existing(root)?;
    let candidate_canonical = canonicalize_existing(candidate)?;
    if is_within(&root_canonical, &candidate_canonical) {
        Ok(())
    } else {
        Err(AppError::Validation(format!(
            "路径越界：{} 不在 {} 之内",
            candidate.display(),
            root.display()
        )))
    }
}

/// 大小写不敏感的前缀包含判断（root 与 candidate 均须已规范化）
pub fn is_within(root: &Path, candidate: &Path) -> bool {
    let root_count = root.components().count();
    let candidate_prefix: PathBuf = candidate.components().take(root_count).collect();
    platform::paths_equal(root, &candidate_prefix)
}

/// 逐段 join：每段必须恰好是一个安全单层目录名（不接受分隔符、`.`、`..`、空串）
pub fn join_segments(root: &Path, segments: &[&str]) -> Result<PathBuf, AppError> {
    let mut result = root.to_path_buf();
    for segment in segments {
        // 反斜杠在 Unix 上不是分隔符，但按跨平台统一规则必须拒绝
        if segment.contains('/') || segment.contains('\\') {
            return Err(AppError::Validation(format!(
                "非法路径段：{segment:?}（不能包含路径分隔符）"
            )));
        }
        let mut components = Path::new(segment).components();
        match (components.next(), components.next()) {
            (Some(Component::Normal(name)), None) if name == OsStr::new(segment) => {
                result.push(segment);
            }
            _ => {
                return Err(AppError::Validation(format!(
                    "非法路径段：{segment:?}（必须是单层安全目录名）"
                )))
            }
        }
    }
    Ok(result)
}

/// 目录名顺延：`projectId`、`projectId-2`、`projectId-3`…
/// `existing_names` 由调用方传入，必须包含全部占用名（含 `removed` / `createFailed` 记录）。
pub fn next_directory_name(existing_names: &[String], project_id: &str) -> String {
    let is_occupied = |name: &str| {
        existing_names
            .iter()
            .any(|item| platform::paths_equal(Path::new(item), Path::new(name)))
    };
    if !is_occupied(project_id) {
        return project_id.to_string();
    }
    let mut index = 2_u32;
    loop {
        let candidate = format!("{project_id}-{index}");
        if !is_occupied(&candidate) {
            return candidate;
        }
        index += 1;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn next_directory_name_skips_occupied_names() {
        assert_eq!(next_directory_name(&[], "p"), "p");
        assert_eq!(next_directory_name(&["p".to_string()], "p"), "p-2");
        assert_eq!(
            next_directory_name(&["p".to_string(), "p-2".to_string()], "p"),
            "p-3"
        );
        // 大小写不敏感占用
        assert_eq!(next_directory_name(&["P".to_string()], "p"), "p-2");
        // removed / createFailed 记录同样占用
        assert_eq!(
            next_directory_name(
                &["p".to_string(), "p-2".to_string(), "p-3".to_string()],
                "p"
            ),
            "p-4"
        );
    }

    #[test]
    fn join_segments_rejects_unsafe_segments() {
        let root = Path::new("/tmp/root");
        assert_eq!(
            join_segments(root, &["7.3.0", "api3"]).unwrap(),
            PathBuf::from("/tmp/root/7.3.0/api3")
        );
        for segment in ["a/b", "..", ".", "", "a\\b"] {
            assert!(
                join_segments(root, &[segment]).is_err(),
                "{segment:?} 应被拒绝"
            );
        }
    }

    #[test]
    fn resolve_allow_missing_keeps_tail() {
        let temp = tempfile::tempdir().unwrap();
        let target = temp.path().join("a").join("b").join("7.3.0");
        let resolved = resolve_allow_missing(&target).unwrap();
        assert!(resolved.ends_with("a/b/7.3.0"));

        // 已存在的部分被 canonicalize（macOS 上 /var → /private/var）
        let resolved_existing = resolve_allow_missing(temp.path()).unwrap();
        assert_eq!(resolved_existing, temp.path().canonicalize().unwrap());
    }

    #[test]
    fn ensure_within_rejects_outside_paths() {
        let temp = tempfile::tempdir().unwrap();
        let inside = temp.path().join("a");
        std::fs::create_dir(&inside).unwrap();
        let outside = tempfile::tempdir().unwrap();

        assert!(ensure_within(temp.path(), &inside).is_ok());
        assert!(ensure_within(temp.path(), outside.path()).is_err());
        // 大小写不敏感（macOS APFS 默认不区分大小写）
        let upper = temp.path().join("A");
        if upper.exists() {
            assert!(ensure_within(temp.path(), &upper).is_ok());
        }
    }

    #[test]
    fn canonicalize_missing_path_reports_validation_error() {
        let temp = tempfile::tempdir().unwrap();
        let missing = temp.path().join("nope");
        let error = canonicalize_existing(&missing).unwrap_err();
        assert_eq!(error.code(), crate::models::ErrorCode::Validation);
    }
}
