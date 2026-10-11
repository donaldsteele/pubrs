//! Table frames (ADR 0024).
//!
//! A table is a grid of columns and rows covered exactly once by cells; a
//! merged cell covers several grid positions. The text of a cell is an
//! ordinary [`crate::Story`] whose chain is `[table frame]`, so styles, find,
//! spell check and undo work in cells like anywhere else.
//!
//! The operations here are pure functions on a [`TableFrame`]: commands call
//! them, supplying pre-allocated story ids for any cell they create.

use serde::{Deserialize, Serialize};

use crate::geom::{Margins, Rect};
use crate::id::{FrameId, IdSource, StoryId};
use crate::model::{Frame, FrameContent, Story, Stroke, VerticalAlign};
use crate::swatch::Paint;

/// Most rows and columns a table may have.
pub const TABLE_MAX_ROWS: usize = 200;
pub const TABLE_MAX_COLS: usize = 40;

/// Border sides in the order of [`Cell::borders`].
pub const TOP: usize = 0;
pub const RIGHT: usize = 1;
pub const BOTTOM: usize = 2;
pub const LEFT: usize = 3;

/// A table: column widths, row minimum heights and cells.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct TableFrame {
    /// Column widths in points, left to right.
    pub cols: Vec<f64>,
    /// Rows, top to bottom.
    pub rows: Vec<Row>,
    /// Cells in row-major order of their top-left position.
    pub cells: Vec<Cell>,
}

/// One table row. The row grows to fit its tallest cell.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct Row {
    /// Smallest height in points.
    pub min_height: f64,
}

/// One cell: a rectangle of grid positions with its own story.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct Cell {
    /// Top-left grid position (0-based).
    pub row: u32,
    pub col: u32,
    /// Grid positions covered, down and across (>= 1).
    #[serde(default = "one")]
    pub row_span: u32,
    #[serde(default = "one")]
    pub col_span: u32,
    /// The cell text; the story's chain is `[table frame]`.
    pub story: StoryId,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fill: Option<Paint>,
    /// Top, right, bottom and left borders.
    #[serde(default)]
    pub borders: [Option<Stroke>; 4],
    #[serde(default = "default_cell_insets")]
    pub insets: Margins,
    #[serde(default)]
    pub valign: VerticalAlign,
}

fn one() -> u32 {
    1
}

/// Default space between a cell edge and its text, in points.
pub fn default_cell_insets() -> Margins {
    Margins::uniform(4.0)
}

/// The look of one cell (everything except its position and text).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct CellStyle {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fill: Option<Paint>,
    #[serde(default)]
    pub borders: [Option<Stroke>; 4],
    #[serde(default = "default_cell_insets")]
    pub insets: Margins,
    #[serde(default)]
    pub valign: VerticalAlign,
}

impl Default for CellStyle {
    fn default() -> Self {
        Self {
            fill: None,
            borders: Default::default(),
            insets: default_cell_insets(),
            valign: VerticalAlign::Top,
        }
    }
}

impl Cell {
    pub fn new(row: u32, col: u32, story: StoryId, style: &CellStyle) -> Self {
        Self {
            row,
            col,
            row_span: 1,
            col_span: 1,
            story,
            fill: style.fill.clone(),
            borders: style.borders.clone(),
            insets: style.insets,
            valign: style.valign,
        }
    }

    pub fn style(&self) -> CellStyle {
        CellStyle {
            fill: self.fill.clone(),
            borders: self.borders.clone(),
            insets: self.insets,
            valign: self.valign,
        }
    }

    pub fn set_style(&mut self, s: &CellStyle) {
        self.fill.clone_from(&s.fill);
        self.borders.clone_from(&s.borders);
        self.insets = s.insets;
        self.valign = s.valign;
    }

    /// Whether the cell covers grid position (`row`, `col`).
    pub fn covers(&self, row: u32, col: u32) -> bool {
        row >= self.row
            && row < self.row + self.row_span
            && col >= self.col
            && col < self.col + self.col_span
    }

    /// Last row covered.
    pub fn last_row(&self) -> u32 {
        self.row + self.row_span - 1
    }

    fn pos(&self, a: Axis) -> u32 {
        match a {
            Axis::Row => self.row,
            Axis::Col => self.col,
        }
    }

    fn span(&self, a: Axis) -> u32 {
        match a {
            Axis::Row => self.row_span,
            Axis::Col => self.col_span,
        }
    }

    fn set_pos(&mut self, a: Axis, v: u32) {
        match a {
            Axis::Row => self.row = v,
            Axis::Col => self.col = v,
        }
    }

    fn set_span(&mut self, a: Axis, v: u32) {
        match a {
            Axis::Row => self.row_span = v,
            Axis::Col => self.col_span = v,
        }
    }
}

/// Rows or columns.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Axis {
    Row,
    Col,
}

impl Axis {
    fn other(self) -> Axis {
        match self {
            Axis::Row => Axis::Col,
            Axis::Col => Axis::Row,
        }
    }

    fn name(self) -> &'static str {
        match self {
            Axis::Row => "row",
            Axis::Col => "column",
        }
    }
}

type Res<T = ()> = Result<T, String>;

impl TableFrame {
    /// A `rows` x `cols` grid of 1x1 cells. `stories` are given in row-major
    /// order and must number `rows * cols`.
    pub fn grid(
        cols: Vec<f64>,
        row_heights: Vec<f64>,
        stories: &[StoryId],
        style: &CellStyle,
    ) -> Res<Self> {
        if stories.len() != cols.len() * row_heights.len() {
            return Err(format!(
                "a {}x{} table needs {} stories",
                row_heights.len(),
                cols.len(),
                cols.len() * row_heights.len()
            ));
        }
        let mut cells = Vec::with_capacity(stories.len());
        for r in 0..row_heights.len() {
            for c in 0..cols.len() {
                cells.push(Cell::new(
                    r as u32,
                    c as u32,
                    stories[r * cols.len() + c],
                    style,
                ));
            }
        }
        let t = Self {
            rows: row_heights
                .into_iter()
                .map(|min_height| Row { min_height })
                .collect(),
            cols,
            cells,
        };
        t.check()?;
        Ok(t)
    }

    pub fn row_count(&self) -> usize {
        self.rows.len()
    }

    pub fn col_count(&self) -> usize {
        self.cols.len()
    }

    /// Total width of all columns.
    pub fn width(&self) -> f64 {
        self.cols.iter().sum()
    }

    /// Index into `cells` of the cell covering a grid position.
    pub fn cell_index_at(&self, row: u32, col: u32) -> Option<usize> {
        self.cells.iter().position(|c| c.covers(row, col))
    }

    pub fn cell_at(&self, row: u32, col: u32) -> Option<&Cell> {
        self.cell_index_at(row, col).map(|i| &self.cells[i])
    }

    pub fn cell_of_story(&self, story: StoryId) -> Option<&Cell> {
        self.cells.iter().find(|c| c.story == story)
    }

    pub fn stories(&self) -> impl Iterator<Item = StoryId> + '_ {
        self.cells.iter().map(|c| c.story)
    }

    /// Structural check: sizes, spans, exact coverage, distinct stories.
    pub fn check(&self) -> Res {
        let (nr, nc) = (self.rows.len(), self.cols.len());
        if nr == 0 || nc == 0 {
            return Err("a table needs at least one row and one column".into());
        }
        if nr > TABLE_MAX_ROWS || nc > TABLE_MAX_COLS {
            return Err(format!(
                "a table has at most {TABLE_MAX_ROWS} rows and {TABLE_MAX_COLS} columns"
            ));
        }
        for w in &self.cols {
            check_width(*w)?;
        }
        for r in &self.rows {
            check_height(r.min_height)?;
        }
        let mut grid = vec![false; nr * nc];
        let mut seen = std::collections::BTreeSet::new();
        for c in &self.cells {
            if c.row_span == 0 || c.col_span == 0 {
                return Err("cell spans must be at least 1".into());
            }
            if c.row as usize + c.row_span as usize > nr
                || c.col as usize + c.col_span as usize > nc
            {
                return Err(format!(
                    "cell at row {} column {} reaches outside the table",
                    c.row, c.col
                ));
            }
            if !seen.insert(c.story) {
                return Err(format!("story {} is used by two cells", c.story));
            }
            if !c.insets.is_valid() {
                return Err("cell insets must be finite and >= 0".into());
            }
            for r in c.row..c.row + c.row_span {
                for k in c.col..c.col + c.col_span {
                    let slot = &mut grid[r as usize * nc + k as usize];
                    if *slot {
                        return Err(format!("cells overlap at row {r} column {k}"));
                    }
                    *slot = true;
                }
            }
        }
        if let Some(i) = grid.iter().position(|g| !g) {
            return Err(format!("no cell covers row {} column {}", i / nc, i % nc));
        }
        Ok(())
    }

    fn sort(&mut self) {
        self.cells.sort_by_key(|c| (c.row, c.col));
    }

    fn lines(&self, a: Axis) -> usize {
        match a {
            Axis::Row => self.rows.len(),
            Axis::Col => self.cols.len(),
        }
    }

    /// Cells a new line at `at` needs: one per position of the other axis
    /// that no spanning cell crosses, for each of `count` lines.
    ///
    /// Checked before anything is counted, so an absurd `count` or an `at`
    /// past the end is an error, never a huge allocation by the caller.
    pub fn insert_needed(&self, a: Axis, at: usize, count: usize) -> Res<usize> {
        let n = self.lines(a);
        if at > n {
            return Err(format!("{} {at} is outside the table", a.name()));
        }
        if count == 0 {
            return Err("insert at least one".into());
        }
        let max = match a {
            Axis::Row => TABLE_MAX_ROWS,
            Axis::Col => TABLE_MAX_COLS,
        };
        if count > max || n + count > max {
            return Err(format!("a table has at most {max} {}s", a.name()));
        }
        Ok(self.crossing(a, at).iter().filter(|x| !**x).count() * count)
    }

    pub fn insert_rows_needed(&self, at: usize, count: usize) -> Res<usize> {
        self.insert_needed(Axis::Row, at, count)
    }

    pub fn insert_cols_needed(&self, at: usize, count: usize) -> Res<usize> {
        self.insert_needed(Axis::Col, at, count)
    }

    /// For each position along the other axis: does one cell cover the lines
    /// on both sides of `at`?
    fn crossing(&self, a: Axis, at: usize) -> Vec<bool> {
        let other = a.other();
        (0..self.lines(other))
            .map(|p| {
                if at == 0 || at >= self.lines(a) {
                    return false;
                }
                self.cells.iter().any(|c| {
                    let (lo, hi) = (c.pos(a) as usize, (c.pos(a) + c.span(a)) as usize);
                    let (olo, ohi) = (
                        c.pos(other) as usize,
                        (c.pos(other) + c.span(other)) as usize,
                    );
                    lo < at && at < hi && olo <= p && p < ohi
                })
            })
            .collect()
    }

    pub fn insert_rows(&mut self, at: usize, count: usize, stories: &[StoryId]) -> Res {
        self.insert_lines(Axis::Row, at, count, stories)
    }

    pub fn insert_cols(&mut self, at: usize, count: usize, stories: &[StoryId]) -> Res {
        self.insert_lines(Axis::Col, at, count, stories)
    }

    pub fn delete_rows(&mut self, at: usize, count: usize) -> Res<Vec<StoryId>> {
        self.delete_lines(Axis::Row, at, count)
    }

    pub fn delete_cols(&mut self, at: usize, count: usize) -> Res<Vec<StoryId>> {
        self.delete_lines(Axis::Col, at, count)
    }

    fn insert_lines(&mut self, a: Axis, at: usize, count: usize, stories: &[StoryId]) -> Res {
        let n = self.lines(a);
        let needed = self.insert_needed(a, at, count)?;
        let crossing = self.crossing(a, at);
        if stories.len() != needed {
            return Err(format!(
                "inserting here creates {needed} cells; {} story ids given",
                stories.len()
            ));
        }
        // Style donors: the cell after the new line, else the one before.
        let other = a.other();
        let donor = |p: usize| -> Option<CellStyle> {
            let find = |line: usize| {
                self.cells.iter().find(|c| {
                    let (lo, hi) = (c.pos(a) as usize, (c.pos(a) + c.span(a)) as usize);
                    let (olo, ohi) = (
                        c.pos(other) as usize,
                        (c.pos(other) + c.span(other)) as usize,
                    );
                    lo <= line && line < hi && olo <= p && p < ohi
                })
            };
            let c = if at < n {
                find(at)
            } else {
                find(at.wrapping_sub(1))
            };
            c.map(|c| CellStyle {
                fill: None,
                ..c.style()
            })
        };
        let donors: Vec<Option<CellStyle>> = (0..self.lines(other)).map(donor).collect();
        let size_donor = if at < n { at } else { n - 1 };
        let new_row = self.rows[size_donor.min(self.rows.len() - 1)];
        let new_col = self.cols[size_donor.min(self.cols.len() - 1)];
        // Shift and extend existing cells.
        for c in &mut self.cells {
            let (lo, hi) = (c.pos(a) as usize, (c.pos(a) + c.span(a)) as usize);
            if lo >= at {
                c.set_pos(a, (lo + count) as u32);
            } else if hi > at {
                c.set_span(a, (hi - lo + count) as u32);
            }
        }
        let mut ids = stories.iter();
        for k in 0..count {
            for (p, cross) in crossing.iter().enumerate() {
                if *cross {
                    continue;
                }
                let style = donors[p].clone().unwrap_or_default();
                let Some(story) = ids.next().copied() else {
                    return Err("not enough story ids".into());
                };
                let (r, c) = match a {
                    Axis::Row => ((at + k) as u32, p as u32),
                    Axis::Col => (p as u32, (at + k) as u32),
                };
                self.cells.push(Cell::new(r, c, story, &style));
            }
        }
        match a {
            Axis::Row => {
                for _ in 0..count {
                    self.rows.insert(at, new_row);
                }
            }
            Axis::Col => {
                for _ in 0..count {
                    self.cols.insert(at, new_col);
                }
            }
        }
        self.sort();
        self.check()
    }

    fn delete_lines(&mut self, a: Axis, at: usize, count: usize) -> Res<Vec<StoryId>> {
        let n = self.lines(a);
        if count == 0 || at + count > n {
            return Err(format!(
                "{}s {at}..{} are outside the table",
                a.name(),
                at + count
            ));
        }
        if count >= n {
            return Err(format!(
                "a table keeps at least one {}; delete the frame instead",
                a.name()
            ));
        }
        let end = at + count;
        let mut removed = Vec::new();
        let mut kept = Vec::with_capacity(self.cells.len());
        for mut c in std::mem::take(&mut self.cells) {
            let (lo, hi) = (c.pos(a) as usize, (c.pos(a) + c.span(a)) as usize);
            if hi <= at {
                kept.push(c);
            } else if lo >= end {
                c.set_pos(a, (lo - count) as u32);
                kept.push(c);
            } else if lo >= at && hi <= end {
                removed.push(c.story);
            } else {
                let overlap = hi.min(end) - lo.max(at);
                c.set_span(a, (hi - lo - overlap) as u32);
                if lo >= at {
                    c.set_pos(a, at as u32);
                }
                kept.push(c);
            }
        }
        self.cells = kept;
        match a {
            Axis::Row => {
                self.rows.drain(at..end);
            }
            Axis::Col => {
                self.cols.drain(at..end);
            }
        }
        self.sort();
        self.check()?;
        Ok(removed)
    }

    /// Merge the grid rectangle `rows` x `cols` (start, span) into its
    /// top-left cell. Returns the kept story and the stories of the cells
    /// that were absorbed, in row-major order.
    pub fn merge(
        &mut self,
        row: u32,
        col: u32,
        row_span: u32,
        col_span: u32,
    ) -> Res<(StoryId, Vec<StoryId>)> {
        if row_span == 0 || col_span == 0 || row_span.saturating_mul(col_span) < 2 {
            return Err("select at least two grid positions to merge".into());
        }
        if row as usize + row_span as usize > self.rows.len()
            || col as usize + col_span as usize > self.cols.len()
        {
            return Err("the merge range reaches outside the table".into());
        }
        let (r1, c1) = (row + row_span, col + col_span);
        let inside = |c: &Cell| {
            c.row >= row && c.col >= col && c.row + c.row_span <= r1 && c.col + c.col_span <= c1
        };
        let touches = |c: &Cell| {
            c.row < r1 && c.col < c1 && c.row + c.row_span > row && c.col + c.col_span > col
        };
        if self.cells.iter().any(|c| touches(c) && !inside(c)) {
            return Err("the merge range cuts through an existing merged cell".into());
        }
        let keep_story = self
            .cell_at(row, col)
            .map(|c| c.story)
            .ok_or("no cell at the merge corner")?;
        let mut removed = Vec::new();
        self.cells.retain(|c| {
            if inside(c) && c.story != keep_story {
                removed.push(c.story);
                false
            } else {
                true
            }
        });
        let k = self
            .cells
            .iter_mut()
            .find(|c| c.story == keep_story)
            .ok_or("merge lost its corner cell")?;
        k.row_span = row_span;
        k.col_span = col_span;
        self.check()?;
        Ok((keep_story, removed))
    }

    /// Cells a split of the cell at (`row`, `col`) creates.
    pub fn split_needed(&self, row: u32, col: u32) -> Option<usize> {
        self.cell_at(row, col)
            .map(|c| (c.row_span * c.col_span) as usize - 1)
    }

    /// Undo a merge: the cell covering (`row`, `col`) becomes 1x1 cells again,
    /// the original keeping the top-left position.
    pub fn split(&mut self, row: u32, col: u32, stories: &[StoryId]) -> Res {
        let i = self
            .cell_index_at(row, col)
            .ok_or_else(|| format!("no cell at row {row} column {col}"))?;
        let (r0, c0, rs, cs) = {
            let c = &self.cells[i];
            (c.row, c.col, c.row_span, c.col_span)
        };
        if rs.saturating_mul(cs) < 2 {
            return Err("that cell is not merged".into());
        }
        if stories.len() != (rs * cs) as usize - 1 {
            return Err(format!(
                "splitting creates {} cells; {} story ids given",
                rs * cs - 1,
                stories.len()
            ));
        }
        let style = self.cells[i].style();
        self.cells[i].row_span = 1;
        self.cells[i].col_span = 1;
        let mut ids = stories.iter();
        for r in r0..r0 + rs {
            for c in c0..c0 + cs {
                if (r, c) != (r0, c0) {
                    let Some(story) = ids.next().copied() else {
                        return Err("not enough story ids".into());
                    };
                    self.cells.push(Cell::new(r, c, story, &style));
                }
            }
        }
        self.sort();
        self.check()
    }

    pub fn set_col_width(&mut self, col: usize, width: f64) -> Res {
        check_width(width)?;
        *self
            .cols
            .get_mut(col)
            .ok_or_else(|| format!("column {col} is outside the table"))? = width;
        Ok(())
    }

    pub fn set_row_height(&mut self, row: usize, min_height: f64) -> Res {
        check_height(min_height)?;
        self.rows
            .get_mut(row)
            .ok_or_else(|| format!("row {row} is outside the table"))?
            .min_height = min_height;
        Ok(())
    }

    /// Scale all columns so they sum to `width`.
    pub fn scale_to_width(&mut self, width: f64) {
        let total = self.width();
        if total > 0.0 && width > 0.0 && (total - width).abs() > 1e-9 {
            let k = width / total;
            for w in &mut self.cols {
                *w *= k;
            }
        }
    }
}

/// A new `rows` x `cols` table frame filling `rect` (equal columns and rows),
/// with an empty story per cell. Returns the frame and its stories.
pub fn new_table(
    ids: &mut dyn IdSource,
    rect: Rect,
    rows: usize,
    cols: usize,
    style: &CellStyle,
) -> Result<(Frame, Vec<Story>), String> {
    if rows == 0 || cols == 0 || rows > TABLE_MAX_ROWS || cols > TABLE_MAX_COLS {
        return Err(format!(
            "a table has 1..={TABLE_MAX_ROWS} rows and 1..={TABLE_MAX_COLS} columns"
        ));
    }
    let fid = FrameId::new(ids);
    let story_ids: Vec<StoryId> = (0..rows * cols).map(|_| StoryId::new(ids)).collect();
    let table = TableFrame::grid(
        vec![rect.width / cols as f64; cols],
        vec![rect.height / rows as f64; rows],
        &story_ids,
        style,
    )?;
    let stories = story_ids.iter().map(|s| Story::new(*s, fid)).collect();
    Ok((
        Frame::new(fid, rect, FrameContent::Table(Box::new(table))),
        stories,
    ))
}

fn check_width(w: f64) -> Res {
    if w.is_finite() && w > 0.0 && w <= crate::command::MAX_DIMENSION {
        Ok(())
    } else {
        Err("column widths must be > 0".into())
    }
}

fn check_height(h: f64) -> Res {
    if h.is_finite() && (0.0..=crate::command::MAX_DIMENSION).contains(&h) {
        Ok(())
    } else {
        Err("row heights must be finite and >= 0".into())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ids(n: usize, from: u64) -> Vec<StoryId> {
        (0..n)
            .map(|i| StoryId(uuid::Uuid::from_u128(u128::from(from) + i as u128)))
            .collect()
    }

    fn table(r: usize, c: usize) -> TableFrame {
        TableFrame::grid(
            vec![50.0; c],
            vec![10.0; r],
            &ids(r * c, 1),
            &CellStyle::default(),
        )
        .unwrap()
    }

    #[test]
    fn insert_through_a_merged_cell_extends_it() {
        let mut t = table(3, 3);
        t.merge(0, 0, 3, 1).unwrap();
        // Insert a row at 1: column 0 is crossed, so two new cells.
        assert_eq!(t.insert_rows_needed(1, 1), Ok(2));
        t.insert_rows(1, 1, &ids(2, 100)).unwrap();
        assert_eq!(t.cell_at(3, 0).unwrap().row_span, 4);
        assert_eq!(t.rows.len(), 4);
    }

    #[test]
    fn delete_shrinks_and_removes() {
        let mut t = table(3, 2);
        t.merge(0, 0, 2, 1).unwrap();
        let gone = t.delete_rows(0, 2).unwrap();
        assert_eq!(gone.len(), 3);
        assert_eq!(t.rows.len(), 1);
        assert!(t.delete_rows(0, 1).is_err());
    }

    #[test]
    fn merge_refuses_partial_overlap() {
        let mut t = table(3, 3);
        t.merge(0, 0, 2, 2).unwrap();
        assert!(t.merge(1, 1, 2, 2).is_err());
        let (keep, gone) = t.merge(0, 0, 3, 3).unwrap();
        assert_eq!(keep, StoryId(uuid::Uuid::from_u128(1)));
        assert_eq!(gone.len(), 5);
    }

    #[test]
    fn split_restores_the_grid() {
        let mut t = table(2, 2);
        t.merge(0, 0, 2, 2).unwrap();
        assert!(t.split(0, 0, &ids(2, 9)).is_err());
        t.split(1, 1, &ids(3, 9)).unwrap();
        assert_eq!(t.cells.len(), 4);
    }
}
