use std::io::{BufRead, Write};
use thiserror::Error;

use crate::protocol::{Request, Response, SessionInfo};

/// Write a request to a writer
pub fn write_request(writer: &mut impl Write, req: &Request) -> Result<(), IpcError> {
    writer.write_all(req.encode().as_bytes())?;
    writer.flush()?;
    Ok(())
}

/// Read a request from a buffered reader
pub fn read_request(reader: &mut impl BufRead) -> Result<Request, IpcError> {
    let mut line = String::new();
    let n = reader.read_line(&mut line)?;
    if n == 0 {
        return Err(IpcError::Io(std::io::Error::new(
            std::io::ErrorKind::UnexpectedEof,
            "connection closed",
        )));
    }
    Request::decode(&line).map_err(IpcError::Protocol)
}

/// Write a response to a writer
pub fn write_response(writer: &mut impl Write, resp: &Response) -> Result<(), IpcError> {
    writer.write_all(resp.encode().as_bytes())?;
    writer.flush()?;
    Ok(())
}

/// Read a response from a buffered reader
pub fn read_response(reader: &mut impl BufRead) -> Result<Response, IpcError> {
    let mut sessions = Vec::new();

    loop {
        let mut line = String::new();
        let n = reader.read_line(&mut line)?;
        if n == 0 {
            return Err(IpcError::Io(std::io::Error::new(
                std::io::ErrorKind::UnexpectedEof,
                "connection closed while reading response",
            )));
        }

        let trimmed = line.trim_end_matches('\n').trim_end_matches('\r');

        if trimmed == "ok" {
            return if sessions.is_empty() {
                Ok(Response::Ok)
            } else {
                Ok(Response::Sessions(sessions))
            };
        }

        if let Some(msg) = trimmed.strip_prefix("error: ") {
            return Ok(Response::Error(msg.to_string()));
        }

        // Must be a session info line
        match SessionInfo::decode_line(trimmed) {
            Ok(info) => sessions.push(info),
            Err(e) => return Err(IpcError::Protocol(e)),
        }
    }
}

/// IPC communication errors
#[derive(Debug, Error)]
pub enum IpcError {
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
    #[error("protocol error: {0}")]
    Protocol(String),
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{BufReader, Cursor};

    #[test]
    fn request_roundtrip() {
        let cases = vec![
            Request::Submit { session_id: None },
            Request::Submit {
                session_id: Some("abc".into()),
            },
            Request::List,
        ];

        for req in cases {
            let mut buf = Vec::new();
            write_request(&mut buf, &req).unwrap();

            let mut reader = BufReader::new(Cursor::new(buf));
            let decoded = read_request(&mut reader).unwrap();
            assert_eq!(decoded, req);
        }
    }

    #[test]
    fn response_ok_roundtrip() {
        let mut buf = Vec::new();
        write_response(&mut buf, &Response::Ok).unwrap();

        let mut reader = BufReader::new(Cursor::new(buf));
        let decoded = read_response(&mut reader).unwrap();
        assert_eq!(decoded, Response::Ok);
    }

    #[test]
    fn response_error_roundtrip() {
        let resp = Response::Error("something went wrong".into());
        let mut buf = Vec::new();
        write_response(&mut buf, &resp).unwrap();

        let mut reader = BufReader::new(Cursor::new(buf));
        let decoded = read_response(&mut reader).unwrap();
        assert_eq!(decoded, resp);
    }

    #[test]
    fn response_sessions_roundtrip() {
        let resp = Response::Sessions(vec![
            SessionInfo {
                id: "s1".into(),
                portal: "file-chooser".into(),
                operation: "open-file".into(),
                title: Some("Pick".into()),
                created: 12345,
                dir: "/tmp/a".into(),
            },
            SessionInfo {
                id: "s2".into(),
                portal: "screenshot".into(),
                operation: "screenshot".into(),
                title: None,
                created: 67890,
                dir: "/tmp/b".into(),
            },
        ]);

        let mut buf = Vec::new();
        write_response(&mut buf, &resp).unwrap();

        let mut reader = BufReader::new(Cursor::new(buf));
        let decoded = read_response(&mut reader).unwrap();
        assert_eq!(decoded, resp);
    }

    #[test]
    fn eof_returns_error() {
        let mut reader = BufReader::new(Cursor::new(Vec::<u8>::new()));
        assert!(read_request(&mut reader).is_err());
        assert!(read_response(&mut reader).is_err());
    }

    #[test]
    fn sequential_messages() {
        let req1 = Request::Submit { session_id: None };
        let req2 = Request::List;

        let mut buf = Vec::new();
        write_request(&mut buf, &req1).unwrap();
        write_request(&mut buf, &req2).unwrap();

        let mut reader = BufReader::new(Cursor::new(buf));
        assert_eq!(read_request(&mut reader).unwrap(), req1);
        assert_eq!(read_request(&mut reader).unwrap(), req2);
    }
}
