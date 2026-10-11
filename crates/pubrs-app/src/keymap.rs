//! Every user action and its keyboard shortcut, in one table. Shortcuts use
//! `Modifiers::COMMAND`, which egui maps to Cmd on macOS and Ctrl elsewhere.
//! The menu bar (egui, or the native macOS menu) is built from this table.

use eframe::egui::{Key, KeyboardShortcut, Modifiers};

/// Things the user can do from menus, toolbars and shortcuts.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Action {
    NewFromTemplate,
    NewBlank,
    Open,
    Save,
    SaveAs,
    SaveAsTemplate,
    ExportPdf,
    SavePrintPackage,
    ExportPng,
    PageSetup,
    Print,
    CloseDocument,
    Quit,
    Undo,
    Redo,
    Cut,
    Copy,
    Paste,
    Delete,
    Duplicate,
    SelectAll,
    InsertText,
    InsertPicture,
    InsertRectangle,
    InsertEllipse,
    InsertLine,
    InsertPage,
    InsertColumnBreak,
    InsertFrameBreak,
    InsertPageBreak,
    DeletePage,
    BringToFront,
    BringForward,
    SendBackward,
    SendToBack,
    Group,
    Ungroup,
    AlignLeft,
    AlignCenter,
    AlignRight,
    AlignTop,
    AlignMiddle,
    AlignBottom,
    DistributeHorizontally,
    DistributeVertically,
    ToggleLock,
    Rotate90,
    LinkTextFrames,
    UnlinkTextFrames,
    Paragraph,
    DropCap,
    Tabs,
    BulletsNumbering,
    Bold,
    Italic,
    Underline,
    ZoomIn,
    ZoomOut,
    ZoomFit,
    ZoomActual,
    ToggleGuides,
    ToggleRulers,
    Find,
    FindNext,
    FindPrevious,
    Replace,
    ToggleSnap,
    Validate,
    /// Review → Spelling… (F7).
    Spelling,
    ColorScheme,
    ShowSwatches,
    NextPage,
    PreviousPage,
    About,
    McpHelp,
}

const fn cmd(key: Key) -> KeyboardShortcut {
    KeyboardShortcut::new(Modifiers::COMMAND, key)
}

const fn cmd_shift(key: Key) -> KeyboardShortcut {
    KeyboardShortcut::new(Modifiers::COMMAND.plus(Modifiers::SHIFT), key)
}

/// The keyboard shortcut for an action, if any.
pub fn shortcut(a: Action) -> Option<KeyboardShortcut> {
    use Action::*;
    Some(match a {
        NewFromTemplate => cmd(Key::N),
        NewBlank => cmd_shift(Key::N),
        Open => cmd(Key::O),
        Save => cmd(Key::S),
        SaveAs => cmd_shift(Key::S),
        ExportPdf => cmd(Key::E),
        Print => cmd(Key::P),
        PageSetup => cmd_shift(Key::P),
        CloseDocument => cmd(Key::W),
        Quit => cmd(Key::Q),
        Undo => cmd(Key::Z),
        Redo => cmd_shift(Key::Z),
        Duplicate => cmd(Key::D),
        SelectAll => cmd(Key::A),
        Find => cmd(Key::F),
        Replace => cmd(Key::H),
        FindNext => KeyboardShortcut::new(Modifiers::NONE, Key::F3),
        FindPrevious => KeyboardShortcut::new(Modifiers::SHIFT, Key::F3),
        Delete => KeyboardShortcut::new(Modifiers::NONE, Key::Delete),
        Group => cmd(Key::G),
        Ungroup => cmd_shift(Key::G),
        BringToFront => cmd_shift(Key::CloseBracket),
        BringForward => cmd(Key::CloseBracket),
        SendBackward => cmd(Key::OpenBracket),
        SendToBack => cmd_shift(Key::OpenBracket),
        ToggleLock => cmd(Key::L),
        Bold => cmd(Key::B),
        Italic => cmd(Key::I),
        Underline => cmd(Key::U),
        Paragraph => KeyboardShortcut::new(Modifiers::COMMAND.plus(Modifiers::ALT), Key::P),
        // As in Publisher and Word.
        InsertPageBreak => cmd(Key::Enter),
        InsertColumnBreak => cmd_shift(Key::Enter),
        ZoomIn => cmd(Key::Plus),
        ZoomOut => cmd(Key::Minus),
        ZoomFit => cmd(Key::Num0),
        ZoomActual => cmd(Key::Num1),
        NextPage => KeyboardShortcut::new(Modifiers::NONE, Key::PageDown),
        PreviousPage => KeyboardShortcut::new(Modifiers::NONE, Key::PageUp),
        Spelling => KeyboardShortcut::new(Modifiers::NONE, Key::F7),
        _ => return None,
    })
}

/// One entry in a menu.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MenuItem {
    Action(&'static str, Action),
    Separator,
    Submenu(&'static str, &'static [MenuItem]),
}

/// A top-level menu.
#[derive(Debug, Clone, Copy)]
pub struct MenuSpec {
    pub title: &'static str,
    pub items: &'static [MenuItem],
}

use MenuItem::{Action as A, Separator as Sep, Submenu as Sub};

/// The menu bar.
pub const MENUS: &[MenuSpec] = &[
    MenuSpec {
        title: "File",
        items: &[
            A("New from Template…", Action::NewFromTemplate),
            A("New Blank Publication", Action::NewBlank),
            A("Open…", Action::Open),
            Sep,
            A("Save", Action::Save),
            A("Save As…", Action::SaveAs),
            A("Save as Template…", Action::SaveAsTemplate),
            Sep,
            A("Export PDF…", Action::ExportPdf),
            A("Save for a Commercial Printer…", Action::SavePrintPackage),
            A("Export Page as PNG…", Action::ExportPng),
            A("Page Setup…", Action::PageSetup),
            A("Print…", Action::Print),
            Sep,
            A("Close", Action::CloseDocument),
            A("Quit", Action::Quit),
        ],
    },
    MenuSpec {
        title: "Edit",
        items: &[
            A("Undo", Action::Undo),
            A("Redo", Action::Redo),
            Sep,
            A("Cut", Action::Cut),
            A("Copy", Action::Copy),
            A("Paste", Action::Paste),
            A("Duplicate", Action::Duplicate),
            A("Delete", Action::Delete),
            Sep,
            A("Select All", Action::SelectAll),
            Sep,
            A("Find…", Action::Find),
            A("Find Next", Action::FindNext),
            A("Find Previous", Action::FindPrevious),
            A("Replace…", Action::Replace),
        ],
    },
    MenuSpec {
        title: "Insert",
        items: &[
            A("Text Box", Action::InsertText),
            A("Picture…", Action::InsertPicture),
            A("Rectangle", Action::InsertRectangle),
            A("Ellipse", Action::InsertEllipse),
            A("Line", Action::InsertLine),
            Sub(
                "Break",
                &[
                    A("Column Break", Action::InsertColumnBreak),
                    A("Frame Break", Action::InsertFrameBreak),
                    A("Page Break", Action::InsertPageBreak),
                ],
            ),
            Sep,
            A("Page", Action::InsertPage),
            A("Delete Page", Action::DeletePage),
        ],
    },
    MenuSpec {
        title: "Arrange",
        items: &[
            A("Bring to Front", Action::BringToFront),
            A("Bring Forward", Action::BringForward),
            A("Send Backward", Action::SendBackward),
            A("Send to Back", Action::SendToBack),
            Sep,
            A("Group", Action::Group),
            A("Ungroup", Action::Ungroup),
            Sub(
                "Align",
                &[
                    A("Left", Action::AlignLeft),
                    A("Center", Action::AlignCenter),
                    A("Right", Action::AlignRight),
                    A("Top", Action::AlignTop),
                    A("Middle", Action::AlignMiddle),
                    A("Bottom", Action::AlignBottom),
                ],
            ),
            Sub(
                "Distribute",
                &[
                    A("Horizontally", Action::DistributeHorizontally),
                    A("Vertically", Action::DistributeVertically),
                ],
            ),
            Sep,
            A("Rotate 90°", Action::Rotate90),
            A("Lock / Unlock", Action::ToggleLock),
        ],
    },
    MenuSpec {
        title: "Text",
        items: &[
            A("Bold", Action::Bold),
            A("Italic", Action::Italic),
            A("Underline", Action::Underline),
            Sep,
            A("Link to Next Text Box", Action::LinkTextFrames),
            A("Break Link", Action::UnlinkTextFrames),
            Sep,
            A("Paragraph…", Action::Paragraph),
            A("Drop Cap…", Action::DropCap),
            A("Tabs…", Action::Tabs),
            A("Bullets and Numbering…", Action::BulletsNumbering),
        ],
    },
    MenuSpec {
        title: "Design",
        items: &[
            A("Colour Scheme…", Action::ColorScheme),
            A("Swatches", Action::ShowSwatches),
        ],
    },
    MenuSpec {
        title: "View",
        items: &[
            A("Zoom In", Action::ZoomIn),
            A("Zoom Out", Action::ZoomOut),
            A("Fit Page", Action::ZoomFit),
            A("Actual Size", Action::ZoomActual),
            Sep,
            A("Next Page", Action::NextPage),
            A("Previous Page", Action::PreviousPage),
            Sep,
            A("Show Rulers", Action::ToggleRulers),
            A("Show Guides", Action::ToggleGuides),
            A("Snap to Guides", Action::ToggleSnap),
            Sep,
            A("Check for Problems…", Action::Validate),
        ],
    },
    MenuSpec {
        title: "Review",
        items: &[A("Spelling…", Action::Spelling)],
    },
    MenuSpec {
        title: "Help",
        items: &[
            A("Use with AI Assistants (MCP)…", Action::McpHelp),
            A("About pubrs", Action::About),
        ],
    },
];

/// Every action that has a shortcut, checking Cmd+Shift combinations before
/// plain Cmd ones (egui's matching ignores extra Shift).
pub fn shortcut_actions() -> Vec<(KeyboardShortcut, Action)> {
    let mut all = Vec::new();
    fn walk(items: &[MenuItem], out: &mut Vec<Action>) {
        for i in items {
            match i {
                MenuItem::Action(_, a) => out.push(*a),
                MenuItem::Submenu(_, sub) => walk(sub, out),
                MenuItem::Separator => {}
            }
        }
    }
    for m in MENUS {
        walk(m.items, &mut all);
    }
    let mut seen = std::collections::HashSet::new();
    all.retain(|a| seen.insert(*a));
    let mut v: Vec<(KeyboardShortcut, Action)> = all
        .into_iter()
        .filter_map(|a| shortcut(a).map(|s| (s, a)))
        .collect();
    v.sort_by_key(|(s, _)| std::cmp::Reverse((s.modifiers.shift, s.modifiers.alt)));
    v
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shortcuts_are_unique() {
        let v = shortcut_actions();
        for (i, (s, a)) in v.iter().enumerate() {
            for (t, b) in &v[i + 1..] {
                assert!(s != t, "{a:?} and {b:?} share a shortcut");
            }
        }
        assert!(
            v.iter().position(|(_, a)| *a == Action::SaveAs)
                < v.iter().position(|(_, a)| *a == Action::Save)
        );
    }
}
