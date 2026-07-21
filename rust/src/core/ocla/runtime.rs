//! Self-contained lifecycle runtime for the OCLA REST API.

use axum::serve;
use tokio::{net::TcpListener, task::JoinHandle};
use tokio_util::sync::CancellationToken;
use tracing::{debug, warn};

use crate::core::config::OclaConfig;

use super::OclaResult;
use super::wire_api::ocla_router;

/// Owns the OCLA REST server task and its graceful-shutdown signal.
pub struct OclaRuntime {
    rest_handle: JoinHandle<()>,
    cancel: CancellationToken,
    rest_port: u16,
}

impl OclaRuntime {
    /// Starts the OCLA REST API on an OS-assigned loopback port.
    pub async fn start(config: &OclaConfig) -> OclaResult<Self> {
        let _ = config;
        let listener = TcpListener::bind("127.0.0.1:0").await.map_err(|error| {
            super::OclaError::InvalidRequest(format!("failed to bind OCLA REST listener: {error}"))
        })?;
        let rest_port = listener
            .local_addr()
            .map_err(|error| {
                super::OclaError::InvalidRequest(format!(
                    "failed to inspect OCLA REST listener: {error}"
                ))
            })?
            .port();
        let cancel = CancellationToken::new();
        let shutdown = cancel.clone();
        let rest_handle = tokio::spawn(async move {
            if let Err(error) = serve(listener, ocla_router())
                .with_graceful_shutdown(shutdown.cancelled_owned())
                .await
            {
                warn!(error = %error, "OCLA REST server stopped with an error");
            }
        });

        Ok(Self {
            rest_handle,
            cancel,
            rest_port,
        })
    }

    /// Requests graceful shutdown and waits for the REST task to finish.
    pub async fn stop(self) -> OclaResult<()> {
        self.cancel.cancel();
        self.rest_handle.await.map_err(|error| {
            super::OclaError::InvalidRequest(format!("OCLA REST task failed: {error}"))
        })?;
        debug!("OCLA REST runtime stopped");
        Ok(())
    }

    /// Returns the OS-assigned REST listener port.
    #[must_use]
    pub fn rest_port(&self) -> u16 {
        self.rest_port
    }

    /// Returns whether the REST server task has not finished.
    #[must_use]
    pub fn is_running(&self) -> bool {
        !self.rest_handle.is_finished()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn start_and_stop_lifecycle() {
        let runtime = OclaRuntime::start(&OclaConfig::default())
            .await
            .expect("runtime starts");
        assert!(runtime.is_running());
        runtime.stop().await.expect("runtime stops");
    }

    #[tokio::test]
    async fn rest_port_is_nonzero() {
        let runtime = OclaRuntime::start(&OclaConfig::default())
            .await
            .expect("runtime starts");
        assert_ne!(runtime.rest_port(), 0);
        runtime.stop().await.expect("runtime stops");
    }

    #[tokio::test]
    async fn double_stop_is_idempotent() {
        let runtime = OclaRuntime::start(&OclaConfig::default())
            .await
            .expect("runtime starts");
        runtime.cancel.cancel();
        runtime.cancel.cancel();
        runtime.stop().await.expect("runtime stops");
    }
}
