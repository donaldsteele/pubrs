//! Platform services, one trait per concern, one implementation per OS
//! selected at compile time, plus [`NullPlatform`] for headless runs.
//!
//! This is the only place (with `pubrs-cli`) where OS-specific code may live.

use std::path::{Path, PathBuf};

mod common;
#[cfg(target_os = "linux")]
mod linux;
#[cfg(target_os = "macos")]
mod macos;
#[cfg(windows)]
mod windows;

pub use common::{DesktopClipboard, DesktopPaths, NativeDialogs, NoClipboard, NullDialogs};

/// Native open/save dialogs.
pub trait FileDialogs {
    fn open_document(&self) -> Option<PathBuf>;
    fn save_document(&self, suggested_name: &str) -> Option<PathBuf>;
    fn pick_image(&self) -> Option<PathBuf>;
    fn save_export(
        &self,
        suggested_name: &str,
        extension: &str,
        description: &str,
    ) -> Option<PathBuf>;
}

/// Where the app keeps its files, by OS convention.
pub trait AppPaths {
    fn config_dir(&self) -> PathBuf;
    /// Autosave / crash-recovery files.
    fn recovery_dir(&self) -> PathBuf;
    fn user_templates_dir(&self) -> PathBuf;
    /// Default folder for new documents and exports.
    fn documents_dir(&self) -> PathBuf;
}

/// Straight (non-premultiplied) RGBA pixels from or for the clipboard.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClipImage {
    pub width: usize,
    pub height: usize,
    pub rgba: Vec<u8>,
}

/// The system clipboard: text, an image, and HTML (with a plain-text
/// alternative). Each `set_*` replaces what was there.
pub trait Clipboard {
    fn get_text(&mut self) -> Option<String>;
    fn set_text(&mut self, text: &str);
    fn get_image(&mut self) -> Option<ClipImage>;
    fn set_image(&mut self, image: ClipImage);
    fn get_html(&mut self) -> Option<String>;
    /// Put `html` on the clipboard with `alt_text` for programs that only
    /// read text.
    fn set_html(&mut self, html: &str, alt_text: &str);
}

/// Font discovery policy.
pub trait SystemFonts {
    /// Scan installed fonts (in addition to the bundled ones).
    fn use_system_fonts(&self) -> bool;
}

/// Options chosen in the app's own print dialog (used where the OS has no
/// print dialog of its own, i.e. Linux/CUPS).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PrintOptions {
    pub printer: Option<String>,
    pub copies: u32,
}

impl Default for PrintOptions {
    fn default() -> Self {
        Self {
            printer: None,
            copies: 1,
        }
    }
}

/// A printer offered in the app's print dialog.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PrinterInfo {
    pub name: String,
    pub is_default: bool,
}

/// Printing. Every OS prints the PDF the exporter produced.
pub trait PrintBackend {
    /// Human-readable name of the print path.
    fn name(&self) -> &'static str;
    /// True if `print_pdf` shows the operating system's own print dialog.
    fn has_system_dialog(&self) -> bool;
    /// Printers for the app's dialog (empty when the system dialog chooses).
    fn printers(&self) -> Vec<PrinterInfo>;
    /// Print `pdf` (showing the system dialog where there is one).
    fn print_pdf(
        &self,
        pdf: &Path,
        job_name: &str,
        options: &PrintOptions,
    ) -> Result<PrintOutcome, String>;
}

/// What happened when printing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PrintOutcome {
    /// Printed through the system dialog (macOS, Windows).
    #[cfg_attr(target_os = "linux", allow(dead_code))]
    Printed,
    /// The user cancelled the system dialog (macOS, Windows).
    #[cfg_attr(target_os = "linux", allow(dead_code))]
    Cancelled,
    /// Sent to a queue (e.g. CUPS job id).
    #[cfg_attr(not(target_os = "linux"), allow(dead_code))]
    Queued(String),
}

/// All platform services.
pub struct Platform {
    pub dialogs: Box<dyn FileDialogs>,
    pub paths: Box<dyn AppPaths>,
    pub clipboard: Box<dyn Clipboard>,
    pub fonts: Box<dyn SystemFonts>,
    pub print: Box<dyn PrintBackend>,
}

impl Platform {
    /// The real services for this OS.
    pub fn native() -> Self {
        Self {
            dialogs: Box::new(NativeDialogs),
            paths: Box::new(DesktopPaths::new()),
            clipboard: Box::new(DesktopClipboard::new()),
            fonts: Box::new(SystemFontsOn),
            print: native_print(),
        }
    }

    /// No dialogs, clipboard or printing; paths under `root` (tests, self-test).
    pub fn null(root: &Path) -> Self {
        Self {
            dialogs: Box::new(NullDialogs),
            paths: Box::new(DesktopPaths::under(root)),
            clipboard: Box::new(NoClipboard::default()),
            fonts: Box::new(NullPlatform),
            print: Box::new(NullPlatform),
        }
    }
}

struct SystemFontsOn;

impl SystemFonts for SystemFontsOn {
    fn use_system_fonts(&self) -> bool {
        true
    }
}

/// Headless implementation: bundled fonts only, printing unavailable.
#[derive(Debug, Default, Clone, Copy)]
pub struct NullPlatform;

impl SystemFonts for NullPlatform {
    fn use_system_fonts(&self) -> bool {
        false
    }
}

impl PrintBackend for NullPlatform {
    fn name(&self) -> &'static str {
        "none"
    }
    fn has_system_dialog(&self) -> bool {
        false
    }
    fn printers(&self) -> Vec<PrinterInfo> {
        Vec::new()
    }
    fn print_pdf(
        &self,
        _pdf: &Path,
        _job_name: &str,
        _options: &PrintOptions,
    ) -> Result<PrintOutcome, String> {
        Err("printing is not available in this session".into())
    }
}

#[cfg(target_os = "linux")]
fn native_print() -> Box<dyn PrintBackend> {
    Box::new(linux::CupsPrint)
}

#[cfg(target_os = "macos")]
fn native_print() -> Box<dyn PrintBackend> {
    Box::new(macos::MacPrint)
}

#[cfg(windows)]
fn native_print() -> Box<dyn PrintBackend> {
    Box::new(windows::WindowsPrint)
}

#[cfg(not(any(target_os = "linux", target_os = "macos", windows)))]
fn native_print() -> Box<dyn PrintBackend> {
    Box::new(NullPlatform)
}

/// Where pubrs-app's output goes. On Windows it is a GUI program without a
/// console window; elsewhere these are no-ops.
pub mod console {
    /// Attach to the terminal pubrs was started from, if any. Returns whether
    /// stderr goes somewhere a person can read it.
    #[cfg(windows)]
    pub use super::windows::attach_parent_console;

    #[cfg(not(windows))]
    pub fn attach_parent_console() -> bool {
        true
    }

    /// Show an error that ends pubrs before its window opens, for when there
    /// is no terminal to print it to.
    pub fn show_fatal_error(message: &str) {
        rfd::MessageDialog::new()
            .set_level(rfd::MessageLevel::Error)
            .set_title("pubrs could not start")
            .set_description(message)
            .set_buttons(rfd::MessageButtons::Ok)
            .show();
    }
}

/// macOS native menu bar (no-op elsewhere).
pub mod menu {
    #[cfg(target_os = "macos")]
    pub use super::macos::NativeMenu;

    /// Placeholder on platforms whose menu bar is drawn by egui.
    #[cfg(not(target_os = "macos"))]
    pub struct NativeMenu;

    #[cfg(not(target_os = "macos"))]
    impl NativeMenu {
        pub fn install(_items: &[crate::keymap::MenuSpec]) -> Option<Self> {
            None
        }
        pub fn poll(&self) -> Vec<crate::keymap::Action> {
            Vec::new()
        }
        pub fn handles(&self, _action: crate::keymap::Action) -> bool {
            false
        }
    }
}
