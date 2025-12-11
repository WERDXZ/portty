# Portty

An XDG Desktop Portal backend for TTY environments. This allows terminal-based applications to handle portal requests like file chooser dialogs by spawning a terminal with helper utilities.

## Implemented Portals

| Portal | Status | Description |
|--------|--------|-------------|
| FileChooser | ✅ | Open/save file dialogs |

## How It Works

1. An application requests a portal action (e.g., open file dialog)
2. The daemon creates a session directory at `/tmp/portty/<uid>/<session-id>/`
3. Shell shims are generated in `<session-dir>/bin/` for portal-specific commands
4. A terminal is spawned with the session bin directory prepended to `$PATH`
5. The user interacts with the terminal to complete the action
6. The result is sent back to the requesting application

## Configuration

Configuration file: `~/.config/portty/config.toml`

```toml
# Root level = default for all portals
# Auto-detects terminal if not set (foot, alacritty, kitty, etc.)
exec = "foot"

# File chooser portal configuration
[file-chooser]
exec = "foot"  # default for all file-chooser operations

# Custom commands available in sessions
# Added to $PATH alongside default shims (sel, submit, cancel)
[file-chooser.bin]
pick = "fzf --multi | sel --stdin"
preview = "bat \"$@\""

# Per-operation overrides
# Priority: operation-specific -> file-chooser -> root default

# SaveFile: auto-confirm with proposed filename
[file-chooser.save-file]
exec = "submit"  # uses submit shim for instant confirmation

# SaveFiles: auto-confirm with proposed directory
[file-chooser.save-files]
exec = "submit"

# Headless mode (no terminal, CLI only):
# Set exec = "" at any level, then use `portty` CLI to interact
```

## Session Environment

When a terminal is spawned for a portal action, these environment variables are set:

| Variable | Description |
|----------|-------------|
| `PORTTY_SESSION` | Unique session identifier |
| `PORTTY_DIR` | Session directory path |
| `PORTTY_SOCK` | Path to the IPC socket |
| `PORTTY_PORTAL` | Portal type (e.g., `file_chooser`) |

The session bin directory (`$PORTTY_DIR/bin`) is prepended to `$PATH`.

## Session Directory Structure

```
/tmp/portty/<uid>/
├── daemon.sock           # Daemon control socket (CLI <-> daemon)
└── <session-id>/
    ├── bin/
    │   ├── sel           # Shell shim -> portty select
    │   ├── submit        # Shell shim -> portty submit
    │   └── cancel        # Shell shim -> portty cancel
    ├── sock              # Session Unix socket for IPC
    └── portal            # Portal type identifier
```

## Session Commands

Commands are generated per-session as shell shims. For the file chooser portal:

### `sel`

Manage file selection.

```bash
# Add files to selection
sel file1.txt file2.txt

# Select files from stdin
find . -name "*.rs" | sel --stdin

# Show current selection (no args)
sel
```

### `submit`

Confirm selection and complete the dialog.

```bash
submit
```

### `cancel`

Cancel the current operation.

```bash
cancel
```

## CLI Usage

The `portty` CLI can control sessions from outside the spawned terminal:

```bash
# List active sessions
portty --list

# Add files to selection
portty select file1.txt file2.txt

# Submit the current session
portty submit

# Target a specific session
portty --session <id> select file.txt
```

When multiple sessions are active, commands target the earliest (oldest) session by default.

## IPC Protocol

Session shims communicate via Unix domain socket using length-prefixed bincode messages.

### Message Format

```
[4 bytes: message length (little-endian u32)]
[N bytes: bincode-serialized payload]
```

### FileChooser Messages

**Request** (shim -> session):

```rust
enum Request {
    GetOptions,              // Get session options
    GetSelection,            // Get current selection
    Select(Vec<String>),     // Select files (URIs)
    Cancel,                  // Cancel operation
}
```

**Response** (session -> shim):

```rust
enum Response {
    Options(SessionOptions), // Session options
    Selection(Vec<String>),  // Current selection
    Ok,                      // Success
    Error(String),           // Error message
}

struct SessionOptions {
    title: String,
    multiple: bool,
    directory: bool,
    save_mode: bool,
    current_name: Option<String>,
    current_folder: Option<String>,
    filters: Vec<Filter>,
    current_filter: Option<usize>,
}
```

### Connecting to the Socket

From Rust:

```rust
use std::os::unix::net::UnixStream;

let mut stream = UnixStream::connect(std::env::var("PORTTY_SOCK")?)?;
// write length-prefixed bincode message
// read length-prefixed bincode response
```

## Implementing a New Portal

1. **Define IPC types** in `crates/ipc/src/ipc/<portal>.rs`

2. **Register commands** in `crates/daemon/src/session.rs`:

```rust
fn default_commands(portal: &str) -> &'static [(&'static str, &'static str)] {
    match portal {
        "file-chooser" => &[("sel", "select"), ("submit", "submit"), ("cancel", "cancel")],
        "my-portal" => &[("my_cmd", "my_cmd"), ("submit", "submit"), ("cancel", "cancel")],
        _ => &[],
    }
}
```

3. **Implement the portal** in `crates/daemon/src/portal/<portal>.rs`

4. **Update the portal file** in `misc/tty.portal`:

```ini
[portal]
DBusName=org.freedesktop.impl.portal.desktop.tty
Interfaces=org.freedesktop.impl.portal.FileChooser;org.freedesktop.impl.portal.MyPortal;
```

## Building

```bash
cargo build --release
```

## Installation

```bash
# Install the daemon
install -Dm755 target/release/porttyd /usr/lib/portty/porttyd

# Install the CLI
install -Dm755 target/release/portty /usr/bin/portty

# Install portal file
install -Dm644 misc/tty.portal /usr/share/xdg-desktop-portal/portals/tty.portal

# Install systemd service (optional)
install -Dm644 misc/portty.service /usr/lib/systemd/user/portty.service
```

## License

MIT
