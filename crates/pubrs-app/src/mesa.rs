//! Mesa llvmpipe for Windows machines without a GPU driver (ADR 0011).
//!
//! The x64 MSI installs Mesa's `opengl32.dll` and `libgallium_wgl.dll` in
//! `bin\mesa`, next to a copy of `pubrs-app.exe`. pubrs-app imports
//! `opengl32.dll` at load time and Windows looks in the exe's own folder
//! first, so only the copy in `bin\mesa` loads Mesa; the one in `bin` keeps
//! the system OpenGL and any GPU driver behind it. When the adapter selector
//! finds no hardware adapter, pubrs restarts itself from `bin\mesa` with
//! `GALLIUM_DRIVER=llvmpipe` (mesa-dist-win otherwise picks its D3D12
//! driver, which runs on WARP again).

use std::path::{Path, PathBuf};
use std::process::{Command, ExitStatus};
use std::sync::atomic::{AtomicBool, Ordering};

/// Set in the restarted process.
const CHILD_ENV: &str = "PUBRS_MESA";
/// Set by the user to stay on the system's software adapter.
const OPT_OUT_ENV: &str = "PUBRS_NO_MESA";

static RESTART: AtomicBool = AtomicBool::new(false);
static UNUSABLE: AtomicBool = AtomicBool::new(false);

/// This process is the restarted copy that draws with Mesa.
pub fn active() -> bool {
    std::env::var_os(CHILD_ENV).is_some()
}

/// Where the installer puts the copy of `exe` that loads Mesa.
fn bundled_path(exe: &Path) -> Option<PathBuf> {
    Some(exe.parent()?.join("mesa").join(exe.file_name()?))
}

/// The Mesa copy of this program, if it is installed and may be used.
pub fn bundled_exe() -> Option<PathBuf> {
    if !cfg!(windows)
        || active()
        || UNUSABLE.load(Ordering::Relaxed)
        || std::env::var_os(OPT_OUT_ENV).is_some()
    {
        return None;
    }
    let exe = std::env::current_exe().ok()?;
    bundled_path(&exe).filter(|p| p.is_file())
}

/// Called by the adapter selector when only software adapters exist.
pub fn request_restart() {
    RESTART.store(true, Ordering::Relaxed);
}

pub fn restart_requested() -> bool {
    RESTART.load(Ordering::Relaxed)
}

/// Run the Mesa copy with this process's arguments and wait for it. If it
/// cannot be started, Mesa is not offered again.
pub fn run_bundled(exe: &Path) -> std::io::Result<ExitStatus> {
    Command::new(exe)
        .args(std::env::args_os().skip(1))
        .env(CHILD_ENV, "1")
        .env("GALLIUM_DRIVER", "llvmpipe")
        .status()
        .inspect_err(|_| UNUSABLE.store(true, Ordering::Relaxed))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mesa_copy_sits_in_a_subfolder_with_the_same_name() {
        let exe = Path::new("C:/Program Files/pubrs/bin/pubrs-app.exe");
        assert_eq!(
            bundled_path(exe).unwrap(),
            Path::new("C:/Program Files/pubrs/bin/mesa/pubrs-app.exe")
        );
    }
}
