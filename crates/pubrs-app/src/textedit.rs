//! In-place text editing: caret/selection state and geometry from layout.

use pubrs_api::core::{Document, FrameId, Point, StoryId};
use pubrs_api::layout::{DocumentLayout, LineBox};

/// Editing state for one story.
#[derive(Debug, Clone, PartialEq)]
pub struct TextEdit {
    /// Frame the user is editing in (for scrolling/visibility).
    pub frame: FrameId,
    pub story: StoryId,
    /// Caret position (character offset).
    pub caret: usize,
    /// Selection anchor; equals `caret` when nothing is selected.
    pub anchor: usize,
    /// Remembered x for up/down movement (frame-local points).
    pub goal_x: Option<f64>,
}

impl TextEdit {
    pub fn new(frame: FrameId, story: StoryId, at: usize) -> Self {
        Self {
            frame,
            story,
            caret: at,
            anchor: at,
            goal_x: None,
        }
    }

    pub fn range(&self) -> (usize, usize) {
        (self.caret.min(self.anchor), self.caret.max(self.anchor))
    }

    pub fn has_selection(&self) -> bool {
        self.caret != self.anchor
    }

    pub fn set(&mut self, at: usize, extend: bool) {
        self.caret = at;
        if !extend {
            self.anchor = at;
        }
    }
}

/// Offset of a story's lines inside `frame`: zero for text frames, the
/// cell's origin for the cell story of a table (cell text is laid out in
/// cell-local space).
pub fn story_shift(layout: &DocumentLayout, frame: FrameId, story: StoryId) -> (f64, f64) {
    layout
        .cell_origin(frame, story)
        .map_or((0.0, 0.0), |p| (p.x, p.y))
}

/// Map a frame-local point to page space.
pub fn frame_to_page(doc: &Document, frame: FrameId, x: f64, y: f64) -> Option<Point> {
    let f = doc.frame(frame)?;
    Some(Point::new(f.rect.x + x, f.rect.y + y).rotated_about(f.rect.center(), f.rotation))
}

/// Map a page point into a frame's local space.
pub fn page_to_frame(doc: &Document, frame: FrameId, p: Point) -> Option<(f64, f64)> {
    let f = doc.frame(frame)?;
    let l = p.rotated_about(f.rect.center(), -f.rotation);
    Some((l.x - f.rect.x, l.y - f.rect.y))
}

/// All lines of a story in flow order with the frame each is in.
pub fn story_lines<'a>(
    doc: &Document,
    layout: &'a DocumentLayout,
    story: StoryId,
) -> Vec<(FrameId, &'a LineBox)> {
    let Some(s) = doc.story(story) else {
        return Vec::new();
    };
    let Some(sl) = layout.stories.get(&story) else {
        return Vec::new();
    };
    s.frames
        .iter()
        .filter_map(|f| sl.frames.get(f).map(|ft| (*f, ft)))
        .flat_map(|(f, ft)| ft.lines.iter().map(move |l| (f, l)))
        .collect()
}

/// Index of the line showing `offset` (the later line at a soft-wrap boundary).
pub fn line_for(lines: &[(FrameId, &LineBox)], offset: usize) -> Option<usize> {
    let mut best = None;
    for (i, (_, l)) in lines.iter().enumerate() {
        if l.chars.start <= offset && offset <= l.chars.end {
            best = Some(i);
            if offset < l.chars.end || l.ends_paragraph {
                break;
            }
        }
    }
    best.or_else(|| (!lines.is_empty()).then(|| lines.len() - 1))
}

/// Caret as a page-space segment (top, bottom) and the frame it is in.
pub fn caret_segment(
    doc: &Document,
    layout: &DocumentLayout,
    story: StoryId,
    offset: usize,
) -> Option<(FrameId, Point, Point)> {
    let lines = story_lines(doc, layout, story);
    let i = line_for(&lines, offset)?;
    let (f, l) = lines[i];
    let (sx, sy) = story_shift(layout, f, story);
    let x = l.caret_x(offset.clamp(l.chars.start, l.chars.end)) + sx;
    Some((
        f,
        frame_to_page(doc, f, x, l.rect.y + sy)?,
        frame_to_page(doc, f, x, l.rect.bottom() + sy)?,
    ))
}

/// Character offset nearest a page point inside one frame of the story.
pub fn offset_at(
    doc: &Document,
    layout: &DocumentLayout,
    story: StoryId,
    frame: FrameId,
    p: Point,
) -> Option<usize> {
    let (x, y) = page_to_frame(doc, frame, p)?;
    let (sx, sy) = story_shift(layout, frame, story);
    let (x, y) = (x - sx, y - sy);
    let lines = story_lines(doc, layout, story);
    let in_frame: Vec<&LineBox> = lines
        .iter()
        .filter(|(f, _)| *f == frame)
        .map(|(_, l)| *l)
        .collect();
    if in_frame.is_empty() {
        // Empty frame of the chain: put the caret at the end of the text before it.
        return lines.iter().map(|(_, l)| l.chars.end).max().or(Some(0));
    }
    let line = in_frame
        .iter()
        .min_by(|a, b| {
            let da = if y < a.rect.y {
                a.rect.y - y
            } else {
                (y - a.rect.bottom()).max(0.0)
            };
            let db = if y < b.rect.y {
                b.rect.y - y
            } else {
                (y - b.rect.bottom()).max(0.0)
            };
            da.total_cmp(&db)
        })
        .copied()?;
    Some(line.offset_at_x(x))
}

/// Offset one line up (`-1`) or down (`1`) from `offset`, keeping `goal_x`.
pub fn vertical(
    doc: &Document,
    layout: &DocumentLayout,
    story: StoryId,
    offset: usize,
    dir: isize,
    goal_x: Option<f64>,
) -> Option<(usize, f64)> {
    let lines = story_lines(doc, layout, story);
    let i = line_for(&lines, offset)?;
    let x = goal_x.unwrap_or_else(|| lines[i].1.caret_x(offset));
    let j = i as isize + dir;
    if j < 0 {
        return Some((0, x));
    }
    match lines.get(j as usize) {
        Some((_, l)) => Some((l.offset_at_x(x), x)),
        None => Some((lines.last()?.1.chars.end, x)),
    }
}

/// Start or end of the line containing `offset`.
pub fn line_edge(
    doc: &Document,
    layout: &DocumentLayout,
    story: StoryId,
    offset: usize,
    end: bool,
) -> Option<usize> {
    let lines = story_lines(doc, layout, story);
    let (_, l) = lines[line_for(&lines, offset)?];
    Some(if end {
        // Stay before a soft-wrap space so the caret remains on this line.
        // A line ending at a break character ends before it, like a
        // paragraph's last line.
        if l.ends_paragraph || ends_at_break(doc, story, l.chars.end) {
            l.chars.end
        } else {
            l.chars.end.saturating_sub(1).max(l.chars.start)
        }
    } else {
        l.chars.start
    })
}

/// Whether the character at story offset `at` is a column, frame or page
/// break (costs one paragraph, not the story).
pub fn ends_at_break(doc: &Document, story: StoryId, at: usize) -> bool {
    let Some(s) = doc.story(story) else {
        return false;
    };
    let (pi, c) = s.locate(at);
    s.paragraphs
        .get(pi)
        .and_then(|p| p.text().chars().nth(c))
        .is_some_and(pubrs_api::core::text::is_break)
}

/// Page-space quads highlighting `start..end`.
pub fn selection_quads(
    doc: &Document,
    layout: &DocumentLayout,
    story: StoryId,
    start: usize,
    end: usize,
) -> Vec<[Point; 4]> {
    let mut out = Vec::new();
    if start >= end {
        return out;
    }
    for (f, l) in story_lines(doc, layout, story) {
        let a = start.max(l.chars.start);
        let b = end.min(l.chars.end);
        if a >= b && !(a == b && b == l.chars.end && end > l.chars.end) {
            continue;
        }
        let x0 = l.caret_x(a);
        let mut x1 = l.caret_x(b);
        if end > l.chars.end && x1 - x0 < 3.0 {
            x1 = x0 + 3.0;
        }
        let (sx, sy) = story_shift(layout, f, story);
        let (x0, x1) = (x0 + sx, x1 + sx);
        let pts = [
            (x0, l.rect.y + sy),
            (x1, l.rect.y + sy),
            (x1, l.rect.bottom() + sy),
            (x0, l.rect.bottom() + sy),
        ];
        let mut q = [Point::default(); 4];
        let mut ok = true;
        for (k, (x, y)) in pts.iter().enumerate() {
            match frame_to_page(doc, f, *x, *y) {
                Some(p) => q[k] = p,
                None => ok = false,
            }
        }
        if ok {
            out.push(q);
        }
    }
    out
}

/// Page-space segments under the character ranges of `items` (sorted by
/// start, not overlapping) on the lines `frame` shows: one per line and
/// range, along the bottom of the selection quads, just under the baseline
/// (half-way down the descent). Costs the frame's lines, not the story's.
pub fn underline_segments<T>(
    doc: &Document,
    layout: &DocumentLayout,
    story: StoryId,
    frame: FrameId,
    items: &[T],
    range: impl Fn(&T) -> (usize, usize),
) -> Vec<(Point, Point)> {
    let mut out = Vec::new();
    let Some(ft) = layout
        .stories
        .get(&story)
        .and_then(|sl| sl.frames.get(&frame))
    else {
        return out;
    };
    for l in &ft.lines {
        let first = items.partition_point(|t| range(t).1 <= l.chars.start);
        for t in &items[first..] {
            let (s, e) = range(t);
            if s >= l.chars.end {
                break;
            }
            let (a, b) = (s.max(l.chars.start), e.min(l.chars.end));
            if a >= b {
                continue;
            }
            let (sx, sy) = story_shift(layout, frame, story);
            let y = l.baseline + (l.rect.bottom() - l.baseline) * 0.5 + sy;
            if let (Some(p), Some(q)) = (
                frame_to_page(doc, frame, l.caret_x(a) + sx, y),
                frame_to_page(doc, frame, l.caret_x(b) + sx, y),
            ) {
                out.push((p, q));
            }
        }
    }
    out
}

/// Previous/next word boundary for Ctrl/Alt+arrow movement.
pub fn word_boundary(text: &[char], offset: usize, forward: bool) -> usize {
    let n = text.len();
    let mut i = offset.min(n);
    if forward {
        while i < n && !text[i].is_alphanumeric() {
            i += 1;
        }
        while i < n && text[i].is_alphanumeric() {
            i += 1;
        }
    } else {
        while i > 0 && !text[i - 1].is_alphanumeric() {
            i -= 1;
        }
        while i > 0 && text[i - 1].is_alphanumeric() {
            i -= 1;
        }
    }
    i
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn word_boundaries() {
        let t: Vec<char> = "Hello, big world".chars().collect();
        assert_eq!(word_boundary(&t, 0, true), 5);
        assert_eq!(word_boundary(&t, 5, true), 10);
        assert_eq!(word_boundary(&t, 16, false), 11);
        assert_eq!(word_boundary(&t, 11, false), 7);
    }
}
