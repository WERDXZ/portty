pub mod file_chooser;

use bincode::{Decode, Encode};
use std::io::{Read, Write};
use thiserror::Error;

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

/// IPC communication errors
#[derive(Debug, Error)]
pub enum IpcError {
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
    #[error("encode error: {0}")]
    Encode(bincode::error::EncodeError),
    #[error("decode error: {0}")]
    Decode(bincode::error::DecodeError),
}

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
    fn roundtrip_preserves_data() {
        let cases = [
            TestMessage { id: 0, name: String::new(), values: vec![] },
            TestMessage { id: 42, name: "hello".into(), values: vec![1, 2, 3] },
            TestMessage { id: u32::MAX, name: "test".repeat(1000), values: (0..100).collect() },
        ];

        for original in cases {
            let mut buf = Vec::new();
            write_message(&mut buf, &original).unwrap();

            let mut cursor = Cursor::new(buf);
            let decoded: TestMessage = read_message(&mut cursor).unwrap();

            assert_eq!(decoded, original);
        }
    }

    #[test]
    fn wire_format_length_prefix() {
        let msg = TestMessage { id: 1, name: "x".into(), values: vec![] };

        let mut buf = Vec::new();
        write_message(&mut buf, &msg).unwrap();

        let len = u32::from_le_bytes([buf[0], buf[1], buf[2], buf[3]]) as usize;
        assert_eq!(len, buf.len() - 4, "length prefix should match payload size");
    }

    #[test]
    fn sequential_messages_framed_correctly() {
        let msg1 = TestMessage { id: 1, name: "first".into(), values: vec![] };
        let msg2 = TestMessage { id: 2, name: "second".into(), values: vec![10, 20] };

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
    fn truncated_data_returns_error() {
        // Incomplete length header (2 bytes instead of 4)
        let buf = vec![0u8, 1u8];
        let mut cursor = Cursor::new(buf);
        assert!(read_message::<TestMessage>(&mut cursor).is_err());

        // Length says 100 but only 2 bytes of payload
        let mut buf = vec![100u8, 0, 0, 0];
        buf.extend_from_slice(&[1, 2]);
        let mut cursor = Cursor::new(buf);
        assert!(read_message::<TestMessage>(&mut cursor).is_err());
    }
}
