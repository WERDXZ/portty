use std::fs;
use std::os::unix::fs::{DirBuilderExt, MetadataExt, PermissionsExt};
use std::path::{Path, PathBuf};

/// Get the base directory for sessions (/tmp/portty/<uid>/)
pub fn base_dir() -> PathBuf {
    let uid = fs::metadata("/proc/self").map(|m| m.uid()).unwrap_or(0);
    PathBuf::from(format!("/tmp/portty/{}", uid))
}

/// Get the daemon socket path
pub fn daemon_socket_path() -> PathBuf {
    base_dir().join("daemon.sock")
}

/// Get the pending directory
pub fn pending_dir() -> PathBuf {
    base_dir().join("pending")
}

/// Get the submissions directory
pub fn submissions_dir() -> PathBuf {
    base_dir().join("submissions")
}

/// Get the daemon control FIFO path
pub fn daemon_ctl_path() -> PathBuf {
    base_dir().join("daemon.ctl")
}

/// Ensure the base directory exists with correct ownership and permissions (0o700).
/// Returns an error if the directory is owned by another user.
pub fn ensure_base_dir() -> std::io::Result<()> {
    let base = base_dir();
    let parent = base.parent().unwrap_or(Path::new("/tmp"));
    fs::DirBuilder::new()
        .recursive(true)
        .mode(0o755)
        .create(parent)?;
    fs::DirBuilder::new()
        .mode(0o700)
        .create(&base)
        .or_else(|e| {
            if e.kind() == std::io::ErrorKind::AlreadyExists {
                Ok(())
            } else {
                Err(e)
            }
        })?;

    // Verify ownership
    let meta = fs::metadata(&base)?;
    let my_uid = fs::metadata("/proc/self").map(|m| m.uid()).unwrap_or(0);
    if meta.uid() != my_uid {
        return Err(std::io::Error::new(
            std::io::ErrorKind::PermissionDenied,
            format!(
                "base directory {} is owned by uid {}, expected {}",
                base.display(),
                meta.uid(),
                my_uid
            ),
        ));
    }

    // Enforce mode 0o700 (fix if we own it but mode is wrong)
    let mode = meta.mode() & 0o777;
    if mode != 0o700 {
        fs::set_permissions(&base, fs::Permissions::from_mode(0o700))?;
    }

    Ok(())
}
