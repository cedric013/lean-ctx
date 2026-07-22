//! Header-based caller identity resolution.

use super::identity::{CallerIdentity, CallerRole};

/// Header names used to resolve caller identity attributes.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ResolverConfig {
    /// Header containing the caller's user identifier.
    pub user_header: String,
    /// Header containing the caller's team identifier.
    pub team_header: String,
    /// Header containing the caller's cost center.
    pub cost_center_header: String,
    /// Header containing the caller's role.
    pub role_header: String,
    /// Header containing the caller's session identifier.
    pub session_header: String,
}

impl Default for ResolverConfig {
    fn default() -> Self {
        Self {
            user_header: "x-user-id".to_string(),
            team_header: "x-team-id".to_string(),
            cost_center_header: "x-cost-center".to_string(),
            role_header: "x-caller-role".to_string(),
            session_header: "x-session-id".to_string(),
        }
    }
}

/// Resolves a caller identity from headers using the supplied configuration.
pub fn resolve_from_headers(
    config: &ResolverConfig,
    headers: &[(String, String)],
) -> CallerIdentity {
    let mut identity = CallerIdentity::default();
    enrich_identity(&mut identity, headers, config);
    identity
}

/// Parses a case-insensitive caller role, defaulting to developer.
pub fn parse_role(value: &str) -> CallerRole {
    match value.trim().to_ascii_lowercase().as_str() {
        "reviewer" => CallerRole::Reviewer,
        "agent" => CallerRole::Agent,
        "system" => CallerRole::System,
        "admin" => CallerRole::Admin,
        _ => CallerRole::Developer,
    }
}

/// Resolves a caller identity using the default header names.
pub fn resolve_with_defaults(headers: &[(String, String)]) -> CallerIdentity {
    resolve_from_headers(&ResolverConfig::default(), headers)
}

/// Adds header-derived attributes without replacing populated identity fields.
pub fn enrich_identity(
    base: &mut CallerIdentity,
    headers: &[(String, String)],
    config: &ResolverConfig,
) {
    if base.user_id.is_none() {
        base.user_id = header_value(headers, &config.user_header);
    }
    if base.team_id.is_none() {
        base.team_id = header_value(headers, &config.team_header);
    }
    if base.cost_center.is_none() {
        base.cost_center = header_value(headers, &config.cost_center_header);
    }
    if base.session_id.is_none() {
        base.session_id = header_value(headers, &config.session_header);
    }
    if base.role == CallerRole::default()
        && let Some(role) = header_value(headers, &config.role_header)
    {
        base.role = parse_role(&role);
    }
}

fn header_value(headers: &[(String, String)], name: &str) -> Option<String> {
    headers
        .iter()
        .find(|(header, _)| header.eq_ignore_ascii_case(name))
        .map(|(_, value)| value)
        .cloned()
}

#[cfg(test)]
mod tests {
    use super::{
        CallerIdentity, CallerRole, ResolverConfig, enrich_identity, parse_role,
        resolve_from_headers, resolve_with_defaults,
    };

    fn headers(values: &[(&str, &str)]) -> Vec<(String, String)> {
        values
            .iter()
            .map(|(name, value)| ((*name).to_string(), (*value).to_string()))
            .collect()
    }

    #[test]
    fn resolve_empty_headers() {
        assert_eq!(resolve_with_defaults(&[]), CallerIdentity::default());
    }

    #[test]
    fn resolve_with_user_and_team() {
        let identity = resolve_with_defaults(&headers(&[
            ("x-user-id", "user-1"),
            ("x-team-id", "team-1"),
        ]));

        assert_eq!(identity.user_id.as_deref(), Some("user-1"));
        assert_eq!(identity.team_id.as_deref(), Some("team-1"));
    }

    #[test]
    fn parse_role_case_insensitive() {
        for value in ["Agent", "AGENT", "agent"] {
            assert_eq!(parse_role(value), CallerRole::Agent);
        }
    }

    #[test]
    fn parse_role_unknown_defaults() {
        assert_eq!(parse_role("unknown"), CallerRole::Developer);
    }

    #[test]
    fn enrich_does_not_overwrite() {
        let mut identity = CallerIdentity {
            user_id: Some("existing".to_string()),
            ..CallerIdentity::default()
        };

        enrich_identity(
            &mut identity,
            &headers(&[("x-user-id", "replacement"), ("x-team-id", "team-1")]),
            &ResolverConfig::default(),
        );

        assert_eq!(identity.user_id.as_deref(), Some("existing"));
        assert_eq!(identity.team_id.as_deref(), Some("team-1"));
    }

    #[test]
    fn custom_header_names() {
        let config = ResolverConfig {
            user_header: "caller".to_string(),
            team_header: "group".to_string(),
            ..ResolverConfig::default()
        };
        let identity = resolve_from_headers(
            &config,
            &headers(&[("Caller", "user-2"), ("GROUP", "team-2")]),
        );

        assert_eq!(identity.user_id.as_deref(), Some("user-2"));
        assert_eq!(identity.team_id.as_deref(), Some("team-2"));
    }
}
