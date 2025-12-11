use std::collections::HashMap;
use std::path::PathBuf;

use serde::Deserialize;

/// Base config fields shared at every level (root, portal, operation)
#[derive(Debug, Clone, Default, Deserialize)]
struct BaseConfig {
    /// Command to execute
    #[serde(default)]
    exec: Option<String>,

    /// Custom bin shims
    #[serde(default)]
    bin: HashMap<String, String>,
}

/// Operation-level config (leaf)
#[derive(Debug, Clone, Default, Deserialize)]
pub struct OperationConfig {
    #[serde(flatten)]
    base: BaseConfig,
}

/// Portal-level config with nested operations
#[derive(Debug, Clone, Default, Deserialize)]
pub struct PortalConfig {
    #[serde(flatten)]
    base: BaseConfig,

    /// Operation-specific configs (unknown keys become operations)
    #[serde(flatten)]
    pub operations: HashMap<String, OperationConfig>,
}

/// Root configuration
#[derive(Debug, Clone, Default, Deserialize)]
pub struct Config {
    #[serde(flatten)]
    base: BaseConfig,

    /// Portal-specific configs (unknown keys become portals)
    #[serde(flatten)]
    pub portals: HashMap<String, PortalConfig>,
}

/// Try to find a terminal emulator
fn detect_terminal() -> Option<String> {
    let terminals = ["foot", "alacritty", "kitty", "wezterm", "ghostty", "xterm"];

    for term in terminals {
        if std::process::Command::new("which")
            .arg(term)
            .output()
            .map(|o| o.status.success())
            .unwrap_or(false)
        {
            return Some(term.to_string());
        }
    }
    None
}

impl Config {
    /// Load config from default location (~/.config/portty/config.toml)
    pub fn load() -> Self {
        Self::config_path()
            .and_then(|path| std::fs::read_to_string(&path).ok())
            .and_then(|content| toml::from_str(&content).ok())
            .unwrap_or_else(|| Self {
                base: BaseConfig {
                    exec: detect_terminal(),
                    bin: HashMap::new(),
                },
                portals: HashMap::new(),
            })
    }

    /// Get config file path
    fn config_path() -> Option<PathBuf> {
        dirs::config_dir().map(|p| p.join("portty/config.toml"))
    }

    /// Resolve exec command for a portal operation.
    /// Priority: operation-specific -> portal-specific -> root default
    pub fn resolve_exec(&self, portal: &str, operation: &str) -> Option<&str> {
        let portal_cfg = self.portals.get(portal);

        // Check operation-specific
        let op_exec = portal_cfg
            .and_then(|p| p.operations.get(operation))
            .and_then(|o| o.base.exec.as_deref());

        // Check portal-level
        let portal_exec = portal_cfg.and_then(|p| p.base.exec.as_deref());

        // Fall back to root
        op_exec
            .or(portal_exec)
            .or(self.base.exec.as_deref())
            .filter(|s| !s.is_empty())
    }

    /// Resolve bin shims for a portal operation (merged from all levels).
    /// Priority: operation-specific overrides portal-level overrides root.
    pub fn resolve_bin(&self, portal: &str, operation: &str) -> HashMap<String, String> {
        let mut bin = self.base.bin.clone();

        if let Some(portal_cfg) = self.portals.get(portal) {
            bin.extend(portal_cfg.base.bin.clone());

            if let Some(op_cfg) = portal_cfg.operations.get(operation) {
                bin.extend(op_cfg.base.bin.clone());
            }
        }

        bin
    }
}
