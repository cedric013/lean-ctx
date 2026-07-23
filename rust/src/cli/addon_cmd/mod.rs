//! `lean-ctx addon` — manage community addons (MCP extensions) (#858).
//!
//! Thin CLI over [`crate::core::addons`]: browse the registry, install an addon
//! (from the registry or a local `lean-ctx-addon.toml`), and remove it. `add`
//! and `remove` wire external code into the MCP gateway, so both pass through
//! the shared confirmation gate (`cli::prompt`).

use std::path::Path;

use super::addon_deps::{
    addon_self_ref, install_declared_deps, refresh_pack_dependencies, resolve_declared_deps,
};
use super::prompt;
use crate::core::addons::manifest::AddonManifest;
use crate::core::addons::revocation::RevocationList;
use crate::core::addons::store::{ArtifactReceipt, InstalledStore};
use crate::core::addons::{artifact_install, bootstrap, install, registry};

mod authoring;
mod commands;
mod display;
mod management;

#[allow(unreachable_pub, unused_imports)]
pub use authoring::*;
pub use commands::*;
#[allow(unreachable_pub, unused_imports)]
pub use display::*;
#[allow(unreachable_pub, unused_imports)]
pub use management::*;

/// Read the value following `flag` in `args` (e.g. `--reason "text"`).
pub(super) fn flag_value(args: &[String], flag: &str) -> Option<String> {
    args.iter()
        .position(|a| a == flag)
        .and_then(|i| args.get(i + 1))
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
}
