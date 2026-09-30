//! 平台抽象（architecture.md §2.2）：所有平台差异集中在本模块，
//! 其它模块不得写 `cfg(windows)` / `cfg(unix)`。

use std::path::{Path, PathBuf};
use std::process::Command;

use crate::error::AppError;

/// 应用配置目录：
/// - macOS：`$HOME/Library/Application Support/WorktreeManager`
/// - Windows：`%APPDATA%\WorktreeManager`
/// - Linux：`$XDG_CONFIG_HOME/WorktreeManager`，缺省 `$HOME/.config/WorktreeManager`
pub fn config_dir() -> PathBuf {
    #[cfg(target_os = "macos")]
    {
        let home = std::env::var_os("HOME").map(PathBuf::from).unwrap_or_default();
        home.join("Library")
            .join("Application Support")
            .join("WorktreeManager")
    }

    #[cfg(target_os = "windows")]
    {
        let appdata = std::env::var_os("APPDATA")
            .map(PathBuf::from)
            .unwrap_or_default();
        appdata.join("WorktreeManager")
    }

    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    {
        let base = std::env::var_os("XDG_CONFIG_HOME")
            .map(PathBuf::from)
            .or_else(|| std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".config")))
            .unwrap_or_default();
        base.join("WorktreeManager")
    }
}

/// 打开目录所用的命令与参数向量（供单元测试逐条比对，不拼接 shell 字符串）
pub fn open_in_file_manager_args(path: &Path) -> Vec<String> {
    let target = path.to_string_lossy().to_string();
    if cfg!(target_os = "macos") {
        vec!["open".to_string(), target]
    } else if cfg!(target_os = "windows") {
        vec!["explorer".to_string(), target]
    } else {
        vec!["xdg-open".to_string(), target]
    }
}

/// 用系统文件管理器打开目录
pub fn open_in_file_manager(path: &Path) -> Result<(), AppError> {
    let args = open_in_file_manager_args(path);
    let Some((program, rest)) = args.split_first() else {
        return Err(AppError::Io("打开目录失败：命令为空".to_string()));
    };
    let status = Command::new(program)
        .args(rest)
        .status()
        .map_err(|error| AppError::Io(format!("打开目录失败：{error}")))?;
    if status.success() {
        Ok(())
    } else {
        Err(AppError::Io(format!(
            "打开目录失败：{program} 退出码 {:?}",
            status.code()
        )))
    }
}

/// 是否符号链接 / reparse point（Windows 含 Junction）
pub fn is_link_like(metadata: &std::fs::Metadata) -> bool {
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        use windows_sys::Win32::Storage::FileSystem::FILE_ATTRIBUTE_REPARSE_POINT;
        metadata.file_type().is_symlink()
            || (metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT) != 0
    }

    #[cfg(not(windows))]
    {
        metadata.file_type().is_symlink()
    }
}

/// 大小写不敏感比较（全平台统一执行，见 architecture.md §2.3）
pub fn paths_equal(a: &Path, b: &Path) -> bool {
    normalize_for_compare(a) == normalize_for_compare(b)
}

fn normalize_for_compare(path: &Path) -> String {
    let text = path.to_string_lossy().replace('\\', "/");
    let trimmed = text.trim_end_matches('/');
    trimmed.to_lowercase()
}

/// 是否绝对路径
pub fn is_absolute(path: &Path) -> bool {
    path.is_absolute()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn config_dir_ends_with_app_name() {
        let dir = config_dir();
        assert_eq!(dir.file_name().and_then(|name| name.to_str()), Some("WorktreeManager"));
    }

    #[test]
    fn paths_equal_is_case_insensitive() {
        assert!(paths_equal(Path::new("A/b"), Path::new("a/B")));
        assert!(paths_equal(Path::new("/Users/me/Work"), Path::new("/Users/me/Work/")));
        assert!(!paths_equal(Path::new("/Users/me/a"), Path::new("/Users/me/b")));
    }

    #[test]
    fn open_args_use_platform_command() {
        let args = open_in_file_manager_args(Path::new("/tmp/工作区 7.3.0"));
        assert_eq!(args.len(), 2);
        assert_eq!(args[1], "/tmp/工作区 7.3.0");
        if cfg!(target_os = "macos") {
            assert_eq!(args[0], "open");
        } else if cfg!(target_os = "windows") {
            assert_eq!(args[0], "explorer");
        } else {
            assert_eq!(args[0], "xdg-open");
        }
    }

    #[test]
    fn absolute_path_detection() {
        assert!(is_absolute(Path::new("/tmp")));
        assert!(!is_absolute(Path::new("tmp")));
    }
}
