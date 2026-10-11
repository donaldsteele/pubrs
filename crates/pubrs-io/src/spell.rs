//! Spell checking with Hunspell dictionaries (plan T20, Owner decision 6).
//!
//! * The engine is `spellbook` (Hunspell-compatible, MPL-2.0). The en_US
//!   dictionary built from SCOWL (`assets/dict/`, credited in `ASSETS.md`) is
//!   compiled into the binary with `include_str!` (about 0.5 MB, so it works
//!   without any installed data) and parsed once per process, on first use,
//!   into a shared [`Arc`].
//! * Other languages: Hunspell `.dic`/`.aff` pairs in the dictionaries folder
//!   (`<config dir>/dictionaries/`), keyed by file stem (`de_DE.dic` with
//!   `de_DE.aff` is `de_DE`). They are parsed on first use. A user pair named
//!   `en_US` replaces the bundled one. Files are UTF-8, or ISO-8859-1 when
//!   they are not valid UTF-8.
//! * Words the user adds live in `user-words.txt` in the same folder, one per
//!   line, and are accepted in every language. Without a folder they last as
//!   long as the checker. Ignored words always last only as long as the
//!   checker.
//! * Each paragraph is checked in its own `language` (BCP 47, see
//!   [`SpellChecker::resolve`]). A language without a dictionary is skipped,
//!   never flagged, and reported as unchecked.
//! * Results are cached per paragraph by (text, dictionary), so after an edit
//!   only the changed paragraph is checked again.
//! * Dictionaries hash with a fixed seed, so suggestions come out in the same
//!   order in every run.

use std::borrow::Cow;
use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::path::{Path, PathBuf};
use std::sync::{Arc, OnceLock};

use pubrs_core::{Document, Owner, StoryId};
use unicode_segmentation::UnicodeSegmentation;

/// A parsed Hunspell dictionary.
pub type Dictionary = spellbook::Dictionary<foldhash::fast::FixedState>;

/// Name of the bundled dictionary.
pub const BUNDLED_DICTIONARY: &str = "en_US";
/// The added-words file inside the dictionaries folder.
pub const USER_WORDS_FILE: &str = "user-words.txt";
/// Copyright and licence of the bundled dictionary (SCOWL and Geoff
/// Kuenning's affix file), to show with the program's notices.
pub const BUNDLED_NOTICE: &str = include_str!("../../../assets/dict/README_en_US.txt");

const BUNDLED_AFF: &str = include_str!("../../../assets/dict/en_US.aff");
const BUNDLED_DIC: &str = include_str!("../../../assets/dict/en_US.dic");

/// The merge-field placeholder character (M24): never part of a checked word.
pub const MERGE_FIELD: char = '\u{FFFC}';

/// Longest word [`SpellChecker::add_word`] accepts, in characters.
pub const MAX_WORD_CHARS: usize = 64;

/// Paragraph results kept before the cache starts over.
const CACHE_LIMIT: usize = 100_000;

/// What to leave unchecked.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SpellOptions {
    /// Skip words written in capitals only ("NASA", "OK"). Default true.
    pub ignore_all_caps: bool,
}

impl Default for SpellOptions {
    fn default() -> Self {
        Self {
            ignore_all_caps: true,
        }
    }
}

/// A word to check: character offsets in the text (end exclusive) and the
/// word as written.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Word {
    pub start: usize,
    pub end: usize,
    pub text: String,
}

/// A misspelt word in a paragraph: character offsets (end exclusive) and the
/// word as written.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Misspelling {
    pub start: usize,
    pub end: usize,
    pub word: String,
}

/// A misspelt word in a story.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Issue {
    pub story: StoryId,
    /// Story character offsets, end exclusive.
    pub start: usize,
    pub end: usize,
    pub word: String,
    /// The paragraph's language tag as the document gives it ("en-US").
    pub language: String,
}

/// The misspellings of one story, in text order.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct StoryReport {
    pub issues: Vec<Issue>,
    /// Language tags of paragraphs with text but no dictionary.
    pub unchecked: BTreeSet<String>,
}

/// The misspellings of a document (or one story), in reading order.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Report {
    pub issues: Vec<Issue>,
    /// Language tags of paragraphs with text but no dictionary, sorted.
    pub unchecked_languages: Vec<String>,
}

/// Paragraph cache counters: `checked` paragraphs were spell checked,
/// `hits` were answered from the cache.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct CacheStats {
    pub hits: u64,
    pub checked: u64,
}

#[derive(Debug, thiserror::Error)]
pub enum SpellError {
    #[error("invalid word {0:?}: {1}")]
    InvalidWord(String, &'static str),
    #[error("cannot write the user dictionary {path}: {source}")]
    Write {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
}

/// The bundled en_US dictionary, parsed on first use (once per process).
pub fn bundled() -> Option<Arc<Dictionary>> {
    static BUNDLED: OnceLock<Option<Arc<Dictionary>>> = OnceLock::new();
    BUNDLED
        .get_or_init(|| match parse(BUNDLED_AFF, BUNDLED_DIC) {
            Ok(d) => Some(Arc::new(d)),
            Err(e) => {
                tracing::error!("the bundled en_US dictionary does not parse: {e}");
                None
            }
        })
        .clone()
}

fn parse(aff: &str, dic: &str) -> Result<Dictionary, String> {
    Dictionary::new_with_hasher(aff, dic, foldhash::fast::FixedState::default())
        .map_err(|e| e.to_string())
}

/// A dictionary file as text: UTF-8 (a BOM is dropped), or ISO-8859-1 when it
/// is not valid UTF-8.
fn read_text(path: &Path) -> Result<String, String> {
    let bytes = std::fs::read(path).map_err(|e| format!("{}: {e}", path.display()))?;
    let bytes = bytes.strip_prefix(b"\xEF\xBB\xBF").unwrap_or(&bytes);
    Ok(match std::str::from_utf8(bytes) {
        Ok(s) => s.to_string(),
        Err(_) => bytes.iter().map(|&b| char::from(b)).collect(),
    })
}

// ------------------------------------------------------------ segmentation

/// Characters that end a chunk of text: white space and control characters,
/// which include the column, frame and page break characters (U+000E,
/// U+001D, U+000C).
fn is_separator(c: char) -> bool {
    c.is_whitespace() || c.is_control()
}

fn is_apostrophe(c: char) -> bool {
    matches!(c, '\'' | '\u{2019}' | '\u{2018}' | '\u{02BC}' | '\u{FF07}')
}

/// Joiners that Unicode word boundaries keep inside a word but that make it
/// something other than a dictionary word: "e.g", "example.com", "snake_case",
/// "a:b".
fn is_joiner(c: char) -> bool {
    (c.is_ascii_punctuation() && c != '\'')
        || matches!(
            c,
            '\u{00B7}'
                | '\u{0387}'
                | '\u{2024}'
                | '\u{2027}'
                | '\u{202F}'
                | '\u{203F}'
                | '\u{2040}'
                | '\u{2054}'
                | '\u{FE13}'
                | '\u{FE52}'
                | '\u{FE55}'
                | '\u{FF0E}'
                | '\u{FF1A}'
                | '\u{FF3F}'
        )
}

/// Maximal runs of non-separator characters with their byte offsets.
fn chunks(text: &str) -> impl Iterator<Item = (usize, &str)> {
    let mut rest = 0;
    std::iter::from_fn(move || {
        let start = rest + text[rest..].find(|c: char| !is_separator(c))?;
        let len = text[start..]
            .find(is_separator)
            .unwrap_or(text.len() - start);
        rest = start + len;
        Some((start, &text[start..start + len]))
    })
}

/// A web address, an e-mail address or a merge field: checked as a whole,
/// never word by word.
fn is_link_or_field(chunk: &str) -> bool {
    if chunk.contains(MERGE_FIELD) || chunk.contains("://") || chunk.contains('@') {
        return true;
    }
    let t = chunk.trim_start_matches(|c: char| !c.is_alphanumeric());
    let prefix = |p: &str| t.get(..p.len()).is_some_and(|s| s.eq_ignore_ascii_case(p));
    prefix("www.") || prefix("mailto:")
}

/// The words of `text` that are spell checked, in order, with character
/// offsets. Words follow Unicode word boundaries (UAX #29): an apostrophe
/// between letters stays in the word ("don't", "O'Neil", "can’t") and
/// hyphenated words are checked part by part. Skipped:
/// * one-letter words and words with a digit ("3rd", "A4", "mp3");
/// * words joined by other punctuation ("e.g", "example.com", "snake_case");
/// * web and e-mail addresses (a chunk with "://", "@", or starting "www."),
///   and any chunk holding a merge field (U+FFFC);
/// * words in capitals only, when `ignore_all_caps` is set.
///
/// White space and control characters (including the break characters)
/// separate words.
pub fn words(text: &str, opts: &SpellOptions) -> Vec<Word> {
    let mut out = Vec::new();
    // Character offset of byte `at`, advanced as words are found in order.
    let (mut at, mut chars) = (0usize, 0usize);
    for (cb, chunk) in chunks(text) {
        if is_link_or_field(chunk) {
            continue;
        }
        for (wb, w) in chunk.unicode_word_indices() {
            let b = cb + wb;
            chars += text[at..b].chars().count();
            at = b;
            let lead = w.chars().take_while(|c| is_apostrophe(*c)).count();
            let core = w.trim_matches(is_apostrophe);
            let n = core.chars().count();
            if n < 2
                || core.chars().any(|c| c.is_numeric() || is_joiner(c))
                || (opts.ignore_all_caps
                    && core.chars().any(char::is_alphabetic)
                    && !core.chars().any(char::is_lowercase))
            {
                continue;
            }
            out.push(Word {
                start: chars + lead,
                end: chars + lead + n,
                text: core.to_string(),
            });
        }
    }
    out
}

/// The word as looked up: typographic apostrophes become ASCII ones.
fn lookup_form(word: &str) -> Cow<'_, str> {
    if word.chars().any(|c| is_apostrophe(c) && c != '\'') {
        Cow::Owned(
            word.chars()
                .map(|c| if is_apostrophe(c) { '\'' } else { c })
                .collect(),
        )
    } else {
        Cow::Borrowed(word)
    }
}

/// Spellings of `w` that a listed word may have: as written, with a
/// lower-case first letter, and in lower case.
fn case_forms(w: &str) -> impl Iterator<Item = Cow<'_, str>> {
    let mut first = w.chars();
    let decap = first.next().filter(|c| c.is_uppercase()).map(|c| {
        let mut s: String = c.to_lowercase().collect();
        s.push_str(first.as_str());
        Cow::Owned(s)
    });
    let lower = w
        .chars()
        .any(char::is_uppercase)
        .then(|| Cow::Owned(w.to_lowercase()));
    std::iter::once(Cow::Borrowed(w)).chain(decap).chain(lower)
}

/// The language and region of a BCP 47 tag ("en-US", "en_us", "sr-Latn-RS"),
/// lower- and upper-cased.
fn parse_tag(tag: &str) -> Option<(String, Option<String>)> {
    let mut parts = tag.trim().split(['-', '_']);
    let lang = parts.next()?.to_ascii_lowercase();
    if !(2..=3).contains(&lang.len()) || !lang.bytes().all(|b| b.is_ascii_alphabetic()) {
        return None;
    }
    let region = parts
        .find(|p| {
            (p.len() == 2 && p.bytes().all(|b| b.is_ascii_alphabetic()))
                || (p.len() == 3 && p.bytes().all(|b| b.is_ascii_digit()))
        })
        .map(str::to_ascii_uppercase);
    Some((lang, region))
}

/// Story ids in reading order: by the page of the story's first frame, then
/// top to bottom and left to right; stories on no page last.
pub fn reading_order(doc: &Document) -> Vec<StoryId> {
    let mut stories: Vec<StoryId> = doc.stories.keys().copied().collect();
    let order = |s: &StoryId| -> (usize, i64, i64, StoryId) {
        let first = doc.story(*s).and_then(|st| st.frames.first().copied());
        let page = first
            .and_then(|f| match doc.owner_of(f) {
                Some(Owner::Page(p)) => doc.pages.iter().position(|q| q.id == p),
                _ => None,
            })
            .unwrap_or(usize::MAX);
        let (x, y) = first
            .and_then(|f| doc.frame(f))
            .map_or((0, 0), |f| (f.rect.x as i64, f.rect.y as i64));
        (page, y, x, *s)
    };
    stories.sort_by_cached_key(order);
    stories
}

// ------------------------------------------------------------ checker

/// Dictionaries, added and ignored words, and the paragraph cache.
pub struct SpellChecker {
    dir: Option<PathBuf>,
    /// User `.aff`/`.dic` pairs by stem.
    user_pairs: BTreeMap<String, (PathBuf, PathBuf)>,
    /// Parsed dictionaries by name; `None` when parsing failed.
    loaded: HashMap<String, Option<Arc<Dictionary>>>,
    user_words: BTreeSet<String>,
    ignored: BTreeSet<String>,
    options: SpellOptions,
    /// Dictionary name → paragraph text → misspellings.
    cache: HashMap<String, HashMap<String, Arc<[Misspelling]>>>,
    cached: usize,
    stats: CacheStats,
    problems: Vec<String>,
}

impl std::fmt::Debug for SpellChecker {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SpellChecker")
            .field("dir", &self.dir)
            .field("dictionaries", &self.dictionaries())
            .field("user_words", &self.user_words.len())
            .finish()
    }
}

impl SpellChecker {
    /// A checker with the bundled dictionary and, when `dir` is given, the
    /// user's dictionaries and added words from that folder (it need not
    /// exist yet). Nothing is parsed until a word is checked.
    pub fn new(dir: Option<PathBuf>) -> Self {
        let mut c = Self {
            dir,
            user_pairs: BTreeMap::new(),
            loaded: HashMap::new(),
            user_words: BTreeSet::new(),
            ignored: BTreeSet::new(),
            options: SpellOptions::default(),
            cache: HashMap::new(),
            cached: 0,
            stats: CacheStats::default(),
            problems: Vec::new(),
        };
        c.scan();
        c
    }

    fn scan(&mut self) {
        let Some(dir) = self.dir.clone() else { return };
        if let Ok(entries) = std::fs::read_dir(&dir) {
            for e in entries.flatten() {
                let p = e.path();
                if p.extension().is_some_and(|x| x.eq_ignore_ascii_case("dic"))
                    && let Some(stem) = p.file_stem().and_then(|s| s.to_str())
                {
                    let aff = p.with_extension("aff");
                    if aff.is_file() {
                        self.user_pairs.insert(stem.to_string(), (aff, p.clone()));
                    } else {
                        self.problems
                            .push(format!("{} has no matching .aff file", p.display()));
                    }
                }
            }
        }
        self.user_words = self.read_user_words();
    }

    fn read_user_words(&self) -> BTreeSet<String> {
        let Some(path) = self.user_words_path() else {
            return BTreeSet::new();
        };
        match std::fs::read_to_string(&path) {
            Ok(s) => s
                .lines()
                .map(str::trim)
                .filter(|l| !l.is_empty() && !l.starts_with('#'))
                .map(|l| lookup_form(l).into_owned())
                .collect(),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => BTreeSet::new(),
            Err(e) => {
                tracing::warn!("cannot read {}: {e}", path.display());
                BTreeSet::new()
            }
        }
    }

    /// The dictionaries folder, if any.
    pub fn dir(&self) -> Option<&Path> {
        self.dir.as_deref()
    }

    /// Where added words are kept, if they are kept on disk.
    pub fn user_words_path(&self) -> Option<PathBuf> {
        self.dir.as_ref().map(|d| d.join(USER_WORDS_FILE))
    }

    /// Problems met reading the dictionaries folder or parsing a dictionary.
    pub fn problems(&self) -> &[String] {
        &self.problems
    }

    pub fn options(&self) -> SpellOptions {
        self.options
    }

    pub fn set_options(&mut self, options: SpellOptions) {
        if options != self.options {
            self.options = options;
            self.clear_cache();
        }
    }

    /// Paragraph cache counters since the checker was made.
    pub fn stats(&self) -> CacheStats {
        self.stats
    }

    fn clear_cache(&mut self) {
        self.cache.clear();
        self.cached = 0;
    }

    /// Names of the dictionaries that can be used, sorted.
    pub fn dictionaries(&self) -> Vec<String> {
        let mut v: BTreeSet<String> = self.user_pairs.keys().cloned().collect();
        v.insert(BUNDLED_DICTIONARY.to_string());
        v.into_iter().collect()
    }

    /// The dictionary for a BCP 47 language tag: the `ll_RR` one for a tag
    /// with a region; else a bare `ll` dictionary; for a tag without a region,
    /// the language's usual one (`en` is en_US) or else the first `ll_*` one.
    /// A region without its own dictionary is not checked with another
    /// region's ("en-GB" is not checked with en_US). Names match without
    /// regard to case or `-`/`_`.
    pub fn resolve(&self, tag: &str) -> Option<String> {
        let (lang, region) = parse_tag(tag)?;
        let names = self.dictionaries();
        let norm = |s: &str| s.to_ascii_lowercase().replace('-', "_");
        let find = |want: &str| names.iter().find(|n| norm(n) == want).cloned();
        if let Some(r) = &region {
            return find(&norm(&format!("{lang}_{r}"))).or_else(|| find(&lang));
        }
        find(&lang)
            .or_else(|| (lang == "en").then(|| find("en_us")).flatten())
            .or_else(|| {
                let prefix = format!("{lang}_");
                names.iter().find(|n| norm(n).starts_with(&prefix)).cloned()
            })
    }

    /// A dictionary by name, parsed on first use.
    pub fn dictionary(&mut self, name: &str) -> Option<Arc<Dictionary>> {
        if let Some(d) = self.loaded.get(name) {
            return d.clone();
        }
        let d = match self.user_pairs.get(name) {
            Some((aff, dic)) => {
                let parsed = read_text(aff)
                    .and_then(|a| read_text(dic).map(|d| (a, d)))
                    .and_then(|(a, d)| parse(&a, &d));
                match parsed {
                    Ok(d) => Some(Arc::new(d)),
                    Err(e) => {
                        let msg = format!("dictionary {name} does not load: {e}");
                        tracing::warn!("{msg}");
                        self.problems.push(msg);
                        None
                    }
                }
            }
            None if name == BUNDLED_DICTIONARY => bundled(),
            None => None,
        };
        self.loaded.insert(name.to_string(), d.clone());
        d
    }

    /// The dictionary name and dictionary for a language tag.
    pub fn dictionary_for(&mut self, tag: &str) -> Option<(String, Arc<Dictionary>)> {
        let name = self.resolve(tag)?;
        let d = self.dictionary(&name)?;
        Some((name, d))
    }

    /// The word was added or ignored (in any of its case forms).
    pub fn is_listed(&self, word: &str) -> bool {
        if self.user_words.is_empty() && self.ignored.is_empty() {
            return false;
        }
        let w = lookup_form(word);
        case_forms(&w)
            .any(|f| self.user_words.contains(f.as_ref()) || self.ignored.contains(f.as_ref()))
    }

    fn accepted(&self, dict: &Dictionary, word: &str) -> bool {
        self.is_listed(word) || dict.check(&lookup_form(word))
    }

    /// Whether `word` is spelt correctly in `tag`; `None` when the language
    /// has no dictionary.
    pub fn check_word(&mut self, word: &str, tag: &str) -> Option<bool> {
        let (_, d) = self.dictionary_for(tag)?;
        Some(self.accepted(&d, word))
    }

    /// Suggested corrections, best first; `None` when the language has no
    /// dictionary.
    pub fn suggest(&mut self, word: &str, tag: &str) -> Option<Vec<String>> {
        let (_, d) = self.dictionary_for(tag)?;
        let mut out = Vec::new();
        d.suggest(&lookup_form(word), &mut out);
        Some(out)
    }

    /// The added words, sorted.
    pub fn user_words(&self) -> impl Iterator<Item = &str> {
        self.user_words.iter().map(String::as_str)
    }

    /// Add a word to the user dictionary (saved to `user-words.txt` when the
    /// checker has a folder). Returns false when it was already there.
    pub fn add_word(&mut self, word: &str) -> Result<bool, SpellError> {
        let w = valid_word(word)?;
        let added = match self.user_words_path() {
            Some(path) => {
                // Another program (or window) may have added words since.
                let mut all = self.read_user_words();
                all.extend(self.user_words.iter().cloned());
                let added = all.insert(w);
                if added {
                    write_words(&path, &all)
                        .map_err(|source| SpellError::Write { path, source })?;
                }
                self.user_words = all;
                added
            }
            None => self.user_words.insert(w),
        };
        self.clear_cache();
        Ok(added)
    }

    /// Accept a word until the checker is dropped. Returns false when it was
    /// already accepted that way.
    pub fn ignore_word(&mut self, word: &str) -> bool {
        let w = lookup_form(word.trim()).into_owned();
        if w.is_empty() {
            return false;
        }
        let added = self.ignored.insert(w);
        if added {
            self.clear_cache();
        }
        added
    }

    /// Misspellings in `text` checked in `tag`, without the cache; `None`
    /// when the language has no dictionary.
    pub fn check_text(&mut self, text: &str, tag: &str) -> Option<Vec<Misspelling>> {
        let (_, d) = self.dictionary_for(tag)?;
        Some(self.misspellings(&d, text))
    }

    fn misspellings(&self, d: &Dictionary, text: &str) -> Vec<Misspelling> {
        words(text, &self.options)
            .into_iter()
            .filter(|w| !self.accepted(d, &w.text))
            .map(|w| Misspelling {
                start: w.start,
                end: w.end,
                word: w.text,
            })
            .collect()
    }

    /// Misspellings in one paragraph's text, from the cache when this text
    /// was checked in this dictionary before; `None` when the language has no
    /// dictionary.
    pub fn check_paragraph(&mut self, text: &str, tag: &str) -> Option<Arc<[Misspelling]>> {
        let (name, d) = self.dictionary_for(tag)?;
        if let Some(hit) = self.cache.get(&name).and_then(|m| m.get(text)) {
            self.stats.hits += 1;
            return Some(hit.clone());
        }
        let found: Arc<[Misspelling]> = self.misspellings(&d, text).into();
        self.stats.checked += 1;
        if self.cached >= CACHE_LIMIT {
            self.clear_cache();
        }
        self.cached += 1;
        self.cache
            .entry(name)
            .or_default()
            .insert(text.to_string(), found.clone());
        Some(found)
    }

    /// Misspellings in one story, each paragraph in its own language.
    pub fn check_story(&mut self, doc: &Document, id: StoryId) -> StoryReport {
        let mut rep = StoryReport::default();
        let Some(story) = doc.story(id) else {
            return rep;
        };
        let mut pos = 0;
        for p in &story.paragraphs {
            let text = p.text();
            let n = text.chars().count();
            let lang = doc.styles.resolve_para(p).language;
            match self.check_paragraph(&text, &lang) {
                Some(found) => rep.issues.extend(found.iter().map(|m| Issue {
                    story: id,
                    start: pos + m.start,
                    end: pos + m.end,
                    word: m.word.clone(),
                    language: lang.clone(),
                })),
                None => {
                    if text.chars().any(char::is_alphabetic) {
                        rep.unchecked.insert(lang);
                    }
                }
            }
            pos += n + 1;
        }
        rep
    }

    /// Misspellings in every story in reading order, or in one story.
    pub fn check_document(&mut self, doc: &Document, only: Option<StoryId>) -> Report {
        let stories = match only {
            Some(s) => vec![s],
            None => reading_order(doc),
        };
        let mut issues = Vec::new();
        let mut unchecked = BTreeSet::new();
        for s in stories {
            let r = self.check_story(doc, s);
            issues.extend(r.issues);
            unchecked.extend(r.unchecked);
        }
        Report {
            issues,
            unchecked_languages: unchecked.into_iter().collect(),
        }
    }
}

/// A word the user may add: one word, no spaces, at most [`MAX_WORD_CHARS`].
fn valid_word(word: &str) -> Result<String, SpellError> {
    let w = word.trim();
    let bad = |why| Err(SpellError::InvalidWord(word.to_string(), why));
    if w.is_empty() {
        return bad("it is empty");
    }
    if w.chars().any(|c| c.is_whitespace() || c.is_control()) {
        return bad("it contains a space or a control character");
    }
    if w.chars().count() > MAX_WORD_CHARS {
        return bad("it is longer than 64 characters");
    }
    if w.starts_with('#') {
        return bad("it starts with #");
    }
    Ok(lookup_form(w).into_owned())
}

/// Write the word list atomically (a temporary file renamed over the old).
fn write_words(path: &Path, words: &BTreeSet<String>) -> std::io::Result<()> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let mut text = String::from("# pubrs user dictionary: one word per line\n");
    for w in words {
        text.push_str(w);
        text.push('\n');
    }
    let tmp = path.with_extension("txt.tmp");
    std::fs::write(&tmp, text)?;
    std::fs::rename(&tmp, path)
}
