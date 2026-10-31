//! Best-effort secret detection and redaction for host input and audit text.
//!
//! This is a safety net, not a secret store. Hosts must read credentials from
//! `.env` or the OS keyring and must never send a detected secret to a model.

/// Kind of secret detected in free text.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SecretKind {
    OpenAi,
    Anthropic,
    Google,
    GitHub,
    Slack,
    Aws,
    PrivateKey,
    Generic,
}

impl SecretKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::OpenAi => "OpenAI API key",
            Self::Anthropic => "Anthropic API key",
            Self::Google => "Google API key",
            Self::GitHub => "GitHub token",
            Self::Slack => "Slack token",
            Self::Aws => "AWS access key",
            Self::PrivateKey => "private key",
            Self::Generic => "credential",
        }
    }
}

/// Detects a likely secret in free text. False positives are acceptable here:
/// the caller should warn and stop rather than risk sending a key to a model.
pub fn detect_secret(text: &str) -> Option<SecretKind> {
    if text.contains("-----BEGIN") && text.contains("PRIVATE KEY-----") {
        return Some(SecretKind::PrivateKey);
    }
    if text.to_ascii_lowercase().contains("authorization: bearer ") {
        return Some(SecretKind::Generic);
    }
    text.split_whitespace().find_map(|token| {
        let token = token.trim_matches(|c: char| {
            matches!(
                c,
                '"' | '\'' | ',' | ';' | ')' | '(' | ']' | '[' | '{' | '}'
            )
        });
        detect_token(token).or_else(|| detect_assignment(token))
    })
}

/// Redacts detected secrets while preserving surrounding text and whitespace.
pub fn redact_secrets(text: &str) -> String {
    if text.contains("-----BEGIN") && text.contains("PRIVATE KEY-----") {
        return "[REDACTED PRIVATE KEY]".to_owned();
    }
    let mut redacted = String::with_capacity(text.len());
    for chunk in text.split_inclusive(char::is_whitespace) {
        let token = chunk.trim_end();
        let trailing = &chunk[token.len()..];
        if detect_secret(token).is_some() {
            redacted.push_str("[REDACTED]");
        } else {
            redacted.push_str(token);
        }
        redacted.push_str(trailing);
    }
    redacted
}

fn detect_token(token: &str) -> Option<SecretKind> {
    let key_chars = token
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.'));
    if token.starts_with("sk-ant-") && token.len() >= 24 && key_chars {
        return Some(SecretKind::Anthropic);
    }
    if token.starts_with("sk-") && token.len() >= 20 && key_chars {
        return Some(SecretKind::OpenAi);
    }
    if token.starts_with("AIza") && token.len() >= 30 && key_chars {
        return Some(SecretKind::Google);
    }
    if (token.starts_with("ghp_") || token.starts_with("github_pat_") || token.starts_with("gho_"))
        && token.len() >= 20
        && key_chars
    {
        return Some(SecretKind::GitHub);
    }
    if (token.starts_with("xoxb-") || token.starts_with("xoxp-") || token.starts_with("xoxa-"))
        && token.len() >= 12
        && key_chars
    {
        return Some(SecretKind::Slack);
    }
    if (token.starts_with("AKIA") || token.starts_with("ASIA"))
        && token.len() == 20
        && token
            .chars()
            .all(|c| c.is_ascii_uppercase() || c.is_ascii_digit())
    {
        return Some(SecretKind::Aws);
    }
    None
}

fn detect_assignment(token: &str) -> Option<SecretKind> {
    let lower = token.to_ascii_lowercase();
    for prefix in [
        "api_key=",
        "apikey=",
        "api-key=",
        "token=",
        "secret=",
        "password=",
    ] {
        if let Some(value) = lower.strip_prefix(prefix) {
            if value.len() >= 8 {
                return Some(SecretKind::Generic);
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_common_provider_keys() {
        assert_eq!(
            detect_secret("sk-1234567890abcdefghij"),
            Some(SecretKind::OpenAi)
        );
        assert_eq!(
            detect_secret("sk-ant-1234567890abcdefghij"),
            Some(SecretKind::Anthropic)
        );
        assert_eq!(
            detect_secret("ghp_1234567890abcdefghij"),
            Some(SecretKind::GitHub)
        );
        assert_eq!(detect_secret("AKIAIOSFODNN7EXAMPLE"), Some(SecretKind::Aws));
        assert_eq!(
            detect_secret("api_key=abcdefgh1234"),
            Some(SecretKind::Generic)
        );
        assert_eq!(
            detect_secret("-----BEGIN PRIVATE KEY-----\nabc\n-----END PRIVATE KEY-----"),
            Some(SecretKind::PrivateKey)
        );
    }

    #[test]
    fn ordinary_text_is_not_a_secret() {
        assert_eq!(detect_secret("please summarize the design"), None);
        assert_eq!(detect_secret("sk-short"), None);
    }

    #[test]
    fn redaction_preserves_surrounding_text() {
        let text = "my key sk-1234567890abcdefghij please";
        let redacted = redact_secrets(text);
        assert!(redacted.contains("[REDACTED]"));
        assert!(!redacted.contains("sk-1234567890abcdefghij"));
        assert!(redacted.starts_with("my key "));
        assert!(redacted.ends_with(" please"));
    }
}
