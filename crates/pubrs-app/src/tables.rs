//! Tables in the GUI (M20): the cell range selection, hit testing of cells
//! and borders from the cached table layout, Tab between cells, the context
//! menu and the commands the menu and the properties panel issue.
//!
//! Everything here reads the table layout the canvas already has
//! (`DocumentLayout::table`), so hovering a border or testing a click costs
//! the cells of one table, and nothing on the keystroke path looks at the
//! whole document.

use eframe::egui::{self, Pos2};
use pubrs_api::core::table::{Axis, CellStyle, TableFrame};
use pubrs_api::core::{Command, Document, FrameId, Point, Rect, StoryId};
use pubrs_api::layout::DocumentLayout;

use crate::app::PubrsApp;
use crate::textedit::{self, TextEdit};
use crate::view::PageXf;

/// The smallest a dragged column or row gets, in points.
pub const MIN_TRACK: f64 = 6.0;
/// Row height of a table inserted from the dialog, in points.
pub const ROW_HEIGHT: f64 = 24.0;
/// How close (screen pixels) the pointer must be to a border to grab it.
pub const BORDER_PX: f32 = 4.0;

/// A rectangle of cells, normalised (`r0 <= r1`, `c0 <= c1`) and extended to
/// cover every merged cell it touches.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TableSel {
    pub frame: FrameId,
    pub r0: u32,
    pub c0: u32,
    pub r1: u32,
    pub c1: u32,
}

impl TableSel {
    pub fn cells(&self) -> usize {
        ((self.r1 - self.r0 + 1) * (self.c1 - self.c0 + 1)) as usize
    }
}

pub fn table(doc: &Document, frame: FrameId) -> Option<&TableFrame> {
    doc.frame(frame)?.as_table()
}

/// The rectangle between two corner cells, grown until it cuts no merged cell.
pub fn range_between(t: &TableFrame, frame: FrameId, a: (u32, u32), b: (u32, u32)) -> TableSel {
    let (mut r0, mut r1) = (a.0.min(b.0), a.0.max(b.0));
    let (mut c0, mut c1) = (a.1.min(b.1), a.1.max(b.1));
    loop {
        let mut grown = false;
        for c in &t.cells {
            let (cr1, cc1) = (c.row + c.row_span - 1, c.col + c.col_span - 1);
            if c.row <= r1 && cr1 >= r0 && c.col <= c1 && cc1 >= c0 {
                if c.row < r0 {
                    r0 = c.row;
                    grown = true;
                }
                if cr1 > r1 {
                    r1 = cr1;
                    grown = true;
                }
                if c.col < c0 {
                    c0 = c.col;
                    grown = true;
                }
                if cc1 > c1 {
                    c1 = cc1;
                    grown = true;
                }
            }
        }
        if !grown {
            break;
        }
    }
    TableSel {
        frame,
        r0,
        c0,
        r1,
        c1,
    }
}

/// The cell range selected on the canvas, if it is still valid (the table is
/// selected, nothing is being edited and the range is inside the grid).
pub fn live_sel(app: &PubrsApp, doc: &Document) -> Option<TableSel> {
    let v = app.view()?;
    let s = v.table_sel?;
    if v.edit.is_some() || v.selection != [s.frame] {
        return None;
    }
    let t = table(doc, s.frame)?;
    (s.r1 < t.rows.len() as u32 && s.c1 < t.cols.len() as u32).then_some(s)
}

/// What the properties panel and the context menu act on.
#[derive(Debug, Clone, Copy)]
pub struct Ctx {
    pub frame: FrameId,
    pub range: TableSel,
    /// The cell the column width and row height fields show.
    pub cur: (u32, u32),
    /// Nothing narrower than the whole table is selected.
    pub whole: bool,
}

/// The cells `frame` (a selected table) offers to edit: the cell being
/// edited, else the selected range, else every cell.
pub fn context(app: &PubrsApp, doc: &Document, frame: FrameId) -> Option<Ctx> {
    let t = table(doc, frame)?;
    if let Some(e) = app.view().and_then(|v| v.edit.as_ref())
        && e.frame == frame
        && let Some(c) = t.cell_of_story(e.story)
    {
        return Some(Ctx {
            frame,
            range: range_between(t, frame, (c.row, c.col), (c.row, c.col)),
            cur: (c.row, c.col),
            whole: false,
        });
    }
    if let Some(s) = live_sel(app, doc).filter(|s| s.frame == frame) {
        return Some(Ctx {
            frame,
            range: s,
            cur: (s.r0, s.c0),
            whole: false,
        });
    }
    Some(Ctx {
        frame,
        range: TableSel {
            frame,
            r0: 0,
            c0: 0,
            r1: t.rows.len() as u32 - 1,
            c1: t.cols.len() as u32 - 1,
        },
        cur: (0, 0),
        whole: true,
    })
}

// ------------------------------------------------------------------ hit testing

/// A cell under a point.
#[derive(Debug, Clone, Copy)]
pub struct Hit {
    pub row: u32,
    pub col: u32,
    pub story: StoryId,
}

/// The cell of `frame` under page point `p`.
pub fn hit_cell(doc: &Document, layout: &DocumentLayout, frame: FrameId, p: Point) -> Option<Hit> {
    let (x, y) = textedit::page_to_frame(doc, frame, p)?;
    let t = table(doc, frame)?;
    let tl = layout.table(frame)?;
    t.cells
        .iter()
        .zip(&tl.cells)
        .find(|(_, cl)| cl.rect.contains(Point::new(x, y)))
        .map(|(c, _)| Hit {
            row: c.row,
            col: c.col,
            story: c.story,
        })
}

/// The grid position nearest to `p` (inside the visible rows), for drags
/// that leave the table.
pub fn nearest_cell(
    doc: &Document,
    layout: &DocumentLayout,
    frame: FrameId,
    p: Point,
) -> Option<(u32, u32)> {
    let (x, y) = textedit::page_to_frame(doc, frame, p)?;
    let t = table(doc, frame)?;
    let tl = layout.table(frame)?;
    let mut col = t.cols.len() - 1;
    let mut acc = 0.0;
    for (i, w) in t.cols.iter().enumerate() {
        acc += w;
        if x < acc {
            col = i;
            break;
        }
    }
    let visible: Vec<usize> = (0..t.rows.len())
        .filter(|i| tl.row_heights[*i] > 0.0)
        .collect();
    let mut row = *visible.last()?;
    for i in &visible {
        if y < tl.row_tops[*i] + tl.row_heights[*i] {
            row = *i;
            break;
        }
    }
    Some((row as u32, col as u32))
}

/// A border the pointer can drag.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Border {
    /// The boundary right of column `col`; `x0` is the column's left edge.
    Col { col: u32, x0: f64 },
    /// The bottom of row `row`; `top` is the row's top edge.
    Row { row: u32, top: f64 },
}

/// The border of `frame` within `tol` page points of `p`. Edges inside a
/// merged cell are not borders.
pub fn border_at(
    doc: &Document,
    layout: &DocumentLayout,
    frame: FrameId,
    p: Point,
    tol: f64,
) -> Option<Border> {
    let (x, y) = textedit::page_to_frame(doc, frame, p)?;
    let t = table(doc, frame)?;
    let tl = layout.table(frame)?;
    let bottom = tl
        .row_tops
        .iter()
        .zip(&tl.row_heights)
        .map(|(a, h)| a + h)
        .fold(0.0_f64, f64::max);
    let width = t.width();
    let row_at = |y: f64| -> Option<usize> {
        (0..t.rows.len())
            .find(|i| tl.row_heights[*i] > 0.0 && y <= tl.row_tops[*i] + tl.row_heights[*i] + 1e-9)
    };
    let col_at = |x: f64| -> usize {
        let mut acc = 0.0;
        for (i, w) in t.cols.iter().enumerate() {
            acc += w;
            if x < acc {
                return i;
            }
        }
        t.cols.len() - 1
    };
    let same = |a: Option<&pubrs_api::core::table::Cell>,
                b: Option<&pubrs_api::core::table::Cell>| {
        a.map(|c| (c.row, c.col)) == b.map(|c| (c.row, c.col))
    };
    let mut best: Option<(f64, Border)> = None;
    if y >= -tol && y <= bottom + tol {
        let r = row_at(y.clamp(0.0, bottom)).unwrap_or(0);
        let mut bx = 0.0;
        for k in 1..t.cols.len() {
            bx += t.cols[k - 1];
            let d = (x - bx).abs();
            if d <= tol
                && !same(
                    t.cell_at(r as u32, (k - 1) as u32),
                    t.cell_at(r as u32, k as u32),
                )
                && best.is_none_or(|(b, _)| d < b)
            {
                best = Some((
                    d,
                    Border::Col {
                        col: (k - 1) as u32,
                        x0: bx - t.cols[k - 1],
                    },
                ));
            }
        }
    }
    if x >= -tol && x <= width + tol {
        let c = col_at(x.clamp(0.0, width));
        for i in 0..t.rows.len() {
            if tl.row_heights[i] <= 0.0 {
                continue;
            }
            let by = tl.row_tops[i] + tl.row_heights[i];
            let d = (y - by).abs();
            let last = i + 1 == t.rows.len();
            if d <= tol
                && (last
                    || !same(
                        t.cell_at(i as u32, c as u32),
                        t.cell_at(i as u32 + 1, c as u32),
                    ))
                && best.is_none_or(|(b, _)| d < b)
            {
                best = Some((
                    d,
                    Border::Row {
                        row: i as u32,
                        top: tl.row_tops[i],
                    },
                ));
            }
        }
    }
    best.map(|(_, b)| b)
}

/// The table whose borders can be dragged now: the one selected alone or
/// being edited.
pub fn active_table(app: &PubrsApp, doc: &Document) -> Option<FrameId> {
    let v = app.view()?;
    let f = match (&v.edit, v.selection.as_slice()) {
        (Some(e), _) => e.frame,
        (None, [f]) => *f,
        _ => return None,
    };
    let fr = doc.frame(f)?;
    (fr.as_table().is_some() && !fr.locked).then_some(f)
}

/// Frame-local point to screen.
pub fn local_to_screen(doc: &Document, xf: PageXf, frame: FrameId, x: f64, y: f64) -> Option<Pos2> {
    textedit::frame_to_page(doc, frame, x, y).map(|p| xf.to_screen(p))
}

/// The four screen corners of the cell range (for the overlay).
pub fn range_corners(
    doc: &Document,
    layout: &DocumentLayout,
    xf: PageXf,
    s: &TableSel,
) -> Option<[Pos2; 4]> {
    let t = table(doc, s.frame)?;
    let tl = layout.table(s.frame)?;
    let x0: f64 = t.cols[..s.c0 as usize].iter().sum();
    let x1: f64 = t.cols[..=s.c1 as usize].iter().sum();
    let y0 = tl.row_tops[s.r0 as usize];
    let y1 = tl.row_tops[s.r1 as usize] + tl.row_heights[s.r1 as usize];
    let at = |x, y| local_to_screen(doc, xf, s.frame, x, y);
    Some([at(x0, y0)?, at(x1, y0)?, at(x1, y1)?, at(x0, y1)?])
}

// ------------------------------------------------------------------ commands

/// Run a structural command as one undo step and let the frame follow the
/// rows: a table never grows its frame by itself, and rows past the frame's
/// bottom have no height. The growth joins the command's undo step.
pub fn run(app: &mut PubrsApp, frame: FrameId, cmd: Command) -> bool {
    app.table_serial += 1;
    let key = format!("table-edit-{}", app.table_serial);
    let ok = app.exec_merge(cmd, &key);
    if ok {
        grow(app, frame, &key);
    }
    ok
}

/// Make the frame at least as tall as its rows (merging into the undo step
/// `key` named).
pub fn grow(app: &mut PubrsApp, frame: FrameId, key: &str) -> bool {
    let Some(id) = app.doc_id() else { return false };
    let Ok(layout) = app.session.layout(Some(id)) else {
        return false;
    };
    let Some(f) = app.doc().and_then(|d| d.frame(frame)) else {
        return false;
    };
    let Some(tl) = layout.table(frame) else {
        return false;
    };
    if tl.natural_height <= f.rect.height + 0.01 {
        return false;
    }
    let r = f.rect;
    app.exec_merge(
        Command::SetFrameRect {
            frame,
            rect: Rect::new(r.x, r.y, r.width, tl.natural_height),
        },
        key,
    )
}

/// Every story a command edits, if it is one of the text commands (a batch
/// lists the stories of all its commands, once each, in order).
pub fn stories_of_command(cmd: &Command) -> Vec<StoryId> {
    fn walk(cmd: &Command, out: &mut Vec<StoryId>) {
        match cmd {
            Command::SetStory { story, .. }
            | Command::InsertText { story, .. }
            | Command::DeleteText { story, .. }
            | Command::ReplaceText { story, .. }
            | Command::ApplyCharProps { story, .. }
            | Command::SetCharStyle { story, .. }
            | Command::ClearCharFormat { story, .. }
            | Command::ApplyParaProps { story, .. }
            | Command::SetParaStyle { story, .. } => {
                if !out.contains(story) {
                    out.push(*story);
                }
            }
            Command::Batch { commands, .. } => {
                for c in commands {
                    walk(c, out);
                }
            }
            _ => {}
        }
    }
    let mut out = Vec::new();
    walk(cmd, &mut out);
    out
}

/// Set a column's width (one undo step).
pub fn set_col_width(app: &mut PubrsApp, frame: FrameId, col: u32, width: f64) -> bool {
    run(
        app,
        frame,
        Command::TableSetColWidth {
            frame,
            col,
            width: width.max(MIN_TRACK),
        },
    )
}

/// Set a row's minimum height (one undo step).
pub fn set_row_height(app: &mut PubrsApp, frame: FrameId, row: u32, height: f64) -> bool {
    run(
        app,
        frame,
        Command::TableSetRowHeight {
            frame,
            row,
            min_height: height.max(MIN_TRACK),
        },
    )
}

/// Change the style of every cell in `ctx`'s range with `edit`, as one
/// undo step that merges with the previous edit of the same control `what`.
pub fn set_style(
    app: &mut PubrsApp,
    t: &TableFrame,
    ctx: &Ctx,
    what: &str,
    edit: impl Fn(&mut CellStyle),
) {
    let r = ctx.range;
    let mut commands: Vec<Command> = t
        .cells
        .iter()
        .filter(|c| c.row >= r.r0 && c.row <= r.r1 && c.col >= r.c0 && c.col <= r.c1)
        .map(|c| {
            let mut style = c.style();
            edit(&mut style);
            Command::TableSetCellStyle {
                frame: ctx.frame,
                row: c.row,
                col: c.col,
                style,
            }
        })
        .collect();
    let cmd = if commands.len() == 1 {
        commands.remove(0)
    } else {
        Command::Batch {
            label: "table_set_cell_style".into(),
            commands,
        }
    };
    let key = format!(
        "prop-cell-{what}-{}-{}-{}-{}-{}",
        ctx.frame, r.r0, r.c0, r.r1, r.c1
    );
    if app.exec_merge(cmd, &key) {
        grow(app, ctx.frame, &key);
    }
}

fn new_ids(app: &mut PubrsApp, n: usize) -> Vec<StoryId> {
    (0..n).map(|_| app.session.new_story_id()).collect()
}

/// Insert `count` rows or columns at `at` (stories allocated up front).
pub fn insert_lines(app: &mut PubrsApp, frame: FrameId, axis: Axis, at: u32, count: u32) -> bool {
    let Some(t) = app.doc().and_then(|d| table(d, frame)).cloned() else {
        return false;
    };
    let needed = match t.insert_needed(axis, at as usize, count as usize) {
        Ok(n) => n,
        Err(e) => {
            app.notify(e);
            return false;
        }
    };
    let stories = new_ids(app, needed);
    let cmd = match axis {
        Axis::Row => Command::TableInsertRows {
            frame,
            at,
            count,
            stories,
        },
        Axis::Col => Command::TableInsertCols {
            frame,
            at,
            count,
            stories,
        },
    };
    run(app, frame, cmd)
}

/// What the context menu offers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TableAct {
    RowAbove,
    RowBelow,
    ColLeft,
    ColRight,
    DeleteRows,
    DeleteCols,
    Merge,
    Split,
    CellProps,
}

/// An open table context menu.
#[derive(Debug, Clone)]
pub struct TableMenu {
    pub frame: FrameId,
    pub range: TableSel,
    pub anchor: Pos2,
    pub can_merge: bool,
    pub can_split: bool,
    /// The document revision the menu was opened at; it closes when that changes.
    pub rev: u64,
}

impl TableMenu {
    /// The menu for the cell `(row, col)` (or the selected range holding it).
    pub fn new(
        app: &PubrsApp,
        doc: &Document,
        frame: FrameId,
        hit: (u32, u32),
        anchor: Pos2,
    ) -> Option<Self> {
        let t = table(doc, frame)?;
        let range = live_sel(app, doc)
            .filter(|s| {
                s.frame == frame && (s.r0..=s.r1).contains(&hit.0) && (s.c0..=s.c1).contains(&hit.1)
            })
            .unwrap_or_else(|| range_between(t, frame, hit, hit));
        let first = t.cell_at(range.r0, range.c0)?;
        let rev = app
            .doc_id()
            .and_then(|id| app.session.get(Some(id)).ok())
            .map_or(0, |o| o.revision());
        let inside = t
            .cells
            .iter()
            .filter(|c| {
                c.row >= range.r0 && c.row <= range.r1 && c.col >= range.c0 && c.col <= range.c1
            })
            .count();
        Some(Self {
            frame,
            range,
            anchor,
            can_merge: inside > 1,
            can_split: first.row_span * first.col_span > 1,
            rev,
        })
    }
}

/// The menu items; returns the one clicked. Shared by the table's own menu
/// and the spelling menu (which lists these after its own items).
pub fn items_ui(ui: &mut egui::Ui, m: &TableMenu) -> Option<TableAct> {
    let mut act = None;
    let mut item = |ui: &mut egui::Ui, name: &str, label: &str, on: bool, a: TableAct| {
        let r = ui.add_enabled(on, egui::Button::new(label));
        crate::dialogs::mark(ui, name, &r);
        if r.clicked() {
            act = Some(a);
        }
    };
    let (rows, cols) = (m.range.r1 - m.range.r0 + 1, m.range.c1 - m.range.c0 + 1);
    item(
        ui,
        "table-row-above",
        "Insert Row Above",
        true,
        TableAct::RowAbove,
    );
    item(
        ui,
        "table-row-below",
        "Insert Row Below",
        true,
        TableAct::RowBelow,
    );
    item(
        ui,
        "table-col-left",
        "Insert Column Left",
        true,
        TableAct::ColLeft,
    );
    item(
        ui,
        "table-col-right",
        "Insert Column Right",
        true,
        TableAct::ColRight,
    );
    ui.separator();
    item(
        ui,
        "table-delete-rows",
        if rows > 1 {
            "Delete Rows"
        } else {
            "Delete Row"
        },
        true,
        TableAct::DeleteRows,
    );
    item(
        ui,
        "table-delete-cols",
        if cols > 1 {
            "Delete Columns"
        } else {
            "Delete Column"
        },
        true,
        TableAct::DeleteCols,
    );
    ui.separator();
    item(
        ui,
        "table-merge",
        "Merge Cells",
        m.can_merge,
        TableAct::Merge,
    );
    item(
        ui,
        "table-split",
        "Split Cell",
        m.can_split,
        TableAct::Split,
    );
    ui.separator();
    item(
        ui,
        "table-cell-props",
        "Cell Properties…",
        true,
        TableAct::CellProps,
    );
    act
}

/// Show the table's own context menu while it is open (the spelling menu
/// carries the items itself when it is open).
pub fn context_menu(app: &mut PubrsApp, ui: &egui::Ui) {
    let Some(menu) = app.table_menu.clone() else {
        return;
    };
    if app.spell.menu.is_some() {
        return;
    }
    // Undo, redo or any other edit while the menu is open closes it.
    if menu_stale(app, &menu) {
        app.table_menu = None;
        return;
    }
    let mut open = true;
    let mut act = None;
    egui::Popup::new(
        egui::Id::new("table-menu"),
        ui.ctx().clone(),
        menu.anchor,
        ui.layer_id(),
    )
    .kind(egui::PopupKind::Menu)
    .layout(egui::Layout::top_down_justified(egui::Align::Min))
    .close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside)
    .open_bool(&mut open)
    .show(|ui| {
        ui.set_min_width(170.0);
        act = items_ui(ui, &menu);
    });
    if let Some(a) = act {
        apply(app, &menu, a);
        open = false;
    }
    if !open {
        app.table_menu = None;
    }
}

/// Whether the document changed (or the table went) since the menu opened.
fn menu_stale(app: &PubrsApp, m: &TableMenu) -> bool {
    let rev = app
        .doc_id()
        .and_then(|id| app.session.get(Some(id)).ok())
        .map_or(0, |o| o.revision());
    rev != m.rev
}

/// Carry out a menu item. Every item is one undo step. The frame and range
/// are checked against the current document first.
pub fn apply(app: &mut PubrsApp, m: &TableMenu, act: TableAct) {
    let (frame, r) = (m.frame, m.range);
    let valid = app
        .doc()
        .and_then(|d| table(d, frame))
        .is_some_and(|t| r.r1 < t.rows.len() as u32 && r.c1 < t.cols.len() as u32);
    if !valid {
        app.notify("The table changed: right-click again");
        return;
    }
    let ok = match act {
        TableAct::RowAbove => insert_lines(app, frame, Axis::Row, r.r0, 1),
        TableAct::RowBelow => insert_lines(app, frame, Axis::Row, r.r1 + 1, 1),
        TableAct::ColLeft => insert_lines(app, frame, Axis::Col, r.c0, 1),
        TableAct::ColRight => insert_lines(app, frame, Axis::Col, r.c1 + 1, 1),
        TableAct::DeleteRows => run(
            app,
            frame,
            Command::TableDeleteRows {
                frame,
                at: r.r0,
                count: r.r1 - r.r0 + 1,
            },
        ),
        TableAct::DeleteCols => run(
            app,
            frame,
            Command::TableDeleteCols {
                frame,
                at: r.c0,
                count: r.c1 - r.c0 + 1,
            },
        ),
        TableAct::Merge => {
            let ok = run(
                app,
                frame,
                Command::TableMergeCells {
                    frame,
                    row: r.r0,
                    col: r.c0,
                    row_span: r.r1 - r.r0 + 1,
                    col_span: r.c1 - r.c0 + 1,
                },
            );
            if ok {
                app.select(vec![frame]);
                if let Some(v) = app.view_mut() {
                    v.table_sel = Some(r);
                }
            }
            return;
        }
        TableAct::Split => {
            let Some(t) = app.doc().and_then(|d| table(d, frame)).cloned() else {
                return;
            };
            let Some(n) = t.split_needed(r.r0, r.c0) else {
                app.notify("The cell is not merged");
                return;
            };
            let stories = new_ids(app, n);
            run(
                app,
                frame,
                Command::TableSplitCell {
                    frame,
                    row: r.r0,
                    col: r.c0,
                    stories,
                },
            )
        }
        TableAct::CellProps => {
            app.focus_cell_props = true;
            return;
        }
    };
    // Rows and columns moved: the old range and the edited cell may be gone.
    // A refused command leaves the edit and the range as they were.
    if ok {
        app.select(vec![frame]);
    }
}

// ------------------------------------------------------------------ keyboard

/// Tab or Shift+Tab in a cell: the next or previous cell in row-major order
/// (merged-over positions have no cell of their own), the caret at the end
/// of its text. Tab in the last cell adds a row and edits its first cell.
pub fn tab_cell(app: &mut PubrsApp, doc: &Document, e: &TextEdit, forward: bool) -> bool {
    let Some(t) = table(doc, e.frame) else {
        return false;
    };
    let Some(i) = t.cells.iter().position(|c| c.story == e.story) else {
        return false;
    };
    let frame = e.frame;
    let target = if forward {
        if i + 1 < t.cells.len() {
            Some(i + 1)
        } else {
            None
        }
    } else {
        i.checked_sub(1)
    };
    let story = match target {
        Some(j) => t.cells[j].story,
        None if forward => {
            let at = t.rows.len() as u32;
            if !insert_lines(app, frame, Axis::Row, at, 1) {
                return true;
            }
            let Some(s) = app
                .doc()
                .and_then(|d| table(d, frame))
                .and_then(|t| t.cell_at(at, 0))
                .map(|c| c.story)
            else {
                return true;
            };
            crate::canvas::begin_edit(app, frame, s, 0);
            return true;
        }
        None => return true,
    };
    let end = app
        .doc()
        .and_then(|d| d.story(story))
        .map_or(0, |s| s.char_len());
    crate::canvas::begin_edit(app, frame, story, end);
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    use pubrs_api::core::SeqIds;
    use pubrs_api::core::table::new_table;

    fn grid() -> (FrameId, TableFrame) {
        let mut ids = SeqIds::new(1);
        let (f, _) = new_table(
            &mut ids,
            Rect::new(0.0, 0.0, 300.0, 120.0),
            4,
            4,
            &CellStyle::default(),
        )
        .expect("table");
        let id = f.id;
        (id, f.as_table().expect("table").clone())
    }

    #[test]
    fn range_is_normalised_and_covers_merged_cells() {
        let (id, mut t) = grid();
        // Reversed corners give the same rectangle.
        let a = range_between(&t, id, (2, 3), (1, 1));
        assert_eq!((a.r0, a.c0, a.r1, a.c1), (1, 1, 2, 3));
        // Merge rows 1-2, columns 0-1; a range touching one corner takes all of it.
        t.merge(1, 0, 2, 2).expect("merge");
        let b = range_between(&t, id, (0, 1), (1, 2));
        assert_eq!((b.r0, b.c0, b.r1, b.c1), (0, 0, 2, 2));
        let single = range_between(&t, id, (2, 1), (2, 1));
        assert_eq!((single.r0, single.c0, single.r1, single.c1), (1, 0, 2, 1));
        assert_eq!(single.cells(), 4);
    }
}
