//! Label, card and booklet tools: sheet_presets_list, doc_set_sheet,
//! doc_set_booklet.

use pubrs_core::{Command, DocumentId, Point, SheetFill, SheetLayout, Size};
use pubrs_io::presets::{sheet_preset, sheet_presets};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::error::{ApiError, ApiResult};
use crate::session::{Changed, Session};

#[derive(Debug, Clone, Default, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SheetPresetsList {}

#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct SheetPresetInfo {
    /// Pass this as `preset` to doc_set_sheet.
    pub id: String,
    pub name: String,
    /// Paper size in points.
    pub sheet_width: f64,
    pub sheet_height: f64,
    pub rows: u32,
    pub cols: u32,
    /// Labels per sheet.
    pub count: u32,
    /// Label size in points: the page size the document must have.
    pub label_width: f64,
    pub label_height: f64,
    /// Top-left corner of the first label, in points.
    pub left: f64,
    pub top: f64,
    /// Distance between neighbouring label corners, in points.
    pub h_pitch: f64,
    pub v_pitch: f64,
}

#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct SheetPresetsOut {
    pub presets: Vec<SheetPresetInfo>,
}

/// A custom label grid, in points.
#[derive(Debug, Clone, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CustomSheet {
    pub sheet_width: f64,
    pub sheet_height: f64,
    pub rows: u32,
    pub cols: u32,
    pub label_width: f64,
    pub label_height: f64,
    /// Distance from the left edge of the sheet to the first label (default 0).
    #[serde(default)]
    pub left: f64,
    /// Distance from the top edge of the sheet to the first label (default 0).
    #[serde(default)]
    pub top: f64,
    /// Space between columns (default 0).
    #[serde(default)]
    pub h_gap: f64,
    /// Space between rows (default 0).
    #[serde(default)]
    pub v_gap: f64,
}

/// How pages fill the cells.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum FillIn {
    /// Every page fills all the cells of its own sheet (default).
    #[default]
    Repeat,
    /// Pages fill the cells in order, one page per label.
    Sequence,
}

#[derive(Debug, Clone, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct DocSetSheet {
    /// Document id (default: the most recently used document).
    #[serde(default)]
    pub doc_id: Option<DocumentId>,
    /// Preset id from sheet_presets_list, e.g. "avery-5160". Omit both
    /// `preset` and `custom` (or pass null) to clear the sheet.
    #[serde(default)]
    pub preset: Option<String>,
    /// A custom grid instead of a preset.
    #[serde(default)]
    pub custom: Option<CustomSheet>,
    #[serde(default)]
    pub fill: FillIn,
}

#[derive(Debug, Clone, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct DocSetBooklet {
    /// Document id (default: the most recently used document).
    #[serde(default)]
    pub doc_id: Option<DocumentId>,
    pub booklet: bool,
}

impl Session {
    pub fn sheet_presets_list(&mut self, _req: SheetPresetsList) -> ApiResult<SheetPresetsOut> {
        Ok(SheetPresetsOut {
            presets: sheet_presets()
                .into_iter()
                .map(|p| SheetPresetInfo {
                    id: p.id.to_string(),
                    name: p.name.to_string(),
                    sheet_width: p.sheet.width,
                    sheet_height: p.sheet.height,
                    rows: p.rows,
                    cols: p.cols,
                    count: p.count(),
                    label_width: p.label.width,
                    label_height: p.label.height,
                    left: p.origin.x,
                    top: p.origin.y,
                    h_pitch: p.pitch.width,
                    v_pitch: p.pitch.height,
                })
                .collect(),
        })
    }

    pub fn doc_set_sheet(&mut self, req: DocSetSheet) -> ApiResult<Changed> {
        let fill = match req.fill {
            FillIn::Repeat => SheetFill::Repeat,
            FillIn::Sequence => SheetFill::Sequence,
        };
        let sheet = match (&req.preset, &req.custom) {
            (Some(_), Some(_)) => {
                return Err(ApiError::invalid(
                    "give either preset or custom, not both",
                    "use preset for a product number or custom for your own grid",
                ));
            }
            (Some(id), None) => {
                let p = sheet_preset(id).ok_or_else(|| {
                    ApiError::invalid(
                        format!("unknown sheet preset '{id}'"),
                        "call sheet_presets_list for the available ids",
                    )
                })?;
                Some(p.layout(fill))
            }
            (None, Some(c)) => Some(SheetLayout {
                preset: "custom".into(),
                sheet_size: Size::new(c.sheet_width, c.sheet_height),
                rows: c.rows,
                cols: c.cols,
                label_size: Size::new(c.label_width, c.label_height),
                origin: Point::new(c.left, c.top),
                pitch: Size::new(c.label_width + c.h_gap, c.label_height + c.v_gap),
                fill,
            }),
            (None, None) => None,
        };
        self.exec(req.doc_id, Command::SetSheet { sheet })
    }

    pub fn doc_set_booklet(&mut self, req: DocSetBooklet) -> ApiResult<Changed> {
        self.exec(
            req.doc_id,
            Command::SetBooklet {
                booklet: req.booklet,
            },
        )
    }
}
