//! Spell checking (M28): find misspelt words in a document or one story,
//! suggest corrections, and add words to the user dictionary.
//!
//! Each paragraph is checked in its own `language`. The en_US dictionary is
//! built in; other languages need a Hunspell `.dic`/`.aff` pair in the
//! session's dictionaries folder ([`crate::SessionConfig::dictionaries_dir`]).
//! A language without a dictionary is reported as unchecked, never flagged.

use pubrs_core::{DocumentId, FrameId, StoryId};
use pubrs_io::spell::{SpellChecker, SpellError, SpellOptions};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use super::yes;
use crate::error::{ApiError, ApiResult};
use crate::session::Session;

#[derive(Debug, Clone, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SpellCheck {
    /// Document id (default: the most recently used document).
    #[serde(default)]
    pub doc_id: Option<DocumentId>,
    /// Check only this text frame's story. Omit both frame_id and story_id to
    /// check every story in the document.
    #[serde(default)]
    pub frame_id: Option<FrameId>,
    #[serde(default)]
    pub story_id: Option<StoryId>,
    /// Skip words written in capitals only, such as acronyms (default true).
    #[serde(default = "yes")]
    pub ignore_all_caps: bool,
}

/// A misspelt word.
#[derive(Debug, Clone, PartialEq, Serialize, JsonSchema)]
pub struct SpellIssue {
    pub story_id: StoryId,
    /// The frame showing the word (omitted when it is not shown).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub frame_id: Option<FrameId>,
    /// Character offsets in the story, end exclusive (as in text_find).
    pub start: usize,
    pub end: usize,
    /// The word as written.
    pub word: String,
    /// The paragraph's language tag; pass it to spell_suggest.
    pub language: String,
}

#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct SpellCheckOut {
    pub count: usize,
    /// Misspelt words in reading order.
    pub issues: Vec<SpellIssue>,
    /// Language tags of paragraphs that were not checked because no
    /// dictionary covers them.
    pub unchecked_languages: Vec<String>,
    /// Dictionaries available: "en_US" and any user dictionaries.
    pub dictionaries: Vec<String>,
}

#[derive(Debug, Clone, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SpellSuggest {
    /// The word to correct.
    pub word: String,
    /// BCP 47 language tag, such as a spell_check issue's language (default
    /// "en-US").
    #[serde(default)]
    pub language: Option<String>,
}

#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct SpellSuggestOut {
    pub word: String,
    /// The dictionary used, e.g. "en_US".
    pub dictionary: String,
    /// The word is spelt correctly (or is in the user dictionary).
    pub correct: bool,
    /// Corrections, best first (may be empty).
    pub suggestions: Vec<String>,
}

#[derive(Debug, Clone, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SpellAddWord {
    /// One word without spaces (at most 64 characters). From now on it is
    /// accepted in every language.
    pub word: String,
}

#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct SpellAddWordOut {
    pub word: String,
    /// False when the word was already in the user dictionary.
    pub added: bool,
    /// The user dictionary file. Omitted when the session keeps added words
    /// in memory only (no dictionaries folder).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
}

fn spell_error(e: SpellError) -> ApiError {
    match e {
        SpellError::InvalidWord(..) => {
            ApiError::invalid(e.to_string(), "pass one word without spaces")
        }
        SpellError::Write { .. } => ApiError::new(
            "io_error",
            e.to_string(),
            "check that the dictionaries folder is writable",
        ),
    }
}

impl Session {
    /// The session's spell checker (made on first use).
    pub fn speller(&mut self) -> &mut SpellChecker {
        let dir = self.config.dictionaries_dir.clone();
        self.speller.get_or_insert_with(|| SpellChecker::new(dir))
    }

    /// Run `f` with the spell checker and the session (for the document).
    fn with_speller<T>(&mut self, f: impl FnOnce(&mut Self, &mut SpellChecker) -> T) -> T {
        let mut sp = match self.speller.take() {
            Some(s) => s,
            None => SpellChecker::new(self.config.dictionaries_dir.clone()),
        };
        let out = f(self, &mut sp);
        self.speller = Some(sp);
        out
    }

    pub fn spell_check(&mut self, req: SpellCheck) -> ApiResult<SpellCheckOut> {
        let only = if req.frame_id.is_some() || req.story_id.is_some() {
            Some(self.resolve_story(req.doc_id, req.frame_id, req.story_id)?)
        } else {
            None
        };
        let layout = self.layout(req.doc_id)?;
        let (report, dictionaries) = self.with_speller(|s, sp| -> ApiResult<_> {
            sp.set_options(SpellOptions {
                ignore_all_caps: req.ignore_all_caps,
            });
            let doc = s.get(req.doc_id)?.doc();
            Ok((sp.check_document(doc, only), sp.dictionaries()))
        })?;
        let doc = self.get(req.doc_id)?.doc();
        let issues: Vec<SpellIssue> = report
            .issues
            .into_iter()
            .map(|i| {
                let frame_id = doc.story(i.story).and_then(|st| {
                    let sl = layout.stories.get(&i.story)?;
                    st.frames.iter().copied().find(|f| {
                        sl.frames
                            .get(f)
                            .is_some_and(|ft| ft.chars.contains(&i.start))
                    })
                });
                SpellIssue {
                    story_id: i.story,
                    frame_id,
                    start: i.start,
                    end: i.end,
                    word: i.word,
                    language: i.language,
                }
            })
            .collect();
        Ok(SpellCheckOut {
            count: issues.len(),
            issues,
            unchecked_languages: report.unchecked_languages,
            dictionaries,
        })
    }

    pub fn spell_suggest(&mut self, req: SpellSuggest) -> ApiResult<SpellSuggestOut> {
        let word = req.word.trim().to_string();
        if word.is_empty() {
            return Err(ApiError::invalid(
                "word is empty",
                "give the word to correct",
            ));
        }
        let tag = req.language.unwrap_or_else(|| "en-US".to_string());
        let sp = self.speller();
        let Some(dictionary) = sp.resolve(&tag) else {
            return Err(ApiError::not_found(
                "dictionary for",
                &format!("{tag:?}"),
                &format!(
                    "available: {}; add a Hunspell .dic/.aff pair to the dictionaries folder for other languages",
                    sp.dictionaries().join(", ")
                ),
            ));
        };
        let correct = sp.check_word(&word, &tag).unwrap_or(false);
        let suggestions = sp.suggest(&word, &tag).ok_or_else(|| {
            ApiError::new(
                "invalid_argument",
                format!("dictionary {dictionary} could not be loaded"),
                sp.problems().last().cloned().unwrap_or_default(),
            )
        })?;
        Ok(SpellSuggestOut {
            word,
            dictionary,
            correct,
            suggestions,
        })
    }

    pub fn spell_add_word(&mut self, req: SpellAddWord) -> ApiResult<SpellAddWordOut> {
        let sp = self.speller();
        let added = sp.add_word(&req.word).map_err(spell_error)?;
        Ok(SpellAddWordOut {
            word: req.word.trim().to_string(),
            added,
            path: sp.user_words_path().map(|p| p.display().to_string()),
        })
    }
}
