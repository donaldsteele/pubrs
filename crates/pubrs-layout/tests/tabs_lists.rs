//! Tab stops, leaders, bullets and numbering (ADR 0019).
#![allow(clippy::unwrap_used)]

use std::path::PathBuf;

use pubrs_core::style::{List, ListFormat, ListKind, TabAlign, TabLeader, TabStop};
use pubrs_core::*;
use pubrs_layout::*;

fn layouter() -> Layouter {
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tests/fonts");
    Layouter::new(FontLibrary::from_dir(&dir).expect("test fonts"))
}

fn builder() -> DocBuilder<SeqIds> {
    DocBuilder::new(SeqIds::new(7), PageSetup::default())
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

fn tabs(stops: Vec<TabStop>) -> ParaProps {
    ParaProps {
        tab_stops: Some(stops),
        ..Default::default()
    }
}

fn stop(pos: f64, align: TabAlign, leader: TabLeader) -> TabStop {
    TabStop { pos, align, leader }
}

fn list(kind: ListKind, level: u8, format: ListFormat, start: Option<u32>) -> ParaProps {
    ParaProps {
        left_indent: Some(18.0 * (f64::from(level) + 1.0)),
        list: Some(List {
            kind,
            format,
            level,
            start,
            ..Default::default()
        }),
        ..Default::default()
    }
}

/// One text frame with `text`; returns the document, frame and story.
fn one_frame(text: &str, w: f64, h: f64) -> (DocBuilder<SeqIds>, FrameId, StoryId) {
    let mut b = builder();
    let p = b.page(0);
    let f = b.text(p, Rect::new(36.0, 36.0, w, h), text).unwrap();
    let s = b.story_of(f);
    (b, f, s)
}

fn lines_of(doc: &Document, f: FrameId) -> Vec<LineBox> {
    layouter()
        .layout_document(doc)
        .frame(f)
        .unwrap()
        .lines
        .clone()
}

/// Caret x for story offset `o` on whichever line holds it.
fn caret(lines: &[LineBox], o: usize) -> f64 {
    let l = lines
        .iter()
        .find(|l| l.chars.start <= o && o <= l.chars.end)
        .unwrap();
    l.caret_x(o)
}

fn leader_xs(l: &LineBox, ch: char) -> Vec<f64> {
    l.runs
        .iter()
        .filter(|r| r.text.chars().all(|c| c == ch) && !r.text.is_empty())
        .flat_map(|r| r.glyphs.iter().map(|g| g.x))
        .collect()
}

#[test]
fn a_tab_without_stops_goes_to_the_36pt_grid() {
    let (b, f, _) = one_frame("A\tB\tC", 300.0, 60.0);
    let lines = lines_of(&b.finish(), f);
    assert_eq!(lines.len(), 1);
    // The text area starts at x = 4.
    assert!(
        (caret(&lines, 2) - 40.0).abs() < 0.01,
        "after the first tab"
    );
    assert!(
        (caret(&lines, 4) - 76.0).abs() < 0.01,
        "after the second tab"
    );
    // The tab itself draws nothing and is not a missing glyph.
    let n: usize = lines[0].runs.iter().map(|r| r.glyphs.len()).sum();
    assert_eq!(n, 3);
    assert!(
        lines[0]
            .runs
            .iter()
            .flat_map(|r| &r.glyphs)
            .all(|g| g.id != 0)
    );
}

#[test]
fn stops_place_left_center_right_and_decimal_text() {
    let (mut b, f, s) = one_frame("\tL\n\tC\n\tR\n\t1.5\n\t12.25\n\t7", 300.0, 200.0);
    for (i, a) in [
        TabAlign::Left,
        TabAlign::Center,
        TabAlign::Right,
        TabAlign::Decimal,
        TabAlign::Decimal,
        TabAlign::Decimal,
    ]
    .into_iter()
    .enumerate()
    {
        set_para(&mut b, s, i, tabs(vec![stop(100.0, a, TabLeader::None)]));
    }
    let doc = b.finish();
    let lines = lines_of(&doc, f);
    let st = |i: usize| doc.story(s).unwrap().paragraph_start(i);
    let origin = 4.0;
    // Left: text starts on the stop; right: text ends on it.
    assert!((caret(&lines, st(0) + 1) - (origin + 100.0)).abs() < 0.01);
    assert!((caret(&lines, st(2) + 2) - (origin + 100.0)).abs() < 0.01);
    // Centre: the text straddles the stop.
    let (c0, c1) = (caret(&lines, st(1) + 1), caret(&lines, st(1) + 2));
    assert!(((c0 + c1) / 2.0 - (origin + 100.0)).abs() < 0.01);
    // Decimal: the point of 1.5 and of 12.25 sit on the stop; 7 (no point)
    // ends on it.
    assert!((caret(&lines, st(3) + 2) - (origin + 100.0)).abs() < 0.01);
    assert!((caret(&lines, st(4) + 3) - (origin + 100.0)).abs() < 0.01);
    assert!((caret(&lines, st(5) + 2) - (origin + 100.0)).abs() < 0.01);
}

#[test]
fn a_comma_language_aligns_decimals_on_the_comma() {
    let (mut b, f, s) = one_frame("\t1,5\n\t12,25", 300.0, 100.0);
    for i in 0..2 {
        set_para(
            &mut b,
            s,
            i,
            ParaProps {
                language: Some("de-DE".into()),
                tab_stops: Some(vec![stop(100.0, TabAlign::Decimal, TabLeader::None)]),
                ..Default::default()
            },
        );
    }
    let doc = b.finish();
    let lines = lines_of(&doc, f);
    let st1 = doc.story(s).unwrap().paragraph_start(1);
    assert!((caret(&lines, 2) - 104.0).abs() < 0.01);
    assert!((caret(&lines, st1 + 3) - 104.0).abs() < 0.01);
}

#[test]
fn tabs_past_the_last_stop_use_the_default_grid() {
    let (mut b, f, s) = one_frame("A\tB\tC\tD", 300.0, 60.0);
    set_para(
        &mut b,
        s,
        0,
        tabs(vec![stop(50.0, TabAlign::Left, TabLeader::None)]),
    );
    let lines = lines_of(&b.finish(), f);
    // First tab: the explicit stop; then the next multiples of 36 past it.
    assert!((caret(&lines, 2) - 54.0).abs() < 0.01);
    assert!((caret(&lines, 4) - 76.0).abs() < 0.01);
    assert!((caret(&lines, 6) - 112.0).abs() < 0.01);
}

#[test]
fn right_tab_with_dot_leaders_lines_up_across_lines() {
    let (mut b, f, s) = one_frame(
        "Tomato soup\t4.50\nGrilled salmon with lemon\t18.00\nTea\t2.00",
        300.0,
        100.0,
    );
    for i in 0..3 {
        set_para(
            &mut b,
            s,
            i,
            tabs(vec![stop(250.0, TabAlign::Right, TabLeader::Dot)]),
        );
    }
    let doc = b.finish();
    let lines = lines_of(&doc, f);
    assert_eq!(lines.len(), 3);
    let story = doc.story(s).unwrap();
    for (i, l) in lines.iter().enumerate() {
        // The price ends on the stop.
        assert!(
            (l.caret_x(l.chars.end) - 254.0).abs() < 0.01,
            "line {i} ends at the stop"
        );
        let dots = leader_xs(l, '.');
        assert!(dots.len() > 10, "line {i} has leaders: {}", dots.len());
        // Leaders stay between the label and the price.
        let before = l.caret_x(story.paragraph_start(i) + story.paragraphs[i].char_len());
        assert!(before <= 254.0);
        let first_tab = l.chars.start + story.paragraphs[i].text().find('\t').unwrap();
        assert!(dots[0] >= l.caret_x(first_tab));
    }
    // A global grid: every dot of every line sits on a multiple of the pitch.
    let pitch = {
        let d = leader_xs(&lines[0], '.');
        d[1] - d[0]
    };
    for l in &lines {
        for x in leader_xs(l, '.') {
            let n = (x - 4.0) / pitch;
            assert!((n - n.round()).abs() < 1e-6, "dot at {x} is off the grid");
        }
    }
}

#[test]
fn dash_and_line_leaders_use_their_glyph() {
    let (mut b, f, s) = one_frame("A\tB\nC\tD", 300.0, 60.0);
    set_para(
        &mut b,
        s,
        0,
        tabs(vec![stop(200.0, TabAlign::Left, TabLeader::Dash)]),
    );
    set_para(
        &mut b,
        s,
        1,
        tabs(vec![stop(200.0, TabAlign::Left, TabLeader::Line)]),
    );
    let lines = lines_of(&b.finish(), f);
    assert!(leader_xs(&lines[0], '-').len() > 5);
    assert!(leader_xs(&lines[1], '_').len() > 5);
    assert!(leader_xs(&lines[1], '-').is_empty());
}

#[test]
fn carets_and_hit_testing_work_across_a_tab_gap() {
    let (mut b, f, s) = one_frame("ab\tcd", 300.0, 60.0);
    set_para(
        &mut b,
        s,
        0,
        tabs(vec![stop(120.0, TabAlign::Left, TabLeader::Dot)]),
    );
    let lines = lines_of(&b.finish(), f);
    let l = &lines[0];
    assert!(l.carets.windows(2).all(|w| w[0].1 <= w[1].1));
    let before = l.caret_x(2);
    let after = l.caret_x(3);
    assert!((after - 124.0).abs() < 0.01);
    assert!(after - before > 50.0);
    // Clicks in the gap land on the nearer side of the tab.
    assert_eq!(l.offset_at_x(before + 5.0), 2);
    assert_eq!(l.offset_at_x(after - 5.0), 3);
    assert_eq!(l.offset_at_x(1000.0), 5);
}

#[test]
fn justified_paragraphs_with_tabs_are_not_stretched() {
    let (mut b, f, s) = one_frame(
        "one two\tthree four five six seven eight nine ten eleven twelve thirteen fourteen",
        200.0,
        100.0,
    );
    set_para(
        &mut b,
        s,
        0,
        ParaProps {
            align: Some(Align::Justify),
            tab_stops: Some(vec![stop(100.0, TabAlign::Left, TabLeader::None)]),
            ..Default::default()
        },
    );
    let lines = lines_of(&b.finish(), f);
    assert!(lines.len() > 1);
    assert!((lines[0].caret_x(8) - 104.0).abs() < 0.01);
    // Word gaps are natural: the first segment is as wide as unjustified.
    let w = lines[0].caret_x(7) - lines[0].caret_x(0);
    assert!(w < 60.0, "first segment is {w} wide");
}

#[test]
fn underline_is_split_around_the_tab_gap() {
    let (mut b, f, s) = one_frame("ab\tcd", 300.0, 60.0);
    b.exec(Command::ApplyCharProps {
        story: s,
        start: 0,
        end: 5,
        props: CharProps {
            underline: Some(true),
            ..Default::default()
        },
    })
    .unwrap();
    let lines = lines_of(&b.finish(), f);
    let l = &lines[0];
    assert_eq!(l.decorations.len(), 2);
    assert!(l.decorations[0].rect.right() <= l.caret_x(2) + 0.01);
    assert!((l.decorations[1].rect.x - l.caret_x(3)).abs() < 0.01);
}

fn labels(lines: &[LineBox]) -> Vec<Option<String>> {
    lines.iter().map(|l| l.list_label.clone()).collect()
}

#[test]
fn nested_numbering_restarts_deeper_levels() {
    let (mut b, f, s) = one_frame("one\ntwo\nsub a\nsub b\nthree\nsub again", 300.0, 200.0);
    use ListFormat::*;
    let items = [
        (0, Decimal),
        (0, Decimal),
        (1, LowerAlpha),
        (1, LowerAlpha),
        (0, Decimal),
        (1, LowerAlpha),
    ];
    for (i, (lv, fmt)) in items.into_iter().enumerate() {
        set_para(&mut b, s, i, list(ListKind::Number, lv, fmt, None));
    }
    let lines = lines_of(&b.finish(), f);
    let got: Vec<String> = labels(&lines).into_iter().map(Option::unwrap).collect();
    assert_eq!(got, ["1.", "2.", "a.", "b.", "3.", "a."]);
}

#[test]
fn label_hangs_left_of_the_text() {
    let (mut b, f, s) = one_frame("first item\nsecond", 300.0, 100.0);
    for i in 0..2 {
        set_para(
            &mut b,
            s,
            i,
            list(ListKind::Number, 0, ListFormat::Decimal, None),
        );
    }
    let lines = lines_of(&b.finish(), f);
    let l = &lines[0];
    // left_indent 18, indent 18: the label at the frame edge, text at 18.
    let label = l.runs.first().unwrap();
    assert_eq!(&*label.text, "1.");
    assert!((label.glyphs[0].x - 4.0).abs() < 0.01);
    let text_run = &l.runs[1];
    assert!((text_run.glyphs[0].x - 22.0).abs() < 0.01);
    // The label is not text: carets and offsets are those of the story.
    assert_eq!(l.chars, 0..10);
    assert_eq!(l.carets.len(), 11);
    assert!((l.caret_x(0) - 22.0).abs() < 0.01);
    assert_eq!(lines[1].chars, 11..17);
}

#[test]
fn bullets_use_the_bullet_character() {
    let (mut b, f, s) = one_frame("alpha\nbeta", 300.0, 100.0);
    set_para(
        &mut b,
        s,
        0,
        ParaProps {
            left_indent: Some(18.0),
            list: Some(List {
                kind: ListKind::Bullet,
                bullet: '\u{2013}',
                ..Default::default()
            }),
            ..Default::default()
        },
    );
    set_para(
        &mut b,
        s,
        1,
        list(ListKind::Bullet, 0, ListFormat::Decimal, None),
    );
    let lines = lines_of(&b.finish(), f);
    assert_eq!(
        labels(&lines),
        [Some("\u{2013}".to_string()), Some("\u{2022}".to_string())]
    );
    assert!(lines[0].runs[0].glyphs[0].id != 0);
}

#[test]
fn start_restarts_and_plain_paragraphs_do_not_break_the_count() {
    let (mut b, f, s) = one_frame("a\nb\nplain\nc\nd\ne", 300.0, 200.0);
    for (i, start) in [(0, None), (1, None), (3, None), (4, Some(10)), (5, None)] {
        set_para(
            &mut b,
            s,
            i,
            list(ListKind::Number, 0, ListFormat::Decimal, start),
        );
    }
    let lines = lines_of(&b.finish(), f);
    let got: Vec<Option<String>> = labels(&lines);
    assert_eq!(
        got,
        [
            Some("1.".into()),
            Some("2.".into()),
            None,
            Some("3.".into()),
            Some("10.".into()),
            Some("11.".into())
        ]
    );
}

#[test]
fn formats_count_in_letters_and_roman_numerals() {
    assert_eq!(ListFormat::UpperAlpha.label(27), "AA.");
    assert_eq!(ListFormat::LowerAlpha.label(26), "z.");
    assert_eq!(ListFormat::LowerRoman.label(14), "xiv.");
    assert_eq!(ListFormat::UpperRoman.label(1994), "MCMXCIV.");
    assert_eq!(ListFormat::Decimal.label(7), "7.");
}

#[test]
fn numbering_continues_across_linked_frames() {
    let mut b = builder();
    let p = b.page(0);
    let text = (1..=12)
        .map(|i| format!("item {i}"))
        .collect::<Vec<_>>()
        .join("\n");
    let f1 = b
        .text(p, Rect::new(36.0, 36.0, 200.0, 60.0), &text)
        .unwrap();
    let f2 = b.text(p, Rect::new(300.0, 36.0, 200.0, 400.0), "").unwrap();
    let s = b.story_of(f1);
    for i in 0..12 {
        set_para(
            &mut b,
            s,
            i,
            list(ListKind::Number, 0, ListFormat::Decimal, None),
        );
    }
    b.exec(Command::LinkFrames { from: f1, to: f2 }).unwrap();
    let doc = b.finish();
    let lay = layouter().layout_document(&doc);
    let a = labels(&lay.frame(f1).unwrap().lines);
    let c = labels(&lay.frame(f2).unwrap().lines);
    assert!(!a.is_empty() && !c.is_empty());
    let all: Vec<String> = a.into_iter().chain(c).map(Option::unwrap).collect();
    let want: Vec<String> = (1..=12).map(|i| format!("{i}.")).collect();
    assert_eq!(all, want);
}

#[test]
fn keep_rewinds_do_not_disturb_numbering() {
    let text = (1..=8)
        .map(|i| format!("item {i}"))
        .collect::<Vec<_>>()
        .join("\n");
    let run = |keep: bool| {
        let mut b = builder();
        let p = b.page(0);
        let f1 = b
            .text(p, Rect::new(36.0, 36.0, 200.0, 56.0), &text)
            .unwrap();
        let f2 = b.text(p, Rect::new(300.0, 36.0, 200.0, 300.0), "").unwrap();
        let s = b.story_of(f1);
        for i in 0..8 {
            set_para(
                &mut b,
                s,
                i,
                ParaProps {
                    keep_with_next: keep.then_some(true),
                    ..list(ListKind::Number, 0, ListFormat::Decimal, None)
                },
            );
        }
        b.exec(Command::LinkFrames { from: f1, to: f2 }).unwrap();
        let doc = b.finish();
        let lay = layouter().layout_document(&doc);
        let mut v = labels(&lay.frame(f1).unwrap().lines);
        v.extend(labels(&lay.frame(f2).unwrap().lines));
        v.into_iter().map(Option::unwrap).collect::<Vec<_>>()
    };
    let want: Vec<String> = (1..=8).map(|i| format!("{i}.")).collect();
    assert_eq!(run(false), want);
    assert_eq!(run(true), want);
}

#[test]
fn empty_list_item_still_shows_its_label() {
    let (mut b, f, s) = one_frame("a\n\nc", 300.0, 100.0);
    for i in 0..3 {
        set_para(
            &mut b,
            s,
            i,
            list(ListKind::Number, 0, ListFormat::Decimal, None),
        );
    }
    let lines = lines_of(&b.finish(), f);
    assert_eq!(
        labels(&lines),
        [Some("1.".into()), Some("2.".into()), Some("3.".into())]
    );
}

#[test]
fn a_style_can_set_tabs_and_a_list() {
    let mut p = ParaProps {
        tab_stops: Some(vec![stop(80.0, TabAlign::Left, TabLeader::None)]),
        ..Default::default()
    };
    let o = ParaProps {
        list: Some(List::default()),
        ..Default::default()
    };
    p = p.merged(&o);
    assert!(p.tab_stops.is_some() && p.list.is_some());
    // An empty list clears stops; kind none clears the list.
    p = p.merged(&ParaProps {
        tab_stops: Some(vec![]),
        list: Some(List {
            kind: ListKind::None,
            ..Default::default()
        }),
        ..Default::default()
    });
    assert_eq!(p.tab_stops, Some(vec![]));
}
