//! 迭代号、目录名、项目标识、分支名与路径校验（跨平台统一规则，architecture.md §2.3）。

use std::path::Path;

use crate::error::AppError;
use crate::platform;

/// 非法字符（Windows 保留规则，全平台统一执行）
const ILLEGAL_CHARS: [char; 9] = ['/', '\\', ':', '*', '?', '"', '<', '>', '|'];

/// Windows 保留设备名（大小写不敏感，含带扩展名形式）
const RESERVED_DEVICE_NAMES: [&str; 22] = [
    "CON", "PRN", "AUX", "NUL", "COM1", "COM2", "COM3", "COM4", "COM5", "COM6", "COM7", "COM8",
    "COM9", "LPT1", "LPT2", "LPT3", "LPT4", "LPT5", "LPT6", "LPT7", "LPT8", "LPT9",
];

/// 迭代号校验：工作区根目录下的单层目录名
pub fn validate_iteration(value: &str) -> Result<(), AppError> {
    validate_single_segment(value, "迭代号")
}

/// 公共目录目标名 / 项目目录名校验
pub fn validate_directory_name(value: &str) -> Result<(), AppError> {
    validate_single_segment(value, "目录名")
}

/// 项目标识校验：单层安全名，且不得与给定集合（公共目录目标名、其它项目标识）大小写不敏感冲突
pub fn validate_project_id(value: &str, conflicts: &[String]) -> Result<(), AppError> {
    validate_single_segment(value, "项目标识")?;
    for conflict in conflicts {
        if platform::paths_equal(Path::new(value), Path::new(conflict)) {
            return Err(AppError::Validation(format!(
                "项目标识「{value}」与「{conflict}」冲突（大小写不敏感）"
            )));
        }
    }
    Ok(())
}

/// 分支名本地规则校验；`git check-ref-format --branch` 由调用方追加执行
pub fn validate_branch_name(value: &str) -> Result<(), AppError> {
    let invalid = |reason: &str| {
        Err(AppError::Validation(format!(
            "分支名「{value}」不合法：{reason}"
        )))
    };

    if value.is_empty() {
        return invalid("不能为空");
    }
    if value.starts_with('-') || value.starts_with('/') || value.ends_with('/') {
        return invalid("不能以 - 或 / 开头/结尾");
    }
    if value.ends_with('.') {
        return invalid("不能以 . 结尾");
    }
    if value.ends_with(".lock") {
        return invalid("不能以 .lock 结尾");
    }
    if value == "@" {
        return invalid("不能是 @");
    }
    if value.contains("..") || value.contains("//") || value.contains("@{") {
        return invalid("不能包含 .. // 或 @{");
    }
    for ch in value.chars() {
        if ch.is_whitespace() || ch.is_control() {
            return invalid("不能包含空白或控制字符");
        }
        if matches!(ch, '^' | '~' | ':' | '?' | '*' | '[' | '\\' | '\u{7f}') {
            return invalid("包含 Git 禁止的字符");
        }
    }
    Ok(())
}

/// 绝对路径校验
pub fn validate_absolute_path(value: &str) -> Result<(), AppError> {
    if value.trim().is_empty() {
        return Err(AppError::Validation("路径不能为空".to_string()));
    }
    if !platform::is_absolute(Path::new(value)) {
        return Err(AppError::Validation(format!("必须是绝对路径：{value}")));
    }
    Ok(())
}

fn validate_single_segment(value: &str, label: &str) -> Result<(), AppError> {
    let invalid = |reason: &str| Err(AppError::Validation(format!("{label}「{value}」{reason}")));

    if value.is_empty() {
        return invalid("不能为空");
    }
    if value == "." || value == ".." {
        return invalid("不能是 . 或 ..");
    }
    for ch in value.chars() {
        if ILLEGAL_CHARS.contains(&ch) {
            return invalid("不能包含路径分隔符或非法字符");
        }
        if ch.is_control() {
            return invalid("不能包含控制字符");
        }
    }
    if value.ends_with(' ') || value.ends_with('.') {
        return invalid("末尾不能是空格或句点");
    }
    let base = value.split('.').next().unwrap_or_default().to_uppercase();
    if RESERVED_DEVICE_NAMES.contains(&base.as_str()) {
        return invalid("不能使用 Windows 保留设备名");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn valid_iterations_pass() {
        for value in ["7.3.0", "sprint-42", "迭代-1", "v1.2.3-rc1"] {
            assert!(validate_iteration(value).is_ok(), "{value} 应通过");
        }
    }

    #[test]
    fn invalid_iterations_are_rejected() {
        let cases = [
            ("", "空串"),
            (".", "单点"),
            ("..", "双点"),
            ("a/b", "路径分隔符"),
            ("a\\b", "反斜杠"),
            ("a:b", "冒号"),
            ("a*b", "星号"),
            ("a?b", "问号"),
            ("a\"b", "双引号"),
            ("a<b", "尖括号"),
            ("a>b", "尖括号"),
            ("a|b", "竖线"),
            ("CON", "保留设备名"),
            ("con", "保留设备名小写"),
            ("con.txt", "保留设备名带扩展名"),
            ("com1", "COM1"),
            ("lpt9.txt", "LPT9"),
            ("x.", "末尾句点"),
            ("x ", "末尾空格"),
            ("\u{1}", "控制字符"),
        ];
        for (value, reason) in cases {
            assert!(
                validate_iteration(value).is_err(),
                "{reason}（{value:?}）应被拒绝"
            );
        }
    }

    #[test]
    fn directory_name_rules_match_iteration_rules() {
        assert!(validate_directory_name("fd-common").is_ok());
        assert!(validate_directory_name("api3-2").is_ok());
        assert!(validate_directory_name("..").is_err());
        assert!(validate_directory_name("a/b").is_err());
        assert!(validate_directory_name("nul").is_err());
    }

    #[test]
    fn project_id_conflicts_are_case_insensitive() {
        let conflicts = vec!["fd-common".to_string(), "api3".to_string()];
        assert!(validate_project_id("web", &conflicts).is_ok());
        assert!(validate_project_id("API3", &conflicts).is_err());
        assert!(validate_project_id("FD-Common", &conflicts).is_err());
        assert!(validate_project_id("..", &conflicts).is_err());
    }

    #[test]
    fn branch_name_local_rules() {
        for value in ["feature/7.3.0", "release-1.2", "fix_#12"] {
            assert!(validate_branch_name(value).is_ok(), "{value} 应通过");
        }
        for value in [
            "",
            "-x",
            "/x",
            "x/",
            "x.",
            "x.lock",
            "@",
            "a..b",
            "a//b",
            "a@{b",
            "a b",
            "a^b",
            "a~b",
            "a:b",
            "a?b",
            "a*b",
            "a[b",
            "a\\b",
            "a\nb",
        ] {
            assert!(validate_branch_name(value).is_err(), "{value:?} 应被拒绝");
        }
    }

    #[test]
    fn absolute_path_validation() {
        assert!(validate_absolute_path("/tmp/x").is_ok());
        assert!(validate_absolute_path("tmp/x").is_err());
        assert!(validate_absolute_path("").is_err());
        #[cfg(target_os = "windows")]
        {
            assert!(validate_absolute_path("C:\\Work").is_ok());
            assert!(validate_absolute_path("\\\\server\\share").is_ok());
        }
    }
}

