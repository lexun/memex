//! Daemon service implementation

use anyhow::Result;

pub struct DaemonService;

impl DaemonService {
    pub fn new() -> Self {
        Self
    }

    pub async fn start(&self) -> Result<()> {
        tracing::info!("Starting Memex daemon...");
        Ok(())
    }

    pub async fn stop(&self) -> Result<()> {
        tracing::info!("Stopping Memex daemon...");
        Ok(())
    }
}

impl Default for DaemonService {
    fn default() -> Self {
        Self::new()
    }
}
