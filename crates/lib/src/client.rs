use std::io::BufReader;
use std::os::unix::net::UnixStream;
use std::path::PathBuf;

use crate::codec::{self, IpcError};
use crate::protocol::{Request, Response, SessionInfo};

/// Errors from the daemon client
#[derive(Debug, thiserror::Error)]
pub enum ClientError {
    #[error("connection failed: {0}")]
    Connection(std::io::Error),
    #[error("IPC error: {0}")]
    Codec(#[from] IpcError),
    #[error("{0}")]
    Server(String),
    #[error("unexpected response from daemon")]
    UnexpectedResponse,
}

/// Client for communicating with the daemon control socket
pub struct DaemonClient {
    socket_path: PathBuf,
}

impl DaemonClient {
    /// Create a client using the default socket path
    pub fn new() -> Self {
        Self {
            socket_path: crate::paths::daemon_socket_path(),
        }
    }

    /// Submit a session or pending entries
    pub fn submit(&self, session_id: Option<&str>) -> Result<(), ClientError> {
        let req = Request::Submit {
            session_id: session_id.map(String::from),
        };
        match self.send(&req)? {
            Response::Ok => Ok(()),
            Response::Error(e) => Err(ClientError::Server(e)),
            _ => Err(ClientError::UnexpectedResponse),
        }
    }

    /// Cancel a session or clear pending entries
    pub fn cancel(&self, session_id: Option<&str>) -> Result<(), ClientError> {
        let req = Request::Cancel {
            session_id: session_id.map(String::from),
        };
        match self.send(&req)? {
            Response::Ok => Ok(()),
            Response::Error(e) => Err(ClientError::Server(e)),
            _ => Err(ClientError::UnexpectedResponse),
        }
    }

    /// Validate submission against portal constraints
    pub fn verify(&self, session_id: Option<&str>) -> Result<(), ClientError> {
        let req = Request::Verify {
            session_id: session_id.map(String::from),
        };
        match self.send(&req)? {
            Response::Ok => Ok(()),
            Response::Error(e) => Err(ClientError::Server(e)),
            _ => Err(ClientError::UnexpectedResponse),
        }
    }

    /// Reset submission to initial state
    pub fn reset(&self, session_id: Option<&str>) -> Result<(), ClientError> {
        let req = Request::Reset {
            session_id: session_id.map(String::from),
        };
        match self.send(&req)? {
            Response::Ok => Ok(()),
            Response::Error(e) => Err(ClientError::Server(e)),
            _ => Err(ClientError::UnexpectedResponse),
        }
    }

    /// List all active sessions
    pub fn list(&self) -> Result<Vec<SessionInfo>, ClientError> {
        match self.send(&Request::List)? {
            Response::Sessions(sessions) => Ok(sessions),
            // Empty session list encodes as "ok\n", which decodes to Response::Ok
            Response::Ok => Ok(Vec::new()),
            Response::Error(e) => Err(ClientError::Server(e)),
        }
    }

    /// Send a raw request and return the raw response
    pub fn send(&self, req: &Request) -> Result<Response, ClientError> {
        let stream = UnixStream::connect(&self.socket_path).map_err(|e| {
            if e.kind() == std::io::ErrorKind::ConnectionRefused
                || e.kind() == std::io::ErrorKind::NotFound
            {
                ClientError::Connection(std::io::Error::new(
                    e.kind(),
                    format!(
                        "cannot connect to daemon socket ({}): is porttyd running?",
                        self.socket_path.display()
                    ),
                ))
            } else {
                ClientError::Connection(e)
            }
        })?;
        let mut writer = &stream;
        let mut reader = BufReader::new(&stream);
        codec::write_request(&mut writer, req)?;
        let resp = codec::read_response(&mut reader)?;
        Ok(resp)
    }
}

impl Default for DaemonClient {
    fn default() -> Self {
        Self::new()
    }
}
