//! 稳定错误对象（code/message）、凭证脱敏与消息截断（architecture.md §4.4）。

use crate::models::ErrorCode;

/// 用户可见 message 的最大字符数（超出以 `…` 结尾）
pub const MESSAGE_LIMIT: usize = 600;

#[derive(Debug, thiserror::Error)]
pub enum AppError {
    #[error("{0}")]
    Validation(String),
    #[error("已有操作执行中：{0}")]
    Busy(String),
    #[error("{0}")]
    NotFound(String),
    #[error("{0}")]
    Conflict(String),
    #[error("{0}")]
    ManifestDamaged(String),
    #[error("{0}")]
    Git(String),
    #[error("{0}")]
    Io(String),
    #[error("{0}")]
    Unsupported(String),
}

impl AppError {
    pub fn code(&self) -> ErrorCode {
        match self {
            AppError::Validation(_) => ErrorCode::Validation,
            AppError::Busy(_) => ErrorCode::Busy,
            AppError::NotFound(_) => ErrorCode::NotFound,
            AppError::Conflict(_) => ErrorCode::Conflict,
            AppError::ManifestDamaged(_) => ErrorCode::ManifestDamaged,
            AppError::Git(_) => ErrorCode::Git,
            AppError::Io(_) => ErrorCode::Io,
            AppError::Unsupported(_) => ErrorCode::Unsupported,
        }
    }

    /// 未脱敏、未截断的原始文案（Busy 在此补全固定前缀）
    pub fn raw_message(&self) -> String {
        match self {
            AppError::Validation(message) => message.clone(),
            AppError::Busy(name) => format!("已有操作执行中：{name}"),
            AppError::NotFound(message) => message.clone(),
            AppError::Conflict(message) => message.clone(),
            AppError::ManifestDamaged(message) => message.clone(),
            AppError::Git(message) => message.clone(),
            AppError::Io(message) => message.clone(),
            AppError::Unsupported(message) => message.clone(),
        }
    }

    /// 用户可见 message：先脱敏，再截断到 600 字符
    pub fn message(&self) -> String {
        truncate_message(&redact_credentials(&self.raw_message()))
    }

    /// 序列化为 `{ "code": "...", "message": "..." }`
    pub fn to_object(&self) -> serde_json::Value {
        serde_json::json!({ "code": self.code(), "message": self.message() })
    }
}

impl From<std::io::Error> for AppError {
    fn from(error: std::io::Error) -> Self {
        AppError::Io(error.to_string())
    }
}

/// 命令可直接返回 `Result<T, AppError>`：序列化为稳定错误对象
impl serde::Serialize for AppError {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        use serde::ser::SerializeMap;
        let mut map = serializer.serialize_map(Some(2))?;
        map.serialize_entry("code", &self.code())?;
        map.serialize_entry("message", &self.message())?;
        map.end()
    }
}

/// 去掉形如 `scheme://user:token@host` 的 `user:token@` 与 `scheme://token@host` 的 `token@`。
/// 手写扫描实现（不引入 regex）。
pub fn redact_credentials(input: &str) -> String {
    let mut output = String::with_capacity(input.len());
    let mut rest = input;
    while let Some(position) = rest.find("://") {
        let authority_start = position + 3;
        output.push_str(&rest[..authority_start]);
        // authority 段结束于 / ? # 或空白
        let mut authority_end = authority_start;
        for (offset, ch) in rest[authority_start..].char_indices() {
            if ch == '/' || ch == '?' || ch == '#' || ch.is_whitespace() {
                break;
            }
            authority_end = authority_start + offset + ch.len_utf8();
        }
        let authority = &rest[authority_start..authority_end];
        match authority.rfind('@') {
            Some(at) => output.push_str(&authority[at + 1..]),
            None => output.push_str(authority),
        }
        rest = &rest[authority_end..];
    }
    output.push_str(rest);
    output
}

/// 截断到 `MESSAGE_LIMIT` 个 Unicode 标量；超长时补 `…`
pub fn truncate_message(input: &str) -> String {
    let mut output: String = input.chars().take(MESSAGE_LIMIT).collect();
    if input.chars().count() > MESSAGE_LIMIT {
        output.push('…');
    }
    output
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn redacts_user_and_token_in_https_url() {
        assert_eq!(
            redact_credentials("无法访问 https://user:token@host/x 请检查网络"),
            "无法访问 https://host/x 请检查网络"
        );
    }

    #[test]
    fn redacts_token_only_ssh_url() {
        assert_eq!(
            redact_credentials("ssh://token@host"),
            "ssh://host"
        );
    }

    #[test]
    fn keeps_urls_without_credentials() {
        assert_eq!(
            redact_credentials("git@host:path/to/repo.git"),
            "git@host:path/to/repo.git"
        );
        assert_eq!(
            redact_credentials("https://host/x"),
            "https://host/x"
        );
    }

    #[test]
    fn redacts_multiple_urls_and_multibyte() {
        assert_eq!(
            redact_credentials("a https://u:p@h1/a b ssh://t@h2，还有中文"),
            "a https://h1/a b ssh://h2，还有中文"
        );
    }

    #[test]
    fn truncates_at_601_characters() {
        let long = "长".repeat(601);
        let truncated = truncate_message(&long);
        assert_eq!(truncated.chars().count(), 601); // 600 字符 + `…`
        assert!(truncated.ends_with('…'));

        let exact = "长".repeat(MESSAGE_LIMIT);
        assert_eq!(truncate_message(&exact), exact);
    }

    #[test]
    fn busy_message_has_fixed_prefix() {
        let error = AppError::Busy("创建工作区".to_string());
        assert_eq!(error.message(), "已有操作执行中：创建工作区");
        assert_eq!(error.code(), crate::models::ErrorCode::Busy);
    }

    #[test]
    fn to_object_shape_and_redaction() {
        let error = AppError::Git("fatal: https://user:token@host/repo 不可访问".to_string());
        let object = error.to_object();
        assert_eq!(object["code"], "git");
        assert_eq!(object["message"], "fatal: https://host/repo 不可访问");
    }
}
