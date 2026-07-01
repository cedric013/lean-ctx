//! Self-hosted org gateway run-mode (`gateway-server` feature).
//!
//! Bundles the proxy (remote bind, enterprise#8), the per-request usage store
//! (Postgres `usage_events`, enterprise#17/#18) and — in later waves — the
//! admin usage API (enterprise#20) into one deployable server.
//!
//! LOCAL_OPTIONAL by classification (`server_capabilities.rs`): compiled in or
//! out, never gated by account/license/plan — an org can self-host its gateway
//! with `cargo build --features gateway-server` and full functionality
//! (Local-Free Invariant; commercial enforcement lives in lean-ctx-enterprise).
//!
//! Fail-open is the design rule (enterprise#12): the store subscribes to the
//! usage stream through `proxy::usage_sink` and persists asynchronously; a slow
//! or down Postgres degrades metering, never live LLM traffic.

pub mod store;
