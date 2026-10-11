//! In-place text editing: caret/selection state and geometry from layout.

use eframe::egui;
use pubrs_api::core::{Command, Document, FrameId, Point, StoryId};
use pubrs_api::layout::{DocumentLayout, LineBox};

use crate::app::PubrsApp;

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
    /// Where the caret is drawn when its offset has two places on a line
    /// that mixes text directions (frame-local x), set by arrow keys and
    /// clicks and cleared by anything else that moves the caret.
    pub visual_x: Option<f64>,
    /// Text an input method is composing at the caret: shown over the page,
    /// not part of the document until it is committed.
    pub preedit: Option<Preedit>,
}

/// Text under composition by an input method.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Preedit {
    pub text: String,
    /// The part the input method is converting now, in characters of `text`.
    pub active: Option<std::ops::Range<usize>>,
}

impl TextEdit {
    pub fn new(frame: FrameId, story: StoryId, at: usize) -> Self {
        Self {
            frame,
            story,
            caret: at,
            anchor: at,
            goal_x: None,
            visual_x: None,
            preedit: None,
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
        self.visual_x = None;
        // A caret moved by click, arrow or edit ends the composition.
        self.preedit = None;
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
    caret_segment_at(doc, layout, story, offset, None)
}

/// [`caret_segment`] drawn at the place nearest `hint` where the offset
/// has more than one (see [`LineBox::slots`]).
pub fn caret_segment_at(
    doc: &Document,
    layout: &DocumentLayout,
    story: StoryId,
    offset: usize,
    hint: Option<f64>,
) -> Option<(FrameId, Point, Point)> {
    let lines = story_lines(doc, layout, story);
    let i = line_for(&lines, offset)?;
    let (f, l) = lines[i];
    let (sx, sy) = story_shift(layout, f, story);
    let o = offset.clamp(l.chars.start, l.chars.end);
    let x = if hint.is_some() {
        l.slot_x(o, hint)
    } else {
        l.caret_x(o)
    } + sx;
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
    offset_at_slot(doc, layout, story, frame, p).map(|s| s.0)
}

/// [`offset_at`] with the x the caret should be drawn at when the line
/// mixes text directions (the visually nearest caret place wins).
pub fn offset_at_slot(
    doc: &Document,
    layout: &DocumentLayout,
    story: StoryId,
    frame: FrameId,
    p: Point,
) -> Option<(usize, Option<f64>)> {
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
        return Some((
            lines.iter().map(|(_, l)| l.chars.end).max().unwrap_or(0),
            None,
        ));
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
    if line.is_bidi() {
        let (o, sx) = line.slot_at_x(x);
        return Some((o, Some(sx)));
    }
    Some((line.offset_at_x(x), None))
}

/// One caret step left or right by position on the line, for lines that mix
/// text directions or run right to left: (new offset, where to draw it).
/// `None` on lines in reading order, where the caller steps by one character.
/// At the visual end of the line the step goes on through the text in the
/// direction the line's edge leads to.
pub fn horizontal(
    doc: &Document,
    layout: &DocumentLayout,
    e: &TextEdit,
    left: bool,
) -> Option<(usize, Option<f64>)> {
    let lines = story_lines(doc, layout, e.story);
    let (_, l) = lines[line_for(&lines, e.caret)?];
    if !l.is_bidi() {
        return None;
    }
    let x = l.slot_x(e.caret, e.visual_x);
    if let Some((o, nx)) = l.visual_step(e.caret, x, left) {
        return Some((o, Some(nx)));
    }
    let backwards = left != l.is_rtl();
    Some((
        if backwards {
            e.caret.saturating_sub(1)
        } else {
            e.caret + 1
        },
        None,
    ))
}

/// Whether the line holding `offset` runs right to left.
pub fn line_is_rtl(doc: &Document, layout: &DocumentLayout, story: StoryId, offset: usize) -> bool {
    let lines = story_lines(doc, layout, story);
    line_for(&lines, offset).is_some_and(|i| lines[i].1.is_rtl())
}

/// Start or end of the line holding the caret, for lines that mix text
/// directions: the start is the edge the line begins at (right for a line
/// that runs right to left). `None` on lines in reading order.
pub fn home_end(
    doc: &Document,
    layout: &DocumentLayout,
    story: StoryId,
    offset: usize,
    end: bool,
) -> Option<(usize, Option<f64>)> {
    let lines = story_lines(doc, layout, story);
    let (_, l) = lines[line_for(&lines, offset)?];
    if !l.is_bidi() {
        return None;
    }
    let skip_end = !(l.ends_paragraph || ends_at_break(doc, story, l.chars.end));
    let (o, x) = l.visual_edge(end != l.is_rtl(), skip_end);
    Some((o, Some(x)))
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
        let mut spans = l.selection_spans(a, b);
        if end > l.chars.end {
            // The line break is part of the selection: a stub after the text.
            if l.is_bidi() {
                let x = l.caret_x(l.chars.end);
                spans.push(if l.is_rtl() {
                    (x - 3.0, x)
                } else {
                    (x, x + 3.0)
                });
            } else if let Some(last) = spans.last_mut()
                && last.1 - last.0 < 3.0
            {
                last.1 = last.0 + 3.0;
            }
        }
        let (sx, sy) = story_shift(layout, f, story);
        for (x0, x1) in spans {
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

/// An input method event while editing text (ADR 0025). Composition text
/// is only remembered here and drawn over the page; nothing reaches the
/// document, so no layout and no command, until the method commits.
pub fn ime_event(app: &mut PubrsApp, e: &TextEdit, ev: &egui::ImeEvent) {
    match ev {
        egui::ImeEvent::Preedit {
            text,
            active_range_chars,
        } => {
            let preedit = (!text.is_empty()).then(|| Preedit {
                text: text.clone(),
                active: active_range_chars.clone(),
            });
            if let Some(ed) = app.view_mut().and_then(|v| v.edit.as_mut()) {
                ed.preedit = preedit;
            }
        }
        egui::ImeEvent::Commit(text) => {
            if let Some(ed) = app.view_mut().and_then(|v| v.edit.as_mut()) {
                ed.preedit = None;
            }
            if !text.is_empty() {
                // The ordinary typing path: merges with the typing before it.
                crate::canvas::edit_insert(app, e, text);
            }
        }
        egui::ImeEvent::DeleteSurrounding {
            before_chars,
            after_chars,
        } => {
            let len = app
                .doc()
                .and_then(|d| d.story(e.story))
                .map_or(0, |s| s.char_len());
            let (s, t) = e.range();
            let from = s.saturating_sub(*before_chars);
            let to = t.saturating_add(*after_chars).min(len);
            let mut commands = Vec::new();
            if to > t {
                commands.push(Command::DeleteText {
                    story: e.story,
                    start: t,
                    end: to,
                });
            }
            if s > from {
                commands.push(Command::DeleteText {
                    story: e.story,
                    start: from,
                    end: s,
                });
            }
            // Same key as typing: delete-surrounding then commit is one step.
            if !commands.is_empty()
                && app.exec_merge(
                    Command::Batch {
                        label: "input method delete".into(),
                        commands,
                    },
                    &format!("type-{}", e.story),
                )
                && let Some(ed) = app.view_mut().and_then(|v| v.edit.as_mut())
            {
                // Selection and caret keep their text; they move up by what
                // was removed before them.
                ed.anchor -= s - from;
                ed.caret -= s - from;
            }
        }
        // The platform layer no longer sends these.
        #[allow(deprecated)]
        egui::ImeEvent::Enabled | egui::ImeEvent::Disabled => {}
    }
}

/// Draw the composition text at the caret: on a plain backing so the page
/// text under it does not show through, underlined (thicker for the part
/// being converted). `top` and `bottom` are the caret in screen points.
/// Returns the text's width, where the candidate window should start.
pub fn draw_preedit(
    painter: &egui::Painter,
    preedit: &Preedit,
    top: egui::Pos2,
    bottom: egui::Pos2,
) -> f32 {
    let height = (bottom - top).length().max(8.0);
    let font = egui::FontId::proportional((height * 0.8).max(6.0));
    let ink = egui::Color32::from_gray(20);
    let galley = painter.layout_no_wrap(preedit.text.clone(), font, ink);
    let width = galley.size().x;
    let origin = egui::pos2(top.x, top.y + (height - galley.size().y).max(0.0) / 2.0);
    painter.rect_filled(
        egui::Rect::from_min_size(top, egui::vec2(width + 2.0, height)),
        0.0,
        egui::Color32::from_rgba_unmultiplied(255, 252, 235, 235),
    );
    painter.galley(origin, galley.clone(), ink);
    let y = top.y + height - 1.0;
    painter.line_segment(
        [egui::pos2(top.x, y), egui::pos2(top.x + width, y)],
        egui::Stroke::new(1.0, ink),
    );
    if let Some(r) = &preedit.active {
        let x_at = |chars: usize| {
            let byte = preedit
                .text
                .char_indices()
                .nth(chars)
                .map_or(preedit.text.len(), |c| c.0);
            galley
                .pos_from_cursor(egui::text::CCursor::new(
                    preedit.text[..byte].chars().count(),
                ))
                .min
                .x
        };
        let (a, b) = (x_at(r.start), x_at(r.end));
        painter.line_segment(
            [egui::pos2(top.x + a, y), egui::pos2(top.x + b, y)],
            egui::Stroke::new(2.5, ink),
        );
    }
    width
}

/// Tell the window where the caret is, so the input method's candidate
/// window opens beside it: `rect` is the text being edited, `caret` the
/// caret (after the composition text, `pre_width` wide), in screen points.
pub fn publish_ime_area(
    ctx: &egui::Context,
    rect: egui::Rect,
    top: egui::Pos2,
    bottom: egui::Pos2,
    pre_width: f32,
) {
    let x = top.x + pre_width;
    let cursor_rect = egui::Rect::from_min_max(
        egui::pos2(x, top.y.min(bottom.y)),
        egui::pos2(x + 1.0, top.y.max(bottom.y)),
    );
    ctx.output_mut(|o| {
        o.ime = Some(egui::output::IMEOutput {
            purpose: egui::IMEPurpose::Normal,
            rect,
            cursor_rect,
            should_interrupt_composition: false,
        });
    });
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

    /// A Hebrew cell of a table: carets fall to the left, hit-testing at a
    /// caret's page position returns that caret, with the cell's shift.
    #[test]
    fn right_to_left_cell_carets_round_trip() {
        use pubrs_api::{Session, SessionConfig, call_tool};
        use serde_json::json;
        let fonts = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tests/fonts");
        let mut s = Session::new(SessionConfig {
            id_seed: Some(5),
            font_dir: Some(fonts),
            ..Default::default()
        });
        let d = call_tool(&mut s, "doc_new", json!({})).unwrap();
        let t = call_tool(
            &mut s,
            "table_add",
            json!({"x": 72, "y": 72, "width": 300, "height": 90, "rows": 2, "cols": 2}),
        )
        .unwrap();
        let cell = call_tool(
            &mut s,
            "table_set_cell_text",
            json!({"frame_id": t["frame_id"], "row": 1, "col": 1,
                   "text": "\u{5e9}\u{5dc}\u{5d5}\u{5dd} \u{5e2}\u{5d5}\u{5dc}\u{5dd}"}),
        )
        .unwrap();
        let _ = d;
        let story: StoryId = serde_json::from_value(cell["story_id"].clone()).unwrap();
        let layout = s.layout(None).unwrap();
        let doc = s.get(None).unwrap().doc().clone();
        let lines = story_lines(&doc, &layout, story);
        assert_eq!(lines.len(), 1);
        let (frame, line) = lines[0];
        assert!(line.is_rtl() && line.is_bidi());
        let (sx, _) = story_shift(&layout, frame, story);
        assert!(sx > 0.0, "the cell is shifted inside the frame");
        let mut xs = Vec::new();
        for o in 0..=9 {
            let (f, top, bottom) = caret_segment_at(&doc, &layout, story, o, None).unwrap();
            assert_eq!(f, frame);
            let mid = Point::new(top.x, (top.y + bottom.y) / 2.0);
            let (hit, _) = offset_at_slot(&doc, &layout, story, frame, mid).unwrap();
            assert_eq!(hit, o, "caret {o} at page x {}", top.x);
            xs.push(top.x);
        }
        assert!(xs.windows(2).all(|w| w[0] > w[1]), "{xs:?}");
        // The selection quads of the whole text are one piece in page space.
        assert_eq!(selection_quads(&doc, &layout, story, 0, 9).len(), 1);
    }
}
