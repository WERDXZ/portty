use std::fmt::Display;
use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::files;

use super::AddResult;

/// Screenshot operation mode
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ScreenshotMode {
    Screenshot { interactive: bool },
    PickColor,
}

impl Display for ScreenshotMode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Screenshot { interactive: true } => write!(f, "Screenshot (interactive)"),
            Self::Screenshot { interactive: false } => write!(f, "Screenshot"),
            Self::PickColor => write!(f, "PickColor"),
        }
    }
}

/// Session options for screenshot portal
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionOptions {
    pub mode: ScreenshotMode,
    pub app_id: String,
    pub modal: bool,
}

/// Validate and transform screenshot submission.
///
/// For screenshot: validates single entry, passes through as-is.
/// For pick-color: validates single entry and color format, strips file:// prefix.
pub fn validate(operation: &str, entries: &[String]) -> Result<Vec<String>, String> {
    if entries.is_empty() {
        return Err("No entries in submission".to_string());
    }
    if entries.len() > 1 {
        return Err(format!("Screenshot expects 1 entry, got {}", entries.len()));
    }

    match operation {
        "pick-color" => {
            let color_str = entries[0].strip_prefix("file://").unwrap_or(&entries[0]);
            parse_color(color_str).ok_or_else(|| {
                format!(
                    "invalid color format: '{}' (expected #rrggbb, 'R G B' floats, or rgb(r,g,b))",
                    color_str
                )
            })?;
            Ok(vec![color_str.to_string()])
        }
        _ => Ok(entries.to_vec()),
    }
}

/// Parse a color string into (r, g, b) floats in [0.0, 1.0]
///
/// Supports:
/// - Hex: `#rrggbb` or `#RRGGBB`
/// - Space-separated floats: `0.5 0.3 0.8`
/// - CSS-like: `rgb(r, g, b)` where r,g,b are 0-255 integers
pub fn parse_color(s: &str) -> Option<(f64, f64, f64)> {
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

    // Try space-separated floats (must be in [0.0, 1.0])
    let parts: Vec<&str> = s.split_whitespace().collect();
    if parts.len() == 3 {
        let r: f64 = parts[0].parse().ok()?;
        let g: f64 = parts[1].parse().ok()?;
        let b: f64 = parts[2].parse().ok()?;
        if (0.0..=1.0).contains(&r) && (0.0..=1.0).contains(&g) && (0.0..=1.0).contains(&b) {
            return Some((r, g, b));
        }
        return None;
    }

    None
}

/// Smart add entries: screenshot always replaces (single entry).
pub fn add_entries(sub_path: &Path, entries: &[String]) -> std::io::Result<AddResult> {
    files::write_lines(sub_path, entries)?;
    Ok(AddResult::Replaced)
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
