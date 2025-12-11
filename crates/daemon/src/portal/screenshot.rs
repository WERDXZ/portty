use std::sync::{Arc, RwLock};

use tracing::{info, instrument};

use crate::config::Config;
use crate::daemon_socket::DaemonState;
use crate::dbus::screenshot::{
    PickColorOptions, PickColorResult, ScreenshotError, ScreenshotHandler, ScreenshotOptions,
    ScreenshotResult,
};

pub use libportty::portal::screenshot::{ScreenshotMode, SessionOptions, parse_color};

/// Screenshot handler that spawns terminals
pub struct TtyScreenshot {
    config: Arc<Config>,
    state: Arc<RwLock<DaemonState>>,
}

impl TtyScreenshot {
    pub fn new(config: Arc<Config>, state: Arc<RwLock<DaemonState>>) -> Self {
        Self { config, state }
    }
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

        let session_options = SessionOptions {
            mode: ScreenshotMode::Screenshot { interactive },
            app_id,
            modal: options.modal().unwrap_or(false),
        };

        let options_json = serde_json::to_value(&session_options)
            .map_err(|e| ScreenshotError::Other(format!("failed to serialize options: {e}")))?;

        let entries = super::run_session(
            "screenshot",
            "screenshot",
            &options_json,
            &[],
            None,
            &self.config,
            &self.state,
        )
        .await?;

        let uri = entries
            .into_iter()
            .next()
            .ok_or_else(|| ScreenshotError::Other("no URI returned from session".to_string()))?;

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

        let session_options = SessionOptions {
            mode: ScreenshotMode::PickColor,
            app_id,
            modal: false,
        };

        let options_json = serde_json::to_value(&session_options)
            .map_err(|e| ScreenshotError::Other(format!("failed to serialize options: {e}")))?;

        let entries = super::run_session(
            "screenshot",
            "pick-color",
            &options_json,
            &[],
            None,
            &self.config,
            &self.state,
        )
        .await?;

        // validate already stripped file:// and verified the color format
        let color_str = entries
            .into_iter()
            .next()
            .ok_or_else(|| ScreenshotError::Other("no color returned from session".to_string()))?;

        let color = parse_color(&color_str).ok_or_else(|| {
            ScreenshotError::Other(format!("invalid color format: '{}'", color_str))
        })?;

        Ok(PickColorResult::new(color))
    }
}
