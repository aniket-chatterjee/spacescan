//! Open a path in the operating system's file manager (best effort).

use std::path::Path;
use std::process::Command;

use crate::constants;

/// Reveal `path` in the platform file manager: Explorer (Windows), Finder
/// (macOS via `open`), or the freedesktop handler (`xdg-open`) elsewhere.
pub fn open_in_file_manager(path: &Path) -> std::io::Result<()> {
    let program = if cfg!(windows) {
        constants::platform::WINDOWS_FILE_MANAGER
    } else if cfg!(target_os = "macos") {
        constants::platform::MACOS_FILE_MANAGER
    } else {
        constants::platform::UNIX_FILE_MANAGER
    };
    Command::new(program).arg(path).spawn().map(|_| ())
}
