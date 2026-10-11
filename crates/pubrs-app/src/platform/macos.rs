//! macOS: native menu bar (NSMenu via muda) and printing through
//! NSPrintOperation on the exported PDF (PDFKit).

use std::collections::HashMap;
use std::path::Path;

use eframe::egui::{Key, KeyboardShortcut};
use muda::accelerator::{Accelerator, Code, Modifiers};
use muda::{Menu, MenuEvent, MenuId, PredefinedMenuItem, Submenu};
use objc2::{AnyThread, MainThreadMarker};
use objc2_app_kit::NSPrintInfo;
use objc2_foundation::{NSString, NSURL};
use objc2_pdf_kit::{PDFDocument, PDFPrintScalingMode};

use super::{PrintBackend, PrintOptions, PrintOutcome, PrinterInfo};
use crate::keymap::{Action, MenuItem, MenuSpec, shortcut};

/// Prints with the native macOS print panel.
pub struct MacPrint;

impl PrintBackend for MacPrint {
    fn name(&self) -> &'static str {
        "macOS print panel"
    }

    fn has_system_dialog(&self) -> bool {
        true
    }

    fn printers(&self) -> Vec<PrinterInfo> {
        Vec::new()
    }

    fn print_pdf(
        &self,
        pdf: &Path,
        job_name: &str,
        _options: &PrintOptions,
    ) -> Result<PrintOutcome, String> {
        let mtm = MainThreadMarker::new().ok_or("printing must run on the main thread")?;
        let url = NSURL::fileURLWithPath(&NSString::from_str(&pdf.display().to_string()));
        // SAFETY: PDFKit initialisers and print operations are called on the main
        // thread with valid, retained Objective-C objects.
        let doc = unsafe { PDFDocument::initWithURL(PDFDocument::alloc(), &url) }
            .ok_or("PDFKit could not open the exported PDF")?;
        let info = NSPrintInfo::sharedPrintInfo();
        // SAFETY: same main-thread and object-validity guarantees as above;
        // `info` is a live NSPrintInfo.
        let op = unsafe {
            doc.printOperationForPrintInfo_scalingMode_autoRotate(
                Some(&info),
                PDFPrintScalingMode::PageScaleNone,
                true,
                mtm,
            )
        }
        .ok_or("could not create a print operation")?;
        op.setJobTitle(Some(&NSString::from_str(job_name)));
        op.setShowsPrintPanel(true);
        op.setShowsProgressPanel(true);
        Ok(if op.runOperation() {
            PrintOutcome::Printed
        } else {
            PrintOutcome::Cancelled
        })
    }
}

fn code_for(key: Key) -> Option<Code> {
    Some(match key {
        Key::A => Code::KeyA,
        Key::B => Code::KeyB,
        Key::D => Code::KeyD,
        Key::E => Code::KeyE,
        Key::G => Code::KeyG,
        Key::I => Code::KeyI,
        Key::L => Code::KeyL,
        Key::N => Code::KeyN,
        Key::O => Code::KeyO,
        Key::P => Code::KeyP,
        Key::Q => Code::KeyQ,
        Key::S => Code::KeyS,
        Key::U => Code::KeyU,
        Key::W => Code::KeyW,
        Key::Z => Code::KeyZ,
        Key::Num0 => Code::Digit0,
        Key::Num1 => Code::Digit1,
        Key::Plus => Code::Equal,
        Key::Minus => Code::Minus,
        Key::OpenBracket => Code::BracketLeft,
        Key::CloseBracket => Code::BracketRight,
        _ => return None,
    })
}

fn accelerator(s: KeyboardShortcut) -> Option<Accelerator> {
    if !s.modifiers.command {
        return None;
    }
    let mut m = Modifiers::META;
    if s.modifiers.shift {
        m |= Modifiers::SHIFT;
    }
    if s.modifiers.alt {
        m |= Modifiers::ALT;
    }
    Some(Accelerator::new(m, code_for(s.logical_key)?))
}

/// The native menu bar. Items carry the same shortcuts as the keymap; when
/// it is installed the app does not handle those shortcuts itself.
pub struct NativeMenu {
    _menu: Menu,
    ids: HashMap<MenuId, Action>,
}

impl NativeMenu {
    /// Build and install the menu bar (call once the app is running).
    pub fn install(menus: &[MenuSpec]) -> Option<Self> {
        let menu = Menu::new();
        let mut ids = HashMap::new();
        let app = Submenu::new("pubrs", true);
        let _ = app.append_items(&[
            &PredefinedMenuItem::about(Some("About pubrs"), None),
            &PredefinedMenuItem::separator(),
            &PredefinedMenuItem::hide(None),
            &PredefinedMenuItem::hide_others(None),
            &PredefinedMenuItem::separator(),
            &PredefinedMenuItem::quit(None),
        ]);
        menu.append(&app).ok()?;
        fn fill(sub: &Submenu, items: &[MenuItem], ids: &mut HashMap<MenuId, Action>) {
            for item in items {
                match item {
                    MenuItem::Action(label, action) => {
                        if *action == Action::Quit {
                            continue;
                        }
                        let id = MenuId::new(format!("{action:?}"));
                        let accel = shortcut(*action).and_then(accelerator);
                        let mi = muda::MenuItem::with_id(id.clone(), label, true, accel);
                        let _ = sub.append(&mi);
                        ids.insert(id, *action);
                    }
                    MenuItem::Separator => {
                        let _ = sub.append(&PredefinedMenuItem::separator());
                    }
                    MenuItem::Submenu(title, children) => {
                        let s = Submenu::new(*title, true);
                        fill(&s, children, ids);
                        let _ = sub.append(&s);
                    }
                }
            }
        }
        for spec in menus {
            let sub = Submenu::new(spec.title, true);
            fill(&sub, spec.items, &mut ids);
            menu.append(&sub).ok()?;
        }
        menu.init_for_nsapp();
        tracing::info!("native macOS menu bar installed");
        Some(Self { _menu: menu, ids })
    }

    /// Actions chosen from the menu since the last call.
    pub fn poll(&self) -> Vec<Action> {
        let mut out = Vec::new();
        while let Ok(ev) = MenuEvent::receiver().try_recv() {
            if let Some(a) = self.ids.get(&ev.id) {
                out.push(*a);
            }
        }
        out
    }

    /// True if `action` is reachable through a native menu shortcut.
    pub fn handles(&self, action: Action) -> bool {
        self.ids.values().any(|a| *a == action)
    }
}
