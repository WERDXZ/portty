pub mod file_chooser;

use bincode::{Decode, Encode};
use std::io::{Read, Write};

/// Bincode configuration
const CONFIG: bincode::config::Configuration = bincode::config::standard();

/// Read a bincode message from a reader
pub fn read_message<T: Decode<()>>(reader: &mut impl Read) -> Result<T, IpcError> {
    // Read length prefix (u32)
    let mut len_buf = [0u8; 4];
    reader.read_exact(&mut len_buf)?;
    let len = u32::from_le_bytes(len_buf) as usize;

    // Read payload
    let mut buf = vec![0u8; len];
    reader.read_exact(&mut buf)?;

    let (msg, _) = bincode::decode_from_slice(&buf, CONFIG).map_err(IpcError::Decode)?;
    Ok(msg)
}

/// Write a bincode message to a writer
pub fn write_message<T: Encode>(writer: &mut impl Write, msg: &T) -> Result<(), IpcError> {
    let buf = bincode::encode_to_vec(msg, CONFIG).map_err(IpcError::Encode)?;
    let len = buf.len() as u32;

    writer.write_all(&len.to_le_bytes())?;
    writer.write_all(&buf)?;
    writer.flush()?;

    Ok(())
}

#[derive(Debug)]
pub enum IpcError {
    Io(std::io::Error),
    Encode(bincode::error::EncodeError),
    Decode(bincode::error::DecodeError),
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
