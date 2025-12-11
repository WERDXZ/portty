//! IPC protocol for daemon control socket
//!
//! Flat protocol for CLI -> Daemon communication.
//! Data operations (edit, clear) are file-based.
//! Control commands (submit, cancel, verify, reset) and management queries (list)
//! go through the daemon socket.
//!
//! # Wire Format
//!
//! Messages are plain text lines terminated by `\n`.
//!
//! ## Request (single line)
//! ```text
//! submit [session_id]
//! cancel [session_id]
//! verify [session_id]
//! reset [session_id]
//! list
//! ```
//!
//! ## Response (one or more lines, terminated by `ok` or `error: ...`)
//! ```text
//! ok
//! error: <message>
//! <id>\t<portal>\t<operation>\t<created>\t<dir>\t<title>\n ... ok
//! ```

/// Request sent to the daemon socket
#[derive(Debug, Clone, PartialEq)]
pub enum Request {
    /// Submit/confirm a session or pending entries
    Submit { session_id: Option<String> },

    /// Cancel a session or clear pending entries
    Cancel { session_id: Option<String> },

    /// Validate submission against portal constraints
    Verify { session_id: Option<String> },

    /// Reset submission to initial state
    Reset { session_id: Option<String> },

    /// List all active sessions
    List,
}

/// Response from the daemon socket
#[derive(Debug, Clone, PartialEq)]
pub enum Response {
    /// Operation completed successfully
    Ok,

    /// Error occurred
    Error(String),

    /// List of active sessions
    Sessions(Vec<SessionInfo>),
}

/// Information about a session
#[derive(Debug, Clone, PartialEq)]
pub struct SessionInfo {
    /// Unique session identifier
    pub id: String,
    /// Portal name (e.g. "file-chooser", "screenshot")
    pub portal: String,
    /// Operation name (e.g. "open-file", "screenshot")
    pub operation: String,
    /// Session title (from portal options)
    pub title: Option<String>,
    /// Unix timestamp when session was created
    pub created: u64,
    /// Path to session directory
    pub dir: String,
}

impl Request {
    /// Encode request as a single newline-terminated line
    pub fn encode(&self) -> String {
        match self {
            Request::Submit { session_id: None } => "submit\n".to_string(),
            Request::Submit {
                session_id: Some(id),
            } => format!("submit {id}\n"),
            Request::Cancel { session_id: None } => "cancel\n".to_string(),
            Request::Cancel {
                session_id: Some(id),
            } => format!("cancel {id}\n"),
            Request::Verify { session_id: None } => "verify\n".to_string(),
            Request::Verify {
                session_id: Some(id),
            } => format!("verify {id}\n"),
            Request::Reset { session_id: None } => "reset\n".to_string(),
            Request::Reset {
                session_id: Some(id),
            } => format!("reset {id}\n"),
            Request::List => "list\n".to_string(),
        }
    }

    /// Decode request from a trimmed line
    pub fn decode(line: &str) -> Result<Self, String> {
        let line = line.trim();
        let (cmd, arg) = match line.split_once(' ') {
            Some((cmd, arg)) => (cmd, Some(arg)),
            None => (line, None),
        };

        match cmd {
            "submit" => Ok(Request::Submit {
                session_id: arg.map(String::from),
            }),
            "cancel" => Ok(Request::Cancel {
                session_id: arg.map(String::from),
            }),
            "verify" => Ok(Request::Verify {
                session_id: arg.map(String::from),
            }),
            "reset" => Ok(Request::Reset {
                session_id: arg.map(String::from),
            }),
            "list" => Ok(Request::List),
            _ => Err(format!("unknown command: {cmd}")),
        }
    }
}

/// Sanitize a field for the tab-separated text protocol.
/// Replaces tabs and newlines with spaces to prevent protocol injection.
fn sanitize_field(s: &str) -> String {
    s.replace(['\t', '\n', '\r'], " ")
}

impl Response {
    /// Encode response as one or more lines
    pub fn encode(&self) -> String {
        match self {
            Response::Ok => "ok\n".to_string(),
            Response::Error(msg) => format!("error: {}\n", sanitize_field(msg)),
            Response::Sessions(sessions) => {
                let mut out = String::new();
                for s in sessions {
                    let title = s.title.as_deref().unwrap_or("");
                    out.push_str(&format!(
                        "{}\t{}\t{}\t{}\t{}\t{}\n",
                        sanitize_field(&s.id),
                        sanitize_field(&s.portal),
                        sanitize_field(&s.operation),
                        s.created,
                        sanitize_field(&s.dir),
                        sanitize_field(title),
                    ));
                }
                out.push_str("ok\n");
                out
            }
        }
    }
}

impl SessionInfo {
    /// Parse a tab-separated session info line
    pub fn decode_line(line: &str) -> Result<Self, String> {
        let parts: Vec<&str> = line.split('\t').collect();
        if parts.len() < 5 {
            return Err(format!(
                "expected at least 5 tab-separated fields, got {}",
                parts.len()
            ));
        }

        let created: u64 = parts[3]
            .parse()
            .map_err(|e| format!("invalid created timestamp: {e}"))?;

        let title = parts.get(5).and_then(|t| {
            if t.is_empty() {
                None
            } else {
                Some(t.to_string())
            }
        });

        Ok(SessionInfo {
            id: parts[0].to_string(),
            portal: parts[1].to_string(),
            operation: parts[2].to_string(),
            created,
            dir: parts[4].to_string(),
            title,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn request_encode_decode_roundtrip() {
        let cases = vec![
            Request::Submit { session_id: None },
            Request::Submit {
                session_id: Some("abc".into()),
            },
            Request::Cancel { session_id: None },
            Request::Cancel {
                session_id: Some("xyz".into()),
            },
            Request::Verify { session_id: None },
            Request::Verify {
                session_id: Some("s1".into()),
            },
            Request::Reset { session_id: None },
            Request::Reset {
                session_id: Some("s2".into()),
            },
            Request::List,
        ];

        for req in cases {
            let encoded = req.encode();
            assert!(encoded.ends_with('\n'));
            let decoded = Request::decode(&encoded).unwrap();
            assert_eq!(decoded, req);
        }
    }

    #[test]
    fn response_ok_encode() {
        assert_eq!(Response::Ok.encode(), "ok\n");
    }

    #[test]
    fn response_error_encode() {
        assert_eq!(
            Response::Error("bad thing".into()).encode(),
            "error: bad thing\n"
        );
    }

    #[test]
    fn response_sessions_roundtrip() {
        let info = SessionInfo {
            id: "sess-1".into(),
            portal: "file-chooser".into(),
            operation: "open-file".into(),
            title: Some("Pick a file".into()),
            created: 1234567890,
            dir: "/tmp/test".into(),
        };
        let resp = Response::Sessions(vec![info.clone()]);
        let encoded = resp.encode();

        // Should have session line + ok
        let lines: Vec<&str> = encoded.lines().collect();
        assert_eq!(lines.len(), 2);
        assert_eq!(lines[1], "ok");

        let decoded = SessionInfo::decode_line(lines[0]).unwrap();
        assert_eq!(decoded, info);
    }

    #[test]
    fn session_info_no_title() {
        let info = SessionInfo {
            id: "s1".into(),
            portal: "screenshot".into(),
            operation: "screenshot".into(),
            title: None,
            created: 999,
            dir: "/tmp/x".into(),
        };
        let resp = Response::Sessions(vec![info.clone()]);
        let encoded = resp.encode();
        let lines: Vec<&str> = encoded.lines().collect();

        let decoded = SessionInfo::decode_line(lines[0]).unwrap();
        assert_eq!(decoded, info);
    }

    #[test]
    fn decode_unknown_command() {
        assert!(Request::decode("foobar").is_err());
    }

    #[test]
    fn decode_session_info_too_few_fields() {
        assert!(SessionInfo::decode_line("a\tb\tc").is_err());
    }

    #[test]
    fn sanitize_title_with_tabs_and_newlines() {
        let info = SessionInfo {
            id: "s1".into(),
            portal: "file-chooser".into(),
            operation: "open-file".into(),
            title: Some("evil\ttitle\nhere".into()),
            created: 100,
            dir: "/tmp/x".into(),
        };
        let resp = Response::Sessions(vec![info]);
        let encoded = resp.encode();
        // Should produce exactly 2 lines: one session + ok
        let lines: Vec<&str> = encoded.lines().collect();
        assert_eq!(lines.len(), 2);
        assert_eq!(lines[1], "ok");
        // Tabs/newlines in title should be replaced with spaces
        let decoded = SessionInfo::decode_line(lines[0]).unwrap();
        assert_eq!(decoded.title.as_deref(), Some("evil title here"));
    }

    #[test]
    fn sanitize_error_with_newline() {
        let resp = Response::Error("line1\nline2".into());
        let encoded = resp.encode();
        // Should be a single line
        assert_eq!(encoded.lines().count(), 1);
        assert_eq!(encoded, "error: line1 line2\n");
    }
}
