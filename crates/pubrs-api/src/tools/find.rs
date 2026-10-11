//! Find and replace across stories and WordArt frames.
//!
//! Every story in the document is searched, which covers text frames, and will
//! cover table cells (M19) automatically because cells are stories. The text
//! of WordArt frames is searched too: those matches have `story_id: null`
//! and the WordArt's `frame_id`, and replacing edits them with
//! `SetWordArt` in the same batch (ADR 0020).

use pubrs_core::text::FindOptions;
use pubrs_core::text::find_matches;
use pubrs_core::{Command, DocumentId, FrameId, Owner, StoryId};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::error::{ApiError, ApiResult};
use crate::session::{Changed, Session};

#[derive(Debug, Clone, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct TextFind {
    /// Document id (default: the most recently used document).
    #[serde(default)]
    pub doc_id: Option<DocumentId>,
    /// Limit the search to this text frame's story. Omit both frame_id and
    /// story_id to search every story in the document.
    #[serde(default)]
    pub frame_id: Option<FrameId>,
    #[serde(default)]
    pub story_id: Option<StoryId>,
    /// Text to find. A "\n" matches a paragraph break.
    pub query: String,
    /// Match letter case exactly (default false).
    #[serde(default)]
    pub case_sensitive: bool,
    /// Match whole words only (default false).
    #[serde(default)]
    pub whole_word: bool,
    /// Only match at or after this character offset. Needs frame_id or story_id.
    #[serde(default)]
    pub start: Option<usize>,
    /// Only match before this character offset. Needs frame_id or story_id.
    #[serde(default)]
    pub end: Option<usize>,
}

#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct FindMatch {
    /// The story (null for a WordArt frame, whose text is not a story).
    pub story_id: Option<StoryId>,
    /// The frame showing the start of the match (null if it is not shown);
    /// the WordArt frame for WordArt matches.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub frame_id: Option<FrameId>,
    /// Character offsets, end exclusive.
    pub start: usize,
    pub end: usize,
    /// The match with a little text either side.
    pub context: String,
}

#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct TextFindOut {
    pub count: usize,
    pub matches: Vec<FindMatch>,
}

#[derive(Debug, Clone, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct TextReplaceAll {
    #[serde(default)]
    pub doc_id: Option<DocumentId>,
    #[serde(default)]
    pub frame_id: Option<FrameId>,
    #[serde(default)]
    pub story_id: Option<StoryId>,
    pub query: String,
    /// Replacement text (empty deletes the matches).
    pub replacement: String,
    #[serde(default)]
    pub case_sensitive: bool,
    #[serde(default)]
    pub whole_word: bool,
    /// Only replace at or after this character offset. Needs frame_id or story_id.
    #[serde(default)]
    pub start: Option<usize>,
    /// Only replace before this character offset. Needs frame_id or story_id.
    #[serde(default)]
    pub end: Option<usize>,
}

#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct TextReplaceAllOut {
    /// Number of replacements made (0 = nothing changed).
    pub count: usize,
    #[serde(flatten)]
    pub change: Changed,
}

struct Hit {
    /// `None` for a WordArt frame.
    story: Option<StoryId>,
    /// The WordArt frame (only when `story` is `None`).
    wordart: Option<FrameId>,
    start: usize,
    end: usize,
}

/// Where text is searched.
#[derive(Clone, Copy)]
enum Src {
    Story(StoryId),
    WordArt(FrameId),
}

impl Session {
    /// All matches in reading order (page, then position of the story's first frame).
    #[allow(clippy::too_many_arguments)]
    fn scan(
        &self,
        doc_id: Option<DocumentId>,
        frame_id: Option<FrameId>,
        story_id: Option<StoryId>,
        query: &str,
        opts: FindOptions,
        start: Option<usize>,
        end: Option<usize>,
    ) -> ApiResult<Vec<Hit>> {
        let single = frame_id.is_some() || story_id.is_some();
        if !single && (start.is_some() || end.is_some()) {
            return Err(ApiError::invalid(
                "start/end need frame_id or story_id",
                "a character range only makes sense within one story",
            ));
        }
        if query.is_empty() {
            return Err(ApiError::invalid("query is empty", "give the text to find"));
        }
        let doc = self.get(doc_id)?.doc();
        let wordart_only = story_id.is_none()
            && frame_id
                .and_then(|f| doc.frame(f))
                .is_some_and(|f| f.as_wordart().is_some());
        let mut sources: Vec<Src> = if wordart_only {
            frame_id.map(Src::WordArt).into_iter().collect()
        } else if single {
            vec![Src::Story(self.resolve_story(doc_id, frame_id, story_id)?)]
        } else {
            doc.stories
                .keys()
                .map(|s| Src::Story(*s))
                .chain(
                    doc.frames
                        .values()
                        .filter(|f| f.as_wordart().is_some())
                        .map(|f| Src::WordArt(f.id)),
                )
                .collect()
        };
        let order = |src: &Src| -> (usize, i64, i64, String) {
            let (first, tie) = match src {
                Src::Story(s) => (
                    doc.story(*s).and_then(|st| st.frames.first().copied()),
                    s.to_string(),
                ),
                Src::WordArt(f) => (Some(*f), f.to_string()),
            };
            let page = first
                .and_then(|f| match doc.owner_of(f) {
                    Some(Owner::Page(p)) => doc.pages.iter().position(|q| q.id == p),
                    _ => None,
                })
                .unwrap_or(usize::MAX);
            let (x, y) = first
                .and_then(|f| doc.frame(f))
                .map_or((0, 0), |f| (f.rect.x as i64, f.rect.y as i64));
            (page, y, x, tie)
        };
        sources.sort_by_cached_key(order);
        let mut out = Vec::new();
        for src in sources {
            let (text, owner) = match src {
                Src::Story(s) => {
                    let Some(story) = doc.story(s) else { continue };
                    (story.plain_text(), (Some(s), None))
                }
                Src::WordArt(f) => {
                    let Some(w) = doc.frame(f).and_then(|f| f.as_wordart()) else {
                        continue;
                    };
                    (w.text.clone(), (None, Some(f)))
                }
            };
            let limit = if single && (start.is_some() || end.is_some()) {
                Some((start.unwrap_or(0), end.unwrap_or(text.chars().count())))
            } else {
                None
            };
            for (a, b) in find_matches(&text, query, opts, limit) {
                out.push(Hit {
                    story: owner.0,
                    wordart: owner.1,
                    start: a,
                    end: b,
                });
            }
        }
        Ok(out)
    }

    pub fn text_find(&mut self, req: TextFind) -> ApiResult<TextFindOut> {
        let opts = FindOptions {
            case_sensitive: req.case_sensitive,
            whole_word: req.whole_word,
        };
        let hits = self.scan(
            req.doc_id,
            req.frame_id,
            req.story_id,
            &req.query,
            opts,
            req.start,
            req.end,
        )?;
        let layout = self.layout(req.doc_id)?;
        let doc = self.get(req.doc_id)?.doc();
        let mut texts: std::collections::BTreeMap<StoryId, Vec<char>> = Default::default();
        let mut matches = Vec::with_capacity(hits.len());
        for h in hits {
            let Some(sid) = h.story else {
                let Some(f) = h.wordart else { continue };
                let Some(w) = doc.frame(f).and_then(|f| f.as_wordart()) else {
                    continue;
                };
                let chars: Vec<char> = w.text.chars().collect();
                let a = h.start.saturating_sub(20);
                let b = (h.end + 20).min(chars.len());
                matches.push(FindMatch {
                    story_id: None,
                    frame_id: Some(f),
                    start: h.start,
                    end: h.end,
                    context: snippet(&chars[a..b]),
                });
                continue;
            };
            let Some(story) = doc.story(sid) else {
                continue;
            };
            let chars = texts
                .entry(sid)
                .or_insert_with(|| story.plain_text().chars().collect());
            let a = h.start.saturating_sub(20);
            let b = (h.end + 20).min(chars.len());
            let context = snippet(&chars[a..b]);
            let sl = layout.stories.get(&sid);
            let shown = |f: &FrameId| {
                sl.and_then(|s| s.frames.get(f))
                    .is_some_and(|ft| ft.chars.contains(&h.start))
            };
            let frame_id = story.frames.iter().copied().find(shown);
            matches.push(FindMatch {
                story_id: Some(sid),
                frame_id,
                start: h.start,
                end: h.end,
                context,
            });
        }
        Ok(TextFindOut {
            count: matches.len(),
            matches,
        })
    }

    pub fn text_replace_all(&mut self, req: TextReplaceAll) -> ApiResult<TextReplaceAllOut> {
        let doc_id = req.doc_id;
        match self.text_replace_all_plan(req)? {
            None => {
                let open = self.get(doc_id)?;
                let doc_id = self.resolve(doc_id)?;
                Ok(TextReplaceAllOut {
                    count: 0,
                    change: Changed {
                        doc_id,
                        revision: open.revision(),
                        affected_ids: Vec::new(),
                    },
                })
            }
            Some((count, command)) => {
                let change = self.exec(doc_id, command)?;
                Ok(TextReplaceAllOut { count, change })
            }
        }
    }

    /// What `text_replace_all` would do: the number of matches and the one
    /// `Batch` command that replaces them all (back to front, WordArt
    /// included), or `None` for no match. A caller that runs commands
    /// itself (the GUI, which grows table frames in the same undo step)
    /// executes the command instead of calling `text_replace_all`.
    pub fn text_replace_all_plan(
        &mut self,
        req: TextReplaceAll,
    ) -> ApiResult<Option<(usize, Command)>> {
        let opts = FindOptions {
            case_sensitive: req.case_sensitive,
            whole_word: req.whole_word,
        };
        let hits = self.scan(
            req.doc_id,
            req.frame_id,
            req.story_id,
            &req.query,
            opts,
            req.start,
            req.end,
        )?;
        if hits.is_empty() {
            return Ok(None);
        }
        let count = hits.len();
        // Back to front so earlier offsets stay valid.
        let mut commands = Vec::new();
        let mut wordart: std::collections::BTreeMap<FrameId, Vec<(usize, usize)>> =
            Default::default();
        for h in hits.into_iter().rev() {
            match (h.story, h.wordart) {
                (Some(story), _) => commands.push(Command::ReplaceText {
                    story,
                    start: h.start,
                    end: h.end,
                    text: req.replacement.clone(),
                }),
                (None, Some(f)) => wordart.entry(f).or_default().push((h.start, h.end)),
                (None, None) => {}
            }
        }
        {
            let doc = self.get(req.doc_id)?.doc();
            for (f, ranges) in wordart {
                let Some(art) = doc.frame(f).and_then(|f| f.as_wordart()) else {
                    continue;
                };
                // `ranges` run back to front, so earlier offsets stay valid.
                let mut chars: Vec<char> = art.text.chars().collect();
                for (a, b) in ranges {
                    chars.splice(a..b, req.replacement.chars());
                }
                let mut art = art.clone();
                art.text = chars.into_iter().collect();
                commands.push(Command::SetWordArt { frame: f, art });
            }
        }
        Ok(Some((
            count,
            Command::Batch {
                label: "replace all".into(),
                commands,
            },
        )))
    }
}

/// A match's surroundings as one line: paragraph, column, frame and page
/// breaks, line separators and other control characters read as spaces.
fn snippet(chars: &[char]) -> String {
    chars
        .iter()
        .map(|c| {
            if c.is_control() || matches!(c, '\u{2028}' | '\u{2029}') {
                ' '
            } else {
                *c
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::snippet;

    #[test]
    fn snippets_show_breaks_as_spaces() {
        let t: Vec<char> = "end\u{c}next\u{e}col\u{1d}frame\npara\u{2028}line\ttab"
            .chars()
            .collect();
        assert_eq!(snippet(&t), "end next col frame para line tab");
    }
}
