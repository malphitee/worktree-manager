//! 原子 JSON 读写（architecture.md §6.1）：
//! 同目录唯一临时文件 → write_all → flush → sync_all → rename；失败时清理临时文件。

use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use serde::de::DeserializeOwned;
use serde::Serialize;

use crate::error::AppError;

/// 读取并解析 JSON；失败返回 `io` 错误（损坏的清单由调用方判 `damaged`）
pub fn read_json<T: DeserializeOwned>(path: &Path) -> Result<T, AppError> {
    let text = fs::read_to_string(path)
        .map_err(|error| AppError::Io(format!("读取失败 {}：{error}", path.display())))?;
    serde_json::from_str(&text)
        .map_err(|error| AppError::Io(format!("解析失败 {}：{error}", path.display())))
}

/// 原子写入 JSON（临时文件 → flush → sync_all → rename），失败时删除临时文件
pub fn write_json_atomic<T: Serialize>(path: &Path, value: &T) -> Result<(), AppError> {
    let Some(parent) = path.parent() else {
        return Err(AppError::Io(format!("无效路径：{}", path.display())));
    };
    fs::create_dir_all(parent)
        .map_err(|error| AppError::Io(format!("创建目录失败 {}：{error}", parent.display())))?;

    let payload = serde_json::to_vec_pretty(value)
        .map_err(|error| AppError::Io(format!("序列化失败：{error}")))?;
    let temp_path = temp_path_for(path);

    let written = (|| -> std::io::Result<()> {
        let mut file = fs::File::create(&temp_path)?;
        file.write_all(&payload)?;
        file.write_all(b"\n")?;
        file.flush()?;
        file.sync_all()?;
        Ok(())
    })();

    if let Err(error) = written {
        let _ = fs::remove_file(&temp_path);
        return Err(AppError::Io(format!(
            "写入失败 {}：{error}",
            path.display()
        )));
    }

    if let Err(error) = fs::rename(&temp_path, path) {
        let _ = fs::remove_file(&temp_path);
        return Err(AppError::Io(format!(
            "写入失败 {}：{error}",
            path.display()
        )));
    }
    Ok(())
}

fn temp_path_for(path: &Path) -> PathBuf {
    let file_name = path
        .file_name()
        .map(|name| name.to_string_lossy().to_string())
        .unwrap_or_else(|| "json".to_string());
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.subsec_nanos())
        .unwrap_or(0);
    let pid = std::process::id();
    path.with_file_name(format!(".{file_name}.tmp-{pid}-{nanos}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::AppConfig;

    #[test]
    fn write_then_read_round_trip() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("config.json");
        let config = AppConfig {
            schema_version: 1,
            workspace_root: Some("/Users/me/Work/workspace".to_string()),
            ..Default::default()
        };
        write_json_atomic(&path, &config).unwrap();
        let back: AppConfig = read_json(&path).unwrap();
        assert_eq!(back, config);

        // 覆盖写入后仍可读
        let updated = AppConfig {
            workspace_root: Some("/Users/me/Other".to_string()),
            ..config
        };
        write_json_atomic(&path, &updated).unwrap();
        let back: AppConfig = read_json(&path).unwrap();
        assert_eq!(back.workspace_root.as_deref(), Some("/Users/me/Other"));
    }

    #[test]
    fn write_failure_leaves_no_temp_file() {
        let temp = tempfile::tempdir().unwrap();
        let readonly = temp.path().join("ro");
        fs::create_dir(&readonly).unwrap();
        let path = readonly.join("config.json");

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&readonly, fs::Permissions::from_mode(0o555)).unwrap();
        }

        let result = write_json_atomic(&path, &AppConfig::default());

        // 以 root 运行时权限不生效：打印原因并跳过断言（不虚报）
        let blocked = result.is_err();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&readonly, fs::Permissions::from_mode(0o755)).unwrap();
        }
        if !blocked {
            eprintln!("跳过：当前用户不受目录权限限制（可能以 root 运行），未能模拟写入失败");
            return;
        }

        let leftovers: Vec<String> = fs::read_dir(&readonly)
            .unwrap()
            .flatten()
            .map(|entry| entry.file_name().to_string_lossy().to_string())
            .collect();
        assert!(
            leftovers.is_empty(),
            "写入失败后不应留下临时文件：{leftovers:?}"
        );
    }

    #[test]
    fn damaged_file_reports_error_without_changing_content() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("config.json");
        let damaged = "{ not json";
        fs::write(&path, damaged).unwrap();

        let result: Result<AppConfig, _> = read_json(&path);
        let error = result.unwrap_err();
        assert_eq!(error.code(), crate::models::ErrorCode::Io);
        assert_eq!(fs::read_to_string(&path).unwrap(), damaged);
    }
}
