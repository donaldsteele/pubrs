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
    let x = l.caret_x(offset.clamp(l.chars.start, l.chars.end));
    Some((
        f,
        frame_to_page(doc, f, x, l.rect.y)?,
        frame_to_page(doc, f, x, l.rect.bottom())?,
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
        if l.ends_paragraph {
            l.chars.end
        } else {
            l.chars.end.saturating_sub(1).max(l.chars.start)
        }
    } else {
        l.chars.start
    })
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
        let pts = [
            (x0, l.rect.y),
            (x1, l.rect.y),
            (x1, l.rect.bottom()),
            (x0, l.rect.bottom()),
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
