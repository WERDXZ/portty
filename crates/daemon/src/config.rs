use std::path::PathBuf;

use serde::Deserialize;

/// Portal-specific configuration
#[derive(Debug, Clone, Default, Deserialize)]
pub struct PortalConfig {
    /// Command to execute for this portal
    /// e.g. "foot -e /bin/sh" or "foot -e fzf-picker"
    pub exec: Option<String>,
}

/// Main configuration
#[derive(Debug, Clone, Deserialize)]
pub struct Config {
    /// Path to the ptt-builtin binary
    #[serde(default = "default_builtin_path")]
    pub builtin_path: String,

    /// Default configuration for all portals
    #[serde(default)]
    pub default: PortalConfig,

    /// File chooser portal config
    #[serde(default, rename = "file-chooser")]
    pub file_chooser: PortalConfig,

    /// Screenshot portal config
    #[serde(default)]
    pub screenshot: PortalConfig,
}

fn default_builtin_path() -> String {
    // Try to find portty-builtin next to the current executable first
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            let sibling = dir.join("portty-builtin");
            if sibling.exists() {
                return sibling.to_string_lossy().into_owned();
            }
        }
    }
    // Fall back to system path
    "/usr/lib/portty/portty-builtin".to_string()
}

impl Default for Config {
    fn default() -> Self {
        Self {
            builtin_path: default_builtin_path(),
            default: PortalConfig::default(),
            file_chooser: PortalConfig::default(),
            screenshot: PortalConfig::default(),
        }
    }
}

impl Config {
    /// Load config from default location (~/.config/portty/config.toml)
    pub fn load() -> Self {
        Self::config_path()
            .and_then(|path| std::fs::read_to_string(&path).ok())
            .and_then(|content| toml::from_str(&content).ok())
            .unwrap_or_default()
    }

    /// Get config file path
    fn config_path() -> Option<PathBuf> {
        dirs::config_dir().map(|p| p.join("portty/config.toml"))
    }

    /// Get exec command for a portal, falling back to default
    pub fn get_exec(&self, portal: &str) -> Option<&str> {
        let portal_config = match portal {
            "file-chooser" => &self.file_chooser,
            "screenshot" => &self.screenshot,
            _ => &self.default,
        };

        portal_config
            .exec
            .as_deref()
            .or(self.default.exec.as_deref())
    }
}
