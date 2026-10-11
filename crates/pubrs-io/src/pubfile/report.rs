//! What an import kept and what it could not, and the budget that bounds it.

use std::collections::HashSet;

use serde::{Deserialize, Serialize};

/// Most items (fonts, colours, runs, paragraphs, stories, pages, shapes, ...)
/// kept from one file, across every file-driven collection together.
pub const MAX_TOTAL_ITEMS: usize = 1_000_000;
/// Work allowance: this many bytes of parsing per input byte, plus a constant.
pub const WORK_PER_INPUT_BYTE: usize = 8;
pub const WORK_BASE: usize = 1 << 20;

/// The global parse budget of one import. Every file-driven collection charges
/// the item counter, every walk over stream bytes charges the work counter, and
/// each (stream, offset, kind) chunk is parsed once. When either counter runs
/// out, parsing stops and the report says so.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Budget {
    items_left: usize,
    work_left: usize,
    exhausted: bool,
    visited: HashSet<(u8, usize, u32)>,
}

impl Default for Budget {
    fn default() -> Self {
        Self {
            items_left: MAX_TOTAL_ITEMS,
            work_left: WORK_BASE,
            exhausted: false,
            visited: HashSet::new(),
        }
    }
}

/// One kind of thing the importer dropped or did not understand.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Dropped {
    /// Short machine-readable kind, for example `unknown-block`.
    pub kind: String,
    /// How many were dropped.
    pub count: usize,
    /// A human-readable detail (the first example seen).
    pub detail: String,
}

/// The result of an import, beside the document.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ImportReport {
    /// Pages created.
    pub pages: usize,
    /// Frames created.
    pub frames: usize,
    /// Everything dropped, by kind.
    pub dropped: Vec<Dropped>,
    /// The parse budget; not part of the serialised report.
    #[serde(skip)]
    pub(crate) budget: Budget,
}

impl ImportReport {
    /// Note one dropped item. Items of the same `kind` share one entry; the
    /// first `detail` is kept.
    pub fn drop_item(&mut self, kind: &str, detail: impl Into<String>) {
        self.drop_many(kind, 1, detail);
    }

    /// Note `count` dropped items of one kind.
    pub fn drop_many(&mut self, kind: &str, count: usize, detail: impl Into<String>) {
        if count == 0 {
            return;
        }
        if let Some(d) = self.dropped.iter_mut().find(|d| d.kind == kind) {
            d.count += count;
        } else {
            self.dropped.push(Dropped {
                kind: kind.to_string(),
                count,
                detail: detail.into(),
            });
        }
    }

    /// Total number of dropped items.
    pub fn dropped_total(&self) -> usize {
        self.dropped.iter().map(|d| d.count).sum()
    }

    /// Whether the parse budget ran out (the import is then partial).
    pub fn truncated(&self) -> bool {
        self.budget.exhausted
    }

    /// Size the work allowance from the total bytes of the input streams.
    pub(crate) fn set_input(&mut self, total_bytes: usize) {
        self.budget.work_left = total_bytes
            .saturating_mul(WORK_PER_INPUT_BYTE)
            .saturating_add(WORK_BASE);
    }

    fn exhaust(&mut self) -> bool {
        if !self.budget.exhausted {
            self.budget.exhausted = true;
            self.drop_item("truncated", "parse budget exhausted");
        }
        false
    }

    /// Whether the budget has run out.
    pub(crate) fn exhausted(&self) -> bool {
        self.budget.exhausted
    }

    /// Charge `n` kept items. `false` (and a report entry) when out of budget.
    pub(crate) fn charge_items(&mut self, n: usize) -> bool {
        if self.budget.exhausted {
            return false;
        }
        if self.budget.items_left < n {
            return self.exhaust();
        }
        self.budget.items_left -= n;
        true
    }

    /// Charge `n` bytes of parsing work.
    pub(crate) fn charge_work(&mut self, n: usize) -> bool {
        if self.budget.exhausted {
            return false;
        }
        if self.budget.work_left < n {
            return self.exhaust();
        }
        self.budget.work_left -= n;
        true
    }

    /// True the first time a (stream, offset, kind) chunk is seen; later
    /// references are skipped and counted as `duplicate-chunk`.
    pub(crate) fn first_visit(&mut self, stream: u8, offset: usize, kind: u32) -> bool {
        if self.budget.visited.insert((stream, offset, kind)) {
            true
        } else {
            self.drop_item(
                "duplicate-chunk",
                format!("stream {stream} offset 0x{offset:x} referenced more than once"),
            );
            false
        }
    }
}
