//! Label, card and postcard sheet presets.
//!
//! The presets are plain data: product numbers and the published sheet
//! geometry (a fact about a product, not artwork). Nothing is copied from any
//! vendor template file. Every value is written in inches or millimetres and
//! converted to points. A unit test proves each grid fits its sheet.

use pubrs_core::geom::{PT_PER_INCH, PT_PER_MM};
use pubrs_core::{Point, SheetFill, SheetLayout, Size};

/// One sheet of labels or cards.
#[derive(Debug, Clone, PartialEq)]
pub struct SheetPreset {
    /// Stable id such as `avery-5160`.
    pub id: &'static str,
    /// Display name, with the label size and count.
    pub name: &'static str,
    /// Paper size in points.
    pub sheet: Size,
    pub rows: u32,
    pub cols: u32,
    /// Label size in points.
    pub label: Size,
    /// Top-left corner of the first label in points.
    pub origin: Point,
    /// Distance between neighbouring label corners in points.
    pub pitch: Size,
}

impl SheetPreset {
    pub fn count(&self) -> u32 {
        self.rows * self.cols
    }

    /// The document-model layout for this preset.
    pub fn layout(&self, fill: SheetFill) -> SheetLayout {
        SheetLayout {
            preset: self.id.to_string(),
            sheet_size: self.sheet,
            rows: self.rows,
            cols: self.cols,
            label_size: self.label,
            origin: self.origin,
            pitch: self.pitch,
            fill,
        }
    }
}

/// Units a preset row is written in.
#[derive(Clone, Copy)]
enum U {
    In,
    Mm,
}

impl U {
    fn pt(self, v: f64) -> f64 {
        match self {
            U::In => v * PT_PER_INCH,
            U::Mm => v * PT_PER_MM,
        }
    }
}

const LETTER: (f64, f64, U) = (8.5, 11.0, U::In);
const A4: (f64, f64, U) = (210.0, 297.0, U::Mm);

/// id, name, paper, rows, cols, label w x h, top, left, pitch h x v, unit.
type Row = (
    &'static str,
    &'static str,
    (f64, f64, U),
    u32,
    u32,
    (f64, f64),
    (f64, f64),
    (f64, f64),
);

#[rustfmt::skip]
const TABLE: &[Row] = &[
    // label (w, h); (top, left); pitch (h, v)  -- all in the paper's unit
    ("avery-5160", "Avery 5160 address, 30 per sheet (2.625 x 1 in)", LETTER, 10, 3, (2.625, 1.0), (0.5, 0.1875), (2.75, 1.0)),
    ("avery-5161", "Avery 5161 address, 20 per sheet (4 x 1 in)", LETTER, 10, 2, (4.0, 1.0), (0.5, 0.15625), (4.1875, 1.0)),
    ("avery-5162", "Avery 5162 address, 14 per sheet (4 x 1.333 in)", LETTER, 7, 2, (4.0, 1.333), (0.8333, 0.15625), (4.1875, 1.333)),
    ("avery-5163", "Avery 5163 shipping, 10 per sheet (4 x 2 in)", LETTER, 5, 2, (4.0, 2.0), (0.5, 0.15625), (4.1875, 2.0)),
    ("avery-5164", "Avery 5164 shipping, 6 per sheet (4 x 3.333 in)", LETTER, 3, 2, (4.0, 3.333), (0.5, 0.15625), (4.1875, 3.333)),
    ("avery-5167", "Avery 5167 return address, 80 per sheet (1.75 x 0.5 in)", LETTER, 20, 4, (1.75, 0.5), (0.5, 0.3), (2.0625, 0.5)),
    ("avery-5371", "Avery 5371 business cards, 10 per sheet (3.5 x 2 in)", LETTER, 5, 2, (3.5, 2.0), (0.5, 0.75), (3.5, 2.0)),
    ("avery-5376", "Avery 5376 business cards, 10 per sheet (3.5 x 2 in)", LETTER, 5, 2, (3.5, 2.0), (0.5, 0.75), (3.5, 2.0)),
    ("avery-5377", "Avery 5377 cards, 8 per sheet (3.5 x 2.5 in)", LETTER, 4, 2, (3.5, 2.5), (0.5, 0.75), (3.5, 2.5)),
    ("avery-8160", "Avery 8160 inkjet address, 30 per sheet (2.625 x 1 in)", LETTER, 10, 3, (2.625, 1.0), (0.5, 0.1875), (2.75, 1.0)),
    ("avery-8163", "Avery 8163 inkjet shipping, 10 per sheet (4 x 2 in)", LETTER, 5, 2, (4.0, 2.0), (0.5, 0.15625), (4.1875, 2.0)),
    ("avery-22805", "Avery 22805 square labels, 12 per sheet (2 x 2 in)", LETTER, 4, 3, (2.0, 2.0), (0.75, 0.875), (2.375, 2.5)),
    ("avery-5305", "Avery 5305 postcards, 4 per sheet (4.25 x 5.5 in)", LETTER, 2, 2, (4.25, 5.5), (0.0, 0.0), (4.25, 5.5)),
    ("avery-8315", "Avery 8315 note cards, 4 per sheet (4.25 x 5.5 in)", LETTER, 2, 2, (4.25, 5.5), (0.0, 0.0), (4.25, 5.5)),
    ("l7160", "A4 L7160 address, 21 per sheet (63.5 x 38.1 mm)", A4, 7, 3, (63.5, 38.1), (15.1, 7.2), (66.0, 38.1)),
    ("l7161", "A4 L7161 address, 18 per sheet (63.5 x 46.6 mm)", A4, 6, 3, (63.5, 46.6), (8.8, 7.2), (66.0, 46.6)),
    ("l7162", "A4 L7162 address, 16 per sheet (99.1 x 33.9 mm)", A4, 8, 2, (99.1, 33.9), (12.9, 4.6), (102.6, 33.9)),
    ("l7163", "A4 L7163 address, 14 per sheet (99.1 x 38.1 mm)", A4, 7, 2, (99.1, 38.1), (15.1, 4.65), (102.6, 38.1)),
    ("l7173", "A4 L7173 shipping, 10 per sheet (99.1 x 57 mm)", A4, 5, 2, (99.1, 57.0), (6.0, 4.65), (102.6, 57.0)),
    ("l7651", "A4 L7651 small labels, 65 per sheet (38.1 x 21.2 mm)", A4, 13, 5, (38.1, 21.2), (10.7, 6.4), (40.4, 21.2)),
    ("c32011", "A4 C32011 business cards, 10 per sheet (85 x 54 mm)", A4, 5, 2, (85.0, 54.0), (11.5, 15.0), (90.0, 54.0)),
    ("j8160", "A4 J8160 inkjet address, 21 per sheet (63.5 x 38.1 mm)", A4, 7, 3, (63.5, 38.1), (15.1, 7.2), (66.0, 38.1)),
    ("generic-2up-letter", "Generic 2-up, Letter (8.5 x 5.5 in)", LETTER, 2, 1, (8.5, 5.5), (0.0, 0.0), (8.5, 5.5)),
    ("generic-4up-letter", "Generic 4-up, Letter (4.25 x 5.5 in)", LETTER, 2, 2, (4.25, 5.5), (0.0, 0.0), (4.25, 5.5)),
    ("generic-2up-a4", "Generic 2-up, A4 (210 x 148.5 mm)", A4, 2, 1, (210.0, 148.5), (0.0, 0.0), (210.0, 148.5)),
    ("generic-4up-a4", "Generic 4-up, A4 (105 x 148.5 mm)", A4, 2, 2, (105.0, 148.5), (0.0, 0.0), (105.0, 148.5)),
];

/// Every sheet preset, in display order.
pub fn sheet_presets() -> Vec<SheetPreset> {
    TABLE
        .iter()
        .map(
            |&(id, name, (pw, ph, u), rows, cols, (lw, lh), (top, left), (gx, gy))| SheetPreset {
                id,
                name,
                sheet: Size::new(u.pt(pw), u.pt(ph)),
                rows,
                cols,
                label: Size::new(u.pt(lw), u.pt(lh)),
                origin: Point::new(u.pt(left), u.pt(top)),
                pitch: Size::new(u.pt(gx), u.pt(gy)),
            },
        )
        .collect()
}

/// Look a preset up by id (case-insensitive).
pub fn sheet_preset(id: &str) -> Option<SheetPreset> {
    sheet_presets()
        .into_iter()
        .find(|p| p.id.eq_ignore_ascii_case(id))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn there_are_at_least_24_unique_presets() {
        let all = sheet_presets();
        assert!(all.len() >= 24);
        let mut ids: Vec<_> = all.iter().map(|p| p.id).collect();
        ids.sort_unstable();
        ids.dedup();
        assert_eq!(ids.len(), all.len());
        for want in [
            "avery-5160",
            "avery-5161",
            "avery-5162",
            "avery-5163",
            "avery-5164",
            "avery-5167",
            "avery-5371",
            "avery-5376",
            "avery-5377",
            "avery-8160",
            "avery-8163",
            "avery-22805",
            "avery-5305",
            "avery-8315",
            "l7160",
            "l7161",
            "l7162",
            "l7163",
            "l7173",
            "l7651",
            "c32011",
            "j8160",
        ] {
            assert!(sheet_preset(want).is_some(), "{want}");
        }
    }

    #[test]
    fn every_grid_fits_inside_its_sheet() {
        for p in sheet_presets() {
            assert!(p.rows >= 1 && p.cols >= 1, "{}", p.id);
            for v in [p.sheet.width, p.sheet.height, p.label.width, p.label.height] {
                assert!(v > 0.0, "{}", p.id);
            }
            assert!(p.pitch.width >= p.label.width - 0.01, "{} h overlap", p.id);
            assert!(
                p.pitch.height >= p.label.height - 0.01,
                "{} v overlap",
                p.id
            );
            let right = p.origin.x + f64::from(p.cols - 1) * p.pitch.width + p.label.width;
            let bottom = p.origin.y + f64::from(p.rows - 1) * p.pitch.height + p.label.height;
            assert!(right <= p.sheet.width + 0.01, "{} too wide", p.id);
            assert!(bottom <= p.sheet.height + 0.01, "{} too tall", p.id);
        }
    }

    #[test]
    fn avery_5160_is_thirty_labels_of_2_625_by_1_inch() {
        let p = sheet_preset("AVERY-5160").unwrap();
        assert_eq!(p.count(), 30);
        assert!((p.label.width - 189.0).abs() < 1e-9 && (p.label.height - 72.0).abs() < 1e-9);
    }
}
