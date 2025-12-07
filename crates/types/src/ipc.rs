pub mod file_chooser;

use serde::{Deserialize, Serialize};
use std::io::{Read, Write};

/// Read a bincode message from a reader
pub fn read_message<T: for<'de> Deserialize<'de>>(reader: &mut impl Read) -> Result<T, IpcError> {
    // Read length prefix (u32)
    let mut len_buf = [0u8; 4];
    reader.read_exact(&mut len_buf)?;
    let len = u32::from_le_bytes(len_buf) as usize;

    // Read payload
    let mut buf = vec![0u8; len];
    reader.read_exact(&mut buf)?;

    bincode::deserialize(&buf).map_err(IpcError::Decode)
}

/// Write a bincode message to a writer
pub fn write_message<T: Serialize>(writer: &mut impl Write, msg: &T) -> Result<(), IpcError> {
    let buf = bincode::serialize(msg).map_err(IpcError::Encode)?;
    let len = buf.len() as u32;

    writer.write_all(&len.to_le_bytes())?;
    writer.write_all(&buf)?;
    writer.flush()?;

    Ok(())
}

#[derive(Debug)]
pub enum IpcError {
    Io(std::io::Error),
    Encode(bincode::Error),
    Decode(bincode::Error),
}

impl From<std::io::Error> for IpcError {
    fn from(e: std::io::Error) -> Self {
        IpcError::Io(e)
    }
}

impl std::fmt::Display for IpcError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            IpcError::Io(e) => write!(f, "IO error: {e}"),
            IpcError::Encode(e) => write!(f, "encode error: {e}"),
            IpcError::Decode(e) => write!(f, "decode error: {e}"),
        }
    }
}

impl std::error::Error for IpcError {}
