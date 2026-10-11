//! Editing operations on stories.
//!
//! Text positions are **character offsets** (Unicode scalar values) into the
//! story's plain text, in which paragraphs are joined by a single `\n`.
//! Offset 0 is before the first character; `char_len()` is after the last.

use crate::model::{Paragraph, Run, Story};
use crate::style::{CharProps, ParaProps};
use serde::{Deserialize, Serialize};

use crate::id::StyleId;

/// The formatting of a run, without its text.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct RunFormat {
    /// Character style.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub style: Option<StyleId>,
    #[serde(flatten)]
    pub props: CharProps,
}

impl RunFormat {
    fn of(run: &Run) -> Self {
        Self {
            style: run.style,
            props: run.props.clone(),
        }
    }

    fn run(&self, text: &str) -> Run {
        Run {
            text: text.to_string(),
            style: self.style,
            props: self.props.clone(),
        }
    }
}

fn char_to_byte(s: &str, chars: usize) -> usize {
    s.char_indices().nth(chars).map_or(s.len(), |(b, _)| b)
}

/// Normalise line endings to `\n`.
pub fn normalize_newlines(text: &str) -> String {
    text.replace("\r\n", "\n").replace('\r', "\n")
}

/// Ensure a run boundary at `at` (chars into the paragraph). Returns the index
/// of the first run starting at or after `at`.
fn split_at(p: &mut Paragraph, at: usize) -> usize {
    let mut pos = 0;
    for i in 0..p.runs.len() {
        if at == pos {
            return i;
        }
        let len = p.runs[i].text.chars().count();
        if at < pos + len {
            let byte = char_to_byte(&p.runs[i].text, at - pos);
            let tail = p.runs[i].text.split_off(byte);
            let mut new = p.runs[i].clone();
            new.text = tail;
            p.runs.insert(i + 1, new);
            return i + 1;
        }
        pos += len;
    }
    p.runs.len()
}

/// Merge adjacent runs with equal formatting and drop empty runs, keeping at
/// least one run (so an empty paragraph remembers its formatting).
pub fn normalize_paragraph(p: &mut Paragraph) {
    if p.runs.is_empty() {
        p.runs.push(Run::default());
        return;
    }
    if p.runs.iter().any(|r| !r.text.is_empty()) {
        p.runs.retain(|r| !r.text.is_empty());
    } else {
        p.runs.truncate(1);
    }
    let mut out: Vec<Run> = Vec::with_capacity(p.runs.len());
    for r in p.runs.drain(..) {
        match out.last_mut() {
            Some(last) if last.same_format(&r) => last.text.push_str(&r.text),
            _ => out.push(r),
        }
    }
    p.runs = out;
}

impl Story {
    /// The story as plain text, paragraphs separated by `\n`.
    pub fn plain_text(&self) -> String {
        let parts: Vec<String> = self.paragraphs.iter().map(Paragraph::text).collect();
        parts.join("\n")
    }

    /// Length in characters, counting one `\n` between paragraphs.
    pub fn char_len(&self) -> usize {
        let n: usize = self.paragraphs.iter().map(Paragraph::char_len).sum();
        n + self.paragraphs.len().saturating_sub(1)
    }

    /// Paragraph index and character offset within it for a story offset.
    /// Offsets past the end clamp to the end.
    pub fn locate(&self, offset: usize) -> (usize, usize) {
        let mut remaining = offset;
        for (i, p) in self.paragraphs.iter().enumerate() {
            let len = p.char_len();
            if remaining <= len {
                return (i, remaining);
            }
            remaining -= len + 1;
        }
        let last = self.paragraphs.len().saturating_sub(1);
        (
            last,
            self.paragraphs.get(last).map_or(0, Paragraph::char_len),
        )
    }

    /// Story offset of the start of paragraph `index`.
    pub fn paragraph_start(&self, index: usize) -> usize {
        self.paragraphs
            .iter()
            .take(index)
            .map(|p| p.char_len() + 1)
            .sum()
    }

    /// The run format in effect at `offset`: that of the character before it,
    /// or of the first run when at the start of a paragraph.
    pub fn format_at(&self, offset: usize) -> RunFormat {
        let (pi, c) = self.locate(offset);
        let Some(p) = self.paragraphs.get(pi) else {
            return RunFormat::default();
        };
        let mut pos = 0;
        for r in &p.runs {
            let len = r.text.chars().count();
            if c > pos && c <= pos + len {
                return RunFormat::of(r);
            }
            pos += len;
        }
        p.runs.first().map(RunFormat::of).unwrap_or_default()
    }

    /// Insert `text` at `at`. `\n` starts a new paragraph that copies the
    /// paragraph format. With no `format`, the text takes the format in effect
    /// at the insertion point.
    pub fn insert_text(&mut self, at: usize, text: &str, format: Option<RunFormat>) {
        if self.paragraphs.is_empty() {
            self.paragraphs.push(Paragraph::default());
        }
        let text = normalize_newlines(text);
        let fmt = format.unwrap_or_else(|| self.format_at(at));
        let (pi, c) = self.locate(at);
        let i = split_at(&mut self.paragraphs[pi], c);
        let tail: Vec<Run> = self.paragraphs[pi].runs.drain(i..).collect();
        let mut lines = text.split('\n');
        let first = lines.next().unwrap_or("");
        self.paragraphs[pi].runs.push(fmt.run(first));
        let mut last = pi;
        for line in lines {
            let template = &self.paragraphs[pi];
            let p = Paragraph {
                style: template.style,
                props: template.props.clone(),
                runs: vec![fmt.run(line)],
            };
            last += 1;
            self.paragraphs.insert(last, p);
        }
        self.paragraphs[last].runs.extend(tail);
        normalize_paragraph(&mut self.paragraphs[pi]);
        normalize_paragraph(&mut self.paragraphs[last]);
    }

    /// Delete the characters in `start..end`. Joining paragraphs keeps the
    /// first paragraph's format.
    pub fn delete_range(&mut self, start: usize, end: usize) {
        let (start, end) = (start.min(end), start.max(end));
        if start == end || self.paragraphs.is_empty() {
            return;
        }
        let keep_fmt = self.format_at(start + 1);
        let (p1, c1) = self.locate(start);
        let (p2, c2) = self.locate(end);
        let j = split_at(&mut self.paragraphs[p2], c2);
        let tail: Vec<Run> = self.paragraphs[p2].runs.drain(j..).collect();
        let i = split_at(&mut self.paragraphs[p1], c1);
        self.paragraphs[p1].runs.truncate(i);
        if self.paragraphs[p1].runs.is_empty() && tail.is_empty() {
            self.paragraphs[p1].runs.push(keep_fmt.run(""));
        }
        self.paragraphs[p1].runs.extend(tail);
        if p2 > p1 {
            self.paragraphs.drain(p1 + 1..=p2);
        }
        normalize_paragraph(&mut self.paragraphs[p1]);
    }

    /// Replace `start..end` with `text`, keeping the format of the replaced text.
    pub fn replace_range(&mut self, start: usize, end: usize, text: &str) {
        let (start, end) = (start.min(end), start.max(end));
        let fmt = if end > start {
            self.format_at(start + 1)
        } else {
            self.format_at(start)
        };
        self.delete_range(start, end);
        self.insert_text(start, text, Some(fmt));
    }

    /// Replace the whole story with plain text, keeping the first run's format
    /// and the first paragraph's format.
    pub fn set_plain_text(&mut self, text: &str) {
        let len = self.char_len();
        self.replace_range(0, len, text);
    }

    /// Apply `f` to every run (split as needed) covering `start..end`.
    pub fn update_runs(&mut self, start: usize, end: usize, f: impl Fn(&mut Run)) {
        let (start, end) = (start.min(end), start.max(end));
        if start == end {
            return;
        }
        let (p1, c1) = self.locate(start);
        let (p2, c2) = self.locate(end);
        for pi in p1..=p2 {
            let p = &mut self.paragraphs[pi];
            let local_start = if pi == p1 { c1 } else { 0 };
            let local_end = if pi == p2 { c2 } else { p.char_len() };
            if local_start >= local_end {
                continue;
            }
            let i = split_at(p, local_start);
            let j = split_at(p, local_end);
            for r in &mut p.runs[i..j] {
                f(r);
            }
            normalize_paragraph(p);
        }
    }

    /// Merge `props` into every run in `start..end`.
    pub fn apply_char_props(&mut self, start: usize, end: usize, props: &CharProps) {
        self.update_runs(start, end, |r| r.props = r.props.merged(props));
    }

    /// Indices of paragraphs touched by `start..end` (at least the one containing `start`).
    pub fn paragraph_span(&self, start: usize, end: usize) -> std::ops::RangeInclusive<usize> {
        let (start, end) = (start.min(end), start.max(end));
        let (p1, _) = self.locate(start);
        let (p2, _) = self.locate(end);
        p1..=p2
    }

    /// Apply `f` to every paragraph touched by `start..end`.
    pub fn update_paragraphs(&mut self, start: usize, end: usize, f: impl Fn(&mut Paragraph)) {
        for pi in self.paragraph_span(start, end) {
            f(&mut self.paragraphs[pi]);
        }
    }

    /// Merge `props` into every paragraph touched by `start..end`.
    pub fn apply_para_props(&mut self, start: usize, end: usize, props: &ParaProps) {
        self.update_paragraphs(start, end, |p| p.props = p.props.merged(props));
    }

    /// Paragraphs covering `start..end`, trimmed to the range.
    pub fn slice(&self, start: usize, end: usize) -> Vec<Paragraph> {
        let mut copy = self.clone();
        let len = copy.char_len();
        let (start, end) = (start.min(end).min(len), start.max(end).min(len));
        copy.delete_range(end, len);
        copy.delete_range(0, start);
        copy.paragraphs
    }

    /// True if the story has no characters.
    pub fn is_empty(&self) -> bool {
        self.char_len() == 0
    }
}

/// Options for [`find_matches`].
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct FindOptions {
    /// Match letter case exactly.
    pub case_sensitive: bool,
    /// Only match whole words (neighbours are not Unicode alphanumerics).
    pub whole_word: bool,
}

fn fold(c: char, case_sensitive: bool) -> char {
    if case_sensitive {
        return c;
    }
    let mut it = c.to_lowercase();
    match (it.next(), it.next()) {
        (Some(l), None) => l,
        _ => c,
    }
}

fn is_word(c: char) -> bool {
    c.is_alphanumeric() || c == '_'
}

/// Find `query` in `text`; returns half-open char-offset ranges, in order,
/// non-overlapping. `limit` restricts matches to lie inside `start..end`.
/// Paragraph breaks are `\n`, so a match only spans paragraphs when the query
/// contains `\n`. An empty query matches nothing.
pub fn find_matches(
    text: &str,
    query: &str,
    opts: FindOptions,
    limit: Option<(usize, usize)>,
) -> Vec<(usize, usize)> {
    let hay: Vec<char> = text.chars().collect();
    let needle: Vec<char> = query
        .chars()
        .map(|c| fold(c, opts.case_sensitive))
        .collect();
    let n = needle.len();
    let mut out = Vec::new();
    if n == 0 || n > hay.len() {
        return out;
    }
    let (lo, hi) = limit.map_or((0, hay.len()), |(a, b)| {
        (a.min(b).min(hay.len()), a.max(b).min(hay.len()))
    });
    let mut i = lo;
    while i + n <= hi {
        let hit = (0..n).all(|k| fold(hay[i + k], opts.case_sensitive) == needle[k]);
        let ok = hit
            && (!opts.whole_word || {
                let before = i > 0 && is_word(hay[i - 1]);
                let after = i + n < hay.len() && is_word(hay[i + n]);
                !(before && is_word(hay[i])) && !(after && is_word(hay[i + n - 1]))
            });
        if ok {
            out.push((i, i + n));
            i += n;
        } else {
            i += 1;
        }
    }
    out
}

impl Story {
    /// Matches of `query` in this story's plain text (see [`find_matches`]).
    pub fn find(
        &self,
        query: &str,
        opts: FindOptions,
        limit: Option<(usize, usize)>,
    ) -> Vec<(usize, usize)> {
        find_matches(&self.plain_text(), query, opts, limit)
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn find_basic_and_case() {
        let t = "Straße straße STRASSE\nÉcole école";
        let o = FindOptions::default();
        assert_eq!(find_matches(t, "straße", o, None), vec![(0, 6), (7, 13)]);
        let cs = FindOptions {
            case_sensitive: true,
            ..o
        };
        assert_eq!(find_matches(t, "straße", cs, None), vec![(7, 13)]);
        assert_eq!(find_matches(t, "école", o, None), vec![(22, 27), (28, 33)]);
        assert!(find_matches(t, "", o, None).is_empty());
    }

    #[test]
    fn find_whole_word_and_paragraphs() {
        let t = "cat concat cat.\ncat\ncats";
        let w = FindOptions {
            whole_word: true,
            ..Default::default()
        };
        assert_eq!(
            find_matches(t, "cat", w, None),
            vec![(0, 3), (11, 14), (16, 19)]
        );
        assert_eq!(
            find_matches(t, "cat", FindOptions::default(), None).len(),
            5
        );
        let d = FindOptions::default();
        assert!(find_matches("ab\ncd", "bc", d, None).is_empty());
        assert_eq!(find_matches("ab\ncd", "b\nc", d, None), vec![(1, 4)]);
    }

    #[test]
    fn find_limit_range() {
        let t = "aa aa aa";
        let o = FindOptions::default();
        assert_eq!(find_matches(t, "aa", o, Some((3, 8))), vec![(3, 5), (6, 8)]);
        assert_eq!(find_matches(t, "aa", o, Some((4, 8))), vec![(6, 8)]);
        let s = story("x foo\nfoo y");
        assert_eq!(s.find("foo", o, None), vec![(2, 5), (6, 9)]);
    }

    use super::*;
    use crate::id::{FrameId, SeqIds, StoryId};

    fn story(text: &str) -> Story {
        let mut ids = SeqIds::new(9);
        let mut s = Story::new(StoryId::new(&mut ids), FrameId::new(&mut ids));
        s.insert_text(0, text, None);
        s
    }

    fn bold() -> CharProps {
        CharProps {
            bold: Some(true),
            ..Default::default()
        }
    }

    #[test]
    fn insert_splits_paragraphs() {
        let s = story("Hello\nWorld");
        assert_eq!(s.paragraphs.len(), 2);
        assert_eq!(s.plain_text(), "Hello\nWorld");
        assert_eq!(s.char_len(), 11);
        assert_eq!(s.locate(5), (0, 5));
        assert_eq!(s.locate(6), (1, 0));
        assert_eq!(s.locate(99), (1, 5));
    }

    #[test]
    fn insert_in_middle_inherits_format() {
        let mut s = story("Hello World");
        s.apply_char_props(0, 5, &bold());
        s.insert_text(2, "XX", None);
        assert_eq!(s.plain_text(), "HeXXllo World");
        assert_eq!(s.paragraphs[0].runs.len(), 2);
        assert_eq!(s.paragraphs[0].runs[0].text, "HeXXllo");
        assert_eq!(s.paragraphs[0].runs[0].props.bold, Some(true));
    }

    #[test]
    fn delete_across_paragraphs_joins() {
        let mut s = story("One\nTwo\nThree");
        s.delete_range(2, 9);
        assert_eq!(s.plain_text(), "Onhree");
        assert_eq!(s.paragraphs.len(), 1);
    }

    #[test]
    fn delete_everything_keeps_one_formatted_paragraph() {
        let mut s = story("abc\ndef");
        s.apply_char_props(0, 7, &bold());
        s.delete_range(0, 7);
        assert_eq!(s.plain_text(), "");
        assert_eq!(s.paragraphs.len(), 1);
        assert_eq!(s.paragraphs[0].runs.len(), 1);
        assert_eq!(s.paragraphs[0].runs[0].props.bold, Some(true));
    }

    #[test]
    fn replace_keeps_format_of_replaced_text() {
        let mut s = story("Hello World");
        s.apply_char_props(6, 11, &bold());
        s.replace_range(6, 11, "There\nFriend");
        assert_eq!(s.plain_text(), "Hello There\nFriend");
        assert_eq!(s.paragraphs[0].runs[1].text, "There");
        assert_eq!(s.paragraphs[0].runs[1].props.bold, Some(true));
        assert_eq!(s.paragraphs[1].runs[0].props.bold, Some(true));
    }

    #[test]
    fn char_props_across_paragraphs_and_unicode() {
        let mut s = story("héllo\nwörld");
        s.apply_char_props(3, 8, &bold());
        let p0 = &s.paragraphs[0];
        assert_eq!(p0.runs[0].text, "hél");
        assert_eq!(p0.runs[1].text, "lo");
        assert_eq!(s.paragraphs[1].runs[0].text, "wö");
        assert_eq!(s.paragraphs[1].runs[0].props.bold, Some(true));
        assert_eq!(s.paragraphs[1].runs[1].text, "rld");
        // Applying the inverse merges runs back.
        s.apply_char_props(
            0,
            11,
            &CharProps {
                bold: Some(true),
                ..Default::default()
            },
        );
        assert_eq!(s.paragraphs[0].runs.len(), 1);
    }

    #[test]
    fn para_props_apply_to_touched_paragraphs() {
        let mut s = story("a\nb\nc");
        s.apply_para_props(
            2,
            2,
            &ParaProps {
                space_after: Some(3.0),
                ..Default::default()
            },
        );
        assert_eq!(s.paragraphs[0].props.space_after, None);
        assert_eq!(s.paragraphs[1].props.space_after, Some(3.0));
        assert_eq!(s.paragraphs[2].props.space_after, None);
    }

    #[test]
    fn slice_extracts_range() {
        let s = story("One\nTwo\nThree");
        let parts = s.slice(2, 9);
        let texts: Vec<String> = parts.iter().map(Paragraph::text).collect();
        assert_eq!(texts, vec!["e", "Two", "T"]);
    }

    #[test]
    fn crlf_is_normalised() {
        let s = story("a\r\nb\rc");
        assert_eq!(s.plain_text(), "a\nb\nc");
    }
}
