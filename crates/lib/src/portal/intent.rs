use serde::{Deserialize, Serialize};
use std::fmt;

/// Top-level family for an intent.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum IntentFamily {
    Path,
    Directory,
    Color,
}

impl fmt::Display for IntentFamily {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Path => write!(f, "path"),
            Self::Directory => write!(f, "directory"),
            Self::Color => write!(f, "color"),
        }
    }
}

impl std::str::FromStr for IntentFamily {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "path" => Ok(Self::Path),
            "directory" => Ok(Self::Directory),
            "color" => Ok(Self::Color),
            _ => Err(format!(
                "unknown family '{s}', expected one of: path, directory, color"
            )),
        }
    }
}

/// A single item within an intent.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", content = "value")]
pub enum IntentItem {
    Path(String),
    Directory(String),
    Color(String),
}

impl IntentItem {
    /// The family of this item.
    pub fn family(&self) -> IntentFamily {
        match self {
            Self::Path(_) => IntentFamily::Path,
            Self::Directory(_) => IntentFamily::Directory,
            Self::Color(_) => IntentFamily::Color,
        }
    }
}

impl fmt::Display for IntentItem {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Path(v) => write!(f, "path: {v}"),
            Self::Directory(v) => write!(f, "directory: {v}"),
            Self::Color(v) => write!(f, "color: {v}"),
        }
    }
}

/// Merge operation for applying items to an intent.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MergeOp {
    /// Merge new items into the intent (compatible families only, promotes cardinality).
    Add,
    /// Replace the entire intent with the given items.
    Set,
}

/// Cardinality constraint.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Cardinality {
    Single,
    Multi,
}

impl fmt::Display for Cardinality {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Single => write!(f, "single"),
            Self::Multi => write!(f, "multi"),
        }
    }
}

/// A typed, mergeable intent representing queued portal input.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Intent {
    pub version: u32,
    pub family: IntentFamily,
    pub cardinality: Cardinality,
    pub items: Vec<IntentItem>,
}

impl Intent {
    /// Create a new intent with a single item.
    pub fn single(item: IntentItem) -> Self {
        Self {
            version: 1,
            family: item.family(),
            cardinality: Cardinality::Single,
            items: vec![item],
        }
    }

    /// Create a new intent with multiple items of the same family.
    pub fn multi(family: IntentFamily, items: Vec<IntentItem>) -> Result<Self, String> {
        if items.is_empty() {
            return Err("cannot create empty multi intent".into());
        }
        for item in &items {
            if item.family() != family {
                return Err(format!(
                    "item family {} does not match intent family {}",
                    item.family(),
                    family
                ));
            }
        }
        Ok(Self {
            version: 1,
            family,
            cardinality: Cardinality::Multi,
            items,
        })
    }

    /// Create an empty intent. This is a no-op placeholder; use `apply` to add items.
    pub fn empty(family: IntentFamily) -> Self {
        Self {
            version: 1,
            family,
            cardinality: Cardinality::Single,
            items: Vec::new(),
        }
    }

    /// Whether this intent has any items.
    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    /// Number of items.
    pub fn len(&self) -> usize {
        self.items.len()
    }

    /// Convenience: add a single item (merge-compatible families, promote cardinality).
    pub fn add(&mut self, item: IntentItem) -> Result<(), String> {
        self.apply(&[item], MergeOp::Add)
    }

    /// Convenience: replace intent with a single item.
    pub fn set(&mut self, item: IntentItem) {
        self.apply(&[item], MergeOp::Set)
            .expect("set is infallible");
    }

    /// Core merge engine.
    pub fn apply(&mut self, new_items: &[IntentItem], op: MergeOp) -> Result<(), String> {
        if new_items.is_empty() {
            return Err("no items to apply".into());
        }

        let incoming_family = new_items[0].family();
        for item in &new_items[1..] {
            if item.family() != incoming_family {
                return Err(format!(
                    "mixed families in incoming items: {} and {}",
                    incoming_family,
                    item.family()
                ));
            }
        }

        match op {
            MergeOp::Set => {
                self.family = incoming_family;
                self.cardinality = if new_items.len() > 1 {
                    Cardinality::Multi
                } else {
                    Cardinality::Single
                };
                self.items = new_items.to_vec();
            }
            MergeOp::Add => {
                if self.is_empty() {
                    self.family = incoming_family;
                    self.cardinality = if new_items.len() > 1 {
                        Cardinality::Multi
                    } else {
                        Cardinality::Single
                    };
                    self.items = new_items.to_vec();
                } else if self.family != incoming_family {
                    return Err(format!(
                        "cannot merge {incoming_family} items into {} intent",
                        self.family
                    ));
                } else {
                    self.items.extend_from_slice(new_items);
                    if self.items.len() > 1 {
                        self.cardinality = Cardinality::Multi;
                    }
                }
            }
        }
        Ok(())
    }

    /// Remove matching items from the intent.
    pub fn remove(&mut self, items: &[IntentItem]) -> Result<usize, String> {
        if items.is_empty() {
            return Err("no items to remove".into());
        }

        let incoming_family = items[0].family();
        for item in &items[1..] {
            if item.family() != incoming_family {
                return Err(format!(
                    "mixed families in incoming items: {} and {}",
                    incoming_family,
                    item.family()
                ));
            }
        }

        if self.is_empty() {
            return Ok(0);
        }

        if self.family != incoming_family {
            return Err(format!(
                "cannot remove {incoming_family} items from {} intent",
                self.family
            ));
        }

        let original_len = self.items.len();
        self.items.retain(|item| !items.contains(item));
        self.cardinality = if self.items.len() > 1 {
            Cardinality::Multi
        } else {
            Cardinality::Single
        };

        Ok(original_len - self.items.len())
    }

    /// Convert items into plain string values (for materialization).
    pub fn values(&self) -> Vec<String> {
        match self.family {
            IntentFamily::Path => self
                .items
                .iter()
                .filter_map(|i| match i {
                    IntentItem::Path(v) => Some(v.clone()),
                    _ => None,
                })
                .collect(),
            IntentFamily::Directory => self
                .items
                .iter()
                .filter_map(|i| match i {
                    IntentItem::Directory(v) => Some(v.clone()),
                    _ => None,
                })
                .collect(),
            IntentFamily::Color => self
                .items
                .iter()
                .filter_map(|i| match i {
                    IntentItem::Color(v) => Some(v.clone()),
                    _ => None,
                })
                .collect(),
        }
    }
}

impl fmt::Display for Intent {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(f, "family:    {}", self.family)?;
        writeln!(f, "cardinality: {}", self.cardinality)?;
        writeln!(f, "items ({}):", self.items.len())?;
        for item in &self.items {
            writeln!(f, "  {item}")?;
        }
        Ok(())
    }
}

impl Default for Intent {
    fn default() -> Self {
        Self {
            version: 1,
            family: IntentFamily::Path,
            cardinality: Cardinality::Single,
            items: Vec::new(),
        }
    }
}

/// Queue storage for pending intent.
pub mod queue {
    use super::Intent;
    use std::path::{Path, PathBuf};

    /// Read pending intent from a directory (looks for `intent.json`).
    pub fn read(pending_dir: &Path) -> Option<Intent> {
        let path = pending_dir.join("intent.json");
        let content = std::fs::read_to_string(&path).ok()?;
        serde_json::from_str(&content).ok()
    }

    /// Write pending intent to a directory.
    pub fn write(pending_dir: &Path, intent: &Intent) -> std::io::Result<()> {
        std::fs::create_dir_all(pending_dir)?;
        let path = pending_dir.join("intent.json");
        let content = serde_json::to_string_pretty(intent)?;
        std::fs::write(path, content)?;
        Ok(())
    }

    /// Clear pending intent.
    pub fn clear(pending_dir: &Path) -> std::io::Result<()> {
        let path = pending_dir.join("intent.json");
        if path.exists() {
            std::fs::remove_file(&path)?;
        }
        Ok(())
    }

    /// Path to the intent file.
    pub fn intent_path(pending_dir: &Path) -> PathBuf {
        pending_dir.join("intent.json")
    }
}

/// Parse a string value into a typed intent item, resolving relative paths.
pub fn parse_item(family: &str, value: &str) -> Result<IntentItem, String> {
    match family {
        "path" => Ok(IntentItem::Path(resolve_path_value(value))),
        "directory" => Ok(IntentItem::Directory(resolve_path_value(value))),
        "color" => Ok(IntentItem::Color(value.to_string())),
        _ => Err(format!(
            "unknown family '{family}', expected one of: path, directory, color"
        )),
    }
}

fn resolve_path_value(value: &str) -> String {
    let value = value.strip_prefix("file://").unwrap_or(value);
    let path = std::path::Path::new(value);
    let resolved = if path.is_absolute() {
        path.to_path_buf()
    } else if let Ok(cwd) = std::env::current_dir() {
        cwd.join(path)
    } else {
        path.to_path_buf()
    };
    resolved.to_string_lossy().into_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn single_path_intent() {
        let mut intent = Intent::single(IntentItem::Path("/home/me/foo.txt".into()));
        assert_eq!(intent.family, IntentFamily::Path);
        assert_eq!(intent.cardinality, Cardinality::Single);
        assert_eq!(intent.len(), 1);

        intent
            .add(IntentItem::Path("/home/me/bar.txt".into()))
            .unwrap();
        assert_eq!(intent.cardinality, Cardinality::Multi);
        assert_eq!(intent.len(), 2);
    }

    #[test]
    fn add_rejects_family_mismatch() {
        let mut intent = Intent::single(IntentItem::Path("/home/me/foo.txt".into()));
        let err = intent.add(IntentItem::Directory("/home/me".into()));
        assert!(err.is_err());
        assert!(err.unwrap_err().contains("cannot merge directory"));
    }

    #[test]
    fn set_replaces() {
        let mut intent = Intent::single(IntentItem::Path("/home/me/foo.txt".into()));
        intent.set(IntentItem::Directory("/home/me".into()));
        assert_eq!(intent.family, IntentFamily::Directory);
        assert_eq!(intent.cardinality, Cardinality::Single);
        assert_eq!(intent.len(), 1);
    }

    #[test]
    fn color_append() {
        let mut intent = Intent::single(IntentItem::Color("#ff0000".into()));
        intent.add(IntentItem::Color("#00ff00".into())).unwrap();
        assert_eq!(intent.cardinality, Cardinality::Multi);
        assert_eq!(intent.len(), 2);
    }

    #[test]
    fn multi_construction_validates_family() {
        let items = vec![
            IntentItem::Path("/a".into()),
            IntentItem::Directory("/tmp".into()),
        ];
        let err = Intent::multi(IntentFamily::Path, items);
        assert!(err.is_err());
    }

    #[test]
    fn directory_append() {
        let mut intent = Intent::single(IntentItem::Directory("/tmp/a".into()));
        intent.add(IntentItem::Directory("/tmp/b".into())).unwrap();
        assert_eq!(intent.family, IntentFamily::Directory);
        assert_eq!(intent.cardinality, Cardinality::Multi);
        assert_eq!(intent.len(), 2);
    }

    #[test]
    fn queue_roundtrip() {
        use tempfile::tempdir;

        let dir = tempdir().unwrap();
        let mut intent = Intent::single(IntentItem::Path("/tmp/test.png".into()));
        intent
            .add(IntentItem::Path("/tmp/test2.png".into()))
            .unwrap();

        queue::write(dir.path(), &intent).unwrap();
        let loaded = queue::read(dir.path()).unwrap();
        assert_eq!(loaded, intent);
    }

    #[test]
    fn parse_path_resolves_relative() {
        let item = parse_item("path", "foo.txt").unwrap();
        match item {
            IntentItem::Path(v) => {
                assert!(v.starts_with('/'), "expected absolute path, got: {v}");
            }
            _ => panic!("expected path item"),
        }
    }

    #[test]
    fn parse_color_passthrough() {
        let item = parse_item("color", "#ff8800").unwrap();
        match item {
            IntentItem::Color(v) => assert_eq!(v, "#ff8800"),
            _ => panic!("expected color item"),
        }
    }

    #[test]
    fn parse_directory_resolves_relative() {
        let item = parse_item("directory", "foo").unwrap();
        match item {
            IntentItem::Directory(v) => {
                assert!(v.starts_with('/'), "expected absolute path, got: {v}");
            }
            _ => panic!("expected directory item"),
        }
    }
}
