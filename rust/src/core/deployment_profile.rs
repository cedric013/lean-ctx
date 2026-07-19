//! Customer Deployment Profiles (P11).
//!
//! Packages the existing gateway_server/ and http_server/team/ into coherent,
//! named deployment profiles. No new binary is created; these are run-mode
//! configurations that absorb the existing KMU Gateway and Team Server.

use serde::{Deserialize, Serialize};

pub const DEPLOYMENT_PROFILE_SCHEMA_VERSION: u16 = 1;

/// Available deployment profiles.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DeploymentProfileKind {
    /// OSS, self-hosted, Auth, Postgres, local views.
    /// Absorbs: `gateway_server/`
    OrgGatewayBase,
    /// OSS, RBAC basis, Aggregation, Webhook, Connectors.
    /// Absorbs: `http_server/team/`
    TeamControlBase,
    /// Commercial add-on (NOT in OSS).
    /// SSO/SCIM, Org Policy, Assurance, Retention, Settlement.
    AiValueGate,
}

/// Feature flags available per deployment profile.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct ProfileFeatureSet {
    pub auth: bool,
    pub postgres_store: bool,
    pub local_views: bool,
    pub rbac: bool,
    pub aggregation: bool,
    pub webhooks: bool,
    pub connectors: bool,
    pub sso_scim: bool,
    pub org_policy: bool,
    pub assurance: bool,
    pub settlement: bool,
}

impl ProfileFeatureSet {
    #[must_use]
    pub fn for_profile(kind: DeploymentProfileKind) -> Self {
        match kind {
            DeploymentProfileKind::OrgGatewayBase => Self {
                auth: true,
                postgres_store: true,
                local_views: true,
                rbac: false,
                aggregation: false,
                webhooks: false,
                connectors: false,
                sso_scim: false,
                org_policy: false,
                assurance: false,
                settlement: false,
            },
            DeploymentProfileKind::TeamControlBase => Self {
                auth: true,
                postgres_store: true,
                local_views: true,
                rbac: true,
                aggregation: true,
                webhooks: true,
                connectors: true,
                sso_scim: false,
                org_policy: false,
                assurance: false,
                settlement: false,
            },
            DeploymentProfileKind::AiValueGate => Self {
                auth: true,
                postgres_store: true,
                local_views: true,
                rbac: true,
                aggregation: true,
                webhooks: true,
                connectors: true,
                sso_scim: true,
                org_policy: true,
                assurance: true,
                settlement: true,
            },
        }
    }
}

/// A resolved deployment profile with metadata.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct DeploymentProfileV1 {
    pub schema_version: u16,
    pub kind: DeploymentProfileKind,
    pub display_name: String,
    pub features: ProfileFeatureSet,
    pub is_oss: bool,
    pub requires_license: bool,
    pub cli_entry_point: String,
}

impl DeploymentProfileV1 {
    #[must_use]
    pub fn resolve(kind: DeploymentProfileKind) -> Self {
        match kind {
            DeploymentProfileKind::OrgGatewayBase => Self {
                schema_version: DEPLOYMENT_PROFILE_SCHEMA_VERSION,
                kind,
                display_name: "Org Gateway Base".into(),
                features: ProfileFeatureSet::for_profile(kind),
                is_oss: true,
                requires_license: false,
                cli_entry_point: "lean-ctx gateway serve".into(),
            },
            DeploymentProfileKind::TeamControlBase => Self {
                schema_version: DEPLOYMENT_PROFILE_SCHEMA_VERSION,
                kind,
                display_name: "Team Control Base".into(),
                features: ProfileFeatureSet::for_profile(kind),
                is_oss: true,
                requires_license: false,
                cli_entry_point: "lean-ctx team serve".into(),
            },
            DeploymentProfileKind::AiValueGate => Self {
                schema_version: DEPLOYMENT_PROFILE_SCHEMA_VERSION,
                kind,
                display_name: "AI Value Gate".into(),
                features: ProfileFeatureSet::for_profile(kind),
                is_oss: false,
                requires_license: true,
                cli_entry_point: "lean-ctx enterprise serve".into(),
            },
        }
    }

    pub fn validate(&self) -> Result<(), DeploymentProfileError> {
        if self.schema_version != DEPLOYMENT_PROFILE_SCHEMA_VERSION {
            return Err(DeploymentProfileError::UnsupportedVersion(
                self.schema_version,
            ));
        }
        if self.kind == DeploymentProfileKind::AiValueGate && self.is_oss {
            return Err(DeploymentProfileError::BoundaryViolation(
                "AI Value Gate cannot be marked as OSS".into(),
            ));
        }
        if !self.is_oss && !self.requires_license {
            return Err(DeploymentProfileError::BoundaryViolation(
                "non-OSS profile must require a license".into(),
            ));
        }
        Ok(())
    }
}

#[derive(Debug, thiserror::Error)]
pub enum DeploymentProfileError {
    #[error("unsupported schema version {0}")]
    UnsupportedVersion(u16),
    #[error("OSS/Commercial boundary violation: {0}")]
    BoundaryViolation(String),
}

// ─── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn org_gateway_is_oss_and_valid() {
        let profile = DeploymentProfileV1::resolve(DeploymentProfileKind::OrgGatewayBase);
        profile.validate().unwrap();
        assert!(profile.is_oss);
        assert!(!profile.requires_license);
        assert_eq!(profile.cli_entry_point, "lean-ctx gateway serve");
    }

    #[test]
    fn team_control_is_oss_and_valid() {
        let profile = DeploymentProfileV1::resolve(DeploymentProfileKind::TeamControlBase);
        profile.validate().unwrap();
        assert!(profile.is_oss);
        assert!(!profile.requires_license);
    }

    #[test]
    fn ai_value_gate_is_commercial() {
        let profile = DeploymentProfileV1::resolve(DeploymentProfileKind::AiValueGate);
        profile.validate().unwrap();
        assert!(!profile.is_oss);
        assert!(profile.requires_license);
    }

    #[test]
    fn rejects_oss_value_gate() {
        let mut profile = DeploymentProfileV1::resolve(DeploymentProfileKind::AiValueGate);
        profile.is_oss = true;
        assert!(profile.validate().is_err());
    }

    #[test]
    fn feature_sets_are_superset_hierarchy() {
        let org = ProfileFeatureSet::for_profile(DeploymentProfileKind::OrgGatewayBase);
        let team = ProfileFeatureSet::for_profile(DeploymentProfileKind::TeamControlBase);
        let gate = ProfileFeatureSet::for_profile(DeploymentProfileKind::AiValueGate);

        assert!(team.rbac && !org.rbac);
        assert!(gate.sso_scim && !team.sso_scim);
        assert!(gate.settlement && !team.settlement);
    }
}
