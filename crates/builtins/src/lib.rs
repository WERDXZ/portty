use std::env;
use std::os::unix::net::UnixStream;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};
use portty_ipc::ipc::{read_message, write_message, IpcError};

pub mod file_chooser;

/// Get socket path from PORTTY_SOCK environment variable
pub fn socket_path() -> Result<PathBuf, &'static str> {
    env::var("PORTTY_SOCK")
        .map(PathBuf::from)
        .map_err(|_| "PORTTY_SOCK not set - are you running inside a portty session?")
}

/// Connect to the daemon socket and send a request
pub fn send_request<Req, Resp>(req: &Req) -> Result<Resp, IpcError>
where
    Req: Serialize,
    Resp: for<'de> Deserialize<'de>,
{
    let sock_path = socket_path().map_err(|e| {
        IpcError::Io(std::io::Error::new(std::io::ErrorKind::NotFound, e))
    })?;

    let mut stream = UnixStream::connect(&sock_path)?;
    write_message(&mut stream, req)?;
    read_message(&mut stream)
}

/// Convert a path or URI to a file:// URI
pub fn to_uri(arg: &str) -> String {
    if arg.contains("://") {
        arg.to_string()
    } else {
        let path = if arg.starts_with('/') {
            PathBuf::from(arg)
        } else {
            env::current_dir().unwrap_or_default().join(arg)
        };
        format!("file://{}", path.display())
    }
}
