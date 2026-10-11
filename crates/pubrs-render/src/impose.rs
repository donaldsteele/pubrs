//! Imposition: label and card sheets, and saddle-stitched booklets.
//!
//! Imposition works on finished [`PageScene`]s. Each output sheet is one
//! sheet-sized scene whose nodes are the document pages' nodes, translated to
//! their cell and clipped to it, so the PDF writer and the rasteriser need no
//! special cases. Sheets carry no bleed.

use pubrs_core::{PageSetup, Rect, SheetFill, SheetLayout};

use crate::scene::{Affine, Node, PageScene};

/// Why a document cannot be imposed.
#[derive(Debug, Clone, PartialEq, thiserror::Error)]
pub enum ImposeError {
    #[error("no pages to impose")]
    NoPages,
    #[error(
        "the label sheet is for {lw:.3} x {lh:.3} in labels but the page is {pw:.3} x {ph:.3} in; set the page size to {lw:.3} x {lh:.3} in first"
    )]
    SizeMismatch { lw: f64, lh: f64, pw: f64, ph: f64 },
}

/// Which document page (zero-based) goes in each cell of each sheet, row by
/// row. `None` is an empty cell.
pub fn sheet_plan(sheet: &SheetLayout, pages: usize) -> Vec<Vec<Option<usize>>> {
    let cells = sheet.cells();
    match sheet.fill {
        SheetFill::Repeat => (0..pages).map(|p| vec![Some(p); cells]).collect(),
        SheetFill::Sequence => (0..pages.div_ceil(cells.max(1)))
            .map(|s| {
                (0..cells)
                    .map(|c| {
                        let p = s * cells + c;
                        (p < pages).then_some(p)
                    })
                    .collect()
            })
            .collect(),
    }
}

/// One side of a booklet sheet: the left and right page (zero-based, `None`
/// for a blank page added by padding).
pub type BookletSide = [Option<usize>; 2];

/// Saddle-stitch order for `pages` pages, padded to a multiple of four. Each
/// entry is one sheet: `[front, back]`. For 8 pages: front [8, 1], back
/// [2, 7]; front [6, 3], back [4, 5] (one-based).
pub fn booklet_plan(pages: usize) -> Vec<[BookletSide; 2]> {
    let m = pages.div_ceil(4) * 4;
    let page = |one_based: usize| (one_based <= pages).then(|| one_based - 1);
    (0..m / 4)
        .map(|k| {
            [
                [page(m - 2 * k), page(2 * k + 1)],
                [page(2 * k + 2), page(m - 2 * k - 1)],
            ]
        })
        .collect()
}

/// Whether imposition applies to this setup.
pub fn is_imposed(setup: &PageSetup) -> bool {
    setup.sheet.is_some() || setup.booklet
}

/// Impose `pages` according to the setup. Without a sheet or booklet the
/// pages come back unchanged. A sheet takes precedence over a booklet (the
/// commands never allow both).
pub fn impose(pages: &[PageScene], setup: &PageSetup) -> Result<Vec<PageScene>, ImposeError> {
    if let Some(sheet) = &setup.sheet {
        impose_sheet(pages, sheet)
    } else if setup.booklet {
        impose_booklet(pages)
    } else {
        Ok(pages.to_vec())
    }
}

/// Put the pages N-up on label sheets.
pub fn impose_sheet(
    pages: &[PageScene],
    sheet: &SheetLayout,
) -> Result<Vec<PageScene>, ImposeError> {
    let first = pages.first().ok_or(ImposeError::NoPages)?;
    let (lw, lh) = (sheet.label_size.width, sheet.label_size.height);
    if (first.width - lw).abs() > 0.5 || (first.height - lh).abs() > 0.5 {
        return Err(ImposeError::SizeMismatch {
            lw: lw / 72.0,
            lh: lh / 72.0,
            pw: first.width / 72.0,
            ph: first.height / 72.0,
        });
    }
    Ok(sheet_plan(sheet, pages.len())
        .into_iter()
        .map(|cells| {
            let mut out = blank(sheet.sheet_size.width, sheet.sheet_size.height);
            for (i, p) in cells.into_iter().enumerate() {
                if let Some(p) = p {
                    let o = sheet.cell_origin(i);
                    place(&mut out, &pages[p], o.x, o.y, lw, lh);
                }
            }
            out
        })
        .collect())
}

/// Reorder pages for a saddle-stitched booklet, 2-up on a sheet twice the
/// page width (front and back of each sheet are separate pages).
pub fn impose_booklet(pages: &[PageScene]) -> Result<Vec<PageScene>, ImposeError> {
    let first = pages.first().ok_or(ImposeError::NoPages)?;
    let (w, h) = (first.width, first.height);
    Ok(booklet_plan(pages.len())
        .into_iter()
        .flatten()
        .map(|side| {
            let mut out = blank(2.0 * w, h);
            for (slot, p) in side.into_iter().enumerate() {
                if let Some(p) = p {
                    place(&mut out, &pages[p], slot as f64 * w, 0.0, w, h);
                }
            }
            out
        })
        .collect())
}

fn blank(width: f64, height: f64) -> PageScene {
    PageScene {
        width,
        height,
        bleed: 0.0,
        nodes: Vec::new(),
    }
}

/// Append `page`'s nodes at (`x`, `y`), clipped to the `w` x `h` cell.
fn place(out: &mut PageScene, page: &PageScene, x: f64, y: f64, w: f64, h: f64) {
    let shift = Affine::translate(x, y);
    let clip = Some(Rect::new(x, y, w, h));
    out.nodes.extend(page.nodes.iter().map(|n| Node {
        transform: n.transform.then(shift),
        clip,
        ..n.clone()
    }));
}

/// How many pages the imposed output has for `pages` document pages.
pub fn output_pages(setup: &PageSetup, pages: usize) -> usize {
    if pages == 0 {
        0
    } else if let Some(sheet) = &setup.sheet {
        sheet_plan(sheet, pages).len()
    } else if setup.booklet {
        booklet_plan(pages).len() * 2
    } else {
        pages
    }
}

/// What the output of a setup is called: `"sheet"`, `"booklet"`, or `None`.
pub fn kind(setup: &PageSetup) -> Option<&'static str> {
    if setup.sheet.is_some() {
        Some("sheet")
    } else if setup.booklet {
        Some("booklet")
    } else {
        None
    }
}
