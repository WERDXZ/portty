use std::sync::{Arc, RwLock};
use std::thread;

use futures_lite::future::yield_now;
use tracing::{debug, info, instrument};

use crate::config::{Config, ScreenshotOp};
use crate::daemon_socket::{DaemonState, RegisteredSession};
use crate::session::{Session, SessionResult};
use portty_ipc::ipc::context::PortalContext;
use portty_ipc::ipc::screenshot::{ScreenshotMode, ScreenshotSessionOptions};
use portty_ipc::portal::screenshot::{
    PickColorOptions, PickColorResult, ScreenshotError, ScreenshotHandler, ScreenshotOptions,
    ScreenshotResult,
};
use portty_ipc::PortalType;

/// Screenshot handler that spawns terminals
pub struct TtyScreenshot {
    config: Arc<Config>,
    state: Arc<RwLock<DaemonState>>,
}

impl TtyScreenshot {
    pub fn new(config: Arc<Config>, state: Arc<RwLock<DaemonState>>) -> Self {
        debug!("Screenshot initialized");
        Self { config, state }
    }

    async fn run_session(
        &self,
        op: ScreenshotOp,
        session_options: ScreenshotSessionOptions,
    ) -> Result<Vec<String>, ScreenshotError> {
        let portal = PortalType::Screenshot;
        let context = PortalContext::Screenshot(session_options);

        // Get operation-specific config
        let exec = self.config.screenshot_exec(op).map(String::from);
        let bin = self.config.screenshot_bin(op);

        let headless = exec.is_none();
        if headless {
            info!(
                ?op,
                "Starting headless screenshot session (use `portty` CLI to interact)"
            );
        } else {
            debug!(?exec, ?op, "Creating screenshot session");
        }

        let mut session = Session::new(portal.as_str(), context, &bin)
            .map_err(|e| ScreenshotError::Other(format!("failed to create session: {e}")))?;

        // Register session
        let session_id = session.id().to_string();
        {
            let mut st = self.state.write().unwrap();
            st.sessions.register(RegisteredSession {
                id: session_id.clone(),
                portal,
                title: session.title().map(String::from),
                created: session.created(),
                socket_path: session.socket_path().to_path_buf(),
            });

            // Transfer pending commands to session
            if !st.queue.pending.is_empty() {
                let pending = std::mem::take(&mut st.queue.pending);
                info!(
                    commands = pending.len(),
                    "Transferring pending commands to session"
                );
                session.apply_pending(pending);
            }
        }

        // Spawn process
        if let Some(ref exec) = exec {
            session
                .spawn(exec, &format!("{} - {}", portal.as_str(), op.as_str()))
                .map_err(|e| ScreenshotError::Other(format!("failed to spawn: {e}")))?;
        }

        // Run session in background thread
        let handle = thread::spawn(move || session.run());

        // Poll until thread completes
        loop {
            if handle.is_finished() {
                break;
            }
            yield_now().await;
        }

        let result = handle
            .join()
            .map_err(|_| ScreenshotError::Other("session thread panicked".to_string()))?
            .map_err(|e| ScreenshotError::Other(format!("session failed: {e}")))?;

        // Unregister session
        {
            let mut st = self.state.write().unwrap();
            st.sessions.unregister(&session_id);
        }

        match result {
            SessionResult::Success { uris } => {
                info!(?uris, "Screenshot session completed successfully");
                Ok(uris)
            }
            SessionResult::Cancelled => {
                info!("Screenshot session cancelled");
                Err(ScreenshotError::Cancelled)
            }
        }
    }
}

/// Parse a color string into (r, g, b) floats in [0.0, 1.0]
///
/// Supports:
/// - Hex: `#rrggbb` or `#RRGGBB`
/// - Space-separated floats: `0.5 0.3 0.8`
/// - CSS-like: `rgb(r, g, b)` where r,g,b are 0-255 integers
fn parse_color(s: &str) -> Option<(f64, f64, f64)> {
    let s = s.trim();

    // Try hex #rrggbb
    if let Some(hex) = s.strip_prefix('#')
        && hex.len() == 6
    {
        let r = u8::from_str_radix(&hex[0..2], 16).ok()?;
        let g = u8::from_str_radix(&hex[2..4], 16).ok()?;
        let b = u8::from_str_radix(&hex[4..6], 16).ok()?;
        return Some((r as f64 / 255.0, g as f64 / 255.0, b as f64 / 255.0));
    }

    // Try rgb(r, g, b)
    if let Some(inner) = s.strip_prefix("rgb(").and_then(|s| s.strip_suffix(')')) {
        let parts: Vec<&str> = inner.split(',').map(|p| p.trim()).collect();
        if parts.len() == 3 {
            let r: u8 = parts[0].parse().ok()?;
            let g: u8 = parts[1].parse().ok()?;
            let b: u8 = parts[2].parse().ok()?;
            return Some((r as f64 / 255.0, g as f64 / 255.0, b as f64 / 255.0));
        }
    }

    // Try space-separated floats
    let parts: Vec<&str> = s.split_whitespace().collect();
    if parts.len() == 3 {
        let r: f64 = parts[0].parse().ok()?;
        let g: f64 = parts[1].parse().ok()?;
        let b: f64 = parts[2].parse().ok()?;
        return Some((r, g, b));
    }

    None
}

impl ScreenshotHandler for TtyScreenshot {
    #[instrument(skip(self, _parent_window, options))]
    async fn screenshot(
        &self,
        _handle: String,
        app_id: String,
        _parent_window: String,
        options: ScreenshotOptions,
    ) -> Result<ScreenshotResult, ScreenshotError> {
        let interactive = options.interactive().unwrap_or(false);
        info!(interactive, "Screenshot request");

        let session_options = ScreenshotSessionOptions {
            mode: ScreenshotMode::Screenshot { interactive },
            app_id,
            modal: options.modal().unwrap_or(false),
        };

        let uris = self
            .run_session(ScreenshotOp::Screenshot, session_options)
            .await?;

        let uri = uris.into_iter().next().ok_or_else(|| {
            ScreenshotError::Other("no URI returned from session".to_string())
        })?;

        Ok(ScreenshotResult::new(uri))
    }

    #[instrument(skip(self, _parent_window, _options))]
    async fn pick_color(
        &self,
        _handle: String,
        app_id: String,
        _parent_window: String,
        _options: PickColorOptions,
    ) -> Result<PickColorResult, ScreenshotError> {
        info!("PickColor request");

        let session_options = ScreenshotSessionOptions {
            mode: ScreenshotMode::PickColor,
            app_id,
            modal: false,
        };

        let uris = self
            .run_session(ScreenshotOp::PickColor, session_options)
            .await?;

        let color_str = uris.into_iter().next().ok_or_else(|| {
            ScreenshotError::Other("no color returned from session".to_string())
        })?;

        // Strip file:// prefix if present (user might submit a color value via select)
        let color_str = color_str.strip_prefix("file://").unwrap_or(&color_str);

        let color = parse_color(color_str).ok_or_else(|| {
            ScreenshotError::Other(format!(
                "invalid color format: '{}' (expected #rrggbb, 'R G B' floats, or rgb(r,g,b))",
                color_str
            ))
        })?;

        Ok(PickColorResult::new(color))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_hex_color() {
        let (r, g, b) = parse_color("#ff8000").unwrap();
        assert!((r - 1.0).abs() < 0.01);
        assert!((g - 0.502).abs() < 0.01);
        assert!((b - 0.0).abs() < 0.01);
    }

    #[test]
    fn parse_hex_color_uppercase() {
        let (r, g, b) = parse_color("#FF0000").unwrap();
        assert!((r - 1.0).abs() < 0.01);
        assert!((g - 0.0).abs() < 0.01);
        assert!((b - 0.0).abs() < 0.01);
    }

    #[test]
    fn parse_hex_black() {
        let (r, g, b) = parse_color("#000000").unwrap();
        assert!((r - 0.0).abs() < 0.01);
        assert!((g - 0.0).abs() < 0.01);
        assert!((b - 0.0).abs() < 0.01);
    }

    #[test]
    fn parse_hex_white() {
        let (r, g, b) = parse_color("#ffffff").unwrap();
        assert!((r - 1.0).abs() < 0.01);
        assert!((g - 1.0).abs() < 0.01);
        assert!((b - 1.0).abs() < 0.01);
    }

    #[test]
    fn parse_rgb_format() {
        let (r, g, b) = parse_color("rgb(255, 128, 0)").unwrap();
        assert!((r - 1.0).abs() < 0.01);
        assert!((g - 0.502).abs() < 0.01);
        assert!((b - 0.0).abs() < 0.01);
    }

    #[test]
    fn parse_space_separated_floats() {
        let (r, g, b) = parse_color("0.5 0.3 0.8").unwrap();
        assert!((r - 0.5).abs() < 0.01);
        assert!((g - 0.3).abs() < 0.01);
        assert!((b - 0.8).abs() < 0.01);
    }

    #[test]
    fn parse_with_whitespace() {
        let (r, g, b) = parse_color("  #ff0000  ").unwrap();
        assert!((r - 1.0).abs() < 0.01);
        assert!((g - 0.0).abs() < 0.01);
        assert!((b - 0.0).abs() < 0.01);
    }

    #[test]
    fn parse_invalid() {
        assert!(parse_color("not a color").is_none());
        assert!(parse_color("#gg0000").is_none());
        assert!(parse_color("#fff").is_none());
        assert!(parse_color("").is_none());
    }
}
