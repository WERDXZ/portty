pub mod ipc;
pub mod portal;
pub mod portal_type;
pub mod protocol;
pub mod queue;
pub mod request;

pub use portal_type::PortalType;
pub use protocol::{
    DaemonExtension, DaemonRequest, DaemonResponse, DaemonResponseExtension, NoExtension,
    QueueStatusInfo, Request, Response, SessionInfo, SessionRequest, SessionResponse,
};

// Re-export bincode traits for downstream crates
pub use bincode::{Decode, Encode};
