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
# Path to the portty-builtin binary
builtin_path = "/usr/lib/portty/portty-builtin"

[default]
# Default terminal command for all portals
exec = "foot"

[file-chooser]
# Override for file chooser portal
exec = "foot"
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
/tmp/portty/<uid>/<session-id>/
├── bin/
│   ├── sel       # Shell shim -> portty-builtin file-chooser select
│   └── cancel    # Shell shim -> portty-builtin file-chooser cancel
├── sock          # Unix domain socket for IPC
└── portal        # Portal type identifier
```

## Builtin Commands

Commands are generated per-session as shell shims. For the file chooser portal:

### `sel`

Manage file selection.

```bash
# Select files (completes the dialog)
sel file1.txt file2.txt

# Select files from stdin
find . -name "*.rs" | sel --stdin

# Show current selection
sel

# Show session options (filters, title, etc.)
sel --options
```

### `cancel`

Cancel the current operation.

```bash
cancel
```

## IPC Protocol

The daemon and builtins communicate via Unix domain socket using length-prefixed bincode messages.

### Message Format

```
[4 bytes: message length (little-endian u32)]
[N bytes: bincode-serialized payload]
```

### FileChooser Messages

**Request** (builtin -> daemon):

```rust
enum Request {
    GetOptions,              // Get session options
    GetSelection,            // Get current selection
    Select(Vec<String>),     // Select files (URIs)
    Cancel,                  // Cancel operation
}
```

**Response** (daemon -> builtin):

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

1. **Define IPC types** in `crates/types/src/ipc/<portal>.rs`:

```rust
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Request {
    // Portal-specific requests
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Response {
    // Portal-specific responses
    Ok,
    Error(String),
}
```

2. **Register commands** in `crates/daemon/src/session.rs`:

```rust
// Returns (shim_name, internal_command) pairs
fn default_commands(portal: &str) -> &'static [(&'static str, &'static str)] {
    match portal {
        "file-chooser" => &[("sel", "select"), ("cancel", "cancel")],
        "my-portal" => &[("my_cmd", "my_cmd"), ("cancel", "cancel")],
        _ => &[],
    }
}
```

3. **Add builtin handler** in `crates/builtins/src/`:

Create `my_portal.rs` with a `dispatch(command, args)` function.

4. **Register in main.rs**:

```rust
// crates/builtins/src/main.rs
match portal.as_str() {
    "file_chooser" => portty_builtins::file_chooser::dispatch(command, rest),
    "my_portal" => portty_builtins::my_portal::dispatch(command, rest),
    // ...
}
```

5. **Implement the portal** in `crates/daemon/src/portal/<portal>.rs`

6. **Update the portal file** in `misc/tty.portal`:

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

# Install the builtin binary
install -Dm755 target/release/portty-builtin /usr/lib/portty/portty-builtin

# Install portal file
install -Dm644 misc/tty.portal /usr/share/xdg-desktop-portal/portals/tty.portal

# Install systemd service (optional)
install -Dm644 misc/portty.service /usr/lib/systemd/user/portty.service
```

## License

MIT
