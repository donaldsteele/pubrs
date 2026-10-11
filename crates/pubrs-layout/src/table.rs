//! Table layout (ADR 0024).
//!
//! Every cell is an ordinary story with one region, in cell-local space: the
//! cell's width minus its insets, as tall as [`CELL_FLOW_HEIGHT`], so the
//! flow measures the text without knowing where the cell sits. A cell's
//! story layout therefore depends only on its own text and the widths and
//! insets of its own cell, and a keystroke recomposes one cell.
//!
//! [`table_pass`] then works out the table: row heights (the tallest single
//! row cell, at least the row's minimum, a spanning cell's extra height going
//! to its last row), clamped to the frame's bottom; the cell rectangles in
//! table space; each cell's vertical alignment; and which cells overflow.
//! Lines of an overflowing cell that do not fit above the frame's bottom are
//! dropped and reported through the cell story's `overflow`.

use std::collections::BTreeMap;
use std::sync::Arc;

use pubrs_core::{Cell, Document, Frame, FrameId, Rect, StoryId, TableFrame, VerticalAlign};

use crate::flow::Region;
use crate::types::{LineBox, Overflow, StoryLayout};

/// Height of a cell's flow region: tall enough to measure any realistic cell.
pub(crate) const CELL_FLOW_HEIGHT: f64 = 20_000.0;

/// Slack when comparing a line's bottom with the space left.
const EPS: f64 = 1e-6;

/// The region a cell's story flows into.
pub(crate) fn cell_region(frame: FrameId, table: &TableFrame, cell: &Cell) -> Region {
    let (c0, c1) = (cell.col as usize, (cell.col + cell.col_span) as usize);
    let width: f64 = table.cols.get(c0..c1).map_or(0.0, |s| s.iter().sum());
    Region {
        frame,
        x: cell.insets.left,
        y: cell.insets.top,
        w: (width - cell.insets.left - cell.insets.right).max(1.0),
        h: CELL_FLOW_HEIGHT,
        valign: VerticalAlign::Top,
        obstacles: Vec::new(),
        page: None,
    }
}

/// Where one cell ended up.
#[derive(Debug, Clone, PartialEq)]
pub struct CellLayout {
    pub story: StoryId,
    /// The cell's outer rectangle in table-local space, clipped to the
    /// frame (zero height for a cell below the frame's bottom).
    pub rect: Rect,
    /// Extra offset down applied to the cell's lines for its vertical
    /// alignment (cell-local lines are laid out from the top inset).
    pub dy: f64,
    /// Some of the cell's text does not fit.
    pub overflow: bool,
}

/// The table pass of one table frame.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct TableLayout {
    /// Top of each row in table-local space (clipped to the frame).
    pub row_tops: Vec<f64>,
    /// Height of each row (clipped to the frame).
    pub row_heights: Vec<f64>,
    /// One per cell, in the order of `TableFrame::cells`.
    pub cells: Vec<CellLayout>,
    /// Cells with hidden text, in cell order.
    pub overflowing_cells: Vec<StoryId>,
    /// Height the rows occupy: their natural total, ignoring the frame.
    pub natural_height: f64,
}

impl TableLayout {
    pub fn cell(&self, story: StoryId) -> Option<&CellLayout> {
        self.cells.iter().find(|c| c.story == story)
    }

    /// Table-local position of the cell's lines (cell-local origin plus the
    /// vertical alignment shift).
    pub fn origin(&self, story: StoryId) -> Option<(f64, f64)> {
        self.cell(story).map(|c| (c.rect.x, c.rect.y + c.dy))
    }
}

fn line_bottom(l: &LineBox) -> f64 {
    let cap = l.runs.first().and_then(|r| r.glyphs.first());
    cap.map_or(l.rect.bottom(), |g| l.rect.bottom().max(g.y))
}

/// Bottom of the text of a cell layout, cell-local; `None` when it has none.
fn content_bottom(layout: &StoryLayout, frame: FrameId) -> Option<f64> {
    layout
        .frames
        .get(&frame)?
        .lines
        .iter()
        .map(line_bottom)
        .fold(None, |a: Option<f64>, b| Some(a.map_or(b, |a| a.max(b))))
}

/// Run the table pass for `frame` over the raw story layouts of its cells.
/// Returns the table layout and, for each cell that overflows, its story
/// layout with the lines that do not fit removed and `overflow` set.
pub(crate) fn table_pass(
    doc: &Document,
    frame: &Frame,
    stories: &BTreeMap<StoryId, Arc<StoryLayout>>,
) -> (TableLayout, Vec<(StoryId, Arc<StoryLayout>)>) {
    let Some(table) = frame.as_table() else {
        return (TableLayout::default(), Vec::new());
    };
    let raw: Vec<Option<&Arc<StoryLayout>>> =
        table.cells.iter().map(|c| stories.get(&c.story)).collect();
    pass(doc, frame, table, &raw)
}

pub(crate) fn pass(
    doc: &Document,
    frame: &Frame,
    table: &TableFrame,
    raw: &[Option<&Arc<StoryLayout>>],
) -> (TableLayout, Vec<(StoryId, Arc<StoryLayout>)>) {
    let bottoms: Vec<f64> = table
        .cells
        .iter()
        .zip(raw)
        .map(|(c, l)| {
            l.and_then(|l| content_bottom(l, frame.id))
                .unwrap_or(c.insets.top)
        })
        .collect();
    // Natural row heights.
    let mut heights: Vec<f64> = table.rows.iter().map(|r| r.min_height).collect();
    let mut order: Vec<usize> = (0..table.cells.len()).collect();
    order.sort_by_key(|i| {
        let c = &table.cells[*i];
        (c.row_span > 1, c.last_row(), c.row)
    });
    for i in order {
        let c = &table.cells[i];
        let needed = bottoms[i] + c.insets.bottom;
        let (r0, r1) = (c.row as usize, c.last_row() as usize);
        let have: f64 = heights[r0..=r1].iter().sum();
        if needed > have {
            heights[r1] += needed - have;
        }
    }
    let natural: f64 = heights.iter().sum();
    // Clip to the frame.
    let limit = frame.rect.height.max(0.0);
    let mut row_tops = Vec::with_capacity(heights.len());
    let mut row_heights = Vec::with_capacity(heights.len());
    let mut acc = 0.0_f64;
    for h in &heights {
        let top = acc.min(limit);
        let bottom = (acc + h).min(limit);
        row_tops.push(top);
        row_heights.push(bottom - top);
        acc += h;
    }
    let mut x_of = Vec::with_capacity(table.cols.len());
    let mut x = 0.0;
    for w in &table.cols {
        x_of.push(x);
        x += w;
    }
    let mut cells = Vec::with_capacity(table.cells.len());
    let mut adjusted = Vec::new();
    let mut overflowing = Vec::new();
    for (i, c) in table.cells.iter().enumerate() {
        let (r0, r1) = (c.row as usize, c.last_row() as usize);
        let (c0, c1) = (c.col as usize, (c.col + c.col_span) as usize);
        let width: f64 = table.cols[c0..c1].iter().sum();
        let top = row_tops[r0];
        let height = row_tops[r1] + row_heights[r1] - top;
        let rect = Rect::new(x_of[c0], top, width, height);
        let room = height - c.insets.bottom;
        let mut layout = CellLayout {
            story: c.story,
            rect,
            dy: 0.0,
            overflow: false,
        };
        if let Some(l) = raw[i] {
            let fits = content_bottom(l, frame.id).is_none_or(|b| b <= room + EPS);
            if fits {
                let factor = match c.valign {
                    VerticalAlign::Top => 0.0,
                    VerticalAlign::Middle => 0.5,
                    VerticalAlign::Bottom => 1.0,
                };
                let used = content_bottom(l, frame.id).unwrap_or(c.insets.top);
                layout.dy = (room - used).max(0.0) * factor;
                // The flow itself can report overflow (a break character).
                if l.overflow.is_some() {
                    layout.overflow = true;
                    overflowing.push(c.story);
                }
            } else if let Some(cut) = truncate(doc, l, frame.id, c.story, room) {
                layout.overflow = cut.overflow.is_some();
                if layout.overflow {
                    overflowing.push(c.story);
                }
                adjusted.push((c.story, Arc::new(cut)));
            }
        }
        cells.push(layout);
    }
    (
        TableLayout {
            row_tops,
            row_heights,
            cells,
            overflowing_cells: overflowing,
            natural_height: natural,
        },
        adjusted,
    )
}

/// `layout` without the lines of `frame` that reach below `room`, with the
/// hidden text reported. `None` when the story is missing.
fn truncate(
    doc: &Document,
    layout: &StoryLayout,
    frame: FrameId,
    story: StoryId,
    room: f64,
) -> Option<StoryLayout> {
    let chars = doc.story(story)?.char_len();
    let mut out = layout.clone();
    let ft = out.frames.get_mut(&frame)?;
    let keep = ft
        .lines
        .iter()
        .position(|l| line_bottom(l) > room + EPS)
        .unwrap_or(ft.lines.len());
    let first_hidden = ft.lines.get(keep).map(|l| l.chars.start);
    ft.lines.truncate(keep);
    ft.chars = match (ft.lines.first(), ft.lines.last()) {
        (Some(a), Some(b)) => a.chars.start..b.chars.end,
        _ => 0..0,
    };
    if let Some(offset) = first_hidden
        && offset < chars
    {
        let hidden = chars - offset;
        out.overflow = Some(match out.overflow {
            Some(o) if o.offset <= offset => o,
            _ => Overflow {
                offset,
                hidden_chars: hidden,
            },
        });
    }
    Some(out)
}
