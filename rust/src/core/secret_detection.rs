use std::sync::OnceLock;

use regex::Regex;

use crate::core::config::{Config, SecretDetectionConfig};

macro_rules! static_regex {
    ($pattern:expr) => {{
        static RE: OnceLock<Regex> = OnceLock::new();
        RE.get_or_init(|| Regex::new($pattern).expect(concat!("invalid regex: ", $pattern)))
    }};
}

#[derive(Debug, Clone)]
pub struct SecretMatch {
    pub pattern_name: &'static str,
    pub line_number: usize,
    pub redacted_preview: String,
}

fn aws_key_re() -> &'static Regex {
    static_regex!(r"AKIA[0-9A-Z]{16}")
}

fn private_key_re() -> &'static Regex {
    static_regex!(r"-----BEGIN (?:RSA |EC |DSA |OPENSSH )?PRIVATE KEY-----")
}

fn github_token_re() -> &'static Regex {
    static_regex!(r"gh[ps]_[A-Za-z0-9_]{36,}")
}

fn anthropic_key_re() -> &'static Regex {
    static_regex!(r"sk-ant-[A-Za-z0-9_\-]{20,}")
}

fn openai_key_re() -> &'static Regex {
    static_regex!(r"sk-[A-Za-z0-9]{20,}")
}

fn generic_api_key_re() -> &'static Regex {
    static_regex!(
        r#"(?i)(?:api[_-]?key|secret[_-]?key|token|password|passwd|access[_-]?token|client[_-]?secret)\s*[=:]\s*['"]?[a-zA-Z0-9_\-]{20,}"#
    )
}

fn high_entropy_b64_re() -> &'static Regex {
    static_regex!(
        r#"(?i)(?:key|token|secret|password|credential|auth)\s*[=:]\s*['"]?[A-Za-z0-9+/=\-_]{40,}"#
    )
}

fn gitlab_pat_re() -> &'static Regex {
    static_regex!(r"glpat-[A-Za-z0-9_\-]{20,}")
}

const BUILTIN_PATTERNS: &[(&str, fn() -> &'static Regex)] = &[
    ("aws_key", aws_key_re),
    ("private_key", private_key_re),
    ("github_token", github_token_re),
    ("anthropic_key", anthropic_key_re),
    ("openai_key", openai_key_re),
    ("gitlab_pat", gitlab_pat_re),
    ("generic_api_key", generic_api_key_re),
    ("high_entropy_secret", high_entropy_b64_re),
];

fn make_redacted_preview(matched: &str) -> String {
    let chars: Vec<char> = matched.chars().collect();
    if chars.len() <= 6 {
        return "***".to_string();
    }
    let prefix: String = chars[..4].iter().collect();
    let suffix: String = chars[chars.len() - 2..].iter().collect();
    format!("{prefix}***{suffix}")
}

pub fn detect_secrets(content: &str) -> Vec<SecretMatch> {
    let mut matches = Vec::new();

    let line_offsets: Vec<usize> = std::iter::once(0)
        .chain(content.match_indices('\n').map(|(i, _)| i + 1))
        .collect();

    let offset_to_line = |byte_offset: usize| -> usize {
        match line_offsets.binary_search(&byte_offset) {
            Ok(i) => i + 1,
            Err(i) => i,
        }
    };

    for &(name, regex_fn) in BUILTIN_PATTERNS {
        let re = regex_fn();
        for m in re.find_iter(content) {
            matches.push(SecretMatch {
                pattern_name: name,
                line_number: offset_to_line(m.start()),
                redacted_preview: make_redacted_preview(m.as_str()),
            });
        }
    }

    matches
}

pub fn detect_secrets_with_custom(content: &str, custom_patterns: &[String]) -> Vec<SecretMatch> {
    let mut matches = detect_secrets(content);

    for pattern_str in custom_patterns {
        if let Ok(re) = Regex::new(pattern_str) {
            let line_offsets: Vec<usize> = std::iter::once(0)
                .chain(content.match_indices('\n').map(|(i, _)| i + 1))
                .collect();

            for m in re.find_iter(content) {
                let line = match line_offsets.binary_search(&m.start()) {
                    Ok(i) => i + 1,
                    Err(i) => i,
                };
                matches.push(SecretMatch {
                    pattern_name: "custom_pattern",
                    line_number: line,
                    redacted_preview: make_redacted_preview(m.as_str()),
                });
            }
        }
    }

    matches
}

pub fn scan_and_redact(
    content: &str,
    config: &SecretDetectionConfig,
) -> (String, Vec<SecretMatch>) {
    if !config.enabled {
        return (content.to_string(), Vec::new());
    }

    let matches = detect_secrets_with_custom(content, &config.custom_patterns);

    if matches.is_empty() || !config.redact {
        return (content.to_string(), matches);
    }

    let mut redacted = content.to_string();
    for &(name, regex_fn) in BUILTIN_PATTERNS {
        let re = regex_fn();
        redacted = re
            .replace_all(&redacted, |_: &regex::Captures| {
                format!("[REDACTED:{name}]")
            })
            .to_string();
    }

    for pattern_str in &config.custom_patterns {
        if let Ok(re) = Regex::new(pattern_str) {
            redacted = re
                .replace_all(&redacted, "[REDACTED:custom_pattern]")
                .to_string();
        }
    }

    (redacted, matches)
}

pub fn scan_and_redact_from_config(content: &str) -> (String, Vec<SecretMatch>) {
    let cfg = Config::load();
    scan_and_redact(content, &cfg.secret_detection)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_aws_key() {
        let input = "aws_key = AKIAIOSFODNN7EXAMPLE";
        let matches = detect_secrets(input);
        assert!(matches.iter().any(|m| m.pattern_name == "aws_key"));
    }

    #[test]
    fn detects_private_key_header() {
        let input = "-----BEGIN RSA PRIVATE KEY-----\nMIIEpAIB...";
        let matches = detect_secrets(input);
        assert!(matches.iter().any(|m| m.pattern_name == "private_key"));
    }

    #[test]
    fn detects_github_token() {
        let input = "export GITHUB_TOKEN=ghp_ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijkl";
        let matches = detect_secrets(input);
        assert!(matches.iter().any(|m| m.pattern_name == "github_token"));
    }

    #[test]
    fn detects_anthropic_key() {
        let input = "ANTHROPIC_API_KEY=sk-ant-api03-abcdef1234567890ABCD";
        let matches = detect_secrets(input);
        assert!(matches.iter().any(|m| m.pattern_name == "anthropic_key"));
    }

    #[test]
    fn detects_openai_key() {
        let input = "OPENAI_API_KEY=sk-abcdefghijklmnopqrstuvwx";
        let matches = detect_secrets(input);
        assert!(matches.iter().any(|m| m.pattern_name == "openai_key"));
    }

    #[test]
    fn detects_gitlab_pat() {
        let input = "token = glpat-xxxxxxxxxxxxxxxxxxxx";
        let matches = detect_secrets(input);
        assert!(matches.iter().any(|m| m.pattern_name == "gitlab_pat"));
    }

    #[test]
    fn detects_generic_api_key() {
        let input = "api_key = abcdefghijklmnopqrstuvwxyz1234567890";
        let matches = detect_secrets(input);
        assert!(matches.iter().any(
            |m| m.pattern_name == "generic_api_key" || m.pattern_name == "high_entropy_secret"
        ));
    }

    #[test]
    fn clean_content_returns_empty() {
        let input = "fn main() { println!(\"hello world\"); }";
        let matches = detect_secrets(input);
        assert!(matches.is_empty());
    }

    #[test]
    fn redacted_preview_format() {
        let preview = make_redacted_preview("AKIAIOSFODNN7EXAMPLE");
        assert!(preview.starts_with("AKIA"));
        assert!(preview.ends_with("LE"));
        assert!(preview.contains("***"));
    }

    #[test]
    fn redacted_preview_short_string() {
        let preview = make_redacted_preview("short");
        assert_eq!(preview, "***");
    }

    #[test]
    fn scan_and_redact_replaces_secrets() {
        let cfg = SecretDetectionConfig {
            enabled: true,
            redact: true,
            custom_patterns: Vec::new(),
        };
        let input = "key = AKIAIOSFODNN7EXAMPLE";
        let (redacted, matches) = scan_and_redact(input, &cfg);
        assert!(!matches.is_empty());
        assert!(redacted.contains("[REDACTED:aws_key]"));
        assert!(!redacted.contains("AKIAIOSFODNN7EXAMPLE"));
    }

    #[test]
    fn scan_without_redact_preserves_content() {
        let cfg = SecretDetectionConfig {
            enabled: true,
            redact: false,
            custom_patterns: Vec::new(),
        };
        let input = "key = AKIAIOSFODNN7EXAMPLE";
        let (output, matches) = scan_and_redact(input, &cfg);
        assert!(!matches.is_empty());
        assert_eq!(output, input);
    }

    #[test]
    fn disabled_detection_returns_unchanged() {
        let cfg = SecretDetectionConfig {
            enabled: false,
            redact: true,
            custom_patterns: Vec::new(),
        };
        let input = "key = AKIAIOSFODNN7EXAMPLE";
        let (output, matches) = scan_and_redact(input, &cfg);
        assert!(matches.is_empty());
        assert_eq!(output, input);
    }

    #[test]
    fn custom_pattern_detected() {
        let cfg = SecretDetectionConfig {
            enabled: true,
            redact: true,
            custom_patterns: vec![r"MYCORP_[A-Z]{10,}".to_string()],
        };
        let input = "value is MYCORP_ABCDEFGHIJKLMNO here";
        let (redacted, matches) = scan_and_redact(input, &cfg);
        assert!(matches.iter().any(|m| m.pattern_name == "custom_pattern"));
        assert!(redacted.contains("[REDACTED:custom_pattern]"));
    }

    #[test]
    fn line_numbers_are_correct() {
        let input = "line1\nline2\nAKIAIOSFODNN7EXAMPLE\nline4";
        let matches = detect_secrets(input);
        assert!(!matches.is_empty());
        assert_eq!(matches[0].line_number, 3);
    }

    #[test]
    fn multiple_secrets_on_different_lines() {
        let input = "AKIAIOSFODNN7EXAMPLE\nclean\nsk-abcdefghijklmnopqrstuvwx";
        let matches = detect_secrets(input);
        assert!(matches.len() >= 2);
        let aws = matches
            .iter()
            .find(|m| m.pattern_name == "aws_key")
            .unwrap();
        assert_eq!(aws.line_number, 1);
        let oai = matches
            .iter()
            .find(|m| m.pattern_name == "openai_key")
            .unwrap();
        assert_eq!(oai.line_number, 3);
    }

    #[test]
    fn ec_private_key_detected() {
        let input = "-----BEGIN EC PRIVATE KEY-----";
        let matches = detect_secrets(input);
        assert!(matches.iter().any(|m| m.pattern_name == "private_key"));
    }

    #[test]
    fn openssh_private_key_detected() {
        let input = "-----BEGIN OPENSSH PRIVATE KEY-----";
        let matches = detect_secrets(input);
        assert!(matches.iter().any(|m| m.pattern_name == "private_key"));
    }
}
