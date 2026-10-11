//! Spell checking (M28): the bundled en_US dictionary, suggestions, user
//! words and dictionaries, per-paragraph languages, word segmentation and the
//! paragraph cache.

use std::path::PathBuf;
use std::sync::Arc;

use pubrs_core::{DocBuilder, Document, PageSetup, Paragraph, Parent, Rect, SeqIds, StoryId};
use pubrs_io::spell::{SpellChecker, SpellError, SpellOptions, USER_WORDS_FILE, words};

/// A fresh, empty folder for one test.
fn scratch(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("pubrs-spell-{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("temp dir");
    dir
}

/// One text frame whose paragraphs have the given (text, language) pairs.
fn doc_with(paras: &[(&str, Option<&str>)]) -> (Document, StoryId) {
    let mut b = DocBuilder::new(SeqIds::new(2801), PageSetup::default());
    let page = b.page(0);
    let paragraphs = paras
        .iter()
        .map(|(t, lang)| {
            let mut p = Paragraph::plain(t);
            p.props.language = lang.map(str::to_string);
            p
        })
        .collect();
    let f = b
        .text_paragraphs(
            Parent::Page(page),
            Rect::new(36.0, 36.0, 400.0, 300.0),
            paragraphs,
        )
        .expect("frame");
    let s = b.story_of(f);
    (b.finish(), s)
}

fn texts(text: &str) -> Vec<String> {
    words(text, &SpellOptions::default())
        .into_iter()
        .map(|w| w.text)
        .collect()
}

#[test]
fn known_good_and_bad_words() {
    let mut c = SpellChecker::new(None);
    for good in [
        "the", "The", "THE", "color", "optimize", "receive", "don't", "can’t", "O'Neil", "Paris",
        "bakery", "bakery's",
    ] {
        assert_eq!(c.check_word(good, "en-US"), Some(true), "{good}");
    }
    for bad in ["teh", "recieve", "optimise", "colour", "bakeyr", "paris"] {
        assert_eq!(c.check_word(bad, "en-US"), Some(false), "{bad}");
    }
    let found = c
        .check_text("Teh baker will recieve the flour.", "en")
        .expect("en has a dictionary");
    let got: Vec<(usize, usize, &str)> = found
        .iter()
        .map(|m| (m.start, m.end, m.word.as_str()))
        .collect();
    assert_eq!(got, [(0, 3, "Teh"), (15, 22, "recieve")]);
}

#[test]
fn suggestions_are_good_and_stable() {
    let mut c = SpellChecker::new(None);
    let s = c.suggest("recieve", "en-US").expect("dictionary");
    assert_eq!(s.first().map(String::as_str), Some("receive"), "{s:?}");
    let teh = c.suggest("teh", "en_US").expect("dictionary");
    assert!(teh.iter().any(|w| w == "the"), "{teh:?}");
    // Same order in another checker (fixed-seed hashing).
    let mut d = SpellChecker::new(None);
    assert_eq!(d.suggest("bakeyr", "en").expect("dictionary"), {
        c.suggest("bakeyr", "en").expect("dictionary")
    });
    assert_eq!(c.suggest("teh", "fr-FR"), None, "no French dictionary");
}

#[test]
fn user_words_persist_across_checkers() {
    let dir = scratch("words");
    let mut c = SpellChecker::new(Some(dir.clone()));
    assert_eq!(c.check_word("pubrs", "en-US"), Some(false));
    assert!(c.add_word("pubrs").expect("added"));
    assert!(!c.add_word(" pubrs ").expect("already there"));
    assert_eq!(c.check_word("pubrs", "en-US"), Some(true));
    let saved = std::fs::read_to_string(dir.join(USER_WORDS_FILE)).expect("file written");
    assert!(saved.lines().any(|l| l == "pubrs"), "{saved}");

    let mut again = SpellChecker::new(Some(dir.clone()));
    for form in ["pubrs", "Pubrs", "PUBRS"] {
        assert_eq!(again.check_word(form, "en-US"), Some(true), "{form}");
    }
    // A word added by another checker in the meantime is kept.
    assert!(again.add_word("Kerncraft").expect("added"));
    assert!(c.add_word("leadingly").expect("added"));
    let saved = std::fs::read_to_string(dir.join(USER_WORDS_FILE)).expect("file");
    for w in ["pubrs", "Kerncraft", "leadingly"] {
        assert!(saved.lines().any(|l| l == w), "{w} missing from {saved}");
    }
    // Typographic apostrophes are stored as ASCII ones.
    assert!(c.add_word("Ma’am’s").expect("added"));
    assert_eq!(c.check_word("Ma'am's", "en"), Some(true));

    for bad in ["", "two words", "#tag"] {
        assert!(
            matches!(c.add_word(bad), Err(SpellError::InvalidWord(..))),
            "{bad:?} accepted"
        );
    }
    // Without a folder words are kept in memory only.
    let mut mem = SpellChecker::new(None);
    assert!(mem.add_word("zorbly").expect("added"));
    assert_eq!(mem.check_word("zorbly", "en"), Some(true));
    assert_eq!(
        SpellChecker::new(None).check_word("zorbly", "en"),
        Some(false)
    );
    // Ignored words last as long as the checker.
    let mut ig = SpellChecker::new(Some(dir.clone()));
    assert!(ig.ignore_word("frobnicate"));
    assert_eq!(ig.check_word("frobnicate", "en"), Some(true));
    assert_eq!(
        SpellChecker::new(Some(dir.clone())).check_word("frobnicate", "en"),
        Some(false)
    );
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn a_user_dictionary_pair_loads() {
    let dir = scratch("pair");
    std::fs::write(
        dir.join("de_DE.aff"),
        "SET UTF-8\nTRY esianrtolcdugmphbyfvkwz\n\nSFX N Y 1\nSFX N 0 en .\n",
    )
    .expect("aff");
    std::fs::write(dir.join("de_DE.dic"), "3\nHaus/N\nStraße\ngrün\n").expect("dic");
    // ISO-8859-1 files load too.
    std::fs::write(dir.join("de_AT.aff"), b"SET ISO8859-1\n".as_slice()).expect("aff");
    std::fs::write(dir.join("de_AT.dic"), b"1\nJ\xE4nner\n".as_slice()).expect("dic");
    // A .dic without its .aff is reported, not loaded.
    std::fs::write(dir.join("it_IT.dic"), "1\nciao\n").expect("dic");

    let mut c = SpellChecker::new(Some(dir.clone()));
    assert_eq!(c.dictionaries(), ["de_AT", "de_DE", "en_US"]);
    assert!(c.problems().iter().any(|p| p.contains("it_IT.dic")));
    for good in ["Haus", "Hausen", "Straße", "grün"] {
        assert_eq!(c.check_word(good, "de-DE"), Some(true), "{good}");
    }
    assert_eq!(c.check_word("Hous", "de-DE"), Some(false));
    assert_eq!(c.resolve("de"), Some("de_AT".into()), "first de_* by name");
    assert_eq!(c.resolve("de-CH"), None, "no de_CH and no bare de");
    assert!(
        c.suggest("Hous", "de-DE")
            .expect("de_DE")
            .contains(&"Haus".to_string())
    );
    assert_eq!(c.check_word("Jänner", "de-AT"), Some(true));
    assert_eq!(c.check_word("ciao", "it"), None, "no .aff, no dictionary");
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn languages_resolve_and_others_are_skipped() {
    let c = SpellChecker::new(None);
    for tag in ["en", "en-US", "en_US", "EN-us", "en-Latn-US"] {
        assert_eq!(c.resolve(tag), Some("en_US".into()), "{tag}");
    }
    for tag in ["en-GB", "fr", "de-DE", "", "x", "zxx"] {
        assert_eq!(c.resolve(tag), None, "{tag}");
    }

    let (doc, s) = doc_with(&[
        ("Teh first paragraf.", None),
        ("Le chaton est trés mignon.", Some("fr-FR")),
        ("Colour and flavour.", Some("en-GB")),
        ("", Some("es")),
        ("Anothr one.", Some("en")),
    ]);
    let mut c = SpellChecker::new(None);
    let r = c.check_document(&doc, None);
    let got: Vec<(&str, usize, usize, &str)> = r
        .issues
        .iter()
        .map(|i| (i.word.as_str(), i.start, i.end, i.language.as_str()))
        .collect();
    let base = "Teh first paragraf.\nLe chaton est trés mignon.\nColour and flavour.\n\n"
        .chars()
        .count();
    assert_eq!(
        got,
        [
            ("Teh", 0, 3, "en-US"),
            ("paragraf", 10, 18, "en-US"),
            ("Anothr", base, base + 6, "en"),
        ]
    );
    assert_eq!(
        r.unchecked_languages,
        ["en-GB", "fr-FR"],
        "empty es is not reported"
    );
    assert!(r.issues.iter().all(|i| i.story == s));
    assert_eq!(c.check_document(&doc, Some(s)), r);
}

#[test]
fn segmentation_edge_cases() {
    // Apostrophes inside words stay; quotes around them do not.
    assert_eq!(
        texts("don't can’t O'Neil dogs' 'tis ‘quoted’"),
        ["don't", "can’t", "O'Neil", "dogs", "tis", "quoted"]
    );
    // Words with digits are skipped.
    assert!(texts("3rd A4 mp3 2026 abc123 4x4").is_empty());
    // Web and e-mail addresses, and joined words.
    assert_eq!(
        texts("see https://exampel.com/pth and www.pubrs.org or me@exmple.com (http://x.y) now"),
        ["see", "and", "or", "now"]
    );
    assert!(texts("e.g. exampel.com snake_case a:b").is_empty());
    // Break characters and other controls separate words, with offsets.
    let w = words(
        "end\u{000E}next\u{001D}more\u{000C}last\tTab",
        &SpellOptions::default(),
    );
    let got: Vec<(usize, usize, &str)> = w
        .iter()
        .map(|w| (w.start, w.end, w.text.as_str()))
        .collect();
    assert_eq!(
        got,
        [
            (0, 3, "end"),
            (4, 8, "next"),
            (9, 13, "more"),
            (14, 18, "last"),
            (19, 22, "Tab")
        ]
    );
    // Merge fields (U+FFFC) and anything glued to them.
    assert_eq!(
        texts("Dear \u{FFFC}, hello \u{FFFC}s there"),
        ["Dear", "hello", "there"]
    );
    // Hyphenated words are checked part by part; one-letter words are not.
    assert_eq!(texts("well-known a I x-ray"), ["well", "known", "ray"]);
    // Capitals only: skipped by default, an option.
    assert_eq!(texts("NASA rocks O'NEIL"), ["rocks"]);
    let all = words(
        "NASA rocks",
        &SpellOptions {
            ignore_all_caps: false,
        },
    );
    assert_eq!(all.len(), 2);
    // Character (not byte) offsets.
    let w = words("naïve café", &SpellOptions::default());
    assert_eq!((w[0].start, w[0].end, w[1].start, w[1].end), (0, 5, 6, 10));
    // Curly apostrophes are checked as ASCII ones and reported as written.
    let mut c = SpellChecker::new(None);
    let found = c.check_text("It’s fine but it’z not", "en").expect("en");
    assert_eq!(found.len(), 1);
    assert_eq!((found[0].word.as_str(), found[0].start), ("it’z", 14));
}

#[test]
fn only_changed_paragraphs_are_checked_again() {
    let (mut doc, s) = doc_with(&[
        ("The first paragrph.", None),
        ("A secnd one.", None),
        ("And a third.", None),
    ]);
    let mut c = SpellChecker::new(None);
    let first = c.check_story(&doc, s);
    assert_eq!(first.issues.len(), 2);
    assert_eq!(c.stats().checked, 3);
    c.check_story(&doc, s);
    assert_eq!((c.stats().checked, c.stats().hits), (3, 3));
    Arc::make_mut(doc.stories.get_mut(&s).expect("story")).paragraphs[1] =
        Paragraph::plain("A second one, fixd.");
    let r = c.check_story(&doc, s);
    assert_eq!((c.stats().checked, c.stats().hits), (4, 5));
    let words: Vec<&str> = r.issues.iter().map(|i| i.word.as_str()).collect();
    assert_eq!(words, ["paragrph", "fixd"]);
    // Adding a word starts the cache over and clears its misspellings.
    c.add_word("fixd").expect("added");
    let r = c.check_story(&doc, s);
    assert_eq!(r.issues.len(), 1);
    assert_eq!(c.stats().checked, 7);
}
