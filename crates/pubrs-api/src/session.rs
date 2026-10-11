//! The session: open documents, command dispatch and shared caches.

use std::collections::{BTreeMap, VecDeque};
use std::path::PathBuf;
use std::sync::Arc;

use pubrs_core::{
    Change, Command, Document, DocumentId, Editor, FrameId, IdSource, MasterId, Owner, PageId,
    RandomIds, SeqIds, StoryId, StyleId, StyleKind, Touched,
};
use pubrs_io::ExportContext;
use pubrs_layout::{CacheStats, DocumentLayout, FontLibrary, StoryLayoutCache};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::error::{ApiError, ApiResult};

/// Session settings.
#[derive(Debug, Clone, Default)]
pub struct SessionConfig {
    /// Make installed system fonts available (otherwise only bundled fonts).
    pub system_fonts: bool,
    /// Deterministic ids (tests, reproducible scripts). `None` = random v4.
    pub id_seed: Option<u64>,
    /// Where user templates are saved and loaded from.
    pub user_templates_dir: Option<PathBuf>,
    /// Font files to use instead of the bundled set (tests).
    pub font_dir: Option<PathBuf>,
    /// Folder that relative paths in save/open/export calls resolve against,
    /// and where files go when no path is given.
    pub output_dir: Option<PathBuf>,
    /// Never use the GPU for interactive renders (small throwaway sessions,
    /// where creating a device costs more than drawing).
    pub software_rendering: bool,
}

/// One open document.
pub struct OpenDoc {
    pub doc_id: DocumentId,
    pub editor: Editor,
    pub path: Option<PathBuf>,
    /// The layout of the current revision, once computed.
    layout: Option<(u64, Arc<DocumentLayout>)>,
    /// Per-story layouts reused across revisions (ADR 0012).
    stories: StoryLayoutCache,
    /// What the most recent commands touched, by the revision they made
    /// (newest last), for damage repaint (ADR 0013).
    changes: VecDeque<(u64, Touched)>,
}

/// Commands remembered per document for damage repaint.
const CHANGE_LOG: usize = 256;

impl OpenDoc {
    pub fn doc(&self) -> &Document {
        self.editor.doc()
    }

    pub fn revision(&self) -> u64 {
        self.editor.revision()
    }

    /// Changes recorded after revision `since` (oldest first). Revisions
    /// made outside `Session::exec` (undo, redo) are missing; see
    /// [`touched_between`].
    pub fn changes_since(&self, since: u64) -> Vec<(u64, Touched)> {
        self.changes
            .iter()
            .filter(|(r, _)| *r > since)
            .cloned()
            .collect()
    }
}

/// The union of what changed between revisions `from` (exclusive) and `to`
/// (inclusive), or `None` when `changes` lacks one of those revisions (an
/// undo, a redo or an evicted entry): then the caller must compare the
/// whole page.
pub fn touched_between(changes: &[(u64, Touched)], from: u64, to: u64) -> Option<Touched> {
    let mut out = Touched::default();
    let mut expect = from + 1;
    for (r, t) in changes.iter().filter(|(r, _)| *r > from && *r <= to) {
        if *r != expect {
            return None;
        }
        expect += 1;
        out.frames.extend(&t.frames);
        out.stories.extend(&t.stories);
        out.pages.extend(&t.pages);
        out.assets.extend(&t.assets);
        out.styles |= t.styles;
        out.setup |= t.setup;
    }
    if expect != to + 1 {
        return None;
    }
    out.frames.sort();
    out.frames.dedup();
    out.stories.sort();
    out.stories.dedup();
    out.pages.sort();
    out.pages.dedup();
    out.assets.sort();
    out.assets.dedup();
    Some(out)
}

/// What a mutating call changed. Included in every mutating tool's result.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Changed {
    pub doc_id: DocumentId,
    /// Document revision after the call; increases by one per change.
    pub revision: u64,
    /// Ids of objects created, changed or removed.
    pub affected_ids: Vec<String>,
}

/// Holds open documents. The single entry point for GUI, CLI and MCP.
pub struct Session {
    docs: BTreeMap<DocumentId, OpenDoc>,
    order: Vec<DocumentId>,
    current: Option<DocumentId>,
    pub(crate) ctx: ExportContext,
    pub(crate) ids: Box<dyn IdSource + Send>,
    pub(crate) config: SessionConfig,
    pub(crate) rasterizer: Option<pubrs_render::Rasterizer>,
    /// Pages rasterised by `page_png` / `page_rgba` on the calling thread.
    pub(crate) rasters: u64,
    pub(crate) templates: pubrs_io::templates::TemplateLibrary,
}

impl std::fmt::Debug for Session {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Session")
            .field("docs", &self.order)
            .field("current", &self.current)
            .finish()
    }
}

impl Default for Session {
    fn default() -> Self {
        Self::new(SessionConfig::default())
    }
}

impl Session {
    pub fn new(config: SessionConfig) -> Self {
        let fonts = match &config.font_dir {
            Some(dir) => FontLibrary::from_dir(dir).unwrap_or_else(|e| {
                tracing::warn!(
                    "cannot read font dir {}: {e}; using bundled fonts",
                    dir.display()
                );
                FontLibrary::bundled()
            }),
            None if config.system_fonts => FontLibrary::with_system_fonts(),
            None => FontLibrary::bundled(),
        };
        let ids: Box<dyn IdSource + Send> = match config.id_seed {
            Some(seed) => Box::new(SeqIds::new(seed)),
            None => Box::new(RandomIds),
        };
        Self {
            docs: BTreeMap::new(),
            order: Vec::new(),
            current: None,
            ctx: ExportContext::new(fonts),
            ids,
            templates: pubrs_io::templates::TemplateLibrary::new(config.user_templates_dir.clone()),
            config,
            rasterizer: None,
            rasters: 0,
        }
    }

    /// How many pages this session rasterised synchronously (`page_png`,
    /// `page_rgba`) so far. The GUI checks that editing adds none.
    pub fn rasters_drawn(&self) -> u64 {
        self.rasters
    }

    pub fn config(&self) -> &SessionConfig {
        &self.config
    }

    /// Resolve a user-supplied path: absolute paths are used as-is, relative
    /// ones are joined to the output folder when one is configured.
    pub fn resolve_path(&self, path: &str) -> PathBuf {
        let p = if cfg!(windows) {
            PathBuf::from(verbatim_separators(path).as_ref())
        } else {
            PathBuf::from(path)
        };
        match &self.config.output_dir {
            Some(dir) if p.is_relative() => dir.join(p),
            _ => p,
        }
    }

    /// A default file path in the output folder for a document (e.g. `bakery-flyer.pdf`).
    pub fn default_path(&self, doc_id: Option<DocumentId>, ext: &str) -> Option<PathBuf> {
        let dir = self.config.output_dir.as_ref()?;
        let title = self.get(doc_id).ok()?.doc().meta.title.clone();
        Some(dir.join(format!("{}.{ext}", slug(&title))))
    }

    pub fn ids(&mut self) -> &mut dyn IdSource {
        &mut *self.ids
    }

    /// Register a newly created or loaded document and make it current.
    pub fn insert(&mut self, doc: Document, path: Option<PathBuf>) -> DocumentId {
        let doc_id = DocumentId::new(&mut *self.ids);
        let mut editor = Editor::new(doc);
        editor.mark_saved();
        self.docs.insert(
            doc_id,
            OpenDoc {
                doc_id,
                editor,
                path,
                layout: None,
                stories: StoryLayoutCache::new(),
                changes: VecDeque::new(),
            },
        );
        self.order.push(doc_id);
        self.current = Some(doc_id);
        doc_id
    }

    /// Remove a document from the session.
    pub fn remove(&mut self, doc_id: DocumentId) -> Option<OpenDoc> {
        self.order.retain(|d| *d != doc_id);
        if self.current == Some(doc_id) {
            self.current = self.order.last().copied();
        }
        self.docs.remove(&doc_id)
    }

    /// Open documents in the order they were opened.
    pub fn documents(&self) -> impl Iterator<Item = &OpenDoc> {
        self.order.iter().filter_map(|d| self.docs.get(d))
    }

    /// The document an optional id refers to (default: the current one).
    pub fn resolve(&self, doc_id: Option<DocumentId>) -> ApiResult<DocumentId> {
        match doc_id.or(self.current) {
            Some(d) if self.docs.contains_key(&d) => Ok(d),
            Some(d) => Err(ApiError::not_found(
                "document",
                &d.to_string(),
                "list open documents with doc_list",
            )),
            None => Err(ApiError::new(
                "no_document",
                "no document is open",
                "create one with doc_new, template_apply or doc_open",
            )),
        }
    }

    pub fn current(&self) -> Option<DocumentId> {
        self.current
    }

    pub fn set_current(&mut self, doc_id: DocumentId) {
        if self.docs.contains_key(&doc_id) {
            self.current = Some(doc_id);
        }
    }

    pub fn get(&self, doc_id: Option<DocumentId>) -> ApiResult<&OpenDoc> {
        let id = self.resolve(doc_id)?;
        Ok(&self.docs[&id])
    }

    pub fn get_mut(&mut self, doc_id: Option<DocumentId>) -> ApiResult<&mut OpenDoc> {
        let id = self.resolve(doc_id)?;
        Ok(self.docs.get_mut(&id).expect("resolved"))
    }

    /// Apply a command to a document (atomically) and report the change.
    pub fn exec(&mut self, doc_id: Option<DocumentId>, command: Command) -> ApiResult<Changed> {
        self.exec_inner(doc_id, command, None)
    }

    /// Like [`Session::exec`] but merges consecutive calls with the same key
    /// into one undo step (used for typing in the GUI).
    pub fn exec_coalesced(
        &mut self,
        doc_id: Option<DocumentId>,
        command: Command,
        key: &str,
    ) -> ApiResult<Changed> {
        self.exec_inner(doc_id, command, Some(key))
    }

    fn exec_inner(
        &mut self,
        doc_id: Option<DocumentId>,
        command: Command,
        key: Option<&str>,
    ) -> ApiResult<Changed> {
        let id = self.resolve(doc_id)?;
        let open = self.docs.get_mut(&id).expect("resolved");
        let before = open.editor.revision();
        let change = match key {
            Some(k) => open.editor.execute_coalesced(command, k)?,
            None => open.editor.execute(command)?,
        };
        // A command that changed nothing keeps the revision: nothing to log.
        if change.revision != before {
            if open.changes.len() == CHANGE_LOG {
                open.changes.pop_front();
            }
            open.changes
                .push_back((change.revision, change.touched.clone()));
        }
        self.current = Some(id);
        Ok(changed(id, &change))
    }

    /// Layout of a document, brought up to date when its revision changed:
    /// only stories whose inputs changed are laid out again (the document's
    /// [`StoryLayoutCache`]; fonts and shaping caches are shared).
    pub fn layout(&mut self, doc_id: Option<DocumentId>) -> ApiResult<Arc<DocumentLayout>> {
        let id = self.resolve(doc_id)?;
        let open = self.docs.get_mut(&id).expect("resolved");
        let rev = open.editor.revision();
        if let Some((r, l)) = &open.layout
            && *r == rev
        {
            return Ok(l.clone());
        }
        let layout = Arc::new(
            open.stories
                .layout(&mut self.ctx.layouter, open.editor.doc()),
        );
        open.layout = Some((rev, layout.clone()));
        Ok(layout)
    }

    /// What the last incremental layout of a document did (stories
    /// recomposed and reused, time spent on cache keys).
    pub fn layout_stats(&self, doc_id: Option<DocumentId>) -> ApiResult<CacheStats> {
        Ok(self.get(doc_id)?.stories.stats())
    }

    /// A document borrowed together with the session's render caches, so
    /// export and drawing need no copy of the document.
    pub(crate) fn doc_with_caches(&mut self, doc_id: DocumentId) -> ApiResult<DocWithCaches<'_>> {
        let open = self.docs.get(&doc_id).ok_or_else(|| {
            ApiError::not_found(
                "document",
                &doc_id.to_string(),
                "list open documents with doc_list",
            )
        })?;
        Ok(DocWithCaches {
            doc: open.doc(),
            revision: open.revision(),
            ctx: &mut self.ctx,
            rasterizer: &mut self.rasterizer,
            software: self.config.software_rendering,
        })
    }

    /// Fresh id from the session's id source.
    pub fn new_frame_id(&mut self) -> FrameId {
        FrameId::new(&mut *self.ids)
    }

    pub fn new_story_id(&mut self) -> StoryId {
        StoryId::new(&mut *self.ids)
    }

    // ------------------------------------------------------------ lookups

    pub fn require_page(&self, doc_id: Option<DocumentId>, page: PageId) -> ApiResult<usize> {
        self.get(doc_id)?.doc().page_index(page).ok_or_else(|| {
            ApiError::not_found("page", &page.to_string(), "list pages with page_list")
        })
    }

    /// The page or master a new frame goes on (default: the first page).
    pub fn owner_for(
        &self,
        doc_id: Option<DocumentId>,
        page_id: Option<PageId>,
        master_id: Option<MasterId>,
    ) -> ApiResult<Owner> {
        let doc = self.get(doc_id)?.doc();
        match (page_id, master_id) {
            (Some(_), Some(_)) => Err(ApiError::invalid(
                "give either page_id or master_id, not both",
                "use page_id for a page, master_id for a master page",
            )),
            (Some(p), None) => {
                self.require_page(doc_id, p)?;
                Ok(Owner::Page(p))
            }
            (None, Some(m)) => doc.master(m).map(|_| Owner::Master(m)).ok_or_else(|| {
                ApiError::not_found("master", &m.to_string(), "list masters with doc_info")
            }),
            (None, None) => Ok(Owner::Page(doc.pages[0].id)),
        }
    }

    pub fn require_frame(&self, doc_id: Option<DocumentId>, frame: FrameId) -> ApiResult<()> {
        if self.get(doc_id)?.doc().frame(frame).is_some() {
            Ok(())
        } else {
            Err(ApiError::not_found(
                "frame",
                &frame.to_string(),
                "list frames with frame_list",
            ))
        }
    }

    /// The story a text target refers to (a text frame or a story id).
    pub fn resolve_story(
        &self,
        doc_id: Option<DocumentId>,
        frame_id: Option<FrameId>,
        story_id: Option<StoryId>,
    ) -> ApiResult<StoryId> {
        let doc = self.get(doc_id)?.doc();
        match (frame_id, story_id) {
            (_, Some(s)) => doc.story(s).map(|_| s).ok_or_else(|| {
                ApiError::not_found(
                    "story",
                    &s.to_string(),
                    "use text_get on a frame to find its story",
                )
            }),
            (Some(f), None) => {
                let frame = doc.frame(f).ok_or_else(|| {
                    ApiError::not_found("frame", &f.to_string(), "list frames with frame_list")
                })?;
                frame.as_text().map(|t| t.story).ok_or_else(|| {
                    ApiError::invalid(
                        format!("frame {f} is not a text frame"),
                        "pass a text frame (frame_list with kind \"text\")",
                    )
                })
            }
            (None, None) => Err(ApiError::invalid(
                "give frame_id or story_id",
                "text tools need a text frame or story",
            )),
        }
    }

    /// Find a style by id or (case-insensitive) name.
    pub fn resolve_style(
        &self,
        doc_id: Option<DocumentId>,
        name_or_id: &str,
        kind: Option<StyleKind>,
    ) -> ApiResult<StyleId> {
        let doc = self.get(doc_id)?.doc();
        let found = name_or_id
            .parse::<StyleId>()
            .ok()
            .and_then(|id| doc.styles.get(id))
            .or_else(|| doc.styles.by_name(name_or_id));
        match found {
            Some(s) if kind.is_none_or(|k| k == s.kind) => Ok(s.id),
            Some(s) => Err(ApiError::invalid(
                format!("style {:?} is a {:?} style", s.name, s.kind),
                "use a paragraph style for paragraphs and a character style for runs",
            )),
            None => Err(ApiError::not_found(
                "style",
                name_or_id,
                "list styles with style_list or create one with style_create",
            )),
        }
    }
}

/// A borrowed document plus the mutable caches that draw it (see
/// [`Session::doc_with_caches`]).
pub(crate) struct DocWithCaches<'a> {
    pub doc: &'a Document,
    pub revision: u64,
    pub ctx: &'a mut ExportContext,
    pub rasterizer: &'a mut Option<pubrs_render::Rasterizer>,
    /// `SessionConfig::software_rendering`.
    pub software: bool,
}

/// Convert an editor change into the API's change report.
pub fn changed(doc_id: DocumentId, change: &Change) -> Changed {
    let t = &change.touched;
    let mut ids: Vec<String> = Vec::new();
    ids.extend(t.frames.iter().map(ToString::to_string));
    ids.extend(t.stories.iter().map(ToString::to_string));
    ids.extend(t.assets.iter().map(ToString::to_string));
    if !t.pages.is_empty() {
        ids.extend(t.pages.iter().map(ToString::to_string));
    }
    ids.sort();
    ids.dedup();
    Changed {
        doc_id,
        revision: change.revision,
        affected_ids: ids,
    }
}

/// File-name-safe version of a title ("Golden Crust!" -> "golden-crust").
pub fn slug(title: &str) -> String {
    let mut s = String::new();
    for c in title.chars() {
        if c.is_alphanumeric() {
            s.extend(c.to_lowercase());
        } else if !s.ends_with('-') && !s.is_empty() {
            s.push('-');
        }
    }
    let s = s.trim_end_matches('-').to_string();
    if s.is_empty() { "untitled".into() } else { s }
}

/// Windows verbatim paths (`\\?\C:\...`, what `canonicalize` returns) accept
/// no `/` separators, but clients often build paths by appending `/name`.
pub fn verbatim_separators(path: &str) -> std::borrow::Cow<'_, str> {
    if path.starts_with(r"\\?\") && path.contains('/') {
        path.replace('/', r"\").into()
    } else {
        path.into()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn verbatim_paths_get_backslashes() {
        assert_eq!(
            verbatim_separators(r"\\?\D:\a\examples\assets/bread.jpg"),
            r"\\?\D:\a\examples\assets\bread.jpg"
        );
        assert_eq!(verbatim_separators("C:/a/b.jpg"), "C:/a/b.jpg");
        assert_eq!(verbatim_separators("/home/a/b.jpg"), "/home/a/b.jpg");
    }

    #[test]
    fn changes_are_logged_for_damage_and_gaps_are_reported() {
        use pubrs_core::{DocBuilder, PageSetup, Rect, Shape};
        let mut b = DocBuilder::new(SeqIds::new(1), PageSetup::default());
        let p = b.page(0);
        let f = b
            .shape(
                p,
                Rect::new(10.0, 10.0, 20.0, 20.0),
                Shape::Rectangle { corner_radius: 0.0 },
            )
            .expect("shape");
        let mut s = Session::new(SessionConfig::default());
        let id = s.insert(b.finish(), None);
        let start = s.get(Some(id)).expect("doc").revision();
        for x in [20.0, 30.0] {
            s.exec(
                Some(id),
                Command::SetFrameRect {
                    frame: f,
                    rect: Rect::new(x, 10.0, 20.0, 20.0),
                },
            )
            .expect("move");
        }
        // A command that changes nothing is not logged.
        s.exec(
            Some(id),
            Command::SetFrameRect {
                frame: f,
                rect: Rect::new(30.0, 10.0, 20.0, 20.0),
            },
        )
        .expect("no-op");
        let open = s.get(Some(id)).expect("doc");
        let now = open.revision();
        let changes = open.changes_since(start);
        assert_eq!(changes.len(), 2);
        let t = touched_between(&changes, start, now).expect("contiguous");
        assert_eq!(t.frames, vec![f]);
        assert_eq!(
            touched_between(&changes[1..], start, now),
            None,
            "a missing revision (undo, redo) is a gap"
        );
        s.get_mut(Some(id)).expect("doc").editor.undo();
        let open = s.get(Some(id)).expect("doc");
        assert_eq!(
            touched_between(&open.changes_since(start), start, open.revision()),
            None,
            "undo is not logged, so the caller compares the whole page"
        );
    }

    #[test]
    fn slugs() {
        assert_eq!(slug("Golden Crust!"), "golden-crust");
        assert_eq!(slug("  "), "untitled");
    }
}
