//! Tables: table_add, table_get, table_set_cell_text, row and column
//! insert/delete, merge and split, cell style, column widths and row
//! heights. Cell text is an ordinary story: every text tool also works on a
//! cell by its `story_id` (from `table_get`).

use pubrs_core::table::{CellStyle, new_table};
use pubrs_core::{
    Color, Command, Dash, DocumentId, FrameId, MasterId, PageId, Paint, StoryId, Stroke,
    TableFrame, VerticalAlign,
};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use super::wordart::NoneOr;
use super::{OptColor, require_confirm};
use crate::error::{ApiError, ApiResult};
use crate::session::{Changed, Session};

#[derive(Debug, Clone, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct TableAdd {
    /// Document id (default: the most recently used document).
    #[serde(default)]
    pub doc_id: Option<DocumentId>,
    #[serde(default)]
    pub page_id: Option<PageId>,
    #[serde(default)]
    pub master_id: Option<MasterId>,
    /// Number of rows (1 to 200).
    pub rows: u32,
    /// Number of columns (1 to 40).
    pub cols: u32,
    /// Left edge in points from the page's left edge.
    pub x: f64,
    /// Top edge in points from the page's top edge (y grows down).
    pub y: f64,
    /// Width in points; the columns share it equally.
    pub width: f64,
    /// Height in points; the rows share it equally (a row grows to fit its text, up to the frame's bottom).
    pub height: f64,
    /// Fill for the first row (a header): a colour or swatch.
    #[serde(default)]
    pub header_fill: Option<Paint>,
    /// Border width in points on every cell side (default 0.5; 0 = no borders).
    #[serde(default)]
    pub border_width: Option<f64>,
    /// Border colour or swatch (default black).
    #[serde(default)]
    pub border_color: Option<Paint>,
    /// Display name shown in frame lists.
    #[serde(default)]
    pub name: Option<String>,
}

#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct TableAddOut {
    pub frame_id: FrameId,
    /// The cells' story ids, row by row (row-major).
    pub story_ids: Vec<StoryId>,
    #[serde(flatten)]
    pub change: Changed,
}

#[derive(Debug, Clone, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct TableRef {
    /// Document id (default: the most recently used document).
    #[serde(default)]
    pub doc_id: Option<DocumentId>,
    /// A table frame.
    pub frame_id: FrameId,
}

/// The four borders of a cell.
#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct BordersOut {
    pub top: Option<Stroke>,
    pub right: Option<Stroke>,
    pub bottom: Option<Stroke>,
    pub left: Option<Stroke>,
}

#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct CellOut {
    /// Top-left grid position (0-based).
    pub row: u32,
    pub col: u32,
    pub row_span: u32,
    pub col_span: u32,
    /// The cell's text story: use it with text_get, text_set, style_apply, text_find...
    pub story_id: StoryId,
    pub text: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fill: Option<Paint>,
    pub borders: BordersOut,
    /// Space between the cell edge and its text, in points (top, right, bottom, left).
    pub insets: pubrs_core::Margins,
    pub valign: VerticalAlign,
    /// Some of the text does not fit in the frame.
    pub overflowing: bool,
}

#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct TableGetOut {
    pub frame_id: FrameId,
    pub rows: usize,
    pub cols: usize,
    /// Column widths in points.
    pub col_widths: Vec<f64>,
    /// Each row's minimum height in points.
    pub row_min_heights: Vec<f64>,
    /// Each row's height as laid out (grown to fit its text, clipped to the frame).
    pub row_heights: Vec<f64>,
    /// Grid of story ids by position; merged cells repeat their story.
    pub grid: Vec<Vec<StoryId>>,
    /// Cells in row-major order of their top-left position.
    pub cells: Vec<CellOut>,
    pub overflowing: bool,
    pub revision: u64,
}

#[derive(Debug, Clone, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct TableSetCellText {
    /// Document id (default: the most recently used document).
    #[serde(default)]
    pub doc_id: Option<DocumentId>,
    pub frame_id: FrameId,
    /// Row of the cell (0-based; any position a merged cell covers).
    pub row: u32,
    pub col: u32,
    /// New text; "\n" separates paragraphs.
    pub text: String,
    /// Interpret `# ` headings and **bold**, *italic*, __underline__.
    #[serde(default)]
    pub markup: bool,
}

#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct TableCellOut {
    pub story_id: StoryId,
    pub length: usize,
    /// True if the text no longer fits the table frame.
    pub overflowing: bool,
    #[serde(flatten)]
    pub change: Changed,
}

#[derive(Debug, Clone, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct TableInsertRows {
    #[serde(default)]
    pub doc_id: Option<DocumentId>,
    pub frame_id: FrameId,
    /// Insert before this row (0-based). Default: after the last row.
    #[serde(default)]
    pub at: Option<u32>,
    /// How many (default 1).
    #[serde(default)]
    pub count: Option<u32>,
}

#[derive(Debug, Clone, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct TableDeleteRows {
    #[serde(default)]
    pub doc_id: Option<DocumentId>,
    pub frame_id: FrameId,
    /// First row to delete (0-based).
    pub at: u32,
    /// How many (default 1). A table keeps at least one row.
    #[serde(default)]
    pub count: Option<u32>,
    /// Must be true: the cells' text is deleted (undo restores it).
    #[serde(default)]
    pub confirm: bool,
}

#[derive(Debug, Clone, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct TableInsertCols {
    #[serde(default)]
    pub doc_id: Option<DocumentId>,
    pub frame_id: FrameId,
    /// Insert before this column (0-based). Default: after the last column.
    #[serde(default)]
    pub at: Option<u32>,
    /// How many (default 1).
    #[serde(default)]
    pub count: Option<u32>,
}

#[derive(Debug, Clone, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct TableDeleteCols {
    #[serde(default)]
    pub doc_id: Option<DocumentId>,
    pub frame_id: FrameId,
    /// First column to delete (0-based).
    pub at: u32,
    /// How many (default 1). A table keeps at least one column.
    #[serde(default)]
    pub count: Option<u32>,
    /// Must be true: the cells' text is deleted (undo restores it).
    #[serde(default)]
    pub confirm: bool,
}

#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct TableEditOut {
    pub frame_id: FrameId,
    /// Rows and columns after the change.
    pub rows: usize,
    pub cols: usize,
    #[serde(flatten)]
    pub change: Changed,
}

#[derive(Debug, Clone, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct TableMergeCells {
    #[serde(default)]
    pub doc_id: Option<DocumentId>,
    pub frame_id: FrameId,
    /// Top-left grid position of the range to merge (0-based).
    pub row: u32,
    pub col: u32,
    /// Rows and columns the merged cell covers (at least 2 positions in all).
    pub row_span: u32,
    pub col_span: u32,
}

#[derive(Debug, Clone, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct TableSplitCell {
    #[serde(default)]
    pub doc_id: Option<DocumentId>,
    pub frame_id: FrameId,
    /// Any grid position of the merged cell.
    pub row: u32,
    pub col: u32,
}

/// One border: colour, width and dash.
#[derive(Debug, Clone, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct BorderIn {
    /// Colour or swatch (default black).
    #[serde(default)]
    pub color: Option<Paint>,
    /// Width in points (default 0.5).
    #[serde(default)]
    pub width: Option<f64>,
    #[serde(default)]
    pub dash: Option<Dash>,
}

impl BorderIn {
    fn stroke(self) -> Stroke {
        Stroke {
            color: self.color.unwrap_or(Paint::Color(Color::BLACK)),
            width: self.width.unwrap_or(0.5),
            dash: self.dash.unwrap_or_default(),
        }
    }
}

#[derive(Debug, Clone, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct TableSetCellStyle {
    #[serde(default)]
    pub doc_id: Option<DocumentId>,
    pub frame_id: FrameId,
    /// Top-left grid position of the cell, or of the first cell of a range.
    pub row: u32,
    pub col: u32,
    /// Apply to every cell in this many rows / columns from (row, col) (default 1 each).
    #[serde(default)]
    pub row_span: Option<u32>,
    #[serde(default)]
    pub col_span: Option<u32>,
    /// Fill colour or swatch, or "none".
    #[serde(default)]
    pub fill: Option<OptColor>,
    /// All four borders at once ({color, width, dash} or "none"); the sides below override it.
    #[serde(default)]
    pub border: Option<NoneOr<BorderIn>>,
    #[serde(default)]
    pub border_top: Option<NoneOr<BorderIn>>,
    #[serde(default)]
    pub border_right: Option<NoneOr<BorderIn>>,
    #[serde(default)]
    pub border_bottom: Option<NoneOr<BorderIn>>,
    #[serde(default)]
    pub border_left: Option<NoneOr<BorderIn>>,
    /// Space between the cell edge and its text on all sides, in points.
    #[serde(default)]
    pub inset: Option<f64>,
    /// Vertical alignment of the text in the cell: top, middle or bottom.
    #[serde(default)]
    pub valign: Option<VerticalAlign>,
}

#[derive(Debug, Clone, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct TableSetColWidth {
    #[serde(default)]
    pub doc_id: Option<DocumentId>,
    pub frame_id: FrameId,
    /// Column index (0-based).
    pub col: u32,
    /// Width in points (> 0). The table frame's width follows the column total.
    pub width: f64,
}

#[derive(Debug, Clone, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct TableSetRowHeight {
    #[serde(default)]
    pub doc_id: Option<DocumentId>,
    pub frame_id: FrameId,
    /// Row index (0-based).
    pub row: u32,
    /// Minimum height in points; the row still grows to fit its text.
    pub min_height: f64,
}

impl Session {
    fn table_of(&self, doc_id: Option<DocumentId>, frame: FrameId) -> ApiResult<TableFrame> {
        let doc = self.get(doc_id)?.doc();
        let f = doc.frame(frame).ok_or_else(|| {
            ApiError::not_found("frame", &frame.to_string(), "list frames with frame_list")
        })?;
        f.as_table().cloned().ok_or_else(|| {
            ApiError::invalid(
                format!("frame {frame} is not a table"),
                "create one with table_add (frame_list with kind \"table\" lists tables)",
            )
        })
    }

    fn cell_story(&self, t: &TableFrame, row: u32, col: u32) -> ApiResult<StoryId> {
        t.cell_at(row, col).map(|c| c.story).ok_or_else(|| {
            ApiError::invalid(
                format!(
                    "no cell at row {row}, column {col} (the table has {} rows and {} columns)",
                    t.rows.len(),
                    t.cols.len()
                ),
                "rows and columns count from 0; table_get shows the grid",
            )
        })
    }

    fn table_edit_out(
        &self,
        doc_id: Option<DocumentId>,
        frame: FrameId,
        change: Changed,
    ) -> ApiResult<TableEditOut> {
        let t = self.table_of(Some(change.doc_id).or(doc_id), frame)?;
        Ok(TableEditOut {
            frame_id: frame,
            rows: t.rows.len(),
            cols: t.cols.len(),
            change,
        })
    }

    pub fn table_add(&mut self, req: TableAdd) -> ApiResult<TableAddOut> {
        let owner = self.owner_for(req.doc_id, req.page_id, req.master_id)?;
        let rect = self.rect_checked(req.x, req.y, req.width, req.height)?;
        if req.rows == 0 || req.cols == 0 || rect.width <= 0.0 || rect.height <= 0.0 {
            return Err(ApiError::invalid(
                "a table needs at least one row and one column and a positive size",
                "give rows, cols, width and height",
            ));
        }
        let bw = req.border_width.unwrap_or(0.5);
        let border = (bw > 0.0).then(|| Stroke {
            color: req.border_color.unwrap_or(Paint::Color(Color::BLACK)),
            width: bw,
            dash: Dash::Solid,
        });
        let style = CellStyle {
            borders: [border.clone(), border.clone(), border.clone(), border],
            ..CellStyle::default()
        };
        let (mut frame, stories) = new_table(
            &mut *self.ids,
            rect,
            req.rows as usize,
            req.cols as usize,
            &style,
        )
        .map_err(|m| ApiError::invalid(m, "at most 200 rows and 40 columns"))?;
        frame.name = req.name;
        if let (Some(fill), Some(t)) = (req.header_fill, frame.as_table_mut()) {
            for c in t.cells.iter_mut().filter(|c| c.row == 0) {
                c.fill = Some(fill.clone());
            }
        }
        let fid = frame.id;
        let story_ids = stories.iter().map(|s| s.id).collect();
        let change = self.exec(
            req.doc_id,
            Command::AddTable {
                parent: owner.into(),
                index: None,
                frame,
                stories,
            },
        )?;
        Ok(TableAddOut {
            frame_id: fid,
            story_ids,
            change,
        })
    }

    pub fn table_get(&mut self, req: TableRef) -> ApiResult<TableGetOut> {
        let t = self.table_of(req.doc_id, req.frame_id)?;
        let layout = self.layout(req.doc_id)?;
        let open = self.get(req.doc_id)?;
        let doc = open.doc();
        let tl = layout.table(req.frame_id);
        let overflow = |s: StoryId| layout.stories.get(&s).is_some_and(|l| l.overflow.is_some());
        let cells: Vec<CellOut> = t
            .cells
            .iter()
            .map(|c| CellOut {
                row: c.row,
                col: c.col,
                row_span: c.row_span,
                col_span: c.col_span,
                story_id: c.story,
                text: doc
                    .story(c.story)
                    .map(|s| s.plain_text())
                    .unwrap_or_default(),
                fill: c.fill.clone(),
                borders: BordersOut {
                    top: c.borders[0].clone(),
                    right: c.borders[1].clone(),
                    bottom: c.borders[2].clone(),
                    left: c.borders[3].clone(),
                },
                insets: c.insets,
                valign: c.valign,
                overflowing: overflow(c.story),
            })
            .collect();
        let grid = (0..t.rows.len() as u32)
            .map(|r| {
                (0..t.cols.len() as u32)
                    .filter_map(|c| t.cell_at(r, c).map(|x| x.story))
                    .collect()
            })
            .collect();
        Ok(TableGetOut {
            frame_id: req.frame_id,
            rows: t.rows.len(),
            cols: t.cols.len(),
            col_widths: t.cols.clone(),
            row_min_heights: t.rows.iter().map(|r| r.min_height).collect(),
            row_heights: tl.map_or_else(Vec::new, |l| l.row_heights.clone()),
            grid,
            overflowing: cells.iter().any(|c| c.overflowing),
            cells,
            revision: open.revision(),
        })
    }

    pub fn table_set_cell_text(&mut self, req: TableSetCellText) -> ApiResult<TableCellOut> {
        let t = self.table_of(req.doc_id, req.frame_id)?;
        let story = self.cell_story(&t, req.row, req.col)?;
        let out = self.text_set(super::text::TextSet {
            doc_id: req.doc_id,
            frame_id: None,
            story_id: Some(story),
            text: req.text,
            markup: req.markup,
        })?;
        Ok(TableCellOut {
            story_id: story,
            length: out.length,
            overflowing: out.overflowing,
            change: out.change,
        })
    }

    fn table_insert(
        &mut self,
        doc_id: Option<DocumentId>,
        frame: FrameId,
        rows: bool,
        at: Option<u32>,
        count: Option<u32>,
    ) -> ApiResult<TableEditOut> {
        let t = self.table_of(doc_id, frame)?;
        let n = if rows { t.rows.len() } else { t.cols.len() } as u32;
        let (at, count) = (at.unwrap_or(n), count.unwrap_or(1));
        // Validated before any id is allocated.
        let needed = if rows {
            t.insert_rows_needed(at as usize, count as usize)
        } else {
            t.insert_cols_needed(at as usize, count as usize)
        }
        .map_err(|m| {
            ApiError::invalid(m, "at counts from 0 up to the number of rows/columns; a table has at most 200 rows and 40 columns")
        })?;
        let stories: Vec<StoryId> = (0..needed).map(|_| self.new_story_id()).collect();
        let command = if rows {
            Command::TableInsertRows {
                frame,
                at,
                count,
                stories,
            }
        } else {
            Command::TableInsertCols {
                frame,
                at,
                count,
                stories,
            }
        };
        let change = self.exec(doc_id, command)?;
        self.table_edit_out(doc_id, frame, change)
    }

    pub fn table_insert_rows(&mut self, req: TableInsertRows) -> ApiResult<TableEditOut> {
        self.table_insert(req.doc_id, req.frame_id, true, req.at, req.count)
    }

    pub fn table_insert_cols(&mut self, req: TableInsertCols) -> ApiResult<TableEditOut> {
        self.table_insert(req.doc_id, req.frame_id, false, req.at, req.count)
    }

    pub fn table_delete_rows(&mut self, req: TableDeleteRows) -> ApiResult<TableEditOut> {
        require_confirm(req.confirm, "deleting table rows")?;
        self.table_of(req.doc_id, req.frame_id)?;
        let change = self.exec(
            req.doc_id,
            Command::TableDeleteRows {
                frame: req.frame_id,
                at: req.at,
                count: req.count.unwrap_or(1),
            },
        )?;
        self.table_edit_out(req.doc_id, req.frame_id, change)
    }

    pub fn table_delete_cols(&mut self, req: TableDeleteCols) -> ApiResult<TableEditOut> {
        require_confirm(req.confirm, "deleting table columns")?;
        self.table_of(req.doc_id, req.frame_id)?;
        let change = self.exec(
            req.doc_id,
            Command::TableDeleteCols {
                frame: req.frame_id,
                at: req.at,
                count: req.count.unwrap_or(1),
            },
        )?;
        self.table_edit_out(req.doc_id, req.frame_id, change)
    }

    pub fn table_merge_cells(&mut self, req: TableMergeCells) -> ApiResult<TableEditOut> {
        self.table_of(req.doc_id, req.frame_id)?;
        let change = self.exec(
            req.doc_id,
            Command::TableMergeCells {
                frame: req.frame_id,
                row: req.row,
                col: req.col,
                row_span: req.row_span,
                col_span: req.col_span,
            },
        )?;
        self.table_edit_out(req.doc_id, req.frame_id, change)
    }

    pub fn table_split_cell(&mut self, req: TableSplitCell) -> ApiResult<TableEditOut> {
        let t = self.table_of(req.doc_id, req.frame_id)?;
        let needed = t.split_needed(req.row, req.col).ok_or_else(|| {
            ApiError::invalid(
                format!("no cell at row {}, column {}", req.row, req.col),
                "rows and columns count from 0; table_get shows the grid",
            )
        })?;
        let stories = (0..needed).map(|_| self.new_story_id()).collect();
        let change = self.exec(
            req.doc_id,
            Command::TableSplitCell {
                frame: req.frame_id,
                row: req.row,
                col: req.col,
                stories,
            },
        )?;
        self.table_edit_out(req.doc_id, req.frame_id, change)
    }

    pub fn table_set_cell_style(&mut self, req: TableSetCellStyle) -> ApiResult<Changed> {
        let t = self.table_of(req.doc_id, req.frame_id)?;
        let (rs, cs) = (req.row_span.unwrap_or(1), req.col_span.unwrap_or(1));
        let cells: Vec<_> = t
            .cells
            .iter()
            .filter(|c| {
                c.row >= req.row
                    && c.row < req.row.saturating_add(rs)
                    && c.col >= req.col
                    && c.col < req.col.saturating_add(cs)
            })
            .collect();
        if cells.is_empty() {
            self.cell_story(&t, req.row, req.col)?;
            return Err(ApiError::invalid(
                "no cell starts in that range",
                "a merged cell is addressed by its top-left position; table_get shows the cells",
            ));
        }
        let sides = [
            req.border_top.as_ref(),
            req.border_right.as_ref(),
            req.border_bottom.as_ref(),
            req.border_left.as_ref(),
        ];
        let mut commands = Vec::new();
        for c in cells {
            let mut style = c.style();
            if let Some(f) = &req.fill {
                style.fill.clone_from(&f.0);
            }
            if let Some(all) = &req.border {
                style.borders = std::array::from_fn(|_| all.0.clone().map(BorderIn::stroke));
            }
            for (i, s) in sides.iter().enumerate() {
                if let Some(s) = s {
                    style.borders[i] = s.0.clone().map(BorderIn::stroke);
                }
            }
            if let Some(i) = req.inset {
                style.insets = pubrs_core::Margins::uniform(i);
            }
            if let Some(v) = req.valign {
                style.valign = v;
            }
            commands.push(Command::TableSetCellStyle {
                frame: req.frame_id,
                row: c.row,
                col: c.col,
                style,
            });
        }
        let command = if commands.len() == 1 {
            commands.remove(0)
        } else {
            Command::Batch {
                label: "table cell style".into(),
                commands,
            }
        };
        self.exec(req.doc_id, command)
    }

    pub fn table_set_col_width(&mut self, req: TableSetColWidth) -> ApiResult<Changed> {
        self.table_of(req.doc_id, req.frame_id)?;
        self.exec(
            req.doc_id,
            Command::TableSetColWidth {
                frame: req.frame_id,
                col: req.col,
                width: req.width,
            },
        )
    }

    pub fn table_set_row_height(&mut self, req: TableSetRowHeight) -> ApiResult<Changed> {
        self.table_of(req.doc_id, req.frame_id)?;
        self.exec(
            req.doc_id,
            Command::TableSetRowHeight {
                frame: req.frame_id,
                row: req.row,
                min_height: req.min_height,
            },
        )
    }
}
