use std::fs;
use std::io::Write;
use std::path::Path;

/// Read non-empty lines from a file. Returns an empty vec on any error.
pub fn read_lines(path: &Path) -> Vec<String> {
    fs::read_to_string(path)
        .unwrap_or_default()
        .lines()
        .filter(|l| !l.is_empty())
        .map(String::from)
        .collect()
}

/// Write lines to a file (one per line). Empty slice writes an empty file.
pub fn write_lines(path: &Path, lines: &[String]) -> std::io::Result<()> {
    if lines.is_empty() {
        fs::write(path, "")
    } else {
        let content = format!("{}\n", lines.join("\n"));
        fs::write(path, content)
    }
}

/// Append lines to a file (creates if missing).
pub fn append_lines(path: &Path, lines: &[String]) -> std::io::Result<()> {
    let mut file = fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)?;
    for line in lines {
        writeln!(file, "{}", line)?;
    }
    Ok(())
}

/// Remove specific lines from a file.
pub fn remove_lines(path: &Path, to_remove: &[String]) -> std::io::Result<()> {
    let existing = read_lines(path);
    let remaining: Vec<String> = existing
        .into_iter()
        .filter(|e| !to_remove.contains(e))
        .collect();
    write_lines(path, &remaining)
}
