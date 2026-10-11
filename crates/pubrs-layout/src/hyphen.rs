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

/// A paragraph's text with soft hyphens inserted at hyphenation points.
#[derive(Debug, Clone)]
pub struct Shadow<'a> {
    pub text: std::borrow::Cow<'a, str>,
    /// Byte offsets in `text` of the inserted soft hyphens, ascending.
    /// User-typed soft hyphens are not listed.
    pub inserted: Vec<usize>,
}

impl<'a> Shadow<'a> {
    /// A shadow with nothing inserted.
    pub fn plain(text: &'a str) -> Self {
        Self {
            text: std::borrow::Cow::Borrowed(text),
            inserted: Vec::new(),
        }
    }

    /// Insert a soft hyphen at every hyphenation point of every word.
    pub fn hyphenated(orig: &'a str, lang: Lang) -> Self {
        let mut text = String::with_capacity(orig.len() + orig.len() / 4);
        let mut inserted = Vec::new();
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
                inserted.push(text.len());
                text.push(SHY);
            }
        }
        text.push_str(&orig[copied..]);
        if inserted.is_empty() {
            return Self::plain(orig);
        }
        Self {
            text: std::borrow::Cow::Owned(text),
            inserted,
        }
    }

    /// Whether the soft hyphen at shadow byte `at` was inserted.
    pub fn is_inserted(&self, at: usize) -> bool {
        self.inserted.binary_search(&at).is_ok()
    }

    /// Inserted soft hyphens that start before shadow byte `b`.
    pub fn inserted_before(&self, b: usize) -> usize {
        self.inserted.partition_point(|&p| p < b)
    }

    /// Shadow byte offset to byte offset in the original paragraph text.
    pub fn to_orig(&self, b: usize) -> usize {
        b - SHY_LEN * self.inserted_before(b)
    }

    /// Original byte offset to shadow byte offset. A soft hyphen inserted
    /// exactly at `b` belongs to the text before it, so it is skipped when
    /// `b` starts a range and included when `b` ends one.
    pub fn from_orig(&self, b: usize) -> usize {
        // Marker k sits at shadow byte p_k = orig position + 2k, so its
        // original position is p_k - 2k. Count markers at or before `b`.
        let n = self
            .inserted
            .iter()
            .enumerate()
            .take_while(|(k, p)| **p - SHY_LEN * k <= b)
            .count();
        b + SHY_LEN * n
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
use pubrs_core::{Color, JustifyLimits};

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
    pub fn new(layout: &mut Layout<Color>, text: &str) -> Self {
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
        bl: &mut BreakLines<'_, Color>,
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
        assert_eq!(s.inserted.len(), 2);
        assert_eq!(s.text.replace(SHY, ""), orig);
        for &p in &s.inserted {
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
    fn typed_soft_hyphen_words_are_left_alone() {
        let s = Shadow::hyphenated("exten\u{ad}sive", Lang::English);
        assert!(s.inserted.is_empty());
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
