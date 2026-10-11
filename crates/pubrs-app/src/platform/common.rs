//! Cross-platform implementations built on rfd, dirs and arboard (each of
//! which uses the native API of the OS underneath).

use std::path::{Path, PathBuf};

use super::{AppPaths, ClipImage, Clipboard, FileDialogs};

/// Native dialogs via rfd.
pub struct NativeDialogs;

impl FileDialogs for NativeDialogs {
    fn open_document(&self) -> Option<PathBuf> {
        rfd::FileDialog::new()
            .set_title("Open publication")
            .add_filter("pubrs publication", &["pubrs"])
            .pick_file()
    }

    fn save_document(&self, suggested_name: &str) -> Option<PathBuf> {
        rfd::FileDialog::new()
            .set_title("Save publication")
            .add_filter("pubrs publication", &["pubrs"])
            .set_file_name(format!("{suggested_name}.pubrs"))
            .save_file()
    }

    fn pick_image(&self) -> Option<PathBuf> {
        rfd::FileDialog::new()
            .set_title("Insert picture")
            .add_filter("Pictures", &["png", "jpg", "jpeg", "svg"])
            .pick_file()
    }

    fn save_export(
        &self,
        suggested_name: &str,
        extension: &str,
        description: &str,
    ) -> Option<PathBuf> {
        rfd::FileDialog::new()
            .set_title(format!("Export {description}"))
            .add_filter(description, &[extension])
            .set_file_name(format!("{suggested_name}.{extension}"))
            .save_file()
    }
}

/// No dialogs (headless): every dialog is cancelled.
pub struct NullDialogs;

impl FileDialogs for NullDialogs {
    fn open_document(&self) -> Option<PathBuf> {
        None
    }
    fn save_document(&self, _: &str) -> Option<PathBuf> {
        None
    }
    fn pick_image(&self) -> Option<PathBuf> {
        None
    }
    fn save_export(&self, _: &str, _: &str, _: &str) -> Option<PathBuf> {
        None
    }
}

/// Per-OS folders from `dirs` (XDG on Linux, Application Support on macOS,
/// AppData on Windows), or everything under one root for tests.
pub struct DesktopPaths {
    root: Option<PathBuf>,
}

impl DesktopPaths {
    pub fn new() -> Self {
        Self { root: None }
    }

    pub fn under(root: &Path) -> Self {
        Self {
            root: Some(root.to_path_buf()),
        }
    }

    fn base(&self, dir: Option<PathBuf>, fallback: &str) -> PathBuf {
        match &self.root {
            Some(r) => r.join(fallback),
            None => dir
                .unwrap_or_else(|| std::env::temp_dir().join("pubrs"))
                .join("pubrs"),
        }
    }
}

impl Default for DesktopPaths {
    fn default() -> Self {
        Self::new()
    }
}

impl AppPaths for DesktopPaths {
    fn config_dir(&self) -> PathBuf {
        self.base(dirs::config_dir(), "config")
    }

    fn recovery_dir(&self) -> PathBuf {
        self.base(dirs::data_local_dir(), "data").join("recovery")
    }

    fn user_templates_dir(&self) -> PathBuf {
        self.base(dirs::data_dir(), "data").join("templates")
    }

    fn documents_dir(&self) -> PathBuf {
        match &self.root {
            Some(r) => r.join("documents"),
            None => dirs::document_dir()
                .or_else(dirs::home_dir)
                .unwrap_or_else(std::env::temp_dir)
                .join("pubrs"),
        }
    }
}

/// The OS clipboard via arboard (falls back to an in-memory clipboard).
pub struct DesktopClipboard {
    inner: Option<arboard::Clipboard>,
    fallback: NoClipboard,
}

impl DesktopClipboard {
    pub fn new() -> Self {
        let inner = arboard::Clipboard::new()
            .map_err(|e| tracing::warn!("system clipboard unavailable: {e}"))
            .ok();
        Self {
            inner,
            fallback: NoClipboard::default(),
        }
    }
}

impl Default for DesktopClipboard {
    fn default() -> Self {
        Self::new()
    }
}

impl Clipboard for DesktopClipboard {
    fn get_text(&mut self) -> Option<String> {
        match &mut self.inner {
            Some(c) => c.get_text().ok(),
            None => self.fallback.get_text(),
        }
    }

    fn set_text(&mut self, text: &str) {
        match &mut self.inner {
            Some(c) => {
                if let Err(e) = c.set_text(text.to_string()) {
                    tracing::warn!("clipboard: {e}");
                }
            }
            None => self.fallback.set_text(text),
        }
    }

    fn get_image(&mut self) -> Option<ClipImage> {
        match &mut self.inner {
            Some(c) => c.get_image().ok().map(|i| ClipImage {
                width: i.width,
                height: i.height,
                rgba: i.bytes.into_owned(),
            }),
            None => self.fallback.get_image(),
        }
    }

    fn set_image(&mut self, image: ClipImage) {
        match &mut self.inner {
            Some(c) => {
                let data = arboard::ImageData {
                    width: image.width,
                    height: image.height,
                    bytes: image.rgba.into(),
                };
                if let Err(e) = c.set_image(data) {
                    tracing::warn!("clipboard: {e}");
                }
            }
            None => self.fallback.set_image(image),
        }
    }

    fn get_html(&mut self) -> Option<String> {
        match &mut self.inner {
            Some(c) => c.get().html().ok(),
            None => self.fallback.get_html(),
        }
    }

    fn set_html(&mut self, html: &str, alt_text: &str) {
        match &mut self.inner {
            Some(c) => {
                if let Err(e) = c.set_html(html, Some(alt_text)) {
                    tracing::warn!("clipboard: {e}");
                }
            }
            None => self.fallback.set_html(html, alt_text),
        }
    }
}

/// An in-memory clipboard. Like the real one, setting one kind of content
/// replaces the others (HTML keeps its text alternative).
#[derive(Debug, Default)]
pub struct NoClipboard {
    text: Option<String>,
    html: Option<String>,
    image: Option<ClipImage>,
}

impl Clipboard for NoClipboard {
    fn get_text(&mut self) -> Option<String> {
        self.text.clone()
    }
    fn set_text(&mut self, text: &str) {
        self.text = Some(text.to_string());
        self.html = None;
        self.image = None;
    }
    fn get_image(&mut self) -> Option<ClipImage> {
        self.image.clone()
    }
    fn set_image(&mut self, image: ClipImage) {
        self.text = None;
        self.html = None;
        self.image = Some(image);
    }
    fn get_html(&mut self) -> Option<String> {
        self.html.clone()
    }
    fn set_html(&mut self, html: &str, alt_text: &str) {
        self.text = Some(alt_text.to_string());
        self.html = Some(html.to_string());
        self.image = None;
    }
}
