//! 公共目录快照复制（requirements.md 判定口径 3）：
//! 排除任意层级 `.git`、不跟随符号链接 / reparse point、先复制到同父目录临时目录再原子 rename、失败清理。

use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use crate::error::AppError;
use crate::platform;

/// 复制 `src` 目录快照到 `dst`（`dst` 必须不存在）
pub fn copy_snapshot(src: &Path, dst: &Path) -> Result<(), AppError> {
    let source_metadata = fs::symlink_metadata(src)
        .map_err(|error| AppError::Io(format!("源目录不可访问 {}：{error}", src.display())))?;
    if platform::is_link_like(&source_metadata) {
        return Err(AppError::Validation(format!(
            "源目录是符号链接，已拒绝复制：{}",
            src.display()
        )));
    }
    if !source_metadata.is_dir() {
        return Err(AppError::Validation(format!(
            "源路径不是目录：{}",
            src.display()
        )));
    }
    if dst.exists() {
        return Err(AppError::Conflict(format!(
            "目标已存在，拒绝覆盖：{}",
            dst.display()
        )));
    }
    let Some(parent) = dst.parent() else {
        return Err(AppError::Validation(format!(
            "目标路径无效：{}",
            dst.display()
        )));
    };
    fs::create_dir_all(parent)
        .map_err(|error| AppError::Io(format!("创建目录失败 {}：{error}", parent.display())))?;

    let temp = temp_dir_for(dst);
    if let Err(error) = copy_dir_recursive(src, &temp) {
        let _ = fs::remove_dir_all(&temp);
        return Err(error);
    }
    if let Err(error) = fs::rename(&temp, dst) {
        let _ = fs::remove_dir_all(&temp);
        return Err(AppError::Io(format!(
            "落位失败 {}：{error}",
            dst.display()
        )));
    }
    Ok(())
}

fn copy_dir_recursive(src: &Path, dst: &Path) -> Result<(), AppError> {
    fs::create_dir_all(dst)
        .map_err(|error| AppError::Io(format!("创建目录失败 {}：{error}", dst.display())))?;
    let entries = fs::read_dir(src)
        .map_err(|error| AppError::Io(format!("读取目录失败 {}：{error}", src.display())))?;

    for entry in entries {
        let entry = entry.map_err(|error| AppError::Io(format!("读取目录项失败：{error}")))?;
        let name = entry.file_name();
        // 排除任意层级的 .git（目录与文件）
        if name == ".git" {
            continue;
        }
        let path = entry.path();
        let metadata = fs::symlink_metadata(&path)
            .map_err(|error| AppError::Io(format!("读取失败 {}：{error}", path.display())))?;
        if platform::is_link_like(&metadata) {
            return Err(AppError::Validation(format!(
                "拒绝复制符号链接 / reparse point：{}",
                path.display()
            )));
        }
        let target = dst.join(&name);
        if metadata.is_dir() {
            copy_dir_recursive(&path, &target)?;
        } else if metadata.is_file() {
            fs::copy(&path, &target).map_err(|error| {
                AppError::Io(format!(
                    "复制文件失败 {} → {}：{error}",
                    path.display(),
                    target.display()
                ))
            })?;
        } else {
            return Err(AppError::Validation(format!(
                "不支持的条目类型，已拒绝复制：{}",
                path.display()
            )));
        }
    }
    Ok(())
}

fn temp_dir_for(dst: &Path) -> PathBuf {
    let name = dst
        .file_name()
        .map(|value| value.to_string_lossy().to_string())
        .unwrap_or_else(|| "copy".to_string());
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.subsec_nanos())
        .unwrap_or(0);
    dst.with_file_name(format!(".tmp-{name}-{}-{nanos}", std::process::id()))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write(path: &Path, content: &str) {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).unwrap();
        }
        fs::write(path, content).unwrap();
    }

    fn assert_no_leftovers(parent: &Path, target_name: &str) {
        for entry in fs::read_dir(parent).unwrap().flatten() {
            let name = entry.file_name().to_string_lossy().to_string();
            assert!(
                !name.starts_with(".tmp-"),
                "不应留下临时目录：{name}"
            );
            assert_ne!(name, target_name, "目标目录不应存在");
        }
    }

    #[test]
    fn excludes_git_directories_and_files_at_any_level() {
        let temp = tempfile::tempdir().unwrap();
        let src = temp.path().join("source");
        write(&src.join("keep.txt"), "keep");
        write(&src.join(".git").join("config"), "[core]");
        write(&src.join("sub").join(".git").join("HEAD"), "ref: refs/heads/main");
        write(&src.join("sub").join("inner.txt"), "inner");
        // worktree 风格的 .git 文件（位于独立子目录，避免与 .git 目录同名冲突）
        write(&src.join("worker").join(".git"), "gitdir: /elsewhere/.git/worktrees/x");
        write(&src.join("worker").join("code.txt"), "code");

        let dst = temp.path().join("target");
        copy_snapshot(&src, &dst).unwrap();

        assert!(dst.join("keep.txt").is_file());
        assert!(dst.join("sub").join("inner.txt").is_file());
        assert!(dst.join("worker").join("code.txt").is_file());
        assert!(!dst.join(".git").exists());
        assert!(!dst.join("sub").join(".git").exists());
        assert!(!dst.join("worker").join(".git").exists());
    }

    #[test]
    fn rejects_symlink_without_partial_output() {
        let temp = tempfile::tempdir().unwrap();
        let src = temp.path().join("source");
        write(&src.join("keep.txt"), "keep");
        #[cfg(unix)]
        std::os::unix::fs::symlink(temp.path(), src.join("link")).unwrap();
        #[cfg(not(unix))]
        {
            eprintln!("跳过：非 Unix 平台未创建符号链接");
            return;
        }

        let dst = temp.path().join("target");
        let error = copy_snapshot(&src, &dst).unwrap_err();
        assert_eq!(error.code(), crate::models::ErrorCode::Validation);
        assert_no_leftovers(temp.path(), "target");
    }

    #[test]
    fn rejects_existing_target() {
        let temp = tempfile::tempdir().unwrap();
        let src = temp.path().join("source");
        write(&src.join("keep.txt"), "keep");
        let dst = temp.path().join("target");
        fs::create_dir_all(&dst).unwrap();

        let error = copy_snapshot(&src, &dst).unwrap_err();
        assert_eq!(error.code(), crate::models::ErrorCode::Conflict);
    }

    #[test]
    fn failure_leaves_no_partial_output() {
        let temp = tempfile::tempdir().unwrap();
        let src = temp.path().join("source");
        write(&src.join("ok.txt"), "ok");
        write(&src.join("sub").join("blocked.txt"), "blocked");

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(
                src.join("sub").join("blocked.txt"),
                fs::Permissions::from_mode(0o000),
            )
            .unwrap();
        }

        let dst = temp.path().join("target");
        let result = copy_snapshot(&src, &dst);

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(
                src.join("sub").join("blocked.txt"),
                fs::Permissions::from_mode(0o644),
            )
            .unwrap();
        }

        if result.is_ok() {
            eprintln!("跳过：当前用户不受文件权限限制（可能以 root 运行），未能模拟中途失败");
            return;
        }
        assert_no_leftovers(temp.path(), "target");
    }
}
