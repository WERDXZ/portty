pub mod ipc;
pub mod portal;
pub mod portal_type;
pub mod protocol;
pub mod queue;
pub mod request;
pub mod session;

pub use portal_type::PortalType;
pub use protocol::{
    DaemonExtension, DaemonRequest, DaemonResponse, DaemonResponseExtension, NoExtension,
    QueueStatusInfo, Request, Response, SessionInfo, SessionRequest, SessionResponse,
};

// Re-export bincode traits for downstream crates
pub use bincode::{Decode, Encode};

#[cfg(test)]
mod tests {
    use super::*;
    use bincode::config::standard;

    #[test]
    fn test_encoding_format() {
        let cfg = standard();
        
        // Base command - same for both types
        let session_clear: SessionRequest = Request::Clear;
        let daemon_clear: DaemonRequest = Request::Clear;
        
        let session_bytes = bincode::encode_to_vec(&session_clear, cfg).unwrap();
        let daemon_bytes = bincode::encode_to_vec(&daemon_clear, cfg).unwrap();
        
        println!("SessionRequest::Clear: {:?}", session_bytes);
        println!("DaemonRequest::Clear:  {:?}", daemon_bytes);
        println!("Wire compatible: {}", session_bytes == daemon_bytes);
        
        // Extended command (daemon only)
        let list: DaemonRequest = Request::Extended(DaemonExtension::ListSessions);
        let get: DaemonRequest = Request::Extended(DaemonExtension::GetSession("abc".into()));
        
        let list_bytes = bincode::encode_to_vec(&list, cfg).unwrap();
        let get_bytes = bincode::encode_to_vec(&get, cfg).unwrap();
        
        println!("\nDaemonRequest::Extended(ListSessions): {:?}", list_bytes);
        println!("DaemonRequest::Extended(GetSession): {:?}", get_bytes);
        
        // Show structure
        println!("\nEncoding breakdown for Extended(ListSessions):");
        println!("  Outer variant tag (Extended=7): {}", list_bytes[0]);
        println!("  Inner variant tag (ListSessions=0): {}", list_bytes[1]);
        
        assert_eq!(session_bytes, daemon_bytes, "Base variants should be wire-compatible");
    }
}
