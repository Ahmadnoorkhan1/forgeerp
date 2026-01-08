//! Offline mode detection and state management.

use thiserror::Error;

// Re-export from shared types module
pub use crate::types::ConnectivityState;

/// Offline mode configuration and state.
#[derive(Debug)]
pub struct OfflineMode {
    state: ConnectivityState,
    api_url: String,
}

#[derive(Debug, Error)]
pub enum OfflineError {
    #[error("client is offline; operation requires network connection")]
    Offline,
    #[error("API connection failed: {0}")]
    ApiConnection(String),
}

impl OfflineMode {
    pub fn new(api_url: String) -> Self {
        Self {
            state: ConnectivityState::Online,
            api_url,
        }
    }

    pub fn state(&self) -> ConnectivityState {
        self.state
    }

    pub fn api_url(&self) -> &str {
        &self.api_url
    }

    /// Mark the client as offline.
    pub fn set_offline(&mut self) {
        self.state = ConnectivityState::Offline;
    }

    /// Mark the client as online.
    pub fn set_online(&mut self) {
        self.state = ConnectivityState::Online;
    }

    /// Check if offline mode is active (read-only).
    pub fn is_offline(&self) -> bool {
        self.state == ConnectivityState::Offline
    }

    /// Ensure the client is online; return error if offline.
    pub fn require_online(&self) -> Result<(), OfflineError> {
        if self.is_offline() {
            Err(OfflineError::Offline)
        } else {
            Ok(())
        }
    }
}

