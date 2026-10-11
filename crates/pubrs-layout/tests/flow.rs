//! Text flow tests: single frames, linked chains, overflow, columns, wrap.
#![allow(clippy::unwrap_used)]

use std::path::PathBuf;

use pubrs_core::*;
use pubrs_layout::*;

const LOREM: &str = "Lorem ipsum dolor sit amet, consectetur adipiscing elit, sed do eiusmod \
tempor incididunt ut labore et dolore magna aliqua. Ut enim ad minim veniam, quis nostrud \
exercitation ullamco laboris nisi ut aliquip ex ea commodo consequat. Duis aute irure dolor in \
reprehenderit in voluptate velit esse cillum dolore eu fugiat nulla pariatur. Excepteur sint \
occaecat cupidatat non proident, sunt in culpa qui officia deserunt mollit anim id est laborum.";

fn fonts() -> FontLibrary {
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tests/fonts");
    FontLibrary::from_dir(&dir).expect("test fonts")
}

fn builder() -> DocBuilder<SeqIds> {
    DocBuilder::new(SeqIds::new(7), PageSetup::default())
}

fn layout(doc: &Document) -> DocumentLayout {
    Layouter::new(fonts()).layout_document(doc)
}

fn line_texts(doc: &Document, ft: &FrameText) -> Vec<String> {
    let story = doc
        .stories
        .values()
        .find(|s| {
            s.frames
                .iter()
                .any(|f| ft.lines.is_empty() || doc.frame(*f).is_some())
        })
        .unwrap();
    let text: Vec<char> = story.plain_text().chars().collect();
    ft.lines
        .iter()
        .map(|l| text[l.chars.clone()].iter().collect::<String>())
        .collect()
}

#[test]
fn short_text_fits_one_line() {
    let mut b = builder();
    let p = b.page(0);
    let f = b
        .text(p, Rect::new(36.0, 36.0, 200.0, 100.0), "Hello world")
        .unwrap();
    let doc = b.finish();
    let lay = layout(&doc);
    let s = &lay.stories[&doc.story_of(f).unwrap()];
    assert!(s.overflow.is_none());
    let ft = &s.frames[&f];
    assert_eq!(ft.lines.len(), 1);
    assert_eq!(ft.chars, 0..11);
    assert_eq!(line_texts(&doc, ft), vec!["Hello world"]);
    let l = &ft.lines[0];
    assert_eq!(l.rect.x, 4.0, "left inset");
    assert!(l.baseline > 4.0 && l.baseline < 4.0 + l.rect.height);
    assert_eq!(l.carets.len(), 12);
    assert!(l.carets.windows(2).all(|w| w[0].1 <= w[1].1));
    assert_eq!(l.offset_at_x(1000.0), 11);
    assert_eq!(l.offset_at_x(0.0), 0);
    assert!(s.missing_fonts.is_empty());
    assert_eq!(s.missing_glyphs, 0);
}

#[test]
fn overflow_then_linking_flows_into_next_frame() {
    let mut b = builder();
    let p = b.page(0);
    let a = b
        .text(p, Rect::new(36.0, 36.0, 150.0, 40.0), LOREM)
        .unwrap();
    let doc = b.doc().clone();
    let story = doc.story_of(a).unwrap();
    let lay = layout(&doc);
    let o = lay.stories[&story].overflow.expect("overflows");
    let total = doc.story(story).unwrap().char_len();
    assert_eq!(o.offset + o.hidden_chars, total);
    assert_eq!(lay.stories[&story].frames[&a].chars.end, o.offset);

    let c = b.text(p, Rect::new(36.0, 100.0, 110.0, 600.0), "").unwrap();
    b.link(a, c).unwrap();
    let doc = b.finish();
    let lay = layout(&doc);
    let s = &lay.stories[&story];
    assert!(s.overflow.is_none(), "{:?}", s.overflow);
    let fa = &s.frames[&a];
    let fc = &s.frames[&c];
    assert_eq!(fa.chars.end, o.offset, "first frame shows the same text");
    assert!(fc.chars.start >= fa.chars.end && fc.chars.start <= fa.chars.end + 1);
    assert_eq!(fc.chars.end, total);
    // The second frame is narrower: every line fits its width.
    for l in &fc.lines {
        assert!(l.rect.width <= 110.0 - 8.0 + 1e-6);
        let last = l.runs.iter().flat_map(|r| r.glyphs.iter()).last().unwrap();
        assert!(last.x <= 110.0, "glyph inside frame");
    }
    // Line y restarts at the top inset of the second frame.
    assert_eq!(fc.lines[0].rect.y, 4.0);
}

#[test]
fn columns_split_the_frame() {
    let mut b = builder();
    let p = b.page(0);
    let f = b
        .text(p, Rect::new(36.0, 36.0, 300.0, 60.0), LOREM)
        .unwrap();
    b.exec(Command::SetTextFrame {
        frame: f,
        props: TextFrameProps {
            columns: Some(2),
            column_gap: Some(12.0),
            ..Default::default()
        },
    })
    .unwrap();
    let doc = b.finish();
    let lay = layout(&doc);
    let ft = lay.frame(f).unwrap();
    let col_w = (300.0 - 8.0 - 12.0) / 2.0;
    let second_x = 4.0 + col_w + 12.0;
    assert!(ft.lines.iter().any(|l| (l.rect.x - 4.0).abs() < 1e-6));
    assert!(ft.lines.iter().any(|l| (l.rect.x - second_x).abs() < 1e-6));
    assert!(ft.lines.iter().all(|l| (l.rect.width - col_w).abs() < 1e-6));
}

#[test]
fn square_wrap_pushes_lines_aside() {
    let mut b = builder();
    let p = b.page(0);
    let f = b.text(p, Rect::new(0.0, 0.0, 300.0, 300.0), LOREM).unwrap();
    let img = b
        .shape(p, Rect::new(0.0, 0.0, 100.0, 60.0), Shape::Ellipse)
        .unwrap();
    b.wrap(
        img,
        TextWrap {
            mode: WrapMode::Square,
            margin: 6.0,
            both_sides: false,
        },
    )
    .unwrap();
    let doc = b.finish();
    let lay = layout(&doc);
    let ft = lay.frame(f).unwrap();
    for l in &ft.lines {
        if l.rect.y < 66.0 {
            assert!(
                l.rect.x >= 106.0 - 1e-6,
                "line beside obstacle at y={}",
                l.rect.y
            );
        }
    }
    assert!(ft.lines.iter().any(|l| l.rect.y >= 66.0 && l.rect.x == 4.0));
}

#[test]
fn top_bottom_wrap_skips_the_band() {
    let mut b = builder();
    let p = b.page(0);
    let f = b.text(p, Rect::new(0.0, 0.0, 300.0, 400.0), LOREM).unwrap();
    let img = b
        .shape(p, Rect::new(50.0, 40.0, 100.0, 50.0), Shape::Ellipse)
        .unwrap();
    b.wrap(
        img,
        TextWrap {
            mode: WrapMode::TopBottom,
            margin: 0.0,
            both_sides: false,
        },
    )
    .unwrap();
    let doc = b.finish();
    let lay = layout(&doc);
    let ft = lay.frame(f).unwrap();
    for l in &ft.lines {
        assert!(l.rect.bottom() <= 40.0 + 1e-6 || l.rect.y >= 90.0 - 1e-6);
    }
    assert!(ft.lines.iter().any(|l| l.rect.y >= 90.0));
}

#[test]
fn wrap_ignores_frames_below_in_z_order() {
    let mut b = builder();
    let p = b.page(0);
    let img = b
        .shape(p, Rect::new(0.0, 0.0, 100.0, 60.0), Shape::Ellipse)
        .unwrap();
    b.wrap(
        img,
        TextWrap {
            mode: WrapMode::Square,
            margin: 0.0,
            both_sides: false,
        },
    )
    .unwrap();
    let f = b.text(p, Rect::new(0.0, 0.0, 300.0, 300.0), LOREM).unwrap();
    let doc = b.finish();
    let lay = layout(&doc);
    assert_eq!(lay.frame(f).unwrap().lines[0].rect.x, 4.0);
}

#[test]
fn alignment_center_and_right() {
    let mut b = builder();
    let p = b.page(0);
    let f = b.text(p, Rect::new(0.0, 0.0, 300.0, 100.0), "Hi").unwrap();
    let s = b.story_of(f);
    let first_x = |doc: &Document| {
        let lay = layout(doc);
        lay.frame(f).unwrap().lines[0].runs[0].glyphs[0].x
    };
    let left = first_x(b.doc());
    b.exec(Command::ApplyParaProps {
        story: s,
        start: 0,
        end: 0,
        props: ParaProps {
            align: Some(Align::Center),
            ..Default::default()
        },
    })
    .unwrap();
    let center = first_x(b.doc());
    b.exec(Command::ApplyParaProps {
        story: s,
        start: 0,
        end: 0,
        props: ParaProps {
            align: Some(Align::Right),
            ..Default::default()
        },
    })
    .unwrap();
    let right = first_x(b.doc());
    assert_eq!(left, 4.0);
    assert!(center > 130.0 && center < 150.0, "{center}");
    assert!(right > 270.0 && right < 296.0, "{right}");
}

#[test]
fn missing_font_falls_back_and_is_reported() {
    let mut b = builder();
    let p = b.page(0);
    let f = b
        .text(p, Rect::new(0.0, 0.0, 300.0, 100.0), "Fallback")
        .unwrap();
    let s = b.story_of(f);
    b.exec(Command::ApplyCharProps {
        story: s,
        start: 0,
        end: 8,
        props: CharProps {
            font_family: Some("Nope Sans Pro".into()),
            ..Default::default()
        },
    })
    .unwrap();
    let doc = b.finish();
    let lay = layout(&doc);
    let sl = &lay.stories[&s];
    assert!(sl.missing_fonts.contains("Nope Sans Pro"));
    let glyphs: usize = sl.frames[&f].lines[0]
        .runs
        .iter()
        .map(|r| r.glyphs.len())
        .sum();
    assert_eq!(glyphs, 8, "fallback font draws every character");
    assert_eq!(sl.missing_glyphs, 0);
}

#[test]
fn bold_uses_bold_face_and_italic_serif_is_synthesised() {
    let mut b = builder();
    let p = b.page(0);
    let f = b
        .text(p, Rect::new(0.0, 0.0, 300.0, 100.0), "Bold Serif")
        .unwrap();
    let s = b.story_of(f);
    b.exec(Command::ApplyCharProps {
        story: s,
        start: 0,
        end: 4,
        props: CharProps {
            bold: Some(true),
            ..Default::default()
        },
    })
    .unwrap();
    b.exec(Command::ApplyCharProps {
        story: s,
        start: 5,
        end: 10,
        props: CharProps {
            font_family: Some("Liberation Serif".into()),
            italic: Some(true),
            ..Default::default()
        },
    })
    .unwrap();
    let doc = b.finish();
    let lay = layout(&doc);
    let runs = &lay.frame(f).unwrap().lines[0].runs;
    assert!(runs.len() >= 2);
    assert!(!runs[0].synthetic_bold, "real bold face in tests/fonts");
    let serif = runs.last().unwrap();
    assert_eq!(
        serif.synthetic_skew,
        Some(14.0),
        "no serif italic face in tests/fonts"
    );
}

#[test]
fn vertical_align_bottom_and_paragraph_spacing() {
    let mut b = builder();
    let p = b.page(0);
    let f = b
        .text(p, Rect::new(0.0, 0.0, 200.0, 200.0), "One\nTwo")
        .unwrap();
    let s = b.story_of(f);
    let ys = |doc: &Document| -> Vec<f64> {
        layout(doc)
            .frame(f)
            .unwrap()
            .lines
            .iter()
            .map(|l| l.rect.y)
            .collect()
    };
    let base = ys(b.doc());
    b.exec(Command::ApplyParaProps {
        story: s,
        start: 0,
        end: 0,
        props: ParaProps {
            space_after: Some(10.0),
            ..Default::default()
        },
    })
    .unwrap();
    let spaced = ys(b.doc());
    assert!((spaced[1] - base[1] - 10.0).abs() < 1e-6);
    b.exec(Command::SetTextFrame {
        frame: f,
        props: TextFrameProps {
            vertical_align: Some(VerticalAlign::Bottom),
            ..Default::default()
        },
    })
    .unwrap();
    let lay = layout(b.doc());
    let last = lay.frame(f).unwrap().lines.last().unwrap().rect;
    assert!((last.bottom() - 196.0).abs() < 1e-6, "{last:?}");
}

#[test]
fn empty_story_has_one_caret_line() {
    let mut b = builder();
    let p = b.page(0);
    let f = b.text(p, Rect::new(0.0, 0.0, 200.0, 100.0), "").unwrap();
    let doc = b.finish();
    let lay = layout(&doc);
    let ft = lay.frame(f).unwrap();
    assert_eq!(ft.lines.len(), 1);
    assert_eq!(ft.lines[0].carets.len(), 1);
    assert_eq!(ft.lines[0].carets[0].0, 0);
}

#[test]
fn unicode_offsets_are_characters() {
    let mut b = builder();
    let p = b.page(0);
    let f = b
        .text(p, Rect::new(0.0, 0.0, 300.0, 100.0), "héllo wörld\nñ")
        .unwrap();
    let doc = b.finish();
    let lay = layout(&doc);
    let ft = lay.frame(f).unwrap();
    assert_eq!(ft.lines[0].chars, 0..11);
    assert_eq!(ft.lines[1].chars, 12..13);
    assert_eq!(ft.lines[0].carets.len(), 12);
    // Glyph text ranges index into the paragraph text at char boundaries.
    for r in &ft.lines[0].runs {
        for g in &r.glyphs {
            assert!(r.text.is_char_boundary(g.text.start) && r.text.is_char_boundary(g.text.end));
        }
    }
}

#[test]
fn layout_is_deterministic_across_font_libraries() {
    let mut b = builder();
    let p = b.page(0);
    b.text(p, Rect::new(0.0, 0.0, 180.0, 500.0), LOREM).unwrap();
    let doc = b.finish();
    let fingerprint = |lay: &DocumentLayout| -> Vec<(u32, i64, i64)> {
        lay.stories
            .values()
            .flat_map(|s| s.frames.values())
            .flat_map(|f| f.lines.iter())
            .flat_map(|l| l.runs.iter())
            .flat_map(|r| r.glyphs.iter())
            .map(|g| (g.id, (g.x * 1000.0) as i64, (g.y * 1000.0) as i64))
            .collect()
    };
    let a = fingerprint(&layout(&doc));
    let b2 = fingerprint(&layout(&doc));
    assert!(!a.is_empty());
    assert_eq!(a, b2);
}

/// Text frame (100,100,300x100) rotated 90 degrees about (250,150) covers the
/// page region x 200..300, y 0..300; its local left end is at the page top.
fn rotated_wrap_doc(obstacle: Rect) -> (Document, FrameId) {
    let mut b = builder();
    let p = b.page(0);
    let f = b
        .text(p, Rect::new(100.0, 100.0, 300.0, 100.0), LOREM)
        .unwrap();
    let ob = b.shape(p, obstacle, Shape::Ellipse).unwrap();
    b.wrap(
        ob,
        TextWrap {
            mode: WrapMode::Square,
            margin: 0.0,
            both_sides: false,
        },
    )
    .unwrap();
    let mut doc = b.finish();
    doc.frame_mut(f).unwrap().rotation = 90.0;
    (doc, f)
}

#[test]
fn wrap_around_obstacle_follows_rotated_text_frame() {
    // Overlaps the top of the rotated frame on the page = its local left end
    // (local x 0..40), so lines must start at or beyond x = 40.
    let (doc, f) = rotated_wrap_doc(Rect::new(200.0, 0.0, 100.0, 40.0));
    let lay = layout(&doc);
    let ft = lay.frame(f).unwrap();
    assert!(!ft.lines.is_empty());
    for l in &ft.lines {
        assert!(l.rect.x >= 40.0 - 1e-6, "line at x={} overlaps", l.rect.x);
    }
}

#[test]
fn wrap_does_not_cut_rotated_text_frame_where_it_does_not_overlap() {
    // Beside the rotated frame on the page. Its page-space box coincides with
    // the unrotated frame's rect, but it does not touch the rotated frame.
    let (doc, f) = rotated_wrap_doc(Rect::new(320.0, 100.0, 60.0, 60.0));
    let lay = layout(&doc);
    let ft = lay.frame(f).unwrap();
    let mut clear = doc.clone();
    for fr in clear.frames.values_mut() {
        std::sync::Arc::make_mut(fr).wrap.mode = WrapMode::None;
    }
    let base = layout(&clear);
    let base_ft = base.frame(f).unwrap();
    assert_eq!(ft.lines.len(), base_ft.lines.len());
    for (a, b) in ft.lines.iter().zip(&base_ft.lines) {
        assert_eq!(a.rect, b.rect);
    }
}

// ------------------------------------------------- M15: keeps, drop caps, breaks

/// Line height of the default 11 pt text at 1.2.
const LINE: f64 = 13.2;
const COL_W: f64 = 240.0;

/// A frame of `cols` columns, each `COL_W` wide and `lines` default lines
/// tall, holding `text`.
fn columns_doc(text: &str, cols: u32, lines: usize) -> (DocBuilder<SeqIds>, FrameId, StoryId) {
    let mut b = builder();
    let p = b.page(0);
    let w = 8.0 + f64::from(cols) * COL_W + f64::from(cols - 1) * 12.0;
    let h = 8.0 + lines as f64 * LINE + 0.5;
    let f = b.text(p, Rect::new(36.0, 36.0, w, h), text).unwrap();
    b.exec(Command::SetTextFrame {
        frame: f,
        props: TextFrameProps {
            columns: Some(cols),
            column_gap: Some(12.0),
            ..Default::default()
        },
    })
    .unwrap();
    let s = b.story_of(f);
    (b, f, s)
}

fn set_para(b: &mut DocBuilder<SeqIds>, s: StoryId, para: usize, props: ParaProps) {
    let start = b.doc().story(s).unwrap().paragraph_start(para);
    b.exec(Command::ApplyParaProps {
        story: s,
        start,
        end: start,
        props,
    })
    .unwrap();
}

/// The column of each line of paragraph `para`.
fn para_columns(doc: &Document, f: FrameId, s: StoryId, para: usize) -> Vec<usize> {
    let story = doc.story(s).unwrap();
    let start = story.paragraph_start(para);
    let end = start + story.paragraphs[para].char_len();
    layout(doc)
        .frame(f)
        .unwrap()
        .lines
        .iter()
        .filter(|l| l.chars.start >= start && l.chars.end <= end)
        .map(|l| ((l.rect.x - 4.0) / (COL_W + 12.0)).round() as usize)
        .collect()
}

/// The shortest start of LOREM that takes exactly `n` lines `COL_W` wide.
fn text_with_lines(n: usize) -> String {
    let words: Vec<&str> = LOREM.split(' ').collect();
    let mut lay = Layouter::new(fonts());
    for k in 1..=words.len() {
        let t = words[..k].join(" ");
        let mut b = builder();
        let p = b.page(0);
        let f = b
            .text(p, Rect::new(0.0, 0.0, COL_W + 8.0, 1000.0), &t)
            .unwrap();
        if lay.layout_document(b.doc()).frame(f).unwrap().lines.len() == n {
            return t;
        }
    }
    panic!("no text of {n} lines");
}

fn widow_orphan(n: u8) -> ParaProps {
    ParaProps {
        widow_orphan: Some(n),
        ..Default::default()
    }
}

/// (chars, rect) of every line: equal fingerprints mean the same layout.
fn line_boxes(doc: &Document, f: FrameId) -> Vec<(std::ops::Range<usize>, Rect)> {
    layout(doc)
        .frame(f)
        .unwrap()
        .lines
        .iter()
        .map(|l| (l.chars.clone(), l.rect))
        .collect()
}

#[test]
fn orphan_moves_the_paragraph_to_the_next_column() {
    let body = text_with_lines(4);
    // Four one-line paragraphs, then a four-line one from the last line of
    // column 1: one line alone at the bottom.
    let (mut b, f, s) = columns_doc(&format!("a\nb\nc\nd\n{body}"), 2, 5);
    assert_eq!(para_columns(b.doc(), f, s, 4), vec![0, 1, 1, 1]);
    set_para(&mut b, s, 4, widow_orphan(2));
    assert_eq!(para_columns(b.doc(), f, s, 4), vec![1, 1, 1, 1]);
    let lay = layout(b.doc());
    let first = lay
        .frame(f)
        .unwrap()
        .lines
        .iter()
        .find(|l| l.chars.start >= 8)
        .unwrap();
    assert!((first.rect.y - 4.0).abs() < 1e-6, "at the top of column 2");
    // Unset keeps v0.1 layout; 0 and 1 turn control off.
    set_para(&mut b, s, 4, widow_orphan(1));
    assert_eq!(para_columns(b.doc(), f, s, 4), vec![0, 1, 1, 1]);
}

#[test]
fn widow_pulls_a_line_into_the_next_column() {
    let body = text_with_lines(4);
    // Two one-line paragraphs, then a four-line one: its last line alone
    // at the top of column 2.
    let (mut b, f, s) = columns_doc(&format!("a\nb\n{body}"), 2, 5);
    assert_eq!(para_columns(b.doc(), f, s, 2), vec![0, 0, 0, 1]);
    set_para(&mut b, s, 2, widow_orphan(2));
    assert_eq!(para_columns(b.doc(), f, s, 2), vec![0, 0, 1, 1]);
    // Three lines each side cannot be had: the paragraph moves whole.
    set_para(&mut b, s, 2, widow_orphan(3));
    assert_eq!(para_columns(b.doc(), f, s, 2), vec![1, 1, 1, 1]);
}

#[test]
fn keep_with_next_moves_a_heading_to_its_text() {
    let body = text_with_lines(3);
    let kwn = ParaProps {
        keep_with_next: Some(true),
        ..Default::default()
    };
    let (mut b, f, s) = columns_doc(&format!("a\nb\nc\nd\nHeading\n{body}"), 2, 5);
    assert_eq!(para_columns(b.doc(), f, s, 4), vec![0]);
    assert_eq!(para_columns(b.doc(), f, s, 5), vec![1, 1, 1]);
    set_para(&mut b, s, 4, kwn.clone());
    assert_eq!(para_columns(b.doc(), f, s, 4), vec![1]);
    assert_eq!(para_columns(b.doc(), f, s, 5), vec![1, 1, 1]);
    // A run of kept paragraphs moves together.
    let (mut b, f, s) = columns_doc(&format!("a\nb\nc\nTitle\nHeading\n{body}"), 2, 5);
    set_para(&mut b, s, 3, kwn.clone());
    set_para(&mut b, s, 4, kwn);
    assert_eq!(para_columns(b.doc(), f, s, 3), vec![1]);
    assert_eq!(para_columns(b.doc(), f, s, 4), vec![1]);
    assert_eq!(para_columns(b.doc(), f, s, 5), vec![1, 1, 1]);
}

#[test]
fn keep_together_moves_a_three_line_paragraph() {
    let body = text_with_lines(3);
    let (mut b, f, s) = columns_doc(&format!("a\nb\nc\n{body}\nafter"), 2, 5);
    assert_eq!(para_columns(b.doc(), f, s, 3), vec![0, 0, 1]);
    set_para(
        &mut b,
        s,
        3,
        ParaProps {
            keep_together: Some(true),
            ..Default::default()
        },
    );
    assert_eq!(para_columns(b.doc(), f, s, 3), vec![1, 1, 1]);
    assert_eq!(para_columns(b.doc(), f, s, 4), vec![1]);
    // A paragraph taller than a column cannot be kept: laid out as usual.
    let long = text_with_lines(7);
    let (mut b, f, s) = columns_doc(&format!("a\n{long}"), 2, 5);
    let before = line_boxes(b.doc(), f);
    set_para(
        &mut b,
        s,
        1,
        ParaProps {
            keep_together: Some(true),
            ..Default::default()
        },
    );
    let after = line_boxes(b.doc(), f);
    assert_eq!(before, after, "laid out as usual");
    assert!(
        layout(b.doc()).stories[&s].overflow.is_none(),
        "no text hidden"
    );
}

/// Keeps never turn text the usual layout shows into overflow (ADR 0015).
#[test]
fn keeps_never_hide_text_at_the_chain_end() {
    let kwn = ParaProps {
        keep_with_next: Some(true),
        ..Default::default()
    };
    // Heading at the bottom of column 1, its five-line text fills column 2
    // exactly: moving the heading would push the last line out.
    let body = text_with_lines(5);
    let (mut b, f, s) = columns_doc(&format!("a\nb\nc\nd\nHeading\n{body}"), 2, 5);
    let plain = line_boxes(b.doc(), f);
    assert!(layout(b.doc()).stories[&s].overflow.is_none());
    set_para(&mut b, s, 4, kwn.clone());
    assert_eq!(line_boxes(b.doc(), f), plain);
    assert!(layout(b.doc()).stories[&s].overflow.is_none());
    // A single region: nowhere to move to.
    let (mut b, f, s) = columns_doc(&format!("a\nb\nc\nd\nHeading\n{body}"), 1, 5);
    let plain = (line_boxes(b.doc(), f), layout(b.doc()).stories[&s].overflow);
    set_para(&mut b, s, 4, kwn);
    assert_eq!(
        (line_boxes(b.doc(), f), layout(b.doc()).stories[&s].overflow),
        plain
    );
    // An orphan move that would push the next paragraph out of the chain:
    // the story is laid out without keeps instead.
    let three = text_with_lines(3);
    let (mut b, f, s) = columns_doc(&format!("a\nb\nc\nd\n{three}\n{three}"), 2, 5);
    let plain = line_boxes(b.doc(), f);
    assert!(layout(b.doc()).stories[&s].overflow.is_none());
    assert_eq!(para_columns(b.doc(), f, s, 4), vec![0, 1, 1]);
    set_para(&mut b, s, 4, widow_orphan(2));
    assert_eq!(line_boxes(b.doc(), f), plain);
    assert!(layout(b.doc()).stories[&s].overflow.is_none());
}

#[test]
fn keeps_never_reach_more_than_one_region_back() {
    let kwn = ParaProps {
        keep_with_next: Some(true),
        ..Default::default()
    };
    // A kept paragraph that starts two columns back is not moved.
    let long = text_with_lines(10);
    let (mut b, f, s) = columns_doc(&format!("{long}\nNext"), 3, 5);
    let plain = line_boxes(b.doc(), f);
    assert_eq!(para_columns(b.doc(), f, s, 1), vec![2]);
    set_para(&mut b, s, 0, kwn.clone());
    assert_eq!(line_boxes(b.doc(), f), plain);
    // A run of kept paragraphs filling a column from its top stays.
    let (mut b, f, s) = columns_doc("h1\nh2\nh3\nh4\nh5\nNext", 2, 5);
    let plain = line_boxes(b.doc(), f);
    for i in 0..5 {
        set_para(&mut b, s, i, kwn.clone());
    }
    assert_eq!(line_boxes(b.doc(), f), plain);
    // Every rule on every paragraph still terminates and is deterministic.
    let text = (0..12)
        .map(|i| LOREM[..40 + i * 23].to_string())
        .collect::<Vec<_>>()
        .join("\n");
    let (mut b, f, s) = columns_doc(&text, 3, 6);
    for i in 0..12 {
        set_para(
            &mut b,
            s,
            i,
            ParaProps {
                keep_with_next: Some(true),
                keep_together: Some(true),
                widow_orphan: Some(3),
                ..Default::default()
            },
        );
    }
    assert_eq!(line_boxes(b.doc(), f), line_boxes(b.doc(), f));
}

fn drop_cap(lines: u8, chars: u8) -> ParaProps {
    ParaProps {
        drop_cap: Some(pubrs_core::style::DropCap { lines, chars }),
        ..Default::default()
    }
}

/// Cap height of a run's font as a fraction of the em.
fn cap_ratio(run: &GlyphRun) -> f64 {
    use skrifa::MetadataProvider;
    let f = skrifa::FontRef::from_index(run.font.bytes(), run.font.index).unwrap();
    let m = f.metrics(
        skrifa::instance::Size::unscaled(),
        skrifa::instance::LocationRef::default(),
    );
    f64::from(m.cap_height.unwrap()) / f64::from(m.units_per_em)
}

#[test]
fn drop_cap_indents_its_lines_and_spans_them() {
    let mut b = builder();
    let p = b.page(0);
    let f = b
        .text(p, Rect::new(36.0, 36.0, 300.0, 400.0), LOREM)
        .unwrap();
    let s = b.story_of(f);
    set_para(&mut b, s, 0, drop_cap(3, 1));
    let lay = layout(b.doc());
    let lines = &lay.frame(f).unwrap().lines;
    let cap = &lines[0].runs[0];
    assert_eq!(cap.glyphs.len(), 1);
    assert_eq!(cap.glyphs[0].text, 0..1, "the cap is the first character");
    // Cap height from the first line's cap height to the third baseline.
    let r = cap_ratio(cap);
    let cap_top = cap.glyphs[0].y - cap.font_size * r;
    let line_top = lines[0].baseline - 11.0 * r;
    assert!((cap.glyphs[0].y - lines[2].baseline).abs() < 1e-6);
    assert!((cap_top - line_top).abs() < 0.01, "{cap_top} vs {line_top}");
    assert!(cap.font_size > 40.0, "{}", cap.font_size);
    // The cap's lines start past it; the fourth line does not.
    let cap_right = cap.glyphs[0].x + cap.glyphs[0].advance;
    assert!((cap.glyphs[0].x - 4.0).abs() < 1e-6);
    for l in &lines[..3] {
        let body_x = l.carets.iter().find(|c| c.0 >= 1).unwrap().1;
        assert!(body_x >= cap_right + 2.0, "{body_x} vs {cap_right}");
    }
    assert!((lines[3].rect.x - 4.0).abs() < 1e-6);
    assert!((lines[1].rect.x - lines[2].rect.x).abs() < 1e-6);
    // Offsets stay right: the cap is on line 1 and can be hit.
    assert_eq!(lines[0].chars.start, 0);
    assert_eq!(lines[0].carets[0], (0, 4.0));
    assert_eq!(lines[0].offset_at_x(5.0), 0);
    assert!(lines[0].carets.windows(2).all(|w| w[0].0 + 1 == w[1].0));
    assert_eq!(
        line_texts(b.doc(), lay.frame(f).unwrap())[0].chars().next(),
        Some('L')
    );
    // Two characters make a wider cap.
    set_para(&mut b, s, 0, drop_cap(3, 2));
    let lay2 = layout(b.doc());
    let cap2 = &lay2.frame(f).unwrap().lines[0].runs[0];
    assert_eq!(cap2.glyphs.len(), 2);
    assert!(cap2.glyphs[1].x > cap.glyphs[0].x);
}

#[test]
fn drop_cap_on_a_short_paragraph_and_at_a_column_end() {
    // A one-line paragraph keeps the cap's three lines.
    let mut b = builder();
    let p = b.page(0);
    let f = b
        .text(p, Rect::new(36.0, 36.0, 300.0, 400.0), "Hi\nNext")
        .unwrap();
    let s = b.story_of(f);
    set_para(&mut b, s, 0, drop_cap(3, 1));
    let lay = layout(b.doc());
    let lines = &lay.frame(f).unwrap().lines;
    assert_eq!(lines.len(), 2);
    assert!(lines[1].rect.y >= lines[0].rect.y + 3.0 * LINE - 0.01);
    let cap = &lines[0].runs[0];
    assert!((cap.glyphs[0].y - (lines[0].baseline + 2.0 * LINE)).abs() < 0.01);
    // A cap that would hang below the column starts in the next one.
    let body = text_with_lines(4);
    let (mut b, f, s) = columns_doc(&format!("a\nb\nc\n{body}"), 2, 5);
    set_para(&mut b, s, 3, drop_cap(3, 1));
    assert_eq!(para_columns(b.doc(), f, s, 3).first(), Some(&1));
    // A paragraph of only the cap character.
    let (mut b, f, s) = columns_doc("A", 2, 5);
    set_para(&mut b, s, 0, drop_cap(2, 3));
    let lay = layout(b.doc());
    let l = &lay.frame(f).unwrap().lines[0];
    assert_eq!(l.chars, 0..1);
    assert_eq!(l.runs[0].glyphs.len(), 1);
    assert_eq!(l.carets.first().map(|c| c.0), Some(0));
    assert_eq!(l.carets.last().map(|c| c.0), Some(1));
    // The cap stops at a line separator or a break character.
    for sep in ['\u{2028}', '\u{2029}', pubrs_core::text::PAGE_BREAK] {
        let (mut b, f, s) = columns_doc(&format!("A{sep}bc"), 2, 5);
        set_para(&mut b, s, 0, drop_cap(2, 3));
        let lay = layout(b.doc());
        let cap = &lay.frame(f).unwrap().lines[0].runs[0];
        assert_eq!(cap.glyphs.len(), 1, "{sep:?}");
        assert_eq!(cap.glyphs[0].text, 0..1, "{sep:?}");
    }
}

/// Story text with `kind`'s break character between "One" and "Two".
fn with_break(kind: pubrs_core::text::BreakKind) -> String {
    format!("One{}Two", kind.char())
}

#[test]
fn column_break_continues_in_the_next_column() {
    use pubrs_core::text::BreakKind;
    let (b, f, s) = columns_doc(&with_break(BreakKind::Column), 2, 5);
    let doc = b.finish();
    let lay = layout(&doc);
    let ft = lay.frame(f).unwrap();
    assert_eq!(ft.lines.len(), 2);
    assert_eq!(ft.lines[0].chars, 0..3, "the break belongs to no line");
    assert_eq!(ft.lines[1].chars, 4..7);
    assert_eq!(para_columns(&doc, f, s, 0), vec![0, 1]);
    assert!((ft.lines[1].rect.y - 4.0).abs() < 1e-6);
    // The break draws nothing; carets sit before and after it.
    let glyphs: usize = ft
        .lines
        .iter()
        .flat_map(|l| &l.runs)
        .map(|r| r.glyphs.len())
        .sum();
    assert_eq!(glyphs, 6);
    assert_eq!(ft.lines[0].carets.last().map(|c| c.0), Some(3));
    assert_eq!(ft.lines[1].carets.first().map(|c| c.0), Some(4));
    assert!(lay.stories[&s].overflow.is_none());
    assert_eq!(lay.stories[&s].missing_glyphs, 0);
}

/// Frames A (two columns) and C on page 1 and B on page 2, linked
/// A → C → B (or just A → C when `with_b` is false).
fn chain_doc(text: &str, with_b: bool) -> (Document, [FrameId; 3]) {
    let mut b = builder();
    let p1 = b.page(0);
    let p2 = b.add_page().unwrap();
    let a = b
        .text(p1, Rect::new(36.0, 36.0, 300.0, 100.0), text)
        .unwrap();
    b.exec(Command::SetTextFrame {
        frame: a,
        props: TextFrameProps {
            columns: Some(2),
            ..Default::default()
        },
    })
    .unwrap();
    let c = b
        .text(p1, Rect::new(36.0, 200.0, 300.0, 100.0), "")
        .unwrap();
    let bb = b.text(p2, Rect::new(36.0, 36.0, 300.0, 100.0), "").unwrap();
    b.link(a, c).unwrap();
    if with_b {
        b.link(c, bb).unwrap();
    }
    (b.finish(), [a, c, bb])
}

#[test]
fn frame_break_continues_in_the_next_frame() {
    use pubrs_core::text::BreakKind;
    let (doc, [a, c, _]) = chain_doc(&with_break(BreakKind::Frame), true);
    let lay = layout(&doc);
    assert_eq!(lay.frame(a).unwrap().chars, 0..3);
    assert_eq!(
        lay.frame(a).unwrap().lines.len(),
        1,
        "not the second column"
    );
    assert_eq!(lay.frame(c).unwrap().chars, 4..7);
}

#[test]
fn page_break_continues_on_a_later_page() {
    use pubrs_core::text::BreakKind;
    let (doc, [a, c, bb]) = chain_doc(&with_break(BreakKind::Page), true);
    let lay = layout(&doc);
    assert_eq!(lay.frame(a).unwrap().chars, 0..3);
    assert!(lay.frame(c).unwrap().lines.is_empty(), "same page: skipped");
    assert_eq!(lay.frame(bb).unwrap().chars, 4..7);
    let s = doc.story_of(a).unwrap();
    assert!(lay.stories[&s].overflow.is_none());
}

#[test]
fn page_break_at_the_chain_end_overflows() {
    use pubrs_core::text::BreakKind;
    let (doc, [a, c, _]) = chain_doc(&with_break(BreakKind::Page), false);
    let lay = layout(&doc);
    let s = doc.story_of(a).unwrap();
    assert_eq!(lay.frame(a).unwrap().chars, 0..3);
    assert!(lay.frame(c).unwrap().lines.is_empty());
    assert_eq!(
        lay.stories[&s].overflow,
        Some(Overflow {
            offset: 4,
            hidden_chars: 3
        })
    );
    // A column break past the last column of the last frame overflows too.
    let (b, f, s) = columns_doc(&format!("One{}", BreakKind::Column.char()), 1, 5);
    let lay = layout(b.doc());
    assert_eq!(lay.frame(f).unwrap().chars, 0..3);
    assert_eq!(
        lay.stories[&s].overflow,
        Some(Overflow {
            offset: 4,
            hidden_chars: 0
        })
    );
}

/// Carets are frame-local like glyphs (before M15 they were measured from
/// the line's start, so the editing caret sat left of indented text).
#[test]
fn carets_are_frame_local_like_glyphs() {
    let mut b = builder();
    let p = b.page(0);
    let f = b
        .text(p, Rect::new(36.0, 36.0, 300.0, 100.0), "Indented line")
        .unwrap();
    let s = b.story_of(f);
    set_para(
        &mut b,
        s,
        0,
        ParaProps {
            left_indent: Some(20.0),
            ..Default::default()
        },
    );
    let lay = layout(b.doc());
    let l = &lay.frame(f).unwrap().lines[0];
    let g = &l.runs[0].glyphs;
    assert!((l.carets[0].1 - 24.0).abs() < 1e-6);
    assert!((l.carets[0].1 - g[0].x).abs() < 1e-6);
    assert!((l.carets[1].1 - g[1].x).abs() < 1e-6);
    assert_eq!(l.offset_at_x(g[1].x + 0.1), 1);
}

/// A bottom-aligned frame lifts a short drop-capped paragraph by its cap,
/// not just its lines, so the cap does not hang below the frame.
#[test]
fn bottom_alignment_counts_the_drop_cap() {
    let mut b = builder();
    let p = b.page(0);
    let f = b
        .text(p, Rect::new(36.0, 36.0, 300.0, 200.0), "Hi")
        .unwrap();
    let s = b.story_of(f);
    set_para(&mut b, s, 0, drop_cap(3, 1));
    b.exec(Command::SetTextFrame {
        frame: f,
        props: TextFrameProps {
            vertical_align: Some(VerticalAlign::Bottom),
            ..Default::default()
        },
    })
    .unwrap();
    let lay = layout(b.doc());
    let l = &lay.frame(f).unwrap().lines[0];
    let cap_base = l.runs[0].glyphs[0].y;
    assert!(cap_base > l.rect.bottom(), "the cap reaches below its line");
    assert!((cap_base - 196.0).abs() < 0.01, "{cap_base}");
}

// ---------------------------------------------------------------- bidi

fn one_line(text: &str) -> (Document, LineBox) {
    let mut b = builder();
    let p = b.page(0);
    let f = b
        .text(p, Rect::new(36.0, 36.0, 400.0, 100.0), text)
        .unwrap();
    let doc = b.finish();
    let lay = layout(&doc);
    let s = &lay.stories[&doc.story_of(f).unwrap()];
    let ft = &s.frames[&f];
    assert_eq!(ft.lines.len(), 1, "one line");
    let line = ft.lines[0].clone();
    (doc, line)
}

const HEBREW: &str = "שלום עולם";
const MIXED: &str = "abc שלום def";

/// Walk right through every caret place of a bidi line.
fn walk_right(l: &LineBox) -> Vec<(usize, f64)> {
    let mut at = l.visual_edge(false, false);
    let mut out = vec![at];
    while let Some(next) = l.visual_step(at.0, at.1, false) {
        assert!(next.1 > at.1, "x rises on every step");
        out.push(next);
        at = next;
    }
    out
}

#[test]
fn rtl_line_carets_fall_and_hit_testing_round_trips() {
    let (_, l) = one_line(HEBREW);
    assert!(l.is_bidi() && l.is_rtl());
    let xs: Vec<f64> = l.carets.iter().map(|c| c.1).collect();
    assert_eq!(xs.len(), 10);
    assert!(
        xs.windows(2).all(|w| w[0] > w[1]),
        "first character is rightmost: {xs:?}"
    );
    for &(o, x) in &l.carets {
        assert_eq!(l.caret_x(o), x);
        assert_eq!(l.offset_at_x(x), o);
        assert_eq!(l.offset_at_x(x + 0.2), o);
    }
    // Past either end of the line the nearest caret is the end's.
    assert_eq!(l.offset_at_x(xs[0] + 50.0), 0);
    assert_eq!(l.offset_at_x(xs[9] - 50.0), 9);
    // Home (start of an RTL line) is its right end, End the left.
    assert_eq!(l.visual_edge(true, false).0, 0);
    assert_eq!(l.visual_edge(false, false).0, 9);
}

#[test]
fn rtl_arrows_move_by_position() {
    let (_, l) = one_line(HEBREW);
    let order: Vec<usize> = walk_right(&l).iter().map(|s| s.0).collect();
    assert_eq!(
        order,
        (0..=9).rev().collect::<Vec<_>>(),
        "left to right is end to start"
    );
    // One step left of the first character is nothing: the line ends.
    let first = l.carets[0];
    assert_eq!(l.visual_step(first.0, first.1, false), None);
    assert_eq!(l.visual_step(0, first.1, true).map(|s| s.0), Some(1));
}

#[test]
fn mixed_line_visits_every_caret_in_visual_order() {
    let (_, l) = one_line(MIXED);
    assert!(l.is_bidi() && !l.is_rtl());
    // The Hebrew word is reversed: its first letter is right of its last.
    let x = |o: usize| l.caret_x(o);
    assert!(x(4) > x(5) && x(5) > x(6) && x(6) > x(7));
    // The Latin text around it is in order.
    assert!(x(0) < x(1) && x(1) < x(2) && x(2) < x(3) && x(9) < x(10) && x(10) < x(11));
    let right = walk_right(&l);
    let mut at = *right.last().unwrap();
    let mut left = vec![at];
    while let Some(n) = l.visual_step(at.0, at.1, true) {
        assert!(n.1 < at.1);
        left.push(n);
        at = n;
    }
    left.reverse();
    // Both walks cover the same places on the line...
    let xs = |w: &[(usize, f64)]| w.iter().map(|s| s.1).collect::<Vec<_>>();
    assert_eq!(xs(&right), xs(&left));
    // ...and together reach every offset. At the two places where the
    // Hebrew word meets the spaces, two offsets share one x; the walk takes
    // the one nearest the offset it comes from.
    let mut seen: Vec<usize> = right.iter().chain(&left).map(|s| s.0).collect();
    seen.sort_unstable();
    seen.dedup();
    assert_eq!(seen, (0..=12).collect::<Vec<_>>(), "{right:?} {left:?}");
    assert_eq!(right.len(), 13, "{right:?}");
    // End of a left-to-right line is its right end.
    assert_eq!(l.visual_edge(true, false).0, 12);
    assert_eq!(l.visual_edge(false, false).0, 0);
}

#[test]
fn mixed_line_hit_testing_picks_the_visually_nearest_place() {
    let (_, l) = one_line(MIXED);
    for (o, x) in l.slots() {
        let (ho, hx) = l.slot_at_x(x + 0.1);
        assert!((hx - x).abs() < 1e-9, "{o} at {x} -> {ho} at {hx}");
        assert_eq!(l.slot_x(o, Some(x)), x);
    }
    for &(o, x) in &l.carets {
        let hit = l.offset_at_x(x);
        assert_eq!(l.caret_x(hit), x, "offset {o} at {x} hits {hit}");
    }
    // The twin place of offset 4 is at the left edge of the word.
    let left_edge = l
        .slots()
        .into_iter()
        .filter(|s| s.0 == 4)
        .map(|s| s.1)
        .fold(f64::MAX, f64::min);
    assert!(left_edge < l.caret_x(7), "{left_edge}");
}

#[test]
fn bidi_selection_is_one_piece_per_visual_run() {
    let (_, l) = one_line(MIXED);
    // The whole Hebrew word: one piece between its outer carets.
    let word = l.selection_spans(4, 8);
    assert_eq!(word.len(), 1, "{word:?}");
    assert!((word[0].1 - l.caret_x(4)).abs() < 0.01);
    assert!(word[0].0 < l.caret_x(7));
    // "c", the space and the first Hebrew letter: Latin text and the word's
    // right end are apart.
    let split = l.selection_spans(2, 5);
    assert_eq!(split.len(), 2, "{split:?}");
    assert!(split[0].1 < split[1].0);
    // Everything: one piece covering the line.
    let all = l.selection_spans(0, 12);
    assert_eq!(all.len(), 1, "{all:?}");
    assert!((all[0].0 - l.caret_x(0)).abs() < 0.01);
    assert!((all[0].1 - l.caret_x(12)).abs() < 0.01);
    // A right-to-left line, same.
    let (_, h) = one_line(HEBREW);
    let part = h.selection_spans(2, 5);
    assert_eq!(part.len(), 1);
    assert!((part[0].1 - h.caret_x(2)).abs() < 0.01 && (part[0].0 - h.caret_x(5)).abs() < 0.01);
}

#[test]
fn left_to_right_lines_carry_no_bidi_data() {
    let (_, l) = one_line("Hello world");
    assert!(l.spans.is_empty() && l.slots().is_empty() && !l.is_bidi());
    let spans = l.selection_spans(2, 6);
    assert_eq!(spans, vec![(l.caret_x(2), l.caret_x(6))]);
}

#[test]
fn tabs_in_right_to_left_text_still_lay_out() {
    // Tab stops are measured from the left edge also in right-to-left
    // paragraphs (TODO.md); this pins that nothing breaks.
    let (_, l) = one_line("שלום\tעולם");
    assert_eq!(l.carets.len(), 10);
    assert!(l.carets.iter().all(|c| c.1.is_finite()));
}

#[test]
fn odd_lines_have_sane_carets() {
    // An empty paragraph, a tab alone, and Hebrew with vowel points
    // (combining marks): no panic, one caret per character boundary, finite
    // positions, and hit-testing returns a caret that exists.
    for text in [
        "",
        "\t",
        "\u{5e9}\u{5b8}\u{5c1}\u{5dc}\u{5d5}\u{5b9}\u{5dd}",
        "a\tb",
    ] {
        let (_, l) = one_line(text);
        let n = text.chars().count();
        assert_eq!(l.carets.len(), n + 1, "{text:?}: {:?}", l.carets);
        assert!(l.carets.iter().all(|c| c.1.is_finite()), "{text:?}");
        assert!(l.spans.is_empty() || l.spans.len() == l.carets.len());
        for &(o, x) in &l.carets {
            assert_eq!(l.caret_x(o), x);
            let hit = l.offset_at_x(x);
            assert_eq!(l.caret_x(hit), x, "{text:?} offset {o}");
        }
        // The walks terminate and stay on the line.
        let start = l.visual_edge(false, false);
        let mut at = start;
        for _ in 0..=n * 2 + 2 {
            match l.visual_step(at.0, at.1, false) {
                Some(next) => {
                    assert!(next.1 > at.1);
                    at = next;
                }
                None => break,
            }
        }
        let spans = l.selection_spans(0, n);
        assert!(spans.iter().all(|s| s.0.is_finite() && s.1 >= s.0));
    }
}
