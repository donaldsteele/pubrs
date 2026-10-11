//! Hyphenation, soft hyphens and bounded justification.
#![allow(clippy::unwrap_used)]

use std::path::PathBuf;

use pubrs_core::*;
use pubrs_layout::*;

const TEXT: &str = "Extraordinary hyphenation demonstrates comprehensive typographic \
craftsmanship throughout international newsletters, particularly considering \
unforgettable communication opportunities.";

fn fonts() -> FontLibrary {
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tests/fonts");
    FontLibrary::from_dir(&dir).expect("test fonts")
}

fn lay(text: &str, width: f64, props: ParaProps) -> (Document, FrameText, FrameId) {
    let mut b = DocBuilder::new(SeqIds::new(14), PageSetup::default());
    let p = b.page(0);
    let f = b
        .text(p, Rect::new(36.0, 36.0, width, 600.0), text)
        .unwrap();
    let s = b.story_of(f);
    b.exec(Command::ApplyParaProps {
        story: s,
        start: 0,
        end: 0,
        props,
    })
    .unwrap();
    let doc = b.finish();
    let l = Layouter::new(fonts()).layout_document(&doc);
    let ft = l.stories[&doc.story_of(f).unwrap()].frames[&f].clone();
    (doc, ft, f)
}

fn justify(hyphenate: bool, limits: Option<JustifyLimits>, lang: &str) -> ParaProps {
    ParaProps {
        align: Some(Align::Justify),
        hyphenate: Some(hyphenate),
        language: Some(lang.into()),
        justify_limits: limits,
        ..Default::default()
    }
}

/// A line ends in a hyphen glyph: its last glyph has no text of its own.
fn hyphenated(l: &LineBox) -> bool {
    l.runs
        .last()
        .and_then(|r| r.glyphs.last())
        .is_some_and(|g| g.text.is_empty() && g.advance > 0.0)
}

fn chars_of(doc: &Document, l: &LineBox) -> String {
    let t: Vec<char> = doc
        .stories
        .values()
        .next()
        .unwrap()
        .plain_text()
        .chars()
        .collect();
    t[l.chars.clone()].iter().collect()
}

#[test]
fn off_by_default_never_hyphenates() {
    let (_, ft, _) = lay(TEXT, 150.0, justify(false, None, "en-US"));
    assert!(ft.lines.iter().all(|l| !hyphenated(l)));
}

#[test]
fn hyphenation_breaks_words_and_keeps_story_offsets() {
    let (doc, ft, _) = lay(TEXT, 150.0, justify(true, None, "en-US"));
    let n = ft.lines.iter().filter(|l| hyphenated(l)).count();
    assert!(n >= 1, "expected a hyphenated line");
    // Offsets are in story characters and contiguous; no hyphen is added.
    let mut at = 0;
    for l in &ft.lines {
        assert_eq!(l.chars.start, at);
        at = l.chars.end;
        assert!(!chars_of(&doc, l).contains('\u{ad}'));
        // Carets cover exactly the line's characters.
        assert_eq!(l.carets.first().unwrap().0, l.chars.start);
        assert_eq!(l.carets.last().unwrap().0, l.chars.end);
        assert_eq!(l.carets.len(), l.chars.len() + 1);
    }
    assert_eq!(at, TEXT.chars().count());
    // A hyphenated line stays inside the column.
    for l in ft.lines.iter().filter(|l| hyphenated(l)) {
        let g = l.runs.last().unwrap().glyphs.last().unwrap();
        assert!(g.x + g.advance <= 150.0 - 4.0 + 0.5, "{}", g.x + g.advance);
    }
}

#[test]
fn language_without_patterns_adds_no_hyphens_and_validate_notes_it() {
    let (doc, ft, _) = lay(TEXT, 150.0, justify(true, None, "el-GR"));
    assert!(ft.lines.iter().all(|l| !hyphenated(l)));
    let w = validate::validate(&doc);
    assert!(
        w.iter()
            .any(|w| w.code == validate::WarningCode::HyphenationUnavailable)
    );
    let (doc, _, _) = lay(TEXT, 150.0, justify(true, None, "de-DE"));
    assert!(validate::validate(&doc).is_empty());
}

#[test]
fn typed_soft_hyphen_is_honoured_without_hyphenate() {
    let text = "aaa bbb ccc ddd eee fff ggg hhh iii jjj kkk lll mmm nnn ooo ppp \
qqq rrr sss tttt\u{ad}uuuuuu vvv";
    let mut hits = 0;
    for w in [90.0, 100.0, 110.0, 120.0, 130.0, 140.0, 150.0] {
        let (doc, ft, _) = lay(text, w, ParaProps::default());
        for l in &ft.lines {
            if hyphenated(l) {
                hits += 1;
                assert!(chars_of(&doc, l).ends_with("tttt\u{ad}"));
            }
            assert!(!chars_of(&doc, l).is_empty() || ft.lines.len() == 1);
        }
    }
    assert!(hits >= 1, "a typed soft hyphen should break some line");
    // Invisible when the line does not end there.
    let (_, ft, _) = lay("tttt\u{ad}uuuuuu", 300.0, ParaProps::default());
    assert!(!hyphenated(&ft.lines[0]));
}

fn right_edge(l: &LineBox) -> f64 {
    l.runs
        .iter()
        .flat_map(|r| {
            r.glyphs
                .iter()
                .filter(|g| !r.text[g.text.clone()].trim().is_empty() || g.text.is_empty())
        })
        .map(|g| g.x + g.advance)
        .fold(0.0, f64::max)
}

#[test]
fn unset_limits_justify_without_bounds() {
    let (_, ft, _) = lay(TEXT, 200.0, justify(false, None, "en-US"));
    let n = ft.lines.len();
    for l in &ft.lines[..n - 1] {
        assert!(
            (right_edge(l) - (200.0 - 4.0)).abs() < 0.6,
            "{}",
            right_edge(l)
        );
    }
}

#[test]
fn limits_leave_over_loose_lines_short_and_never_stretch_the_last() {
    let tight = JustifyLimits {
        max_word_space: 1.0,
        letter_spacing: 0.0,
        unbounded: false,
    };
    let (_, ft, _) = lay(TEXT, 200.0, justify(false, Some(tight), "en-US"));
    let n = ft.lines.len();
    let short = ft.lines[..n - 1]
        .iter()
        .filter(|l| right_edge(l) < 200.0 - 4.0 - 1.0)
        .count();
    assert!(
        short >= 1,
        "lines needing more than the caps are left short"
    );
    assert!(right_edge(&ft.lines[n - 1]) < 200.0 - 4.0 - 1.0);
    // Letter spacing takes the remainder up to its cap, the line still fills.
    let lax = JustifyLimits {
        max_word_space: 1.0,
        letter_spacing: 1.0,
        unbounded: false,
    };
    let (_, ft, _) = lay(TEXT, 200.0, justify(false, Some(lax), "en-US"));
    for l in &ft.lines[..ft.lines.len() - 1] {
        assert!((right_edge(l) - 196.0).abs() < 0.6, "{}", right_edge(l));
        assert!((l.rect.width - 192.0).abs() < 0.6 || l.rect.width > 150.0);
    }
}

#[test]
fn hyphenation_tightens_limited_justification() {
    let lim = JustifyLimits::default();
    let (_, plain, _) = lay(TEXT, 150.0, justify(false, Some(lim), "en-US"));
    let (_, hy, _) = lay(TEXT, 150.0, justify(true, Some(lim), "en-US"));
    let short = |ft: &FrameText| {
        ft.lines[..ft.lines.len() - 1]
            .iter()
            .filter(|l| right_edge(l) < 150.0 - 8.0 - 1.0)
            .count()
    };
    assert!(short(&hy) <= short(&plain));
    assert!(hy.lines.iter().any(hyphenated));
}

fn line_texts(doc: &Document, ft: &FrameText) -> Vec<String> {
    ft.lines.iter().map(|l| chars_of(doc, l)).collect()
}

fn byte_of_char(text: &str, c: usize) -> usize {
    text.char_indices().nth(c).map_or(text.len(), |(b, _)| b)
}

#[test]
fn right_to_left_run_falls_back_to_plain_layout() {
    // The line analysis cannot follow right-to-left text: no markers are
    // used, so the layout is exactly the unhyphenated one.
    let text = "Extraordinary hyphenation \u{5e9}\u{5dc}\u{5d5}\u{5dd} demonstrates comprehensive \
typographic craftsmanship throughout international newsletters, particularly considering \
unforgettable communication opportunities.";
    let (doc, on, _) = lay(text, 150.0, justify(true, None, "en-US"));
    let (_, off, _) = lay(text, 150.0, justify(false, None, "en-US"));
    assert_eq!(line_texts(&doc, &on), line_texts(&doc, &off));
    assert_eq!(sig(&on), sig(&off));
}

#[test]
fn many_inserted_markers_map_back_to_story_offsets() {
    let (doc, ft, _) = lay(TEXT, 330.0, justify(true, None, "en-US"));
    let story = doc.stories.values().next().unwrap().plain_text();
    for l in &ft.lines {
        let (b0, b1) = (
            byte_of_char(&story, l.chars.start),
            byte_of_char(&story, l.chars.end),
        );
        let mut shown = String::new();
        for r in &l.runs {
            for g in &r.glyphs {
                assert!(
                    g.text.start >= b0 && g.text.end <= b1,
                    "{:?} vs {b0}..{b1}",
                    g.text
                );
                shown.push_str(&r.text[g.text.clone()]);
            }
        }
        assert_eq!(shown, story[b0..b1], "glyphs cover exactly the line's text");
        assert!(!shown.contains('\u{ad}'));
        let xs: Vec<f64> = l.carets.iter().map(|c| c.1).collect();
        assert!(xs.windows(2).all(|w| w[0] <= w[1] + 1e-6), "{xs:?}");
        for (k, c) in l.carets.iter().enumerate() {
            assert_eq!(c.0, l.chars.start + k);
        }
    }
}

#[test]
fn typed_soft_hyphen_beside_a_hyphenated_word() {
    let text = "Extraordinary\u{ad}hyphenation demonstrates comprehensive typographic \
craftsmanship throughout international newsletters.";
    for w in [90.0, 120.0, 150.0, 200.0] {
        let (doc, ft, _) = lay(text, w, justify(true, None, "en-US"));
        let mut at = 0;
        for l in &ft.lines {
            assert_eq!(l.chars.start, at);
            at = l.chars.end;
        }
        assert_eq!(at, text.chars().count());
        let _ = doc;
    }
}

#[test]
fn the_last_word_of_a_paragraph_can_be_hyphenated() {
    let text = "We celebrate the extraordinary internationalization";
    let mut seen = false;
    for w in (40..200).step_by(6) {
        let (_, ft, _) = lay(text, f64::from(w), justify(true, None, "en-US"));
        let n = ft.lines.len();
        let mut at = 0;
        for l in &ft.lines {
            assert_eq!(l.chars.start, at);
            at = l.chars.end;
        }
        assert_eq!(at, text.chars().count());
        assert!(ft.lines[n - 1].ends_paragraph);
        assert!(!hyphenated(&ft.lines[n - 1]));
        seen |= ft.lines[..n - 1].iter().any(hyphenated);
    }
    assert!(seen);
}

#[test]
fn letter_spaced_lines_have_consistent_carets_and_hit_testing() {
    let lax = JustifyLimits {
        max_word_space: 1.0,
        letter_spacing: 1.0,
        unbounded: false,
    };
    let (_, ft, _) = lay(TEXT, 200.0, justify(true, Some(lax), "en-US"));
    let mut checked = 0;
    for l in &ft.lines[..ft.lines.len() - 1] {
        let xs: Vec<f64> = l.carets.iter().map(|c| c.1).collect();
        assert!(xs.windows(2).all(|w| w[0] < w[1]), "{xs:?}");
        for (off, x) in &l.carets[..l.carets.len() - 1] {
            assert_eq!(l.offset_at_x(*x + 0.01), *off);
            assert!((l.caret_x(*off) - *x).abs() < 1e-9);
        }
        // The line's last caret is at its right edge, inside the column.
        let last = *xs.last().unwrap();
        assert!(last <= 200.0 - 4.0 + 0.6, "{last}");
        assert!(last >= 200.0 - 4.0 - 8.0, "{last}");
        checked += 1;
    }
    assert!(checked >= 3);
}

#[test]
fn unbounded_overrides_limits_and_matches_unset() {
    let tight = JustifyLimits {
        max_word_space: 1.0,
        letter_spacing: 0.0,
        unbounded: false,
    };
    let off = JustifyLimits {
        unbounded: true,
        ..Default::default()
    };
    let (_, with, _) = lay(TEXT, 200.0, justify(false, Some(tight), "en-US"));
    let (_, unb, _) = lay(TEXT, 200.0, justify(false, Some(off), "en-US"));
    let (_, unset, _) = lay(TEXT, 200.0, justify(false, None, "en-US"));
    assert_ne!(sig(&with), sig(&unset));
    assert_eq!(sig(&unb), sig(&unset));
}

#[test]
fn incremental_layout_equals_full_when_toggling_hyphenation_and_limits() {
    let mut b = DocBuilder::new(SeqIds::new(14), PageSetup::default());
    let p = b.page(0);
    let f = b
        .text(p, Rect::new(36.0, 36.0, 150.0, 500.0), TEXT)
        .unwrap();
    let s = b.story_of(f);
    let mut ed = Editor::new(b.finish());
    let (mut lay, mut cache) = (Layouter::new(fonts()), StoryLayoutCache::new());
    let steps = [
        ParaProps {
            align: Some(Align::Justify),
            ..Default::default()
        },
        ParaProps {
            hyphenate: Some(true),
            ..Default::default()
        },
        ParaProps {
            language: Some("de-DE".into()),
            ..Default::default()
        },
        ParaProps {
            language: Some("el-GR".into()),
            ..Default::default()
        },
        ParaProps {
            language: Some("en-US".into()),
            justify_limits: Some(JustifyLimits::default()),
            ..Default::default()
        },
        ParaProps {
            justify_limits: Some(JustifyLimits {
                unbounded: true,
                ..Default::default()
            }),
            ..Default::default()
        },
        ParaProps {
            hyphenate: Some(false),
            ..Default::default()
        },
    ];
    let mut layouts = Vec::new();
    for props in steps {
        ed.execute(Command::ApplyParaProps {
            story: s,
            start: 0,
            end: 0,
            props,
        })
        .unwrap();
        let inc = cache.layout(&mut lay, ed.doc());
        let full = lay.layout_document(ed.doc());
        assert_eq!(inc.stories.get(&s), full.stories.get(&s));
        layouts.push(full.stories[&s].clone());
    }
    // Turning hyphenation on really changed the layout, and off restored it.
    assert_ne!(layouts[0], layouts[1]);
    assert_eq!(layouts[0], layouts[6]);
}

/// A layout without font handles, comparable across layouters.
fn sig(ft: &FrameText) -> Vec<String> {
    ft.lines
        .iter()
        .map(|l| {
            let g: Vec<_> = l
                .runs
                .iter()
                .flat_map(|r| {
                    r.glyphs
                        .iter()
                        .map(|g| (g.id, g.x, g.advance, g.text.clone()))
                })
                .collect();
            format!("{:?} {:?} {:?} {g:?}", l.chars, l.rect, l.carets)
        })
        .collect()
}

#[test]
fn very_narrow_columns_stay_consistent() {
    // Emergency breaks, unplaceable line ends and the fallback to plain
    // layout must never lose or duplicate text, or push a hyphen outside.
    let total = TEXT.chars().count();
    for w in (12..80).step_by(4) {
        for p in [
            justify(true, None, "en-US"),
            justify(true, Some(JustifyLimits::default()), "de-DE"),
        ] {
            let (_, ft, _) = lay(TEXT, f64::from(w), p);
            let mut at = 0;
            for l in &ft.lines {
                assert_eq!(l.chars.start, at, "width {w}");
                at = l.chars.end;
                if hyphenated(l) && l.runs.iter().map(|r| r.glyphs.len()).sum::<usize>() > 2 {
                    let g = l.runs.last().unwrap().glyphs.last().unwrap();
                    assert!(g.x + g.advance <= f64::from(w) - 4.0 + 0.6, "width {w}");
                }
            }
            assert_eq!(at, total, "width {w}");
        }
    }
}
