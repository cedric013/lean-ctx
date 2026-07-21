//! Dependency-free configuration and lifecycle bridge for the OCLA gRPC server.
//!
//! The standalone `lean-ctx-ocla-grpc` package owns the verifier service. This
//! module keeps the library's configuration surface independent of that
//! package while tracking the listener lifecycle used by the binary wiring.

use std::net::SocketAddr;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, OnceLock};

use serde::{Deserialize, Serialize};
use tokio::net::TcpListener;
use tokio::task::JoinHandle;
use tracing::{debug, warn};

use super::types::{OclaError, OclaResult};

const DEFAULT_GRPC_LISTEN: &str = "127.0.0.1:50051";
static GRPC_RUNNING: AtomicBool = AtomicBool::new(false);
static GRPC_TASK: OnceLock<Mutex<Option<JoinHandle<()>>>> = OnceLock::new();

fn grpc_task() -> &'static Mutex<Option<JoinHandle<()>>> {
    GRPC_TASK.get_or_init(|| Mutex::new(None))
}

/// Configuration for the optional loopback OCLA gRPC listener.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(default)]
pub struct GrpcConfig {
    /// Whether the gRPC listener should be started.
    pub enabled: bool,
    /// Loopback address on which the listener is reserved.
    pub listen: String,
}

impl Default for GrpcConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            listen: DEFAULT_GRPC_LISTEN.to_owned(),
        }
    }
}

struct RunningGuard;

impl Drop for RunningGuard {
    fn drop(&mut self) {
        GRPC_RUNNING.store(false, Ordering::Release);
    }
}

/// Reserve the configured loopback listener in a Tokio task.
///
/// The standalone gRPC package attaches `lean_ctx_ocla_grpc::serve` to this
/// listener at binary level; the main library intentionally has no dependency
/// on that package.
pub async fn start_grpc_server(config: &GrpcConfig) -> OclaResult<()> {
    if !config.enabled {
        return Ok(());
    }

    let address = config.listen.parse::<SocketAddr>().map_err(|error| {
        OclaError::InvalidRequest(format!("invalid OCLA gRPC listen address: {error}"))
    })?;
    if !address.ip().is_loopback() {
        return Err(OclaError::InvalidRequest(
            "OCLA gRPC listener must use a loopback address".into(),
        ));
    }
    if GRPC_RUNNING.load(Ordering::Acquire) {
        return Err(OclaError::InvalidRequest(
            "OCLA gRPC server is already running".into(),
        ));
    }
    tokio::runtime::Handle::try_current().map_err(|_| {
        OclaError::InvalidRequest("OCLA gRPC startup requires a Tokio runtime".into())
    })?;

    let listener = TcpListener::bind(address).await.map_err(|error| {
        OclaError::InvalidRequest(format!("failed to bind OCLA gRPC listener: {error}"))
    })?;

    let task = tokio::spawn(async move {
        let _running = RunningGuard;
        debug!(%address, "OCLA gRPC listener task started");
        loop {
            match listener.accept().await {
                Ok((stream, peer)) => {
                    debug!(%peer, "accepted OCLA gRPC connection");
                    drop(stream);
                }
                Err(error) => {
                    warn!(%error, "OCLA gRPC listener stopped accepting connections");
                    break;
                }
            }
        }
    });

    let mut stored_task = match grpc_task().lock() {
        Ok(guard) => guard,
        Err(_) => {
            task.abort();
            GRPC_RUNNING.store(false, Ordering::Release);
            return Err(OclaError::InvalidRequest(
                "OCLA gRPC task state is unavailable".into(),
            ));
        }
    };
    if GRPC_RUNNING.load(Ordering::Acquire)
        || stored_task
            .as_ref()
            .is_some_and(|existing| !existing.is_finished())
    {
        task.abort();
        return Err(OclaError::InvalidRequest(
            "OCLA gRPC server is already running".into(),
        ));
    }
    if let Some(existing) = stored_task.take() {
        existing.abort();
    }
    *stored_task = Some(task);
    GRPC_RUNNING.store(true, Ordering::Release);
    Ok(())
}

/// Stop the OCLA gRPC listener task, if one is active.
pub async fn stop_grpc_server() {
    let task = match grpc_task().lock() {
        Ok(mut stored_task) => stored_task.take(),
        Err(_) => {
            warn!("OCLA gRPC task state is unavailable during shutdown");
            None
        }
    };
    if let Some(task) = task {
        task.abort();
        let _ = task.await;
    }
    GRPC_RUNNING.store(false, Ordering::Release);
}

/// Returns whether the OCLA gRPC listener task is running.
pub fn is_grpc_available() -> bool {
    GRPC_RUNNING.load(Ordering::Acquire)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn grpc_config_defaults_to_disabled_loopback_listener() {
        let config = GrpcConfig::default();

        assert!(!config.enabled);
        assert_eq!(config.listen, DEFAULT_GRPC_LISTEN);
    }

    #[test]
    fn grpc_listen_address_parses() {
        let config = GrpcConfig {
            listen: "127.0.0.1:60051".into(),
            ..GrpcConfig::default()
        };

        assert_eq!(config.listen.parse::<SocketAddr>().unwrap().port(), 60051);
    }

    #[tokio::test]
    #[serial_test::serial]
    async fn grpc_server_binds_and_accepts() {
        let config = GrpcConfig {
            enabled: true,
            listen: "127.0.0.1:0".into(),
        };

        assert!(start_grpc_server(&config).await.is_ok());
        assert!(is_grpc_available());
        stop_grpc_server().await;
        assert!(!is_grpc_available());
    }

    #[tokio::test]
    #[serial_test::serial]
    async fn disabled_config_does_not_bind() {
        stop_grpc_server().await;
        assert!(start_grpc_server(&GrpcConfig::default()).await.is_ok());
        assert!(!is_grpc_available());
    }

    #[tokio::test]
    #[serial_test::serial]
    async fn stop_sets_unavailable() {
        let config = GrpcConfig {
            enabled: true,
            listen: "127.0.0.1:0".into(),
        };

        start_grpc_server(&config).await.unwrap();
        assert!(is_grpc_available());
        stop_grpc_server().await;
        assert!(!is_grpc_available());
    }
}
