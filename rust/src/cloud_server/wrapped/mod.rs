//! Hosted opt-in Wrapped permalink (`/api/wrapped`) — the public side of the viral loop.
//!
//! Anonymous publish returns a public `id` + one-time `edit_token`; the token authorizes
//! delete and the optional account `claim`. Only a closed whitelist of aggregate fields is
//! accepted (`deny_unknown_fields`); no repo names, paths, code, history or raw IPs are stored.
//!
//! Contract: `docs/contracts/wrapped-permalink-v1.md`.

mod cards;
mod common;
mod leaderboard;
mod link;
mod payload;
mod publish;
mod render;
mod signed;

#[allow(unused_imports)]
pub(in crate::cloud_server) use cards::*;
#[allow(unused_imports)]
pub(in crate::cloud_server) use common::*;
#[allow(unused_imports)]
pub(in crate::cloud_server) use leaderboard::*;
#[allow(unused_imports)]
pub(in crate::cloud_server) use link::*;
#[allow(unused_imports)]
pub(in crate::cloud_server) use payload::*;
#[allow(unused_imports)]
pub(in crate::cloud_server) use publish::*;
#[allow(unused_imports)]
pub(in crate::cloud_server) use render::*;
#[allow(unused_imports)]
pub(in crate::cloud_server) use signed::*;

#[cfg(test)]
mod tests;
