//! Generic session protocol wrapper
//!
//! All portal sessions share Submit and Cancel commands.
//! Portal-specific commands are wrapped in the Portal variant.

use serde::{Deserialize, Serialize};

/// Generic session request with shared commands
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Request<P> {
    /// Submit/confirm the selection and close the session
    Submit,
    /// Cancel the operation and close the session
    Cancel,
    /// Portal-specific command
    Portal(P),
}

/// Generic session response
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Response<R> {
    /// Operation succeeded
    Ok,
    /// Error occurred
    Error(String),
    /// Portal-specific response
    Portal(R),
}
