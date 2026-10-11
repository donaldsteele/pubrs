//! Find and replace across stories.
//!
//! Every story in the document is searched, which covers text frames, and will
//! cover table cells (M19) automatically because cells are stories.
//! TODO(M25): WordArt text must be added to `scan` once it exists.

use pubrs_core::text::FindOptions;
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
    pub story_id: StoryId,
    /// The frame showing the start of the match (null if it is not shown).
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
    story: StoryId,
    start: usize,
    end: usize,
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
        let mut stories: Vec<StoryId> = if single {
            vec![self.resolve_story(doc_id, frame_id, story_id)?]
        } else {
            doc.stories.keys().copied().collect()
        };
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
        stories.sort_by_key(order);
        let mut out = Vec::new();
        for s in stories {
            let Some(story) = doc.story(s) else { continue };
            let limit = if single && (start.is_some() || end.is_some()) {
                Some((start.unwrap_or(0), end.unwrap_or(story.char_len())))
            } else {
                None
            };
            for (a, b) in story.find(query, opts, limit) {
                out.push(Hit {
                    story: s,
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
            let Some(story) = doc.story(h.story) else {
                continue;
            };
            let chars = texts
                .entry(h.story)
                .or_insert_with(|| story.plain_text().chars().collect());
            let a = h.start.saturating_sub(20);
            let b = (h.end + 20).min(chars.len());
            let context: String = chars[a..b].iter().collect::<String>().replace('\n', " ");
            let sl = layout.stories.get(&h.story);
            let shown = |f: &FrameId| {
                sl.and_then(|s| s.frames.get(f))
                    .is_some_and(|ft| ft.chars.contains(&h.start))
            };
            let frame_id = story.frames.iter().copied().find(shown);
            matches.push(FindMatch {
                story_id: h.story,
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
            let open = self.get(req.doc_id)?;
            let doc_id = self.resolve(req.doc_id)?;
            return Ok(TextReplaceAllOut {
                count: 0,
                change: Changed {
                    doc_id,
                    revision: open.revision(),
                    affected_ids: Vec::new(),
                },
            });
        }
        let count = hits.len();
        // Back to front so earlier offsets stay valid.
        let commands = hits
            .into_iter()
            .rev()
            .map(|h| Command::ReplaceText {
                story: h.story,
                start: h.start,
                end: h.end,
                text: req.replacement.clone(),
            })
            .collect();
        let change = self.exec(
            req.doc_id,
            Command::Batch {
                label: "replace all".into(),
                commands,
            },
        )?;
        Ok(TextReplaceAllOut { count, change })
    }
}
