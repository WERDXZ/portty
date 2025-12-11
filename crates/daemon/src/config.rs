use std::collections::HashMap;
use std::fmt::Display;
use std::path::PathBuf;

use serde::Deserialize;

/// Portal configuration - extensible via generic parameter
#[derive(Debug, Clone, Deserialize)]
pub struct PortalConfig<Ext = ()> {
    /// Command to execute for this portal
    #[serde(default)]
    pub exec: Option<String>,

    /// Custom bin shims for this portal
    #[serde(default)]
    pub bin: HashMap<String, String>,

    /// Extension fields (flattened into this table)
    #[serde(flatten)]
    pub ext: Ext,
}

impl<Ext: Default> Default for PortalConfig<Ext> {
    fn default() -> Self {
        Self {
            exec: None,
            bin: HashMap::new(),
            ext: Ext::default(),
        }
    }
}

/// File chooser operation types
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FileChooserOp {
    OpenFile,
    SaveFile,
    SaveFiles,
}

impl FileChooserOp {
    pub fn as_str(&self) -> &'static str {
        match &self {
            Self::OpenFile => "Open File",
            Self::SaveFile => "Save File",
            Self::SaveFiles => "Save Files",
        }
    }
}

impl Display for FileChooserOp {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

/// File chooser extension - sub-operation configs
#[derive(Debug, Clone, Default, Deserialize)]
pub struct FileChooserExt {
    #[serde(default, rename = "open-file")]
    pub open_file: Option<PortalConfig>,

    #[serde(default, rename = "save-file")]
    pub save_file: Option<PortalConfig>,

    #[serde(default, rename = "save-files")]
    pub save_files: Option<PortalConfig>,
}

/// Screenshot operation types
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScreenshotOp {
    Screenshot,
    PickColor,
}

impl ScreenshotOp {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Screenshot => "Screenshot",
            Self::PickColor => "Pick Color",
        }
    }
}

impl Display for ScreenshotOp {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

/// Screenshot extension - sub-operation configs
#[derive(Debug, Clone, Default, Deserialize)]
pub struct ScreenshotExt {
    #[serde(default)]
    pub screenshot: Option<PortalConfig>,

    #[serde(default, rename = "pick-color")]
    pub pick_color: Option<PortalConfig>,
}

/// Root extension - all portal configs
#[derive(Debug, Clone, Default, Deserialize)]
pub struct RootExt {
    #[serde(default, rename = "file-chooser")]
    pub file_chooser: Option<PortalConfig<FileChooserExt>>,

    #[serde(default)]
    pub screenshot: Option<PortalConfig<ScreenshotExt>>,
}

/// Main configuration - just a PortalConfig with RootExt
pub type Config = PortalConfig<RootExt>;

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
                exec: detect_terminal(),
                bin: HashMap::new(),
                ext: RootExt::default(),
            })
    }

    /// Get config file path
    fn config_path() -> Option<PathBuf> {
        dirs::config_dir().map(|p| p.join("portty/config.toml"))
    }

    /// Get effective exec for file-chooser operation
    /// Priority: operation-specific → file-chooser → root default
    pub fn file_chooser_exec(&self, op: FileChooserOp) -> Option<&str> {
        let fc = self.ext.file_chooser.as_ref();

        // Check operation-specific config
        let op_exec = fc.and_then(|fc| {
            let op_config = match op {
                FileChooserOp::OpenFile => fc.ext.open_file.as_ref(),
                FileChooserOp::SaveFile => fc.ext.save_file.as_ref(),
                FileChooserOp::SaveFiles => fc.ext.save_files.as_ref(),
            };
            op_config.and_then(|c| c.exec.as_deref())
        });

        // Check file-chooser base config
        let fc_exec = fc.and_then(|fc| fc.exec.as_deref());

        // Fall back to root default
        op_exec
            .or(fc_exec)
            .or(self.exec.as_deref())
            .filter(|s| !s.is_empty())
    }

    /// Get effective bin shims for file-chooser operation (merged)
    pub fn file_chooser_bin(&self, op: FileChooserOp) -> HashMap<String, String> {
        let mut bin = self.bin.clone();

        if let Some(fc) = &self.ext.file_chooser {
            // Merge file-chooser base bins
            bin.extend(fc.bin.clone());

            // Merge operation-specific bins
            let op_config = match op {
                FileChooserOp::OpenFile => fc.ext.open_file.as_ref(),
                FileChooserOp::SaveFile => fc.ext.save_file.as_ref(),
                FileChooserOp::SaveFiles => fc.ext.save_files.as_ref(),
            };
            if let Some(c) = op_config {
                bin.extend(c.bin.clone());
            }
        }

        bin
    }

    /// Get effective exec for screenshot operation
    /// Priority: operation-specific → screenshot → root default
    pub fn screenshot_exec(&self, op: ScreenshotOp) -> Option<&str> {
        let sc = self.ext.screenshot.as_ref();

        // Check operation-specific config
        let op_exec = sc.and_then(|sc| {
            let op_config = match op {
                ScreenshotOp::Screenshot => sc.ext.screenshot.as_ref(),
                ScreenshotOp::PickColor => sc.ext.pick_color.as_ref(),
            };
            op_config.and_then(|c| c.exec.as_deref())
        });

        // Check screenshot base config
        let sc_exec = sc.and_then(|sc| sc.exec.as_deref());

        // Fall back to root default
        op_exec
            .or(sc_exec)
            .or(self.exec.as_deref())
            .filter(|s| !s.is_empty())
    }

    /// Get effective bin shims for screenshot operation (merged)
    pub fn screenshot_bin(&self, op: ScreenshotOp) -> HashMap<String, String> {
        let mut bin = self.bin.clone();

        if let Some(sc) = &self.ext.screenshot {
            // Merge screenshot base bins
            bin.extend(sc.bin.clone());

            // Merge operation-specific bins
            let op_config = match op {
                ScreenshotOp::Screenshot => sc.ext.screenshot.as_ref(),
                ScreenshotOp::PickColor => sc.ext.pick_color.as_ref(),
            };
            if let Some(c) = op_config {
                bin.extend(c.bin.clone());
            }
        }

        bin
    }
}
