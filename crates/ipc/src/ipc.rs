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

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    #[derive(Debug, Clone, PartialEq, Encode, Decode)]
    struct TestMessage {
        id: u32,
        name: String,
        values: Vec<i64>,
    }

    #[test]
    fn test_write_read_roundtrip() {
        let original = TestMessage {
            id: 42,
            name: "hello".into(),
            values: vec![1, 2, 3],
        };

        let mut buf = Vec::new();
        write_message(&mut buf, &original).unwrap();

        let mut cursor = Cursor::new(buf);
        let decoded: TestMessage = read_message(&mut cursor).unwrap();

        assert_eq!(decoded, original);
    }

    #[test]
    fn test_message_format() {
        let msg = TestMessage {
            id: 1,
            name: "x".into(),
            values: vec![],
        };

        let mut buf = Vec::new();
        write_message(&mut buf, &msg).unwrap();

        // First 4 bytes should be length prefix (little-endian u32)
        let len = u32::from_le_bytes([buf[0], buf[1], buf[2], buf[3]]) as usize;
        assert_eq!(len, buf.len() - 4, "Length prefix should match payload size");
    }

    #[test]
    fn test_multiple_messages() {
        let msg1 = TestMessage { id: 1, name: "a".into(), values: vec![] };
        let msg2 = TestMessage { id: 2, name: "b".into(), values: vec![10, 20] };

        let mut buf = Vec::new();
        write_message(&mut buf, &msg1).unwrap();
        write_message(&mut buf, &msg2).unwrap();

        let mut cursor = Cursor::new(buf);
        let decoded1: TestMessage = read_message(&mut cursor).unwrap();
        let decoded2: TestMessage = read_message(&mut cursor).unwrap();

        assert_eq!(decoded1, msg1);
        assert_eq!(decoded2, msg2);
    }

    #[test]
    fn test_empty_string() {
        let msg = TestMessage { id: 0, name: String::new(), values: vec![] };

        let mut buf = Vec::new();
        write_message(&mut buf, &msg).unwrap();

        let mut cursor = Cursor::new(buf);
        let decoded: TestMessage = read_message(&mut cursor).unwrap();

        assert_eq!(decoded.name, "");
    }

    #[test]
    fn test_large_values() {
        let msg = TestMessage {
            id: u32::MAX,
            name: "test".repeat(1000),
            values: (0..100).collect(),
        };

        let mut buf = Vec::new();
        write_message(&mut buf, &msg).unwrap();

        let mut cursor = Cursor::new(buf);
        let decoded: TestMessage = read_message(&mut cursor).unwrap();

        assert_eq!(decoded, msg);
    }

    #[test]
    fn test_read_incomplete_length() {
        // Only 2 bytes when we need 4
        let buf = vec![0u8, 1u8];
        let mut cursor = Cursor::new(buf);
        let result: Result<TestMessage, _> = read_message(&mut cursor);

        assert!(result.is_err());
    }

    #[test]
    fn test_read_incomplete_payload() {
        // Length says 100 but only 2 bytes of payload
        let mut buf = vec![100u8, 0, 0, 0]; // length = 100
        buf.extend_from_slice(&[1, 2]); // only 2 bytes

        let mut cursor = Cursor::new(buf);
        let result: Result<TestMessage, _> = read_message(&mut cursor);

        assert!(result.is_err());
    }

    #[test]
    fn test_ipc_error_display() {
        let io_err = IpcError::Io(std::io::Error::new(std::io::ErrorKind::NotFound, "test"));
        assert!(io_err.to_string().contains("IO error"));
    }
}
