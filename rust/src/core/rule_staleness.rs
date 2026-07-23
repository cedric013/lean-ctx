#![allow(dead_code)] // Reserved: doctor/CLI rule audit integration pending
//! Rule staleness detection (#1141): identifies agent config rules that
//! reference non-existent files, deprecated APIs, or removed patterns.
//!
//! Parses rules for file path references and code patterns, then validates
//! them against the current codebase state. Stale rules waste context budget
//! and may actively mislead the agent.
//!
//! Determinism (#498): same rules + same filesystem → same staleness report.
use std::path::Path;

/// A staleness finding for a single rule.
#[derive(Debug, Clone)]
pub struct StalenessFinding {
    pub rule_id: String,
    pub rule_path: String,
    pub reason: StalenessReason,
    pub evidence: String,
}

/// Why a rule is considered stale.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StalenessReason {
    ReferencesDeletedFile,
    ReferencesDeletedDirectory,
    ReferencesDeprecatedPattern,
    NoMatchingCodePattern,
    EmptyOrTrivial,
}

impl std::fmt::Display for StalenessReason {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::ReferencesDeletedFile => write!(f, "references a deleted file"),
            Self::ReferencesDeletedDirectory => write!(f, "references a deleted directory"),
            Self::ReferencesDeprecatedPattern => write!(f, "references a deprecated pattern"),
            Self::NoMatchingCodePattern => write!(f, "pattern not found in codebase"),
            Self::EmptyOrTrivial => write!(f, "rule is empty or trivial"),
        }
    }
}

/// Audit result for a set of rules.
#[derive(Debug)]
pub struct AuditReport {
    pub findings: Vec<StalenessFinding>,
    pub rules_checked: usize,
    pub rules_stale: usize,
    pub estimated_wasted_tokens: usize,
}

/// Audit rules for staleness against the project root.
pub fn audit_rules(
    rules: &[(String, String, String)], // (id, path, content)
    project_root: &Path,
) -> AuditReport {
    let mut findings = Vec::new();

    for (id, rule_path, content) in rules {
        let mut rule_findings = Vec::new();

        // Check for empty/trivial rules
        let meaningful_chars = content.chars().filter(|c| c.is_alphanumeric()).count();
        if meaningful_chars < 20 {
            rule_findings.push(StalenessFinding {
                rule_id: id.clone(),
                rule_path: rule_path.clone(),
                reason: StalenessReason::EmptyOrTrivial,
                evidence: format!("Only {meaningful_chars} meaningful characters"),
            });
        }

        // Check file references
        for file_ref in extract_file_references(content) {
            let full_path = project_root.join(&file_ref);
            if !full_path.exists() && looks_like_real_path(&file_ref) {
                let reason = if file_ref.contains('/') && !file_ref.contains('.') {
                    StalenessReason::ReferencesDeletedDirectory
                } else {
                    StalenessReason::ReferencesDeletedFile
                };
                rule_findings.push(StalenessFinding {
                    rule_id: id.clone(),
                    rule_path: rule_path.clone(),
                    reason,
                    evidence: format!("`{file_ref}` does not exist"),
                });
            }
        }

        // Check for deprecated pattern indicators
        for indicator in extract_deprecation_indicators(content) {
            rule_findings.push(StalenessFinding {
                rule_id: id.clone(),
                rule_path: rule_path.clone(),
                reason: StalenessReason::ReferencesDeprecatedPattern,
                evidence: indicator,
            });
        }

        findings.extend(rule_findings);
    }

    let rules_stale = findings
        .iter()
        .map(|f| &f.rule_id)
        .collect::<std::collections::HashSet<_>>()
        .len();

    let estimated_wasted: usize = rules
        .iter()
        .filter(|(id, _, _)| findings.iter().any(|f| &f.rule_id == id))
        .map(|(_, _, content)| content.len() / 4)
        .sum();

    AuditReport {
        findings,
        rules_checked: rules.len(),
        rules_stale,
        estimated_wasted_tokens: estimated_wasted,
    }
}

/// Extract file/directory path references from rule content.
fn extract_file_references(content: &str) -> Vec<String> {
    let mut refs = Vec::new();

    for word in content.split_whitespace() {
        let cleaned = word.trim_matches(|c: char| {
            c == '`'
                || c == '"'
                || c == '\''
                || c == '('
                || c == ')'
                || c == '['
                || c == ']'
                || c == ','
                || c == ';'
        });

        if looks_like_file_path(cleaned) && looks_like_real_path(cleaned) {
            refs.push(cleaned.to_string());
        }
    }

    // Also check backtick-quoted paths
    let mut in_backtick = false;
    let mut current = String::new();
    for ch in content.chars() {
        if ch == '`' {
            if in_backtick && looks_like_file_path(&current) && looks_like_real_path(&current) {
                refs.push(current.clone());
            }
            current.clear();
            in_backtick = !in_backtick;
        } else if in_backtick {
            current.push(ch);
        }
    }

    refs.sort();
    refs.dedup();
    refs
}

/// Heuristic: does this string look like a file path?
fn looks_like_file_path(s: &str) -> bool {
    if s.len() < 5 || s.len() > 200 {
        return false;
    }
    // Must contain a dot (extension) or slash (directory)
    let has_extension = s.contains('.') && !s.starts_with('.');
    let has_slash = s.contains('/');

    if !has_extension && !has_slash {
        return false;
    }

    // Known extensions
    let known_ext = [
        ".rs", ".ts", ".tsx", ".js", ".jsx", ".py", ".go", ".java", ".toml", ".yaml", ".yml",
        ".json", ".md", ".sh", ".css", ".html", ".sql", ".proto", ".graphql",
    ];
    if has_extension && known_ext.iter().any(|ext| s.ends_with(ext)) {
        return true;
    }

    // Looks like a relative path with directories
    if has_slash && !s.starts_with("http") && !s.starts_with("//") {
        return s.chars().all(|c| c.is_alphanumeric() || "/-_.".contains(c));
    }

    false
}

/// Filter out obvious non-paths (URLs, globs meant as patterns, etc.)
fn looks_like_real_path(s: &str) -> bool {
    if s.starts_with("http") || s.starts_with("//") {
        return false;
    }
    if s.contains("**") || s.starts_with("*.") {
        return false; // glob pattern, not a real path
    }
    if s.starts_with('$') || s.starts_with('~') {
        return false; // variable or home dir
    }
    true
}

/// Detect indicators that a rule references deprecated things.
fn extract_deprecation_indicators(content: &str) -> Vec<String> {
    let mut indicators = Vec::new();
    let lower = content.to_lowercase();

    let deprecated_phrases = [
        ("todo: remove", "Contains TODO to remove"),
        ("deprecated", "Mentions deprecated"),
        ("no longer needed", "Marked as no longer needed"),
        ("legacy - ", "Marked as legacy"),
        ("temporary fix", "Marked as temporary fix"),
        ("hack:", "Contains hack marker"),
        ("workaround for", "Describes a workaround"),
    ];

    for (phrase, desc) in &deprecated_phrases {
        if lower.contains(phrase) {
            indicators.push(format!("{desc}: found `{phrase}`"));
        }
    }

    indicators
}

/// Format the audit report for CLI output.
pub fn format_report(report: &AuditReport) -> String {
    let mut out = String::new();
    out.push_str(&format!(
        "Rule Staleness Audit: {} rules checked, {} stale ({} wasted tokens)\n",
        report.rules_checked, report.rules_stale, report.estimated_wasted_tokens
    ));
    out.push_str(&"─".repeat(60));
    out.push('\n');

    if report.findings.is_empty() {
        out.push_str("All rules are current. No staleness detected.\n");
        return out;
    }

    let mut current_rule = String::new();
    for finding in &report.findings {
        if finding.rule_id != current_rule {
            out.push_str(&format!("\n{} ({})\n", finding.rule_id, finding.rule_path));
            finding.rule_id.clone_into(&mut current_rule);
        }
        out.push_str(&format!("  ⚠ {}: {}\n", finding.reason, finding.evidence));
    }

    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    #[test]
    fn detects_deleted_file_references() {
        let dir = TempDir::new().unwrap();
        fs::write(dir.path().join("existing.rs"), "fn main() {}").unwrap();

        let rules = vec![(
            "test-rule".to_string(),
            ".cursor/rules/test.mdc".to_string(),
            "Always check `existing.rs` and `deleted.rs` before modifying.".to_string(),
        )];

        let report = audit_rules(&rules, dir.path());
        assert!(
            report
                .findings
                .iter()
                .any(|f| f.reason == StalenessReason::ReferencesDeletedFile
                    && f.evidence.contains("deleted.rs"))
        );
    }

    #[test]
    fn detects_empty_rules() {
        let dir = TempDir::new().unwrap();
        let rules = vec![(
            "empty-rule".to_string(),
            ".cursor/rules/empty.mdc".to_string(),
            "# TODO\n\n".to_string(),
        )];

        let report = audit_rules(&rules, dir.path());
        assert!(
            report
                .findings
                .iter()
                .any(|f| f.reason == StalenessReason::EmptyOrTrivial)
        );
    }

    #[test]
    fn detects_deprecation_markers() {
        let dir = TempDir::new().unwrap();
        let rules = vec![(
            "legacy-rule".to_string(),
            ".cursor/rules/old.mdc".to_string(),
            "This is a temporary fix for the auth bug. TODO: remove after v2 ships. Use the deprecated auth_v1 module.".to_string(),
        )];

        let report = audit_rules(&rules, dir.path());
        assert!(report.rules_stale > 0);
        assert!(
            report
                .findings
                .iter()
                .any(|f| f.reason == StalenessReason::ReferencesDeprecatedPattern)
        );
    }

    #[test]
    fn existing_files_are_not_flagged() {
        let dir = TempDir::new().unwrap();
        fs::create_dir_all(dir.path().join("src/proxy")).unwrap();
        fs::write(dir.path().join("src/proxy/forward.rs"), "fn forward() {}").unwrap();

        let rules = vec![(
            "good-rule".to_string(),
            ".cursor/rules/proxy.mdc".to_string(),
            "The proxy logic lives in `src/proxy/forward.rs`. Keep it under 1500 lines."
                .to_string(),
        )];

        let report = audit_rules(&rules, dir.path());
        let file_findings: Vec<_> = report
            .findings
            .iter()
            .filter(|f| f.reason == StalenessReason::ReferencesDeletedFile)
            .collect();
        assert!(
            file_findings.is_empty(),
            "Existing file should not be flagged"
        );
    }

    #[test]
    fn ignores_urls_and_globs() {
        let refs = extract_file_references("Check https://docs.rs/tokio and use **/*.rs pattern");
        assert!(!refs.iter().any(|r| r.contains("https")));
        assert!(!refs.iter().any(|r| r.contains("**")));
    }

    #[test]
    fn report_format_is_readable() {
        let report = AuditReport {
            findings: vec![StalenessFinding {
                rule_id: "test".into(),
                rule_path: ".cursor/rules/test.mdc".into(),
                reason: StalenessReason::ReferencesDeletedFile,
                evidence: "`old.rs` does not exist".into(),
            }],
            rules_checked: 5,
            rules_stale: 1,
            estimated_wasted_tokens: 42,
        };
        let formatted = format_report(&report);
        assert!(formatted.contains("5 rules checked"));
        assert!(formatted.contains("1 stale"));
        assert!(formatted.contains("old.rs"));
    }
}
