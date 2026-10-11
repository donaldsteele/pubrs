//! Hyphenation and justification limits.
//!
//! Words are split with `hypher` (TeX patterns). parley 0.12 has no
//! soft-hyphen or hyphenation support, so the flow lays the paragraph out
//! over a *shadow* text: the story text with a U+00AD inserted at each
//! hyphenation point. parley treats U+00AD as a line-break opportunity and
//! shapes it with no width; the flow decides per line whether to take the
//! break, reserves the hyphen's width and draws the hyphen glyph itself.
//! Story offsets never include the inserted characters (ADR 0014).

use std::cell::RefCell;
use std::collections::HashMap;

use hypher::Lang;

/// The soft hyphen.
pub const SHY: char = '\u{ad}';
/// UTF-8 length of [`SHY`].
pub const SHY_LEN: usize = 2;

/// Fraction of the line width a ragged line may fall short before a
/// hyphenation point is taken.
pub const RAGGED_ZONE: f64 = 0.12;

/// The `hypher` language for a BCP 47 tag (`en-US`, `de_DE`, `fr`), if its
/// patterns are bundled.
pub fn lang_of(tag: &str) -> Option<Lang> {
    let primary = tag.split(['-', '_']).next()?.to_ascii_lowercase();
    let b = primary.as_bytes();
    if b.len() != 2 {
        return None;
    }
    Lang::from_iso([b[0], b[1]])
}

/// Whether `tag` has bundled patterns.
pub fn supported(tag: &str) -> bool {
    lang_of(tag).is_some()
}

type WordCache = HashMap<(Lang, Box<str>), Box<[u16]>>;
type GlyphCache = HashMap<((u64, u32), u32), Option<(u32, f32)>>;

thread_local! {
    static WORDS: RefCell<WordCache> = RefCell::new(HashMap::new());
}

const WORD_CACHE_LIMIT: usize = 20_000;

/// Byte offsets inside `word` where it may be broken, honouring the
/// language's minimum left/right lengths. Memoised per thread.
pub fn points(word: &str, lang: Lang) -> Vec<usize> {
    let (left, right) = lang.bounds();
    if word.chars().count() < left + right || word.len() > 64 {
        return Vec::new();
    }
    WORDS.with(|c| {
        let mut c = c.borrow_mut();
        if let Some(v) = c.get(&(lang, word.into())) {
            return v.iter().map(|&b| usize::from(b)).collect();
        }
        let mut offs = Vec::new();
        let mut at = 0usize;
        let mut syl = hypher::hyphenate_bounded(word, lang, left, right).peekable();
        while let Some(s) = syl.next() {
            at += s.len();
            if syl.peek().is_some() {
                offs.push(at);
            }
        }
        if c.len() >= WORD_CACHE_LIMIT {
            c.clear();
        }
        c.insert(
            (lang, word.into()),
            offs.iter().map(|&b| b as u16).collect(),
        );
        offs
    })
}

/// One place where the shadow text differs from the paragraph's story text
/// (ADR 0014, ADR 0023): an inserted soft hyphen, or a merge field shown as
/// its value (or its «Name»).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Edit {
    /// Shadow byte where the edit starts.
    pub at: usize,
    /// Its length in the shadow text.
    pub len: usize,
    /// Byte in the story text (the paragraph body) it replaces.
    pub orig: usize,
    /// Length of the story text it replaces (0 for a soft hyphen).
    pub orig_len: usize,
    /// An inserted soft hyphen.
    pub shy: bool,
    /// Inserted soft hyphens up to and including this edit.
    pub shy_after: usize,
}

/// A paragraph's text as it is shaped: the story text with soft hyphens
/// inserted at hyphenation points and merge fields replaced by what they
/// show. `edits` is the table that maps bytes between the two.
#[derive(Debug, Clone)]
pub struct Shadow<'a> {
    pub text: std::borrow::Cow<'a, str>,
    /// Edits in ascending, non-overlapping order. User-typed soft hyphens
    /// are not listed.
    pub edits: Vec<Edit>,
}

impl<'a> Shadow<'a> {
    /// A shadow with nothing inserted.
    pub fn plain(text: &'a str) -> Self {
        Self {
            text: std::borrow::Cow::Borrowed(text),
            edits: Vec::new(),
        }
    }

    /// Insert a soft hyphen at every hyphenation point of every word.
    pub fn hyphenated(orig: &'a str, lang: Lang) -> Self {
        let mut text = String::with_capacity(orig.len() + orig.len() / 4);
        let mut edits: Vec<Edit> = Vec::new();
        let mut copied = 0usize;
        let mut word_start: Option<usize> = None;
        let mut chars = orig.char_indices().peekable();
        while let Some((i, ch)) = chars.next() {
            if ch.is_alphabetic() {
                word_start.get_or_insert(i);
                if chars.peek().is_some() {
                    continue;
                }
            }
            let Some(ws) = word_start.take() else {
                continue;
            };
            let we = if ch.is_alphabetic() { orig.len() } else { i };
            // A word next to a typed soft hyphen was split by hand.
            let before = orig[..ws].chars().next_back();
            let after = orig[we..].chars().next();
            if before == Some(SHY) || after == Some(SHY) {
                continue;
            }
            let word = &orig[ws..we];
            for p in points(word, lang) {
                text.push_str(&orig[copied..ws + p]);
                copied = ws + p;
                edits.push(Edit {
                    at: text.len(),
                    len: SHY_LEN,
                    orig: ws + p,
                    orig_len: 0,
                    shy: true,
                    shy_after: edits.len() + 1,
                });
                text.push(SHY);
            }
        }
        text.push_str(&orig[copied..]);
        if edits.is_empty() {
            return Self::plain(orig);
        }
        Self {
            text: std::borrow::Cow::Owned(text),
            edits,
        }
    }

    /// Show merge fields: each `(range, shown)` replaces the story bytes
    /// `range` (a field, plus a collapsed space) by `shown`. Ranges are
    /// ascending and disjoint and lie between hyphenation points, never
    /// across one. Apply this before any other in-place text mapping.
    pub fn substitute(&mut self, subs: &[(std::ops::Range<usize>, String)]) {
        if subs.is_empty() {
            return;
        }
        let old = std::mem::take(&mut self.edits);
        let this = Shadow {
            text: std::borrow::Cow::Borrowed(""),
            edits: old.clone(),
        };
        let src: &str = &self.text;
        let mut text = String::with_capacity(src.len() + 32);
        let mut copied = 0usize;
        let mut fields: Vec<Edit> = Vec::with_capacity(subs.len());
        // Change in length each field makes, and where its story bytes end.
        let mut deltas: Vec<(usize, isize)> = Vec::with_capacity(subs.len());
        for (range, shown) in subs {
            let (s0, s1) = (this.from_orig(range.start), this.from_orig(range.end));
            deltas.push((range.end, shown.len() as isize - (s1 - s0) as isize));
            text.push_str(&src[copied..s0]);
            fields.push(Edit {
                at: text.len(),
                len: shown.len(),
                orig: range.start,
                orig_len: range.end - range.start,
                shy: false,
                shy_after: 0,
            });
            text.push_str(shown);
            copied = s1;
        }
        text.push_str(&src[copied..]);
        // Soft hyphens move by the change in length of the fields before them.
        let mut all: Vec<Edit> = old
            .into_iter()
            .map(|mut e| {
                let delta: isize = deltas
                    .iter()
                    .filter(|(end, _)| *end <= e.orig)
                    .map(|(_, d)| d)
                    .sum();
                e.at = (e.at as isize + delta) as usize;
                e
            })
            .collect();
        all.extend(fields);
        all.sort_by_key(|e| (e.at, e.orig));
        let mut shy = 0;
        for e in &mut all {
            if e.shy {
                shy += 1;
            }
            e.shy_after = shy;
        }
        self.text = std::borrow::Cow::Owned(text);
        self.edits = all;
    }

    /// Whether any merge field was substituted.
    pub fn has_fields(&self) -> bool {
        self.edits.iter().any(|e| !e.shy)
    }

    /// Inserted soft hyphens.
    pub fn shy_count(&self) -> usize {
        self.edits.last().map_or(0, |e| e.shy_after)
    }

    /// Shadow byte offsets of the inserted soft hyphens.
    pub fn shy_positions(&self) -> impl Iterator<Item = usize> + '_ {
        self.edits.iter().filter(|e| e.shy).map(|e| e.at)
    }

    /// Whether the soft hyphen at shadow byte `at` was inserted.
    pub fn is_inserted(&self, at: usize) -> bool {
        let i = self.edits.partition_point(|e| e.at < at);
        self.edits.get(i).is_some_and(|e| e.shy && e.at == at)
    }

    /// Inserted soft hyphens that start before shadow byte `b`.
    pub fn inserted_before(&self, b: usize) -> usize {
        let i = self.edits.partition_point(|e| e.at < b);
        if i == 0 {
            0
        } else {
            self.edits[i - 1].shy_after
        }
    }

    /// Shadow byte offset to byte offset in the paragraph's story text. A
    /// byte inside a field's shown value maps to the field's start; one just
    /// after it, to just after the field.
    pub fn to_orig(&self, b: usize) -> usize {
        let n = self.edits.partition_point(|e| e.at + e.len <= b);
        if let Some(e) = self.edits.get(n)
            && e.at < b
        {
            return e.orig;
        }
        match n.checked_sub(1).map(|i| &self.edits[i]) {
            Some(e) => e.orig + e.orig_len + (b - (e.at + e.len)),
            None => b,
        }
    }

    /// Story byte offset to shadow byte offset. A soft hyphen inserted
    /// exactly at `b` belongs to the text before it, so it is skipped when
    /// `b` starts a range and included when `b` ends one. A byte inside a
    /// replaced field (the collapsed space) maps to the start of its shown
    /// text.
    pub fn from_orig(&self, b: usize) -> usize {
        let n = self.edits.partition_point(|e| e.orig + e.orig_len <= b);
        if let Some(e) = self.edits.get(n)
            && e.orig < b
        {
            return e.at;
        }
        match n.checked_sub(1).map(|i| &self.edits[i]) {
            Some(e) => e.at + e.len + (b - (e.orig + e.orig_len)),
            None => b,
        }
    }

    /// Shadow byte offset to byte offset in the displayed text: the shadow
    /// text without its inserted soft hyphens (the story text with fields
    /// shown). Equal to [`Shadow::to_orig`] when no field was substituted.
    pub fn to_display(&self, b: usize) -> usize {
        let n = self.edits.partition_point(|e| e.at + e.len <= b);
        let shy = if n == 0 {
            0
        } else {
            self.edits[n - 1].shy_after
        };
        b - SHY_LEN * shy
    }
}

/// What to do with a justified line that has `free` points to fill.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum JustifyPlan {
    /// Word spaces alone absorb it: parley's justification as is.
    WordSpaces,
    /// Word spaces at their cap plus this much letter spacing in total.
    Letters(f64),
    /// More than both caps allow: leave the line short.
    Short,
}

/// Plan the stretch of a justified line: `cap_words` is the most the word
/// spaces may add, `cap_letters` the most letter spacing may add.
pub fn justify_plan(free: f64, cap_words: f64, cap_letters: f64) -> JustifyPlan {
    const EPS: f64 = 0.01;
    if free <= cap_words + EPS {
        JustifyPlan::WordSpaces
    } else if free <= cap_words + cap_letters + EPS {
        JustifyPlan::Letters(free - cap_words)
    } else {
        JustifyPlan::Short
    }
}

/// Whether a line of natural width `natural` in `width` is loose enough
/// to try a hyphenation point: justified lines when word spaces alone
/// cannot absorb the gap, ragged lines when the gap passes the zone.
pub fn is_loose(justified: bool, gap: f64, width: f64, cap_words: f64) -> bool {
    if justified {
        gap > cap_words
    } else {
        gap > width * RAGGED_ZONE
    }
}

// ---------------------------------------------------------------------
// Per-line decisions while breaking a paragraph (used by `flow`).
// ---------------------------------------------------------------------

use parley::{BreakLines, BreakReason, Layout, YieldData};
use pubrs_core::{Ink, JustifyLimits};

use crate::types::FontHandle;

/// Where a shaped cluster starts, measured on an unconstrained layout.
#[derive(Debug, Clone, Copy)]
struct Bound {
    byte: usize,
    x: f32,
    size: f32,
}

/// What the flow records for each accepted line.
#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct LineInfo {
    /// Shadow byte where the line ends.
    pub end: usize,
    /// Extra letter spacing per character, points (justified lines past
    /// their word-space cap).
    pub letter: f64,
    /// Full width of a line stretched with letter spacing.
    pub full_w: Option<f32>,
    /// The line ends with a hyphen whose width was reserved.
    pub hyphen: bool,
}

/// Inputs that stay fixed for a paragraph.
pub(crate) struct BreakParams<'a> {
    pub shadow: &'a Shadow<'a>,
    pub justify: bool,
    pub limits: Option<JustifyLimits>,
}

/// Knowledge about a paragraph's shaped text that parley does not expose
/// during line breaking: where each line ends (inferred from its advance
/// against cluster positions of an unconstrained layout), the width of the
/// hyphen at each soft hyphen, and what to do with loose lines.
pub(crate) struct LineCx {
    bounds: Vec<Bound>,
    shy_fonts: HashMap<usize, (FontHandle, f32)>,
    glyphs: GlyphCache,
    /// Shadow byte where the next line starts.
    pub pos: usize,
    ok: bool,
    /// The decision for the line `break_line` returned last.
    pub pending: Option<LineInfo>,
}

impl LineCx {
    /// Whether the line analysis can follow this paragraph.
    pub fn ok(&self) -> bool {
        self.ok
    }

    /// Measure `layout` (shaped, not yet broken) over `text`.
    pub fn new(layout: &mut Layout<Ink>, text: &str) -> Self {
        let mut cx = Self {
            bounds: Vec::new(),
            shy_fonts: HashMap::new(),
            glyphs: HashMap::new(),
            pos: 0,
            ok: !text.is_empty(),
            pending: None,
        };
        if text.is_empty() {
            return cx;
        }
        layout.break_all_lines(None);
        let mut base = 0.0f32;
        let mut last_size = 0.0f32;
        for line in layout.lines() {
            let mut x = base;
            for run in line.runs() {
                let size = run.font_size();
                last_size = size;
                for c in run.visual_clusters() {
                    let r = c.text_range();
                    if let Some(t) = text.get(r.clone()) {
                        for (i, _) in t.match_indices(SHY) {
                            let f = run.font();
                            cx.shy_fonts.insert(
                                r.start + i,
                                (
                                    FontHandle {
                                        data: f.data.clone(),
                                        index: f.index,
                                    },
                                    size,
                                ),
                            );
                        }
                    }
                    cx.bounds.push(Bound {
                        byte: r.start,
                        x,
                        size,
                    });
                    x += c.advance();
                }
            }
            base = x;
        }
        cx.bounds.push(Bound {
            byte: text.len(),
            x: base,
            size: last_size,
        });
        if !cx.bounds.windows(2).all(|w| w[0].byte < w[1].byte) {
            cx.ok = false;
        }
        cx
    }

    fn idx(&self, byte: usize) -> Option<usize> {
        self.bounds.binary_search_by_key(&byte, |b| b.byte).ok()
    }

    fn x_at(&self, byte: usize) -> Option<f32> {
        self.idx(byte).map(|i| self.bounds[i].x)
    }

    /// The cluster boundary at which a line starting at `s` with this
    /// advance ends.
    fn find_end(&self, s: usize, adv: f32) -> Option<usize> {
        let i = self.idx(s)?;
        let x0 = self.bounds[i].x;
        let tol = 0.02 + adv.abs() * 1e-5;
        let mut best = None;
        for b in &self.bounds[i + 1..] {
            let d = b.x - x0;
            if d > adv + tol {
                break;
            }
            if (d - adv).abs() <= tol {
                best = Some(b.byte);
            }
        }
        best
    }

    /// Word spaces, their total advance, characters and mean font size of
    /// the content `s..e`.
    fn stats(&self, text: &str, s: usize, e: usize) -> Option<(usize, f64, usize, f64)> {
        let (i0, i1) = (self.idx(s)?, self.idx(e)?);
        let (mut n, mut sum, mut chars, mut sizes) = (0usize, 0.0f64, 0usize, 0.0f64);
        for k in i0..i1 {
            let (b, nx) = (self.bounds[k], self.bounds[k + 1]);
            let part = text.get(b.byte..nx.byte).unwrap_or("");
            if part == " " || part == "\u{a0}" {
                n += 1;
                sum += f64::from(nx.x - b.x);
            }
            chars += part.chars().filter(|c| *c != SHY).count();
            sizes += f64::from(b.size);
        }
        let k = (i1 - i0).max(1) as f64;
        Some((n, sum, chars, sizes / k))
    }

    /// The font's hyphen glyph and its advance at `size` for the soft
    /// hyphen at shadow byte `m`.
    pub fn hyphen_glyph(&mut self, m: usize) -> Option<(FontHandle, f32, u32, f32)> {
        let (font, size) = self.shy_fonts.get(&m)?.clone();
        let key = (font.key(), size.to_bits());
        let found = *self.glyphs.entry(key).or_insert_with(|| {
            use skrifa::MetadataProvider;
            let f = skrifa::FontRef::from_index(font.bytes(), font.index).ok()?;
            let gid = f.charmap().map('-')?;
            let adv = f
                .glyph_metrics(
                    skrifa::instance::Size::new(size),
                    skrifa::instance::LocationRef::default(),
                )
                .advance_width(gid)?;
            Some((gid.to_u32(), adv))
        });
        found.map(|(g, a)| (font, size, g, a))
    }

    fn width(&self, a: usize, b: usize) -> f32 {
        match (self.x_at(a), self.x_at(b)) {
            (Some(x0), Some(x1)) => x1 - x0,
            _ => 0.0,
        }
    }

    /// Whether the line, if it ended before the word holding the soft
    /// hyphen at `m`, would be loose.
    fn loose(&self, p: &BreakParams<'_>, text: &str, s: usize, m: usize, w: f32) -> bool {
        let Some(sp) = text[s..m].rfind(char::is_whitespace) else {
            return true;
        };
        let alt = text[..s + sp].trim_end_matches(char::is_whitespace).len();
        if alt <= s {
            return true;
        }
        let Some((_, sum, _, _)) = self.stats(text, s, alt) else {
            return true;
        };
        let gap = f64::from(w - self.width(s, alt));
        let cap = (p.limits.unwrap_or_default().max_word_space - 1.0).max(0.0) * sum;
        is_loose(p.justify, gap, f64::from(w), cap)
    }

    /// Break the next line into at most `w` points. Takes a hyphenation
    /// point only when the line would otherwise be loose (always for a
    /// soft hyphen the user typed), reserves the hyphen's width, and caps
    /// justified stretch. The result is recorded in `self.pending`.
    pub fn break_line(
        &mut self,
        bl: &mut BreakLines<'_, Ink>,
        p: &BreakParams<'_>,
        text: &str,
        w: f32,
    ) -> Option<YieldData> {
        self.pending = None;
        let mut data = bl.break_next()?;
        if !self.ok {
            return Some(data);
        }
        let mut reduced = false;
        for _ in 0..12 {
            let YieldData::LineBreak(d) = &data else {
                return Some(data);
            };
            let s = self.pos;
            let Some(e) = self.find_end(s, d.metrics.advance) else {
                self.ok = false;
                return Some(data);
            };
            let regular = matches!(d.reason, BreakReason::Regular | BreakReason::Emergency);
            let adv = d.metrics.advance;
            let hang = d.metrics.hanging_advance;
            let shy_end = e >= s + SHY_LEN && text.get(e - SHY_LEN..e) == Some("\u{ad}");
            let mut hyphen: Option<(usize, f32, f32)> = None; // (marker, hyphen w, marker w)
            if shy_end {
                let m = e - SHY_LEN;
                let mw = self.width(m, e);
                let h = self.hyphen_glyph(m).map(|g| g.3);
                let first_word = !text[s..m].contains(char::is_whitespace);
                let fits = h.is_some_and(|h| adv - mw + h <= w + 0.01);
                let take = fits
                    && (first_word || !p.shadow.is_inserted(m) || self.loose(p, text, s, m, w));
                if take {
                    hyphen = Some((m, h.unwrap_or(0.0), mw));
                } else {
                    // Look for a break before this one.
                    bl.revert();
                    bl.state_mut()
                        .set_line_max_advance(((adv - mw) - 0.05).clamp(1.0, w.max(1.0)));
                    reduced = true;
                    data = bl.break_next()?;
                    continue;
                }
            }
            // Content to fill, and what to tell parley about the width.
            let (content, gaps_extra) = match hyphen {
                Some((_, h, mw)) => (adv - mw + h, 1usize),
                None => (adv - hang, 0),
            };
            let mut width: Option<f32> = match hyphen {
                Some((_, h, mw)) => Some(w - h + mw),
                None => reduced.then_some(w),
            };
            let mut letter = 0.0f64;
            let free = f64::from(w - content);
            if let Some(lim) = p.limits.filter(|_| p.justify && regular && free > 0.01) {
                let trimmed = text[..e].trim_end_matches(char::is_whitespace).len().max(s);
                let upto = hyphen.map_or(trimmed, |(m, _, _)| m);
                if let Some((n, sum, chars, size)) = self.stats(text, s, upto) {
                    let _ = n;
                    let gaps = (chars + gaps_extra).saturating_sub(1);
                    let cap_words = (lim.max_word_space - 1.0).max(0.0) * sum;
                    let cap_letters = lim.letter_spacing * size * gaps as f64;
                    let parley_content = adv - hang;
                    match justify_plan(free, cap_words, cap_letters) {
                        JustifyPlan::WordSpaces => {}
                        JustifyPlan::Letters(extra) => {
                            width = Some(parley_content + cap_words as f32);
                            letter = extra / gaps.max(1) as f64;
                        }
                        JustifyPlan::Short => width = Some(parley_content),
                    }
                }
            }
            if let Some(width) = width {
                bl.set_prior_line_width(width);
            }
            self.pending = Some(LineInfo {
                end: e,
                hyphen: hyphen.is_some(),
                letter,
                full_w: (letter > 0.0).then_some(w),
            });
            return Some(data);
        }
        self.ok = false;
        Some(data)
    }
}
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn language_tags() {
        assert!(supported("en-US"));
        assert!(supported("de_DE"));
        assert!(supported("PT-br"));
        assert!(!supported("xx"));
        assert!(!supported("el-GR"));
        assert!(!supported(""));
    }

    #[test]
    fn core_list_matches_patterns() {
        for l in pubrs_core::validate::HYPHENATION_LANGUAGES {
            assert!(supported(l), "{l}");
        }
        for a in b'a'..=b'z' {
            for b in b'a'..=b'z' {
                let tag = String::from_utf8(vec![a, b]).unwrap();
                assert_eq!(
                    supported(&tag),
                    pubrs_core::validate::hyphenation_supported(&tag),
                    "{tag}"
                );
            }
        }
    }

    #[test]
    fn english_points() {
        assert_eq!(points("extensive", Lang::English), vec![2, 5]);
        assert!(points("cat", Lang::English).is_empty());
    }

    #[test]
    fn shadow_maps_back() {
        let orig = "An extensive list.";
        let s = Shadow::hyphenated(orig, Lang::English);
        assert_eq!(s.shy_count(), 2);
        assert_eq!(s.text.replace(SHY, ""), orig);
        for p in s.shy_positions() {
            assert!(s.is_inserted(p));
            assert_eq!(s.to_orig(p), s.to_orig(p + SHY_LEN));
        }
        assert_eq!(s.to_orig(s.text.len()), orig.len());
        // Round trip at every original char boundary.
        for (i, _) in orig.char_indices() {
            assert_eq!(s.to_orig(s.from_orig(i)), i);
        }
    }

    #[test]
    fn field_edits_map_both_ways() {
        // "ab " + field (3 bytes) + " extensive": hyphenation points sit
        // after the field, so they move when the field's text changes length.
        let orig = "ab \u{fffc} extensive";
        let mut s = Shadow::hyphenated(orig, Lang::English);
        let shy: Vec<usize> = s.shy_positions().collect();
        s.substitute(&[(3..6, "LONGER".to_string())]);
        assert_eq!(s.text.replace(SHY, ""), "ab LONGER extensive");
        assert!(s.has_fields());
        let moved: Vec<usize> = s.shy_positions().collect();
        assert_eq!(moved.len(), shy.len());
        assert!(moved.iter().zip(&shy).all(|(m, o)| *m == o + 3));
        // Inside the value maps to the field's start, just after it to its end.
        assert_eq!(s.to_orig(3), 3);
        assert_eq!(s.to_orig(5), 3);
        assert_eq!(s.to_orig(9), 6);
        assert_eq!(s.from_orig(3), 3);
        assert_eq!(s.from_orig(6), 9);
        for (i, _) in orig.char_indices().filter(|(i, _)| *i != 4 && *i != 5) {
            assert_eq!(s.to_orig(s.from_orig(i)), i, "orig byte {i}");
        }
        // Displayed bytes ignore inserted soft hyphens only.
        assert_eq!(s.to_display(9), 9);
        assert_eq!(s.to_display(s.text.len()), "ab LONGER extensive".len());
        // An empty value that takes the space after it: the range covers both.
        let mut e = Shadow::plain("ab \u{fffc} cd");
        e.substitute(&[(3..7, String::new())]);
        assert_eq!(e.text, "ab cd");
        assert_eq!(e.to_orig(3), 7);
        assert_eq!(e.from_orig(3), 3);
        assert_eq!(e.from_orig(6), 3, "inside the replaced range");
        assert_eq!(e.from_orig(7), 3);
        assert_eq!(e.to_orig(e.text.len()), "ab \u{fffc} cd".len());
    }

    #[test]
    fn typed_soft_hyphen_words_are_left_alone() {
        let s = Shadow::hyphenated("exten\u{ad}sive", Lang::English);
        assert_eq!(s.shy_count(), 0);
    }

    #[test]
    fn plans() {
        assert_eq!(justify_plan(3.0, 4.0, 5.0), JustifyPlan::WordSpaces);
        assert_eq!(justify_plan(6.0, 4.0, 5.0), JustifyPlan::Letters(2.0));
        assert_eq!(justify_plan(10.0, 4.0, 5.0), JustifyPlan::Short);
    }

    #[test]
    fn looseness() {
        assert!(is_loose(true, 10.0, 200.0, 4.0));
        assert!(!is_loose(true, 3.0, 200.0, 4.0));
        assert!(is_loose(false, 40.0, 200.0, 4.0));
        assert!(!is_loose(false, 10.0, 200.0, 4.0));
    }
}
