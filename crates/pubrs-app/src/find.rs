//! Find bar and Replace window.
//!
//! Matching is done by the session (`text_find`, `text_replace_all`), so the
//! GUI, scripts and MCP clients agree on what a match is. This module owns the
//! query state, stepping through matches and selecting them in the text.

use std::collections::BTreeMap;
use std::sync::Arc;

use eframe::egui::{self, Key};
use pubrs_api::core::text::FindOptions;
use pubrs_api::core::{Command, DocumentId, Frame, FrameId, Owner, Story, StoryId};
use pubrs_api::tools::find::{FindMatch, TextFind, TextReplaceAll};

use crate::app::PubrsApp;
use crate::textedit::TextEdit;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum Mode {
    #[default]
    Closed,
    /// The find bar along the bottom.
    Bar,
    /// The Find and Replace window.
    Replace,
}

type StoryMatches = BTreeMap<StoryId, (Arc<Story>, Vec<(usize, usize)>)>;
/// WordArt frames with matches, by frame (their text is not a story).
type WordArtMatches = BTreeMap<FrameId, (Arc<Frame>, Vec<(usize, usize)>)>;
type Scope = Option<(StoryId, usize, usize)>;

/// What the cached matches were found with.
#[derive(Clone, PartialEq)]
struct CacheKey {
    doc: DocumentId,
    query: String,
    case_sensitive: bool,
    whole_word: bool,
    scope: Scope,
}

/// Match ranges per story. A story whose `Arc` is unchanged keeps its ranges,
/// so after an edit only the edited story is scanned again.
#[derive(Default)]
pub struct MatchCache {
    key: Option<CacheKey>,
    rev: u64,
    stories: StoryMatches,
    wordart: WordArtMatches,
    total: usize,
    /// Stories scanned so far (for tests).
    pub scans: usize,
}

impl MatchCache {
    fn update(
        &mut self,
        key: CacheKey,
        rev: u64,
        stories: &BTreeMap<StoryId, Arc<Story>>,
        frames: &BTreeMap<FrameId, Arc<Frame>>,
    ) {
        if self.key.as_ref() == Some(&key) && self.rev == rev {
            return;
        }
        let same_key = self.key.as_ref() == Some(&key);
        let old = if same_key {
            std::mem::take(&mut self.stories)
        } else {
            BTreeMap::new()
        };
        let old_wordart = if same_key {
            std::mem::take(&mut self.wordart)
        } else {
            BTreeMap::new()
        };
        let opts = FindOptions {
            case_sensitive: key.case_sensitive,
            whole_word: key.whole_word,
        };
        let mut new = BTreeMap::new();
        let mut total = 0;
        if !key.query.is_empty() {
            for (id, arc) in stories {
                if key.scope.is_some_and(|s| s.0 != *id) {
                    continue;
                }
                let ranges = match old.get(id) {
                    Some((a, r)) if Arc::ptr_eq(a, arc) => r.clone(),
                    _ => {
                        self.scans += 1;
                        arc.find(&key.query, opts, key.scope.map(|s| (s.1, s.2)))
                    }
                };
                total += ranges.len();
                new.insert(*id, (arc.clone(), ranges));
            }
        }
        // WordArt text is searched too, unless the search is limited to a
        // selection in a story.
        let mut wordart = BTreeMap::new();
        if !key.query.is_empty() && key.scope.is_none() {
            for (id, arc) in frames {
                let Some(w) = arc.as_wordart() else { continue };
                let ranges = match old_wordart.get(id) {
                    Some((a, r)) if Arc::ptr_eq(a, arc) => r.clone(),
                    _ => pubrs_api::core::text::find_matches(&w.text, &key.query, opts, None),
                };
                total += ranges.len();
                wordart.insert(*id, (arc.clone(), ranges));
            }
        }
        self.wordart = wordart;
        self.stories = new;
        self.total = total;
        self.key = Some(key);
        self.rev = rev;
    }

    /// The first match in a WordArt frame.
    fn wordart_match(&self, frame: FrameId) -> Option<(usize, usize)> {
        self.wordart
            .get(&frame)
            .and_then(|(_, r)| r.first().copied())
    }

    fn contains(&self, story: StoryId, lo: usize, hi: usize) -> bool {
        self.stories
            .get(&story)
            .is_some_and(|(_, r)| r.contains(&(lo, hi)))
    }
}

#[derive(Default)]
pub struct FindState {
    pub mode: Mode,
    pub query: String,
    pub replacement: String,
    pub case_sensitive: bool,
    pub whole_word: bool,
    pub selection_only: bool,
    /// The text selected when "selection only" was ticked.
    scope: Option<(StoryId, usize, usize)>,
    /// Put the focus in the query field (and select its text) next frame.
    focus: bool,
    pub cache: MatchCache,
    /// Revision the remembered selection was taken at.
    scope_rev: u64,
    /// Replacements made by the last Replace All.
    pub last_replaced: Option<usize>,
}

impl FindState {
    pub fn match_count(&self) -> usize {
        self.cache.total
    }
}

const QUERY_ID: &str = "find-query";

/// Open the bar or the Replace window and focus the query.
pub fn open(app: &mut PubrsApp, mode: Mode) {
    if app.view().is_none() {
        return;
    }
    // A short selection in the text becomes the query.
    if let Some(e) = app.view().and_then(|v| v.edit.clone())
        && e.has_selection()
        && let Some(doc) = app.doc()
        && let Some(story) = doc.story(e.story)
    {
        let (lo, hi) = e.range();
        let text: String = story.plain_text().chars().skip(lo).take(hi - lo).collect();
        if !text.contains('\n') && text.chars().count() <= 100 {
            app.find.query = text;
        }
    }
    app.find.mode = mode;
    app.find.focus = true;
}

pub fn close(app: &mut PubrsApp) {
    app.find.mode = Mode::Closed;
}

fn refresh(app: &mut PubrsApp) {
    let Some(doc_id) = app.doc_id() else { return };
    let Ok(open) = app.session.get(Some(doc_id)) else {
        return;
    };
    let rev = open.revision();
    // A remembered selection is only good for the revision it was taken at.
    if app.find.scope.is_some() && app.find.scope_rev != rev {
        app.find.scope = None;
        app.find.selection_only = false;
    }
    let f = &app.find;
    let key = CacheKey {
        doc: doc_id,
        query: f.query.clone(),
        case_sensitive: f.case_sensitive,
        whole_word: f.whole_word,
        scope: if f.selection_only { f.scope } else { None },
    };
    let stories = open.doc().stories.clone();
    let frames = open.doc().frames.clone();
    app.find.cache.update(key, rev, &stories, &frames);
}

/// The page index a frame is on, if it is on a page.
fn page_of(app: &PubrsApp, frame: FrameId) -> Option<usize> {
    let doc = app.doc()?;
    match doc.owner_of(frame)? {
        Owner::Page(p) => doc.pages.iter().position(|q| q.id == p),
        Owner::Master(_) => None,
    }
}

/// Matches that can be shown: on a page, in a frame that displays them. Found
/// when the user asks for Next/Previous/Replace, never while editing.
fn navigable(app: &mut PubrsApp) -> Vec<FindMatch> {
    let Some(doc_id) = app.doc_id() else {
        return Vec::new();
    };
    let f = &app.find;
    let scope = if f.selection_only { f.scope } else { None };
    let req = TextFind {
        doc_id: Some(doc_id),
        frame_id: None,
        story_id: scope.map(|s| s.0),
        query: f.query.clone(),
        case_sensitive: f.case_sensitive,
        whole_word: f.whole_word,
        start: scope.map(|s| s.1),
        end: scope.map(|s| s.2),
    };
    let found = app
        .session
        .text_find(req)
        .map(|o| o.matches)
        .unwrap_or_default();
    // One stop per WordArt frame: its matches all select the frame.
    let mut seen = std::collections::BTreeSet::new();
    found
        .into_iter()
        .filter(|m| m.frame_id.is_some_and(|f| page_of(app, f).is_some()))
        .filter(|m| m.story_id.is_some() || m.frame_id.is_some_and(|f| seen.insert(f)))
        .collect()
}

/// What a match is in: a story, or a WordArt frame.
fn source(m: &FindMatch) -> (Option<StoryId>, Option<FrameId>) {
    (
        m.story_id,
        m.story_id.is_none().then_some(m.frame_id).flatten(),
    )
}

/// Select a match: go to its page, scroll to its frame, select its text.
fn goto(app: &mut PubrsApp, m: &FindMatch) {
    let Some(frame) = m.frame_id else { return };
    let Some(page) = page_of(app, frame) else {
        return;
    };
    let rect = app.doc().and_then(|d| d.frame(frame)).map(|f| f.rect);
    if let Some(v) = app.view_mut() {
        v.page = page;
        v.selection = vec![frame];
        // WordArt text is not edited in place: its frame is selected.
        v.edit = m.story_id.map(|story| TextEdit {
            frame,
            story,
            caret: m.end,
            anchor: m.start,
            goal_x: None,
        });
        v.scroll_to = rect;
    }
}

/// Select the next (or previous) match after the current selection, wrapping.
pub fn step(app: &mut PubrsApp, forward: bool) {
    if app.view().is_none() {
        return;
    }
    if app.find.query.is_empty() {
        open(app, Mode::Bar);
        return;
    }
    refresh(app);
    let all = navigable(app);
    if all.is_empty() {
        let hidden = app.find.match_count() > 0;
        app.notify(if hidden {
            "Matches are only in text that is not shown (overflow or master page)"
        } else {
            "No matches"
        });
        return;
    }
    let edit = app.view().and_then(|v| v.edit.clone());
    // Where stepping continues from: the text being edited, or a selected
    // WordArt frame that has a match.
    let from = match &edit {
        Some(e) => Some(((Some(e.story), None), e.range())),
        None => app
            .view()
            .and_then(|v| v.selection.first().copied())
            .filter(|f| {
                app.doc()
                    .and_then(|d| d.frame(*f))
                    .is_some_and(|f| f.as_wordart().is_some())
            })
            .map(|f| ((None, Some(f)), (0, usize::MAX))),
    };
    let pick = match from {
        None => {
            if forward {
                all.first()
            } else {
                all.last()
            }
        }
        Some((src, (lo, hi))) => {
            let order: Vec<_> = {
                let mut v = Vec::new();
                for m in &all {
                    if !v.contains(&source(m)) {
                        v.push(source(m));
                    }
                }
                v
            };
            let rank = |s: (Option<StoryId>, Option<FrameId>)| order.iter().position(|x| *x == s);
            let cur = rank(src);
            if forward {
                all.iter()
                    .find(|m| match (rank(source(m)), cur) {
                        (Some(a), Some(b)) => a > b || (a == b && m.start >= hi),
                        _ => false,
                    })
                    .or_else(|| {
                        // The edited story has no match (or none left): the
                        // first match of a later story, else wrap.
                        cur.is_none().then(|| all.first()).flatten()
                    })
                    .or(all.first())
            } else {
                all.iter()
                    .rev()
                    .find(|m| match (rank(source(m)), cur) {
                        (Some(a), Some(b)) => a < b || (a == b && m.end <= lo),
                        _ => false,
                    })
                    .or_else(|| cur.is_none().then(|| all.last()).flatten())
                    .or(all.last())
            }
        }
    };
    if let Some(m) = pick.cloned() {
        goto(app, &m);
    }
}

fn selection_scope(app: &PubrsApp) -> Option<(StoryId, usize, usize)> {
    let e = app.view()?.edit.as_ref()?;
    e.has_selection().then(|| {
        let (lo, hi) = e.range();
        (e.story, lo, hi)
    })
}

/// Replace the selected match, then select the next one. When the selection is
/// not a match yet, just select the next match.
pub fn replace_current(app: &mut PubrsApp) {
    refresh(app);
    let Some(e) = app.view().and_then(|v| v.edit.clone()) else {
        // A selected WordArt frame with a match: replace its first one.
        let sel = app.view().and_then(|v| v.selection.first().copied());
        if let Some(frame) = sel
            && let Some((lo, hi)) = app.find.cache.wordart_match(frame)
            && let Some(mut art) = app
                .doc()
                .and_then(|d| d.frame(frame))
                .and_then(|f| f.as_wordart().cloned())
        {
            let mut chars: Vec<char> = art.text.chars().collect();
            chars.splice(lo..hi, app.find.replacement.chars());
            art.text = chars.into_iter().collect();
            app.exec(Command::SetWordArt { frame, art });
            refresh(app);
        }
        return step(app, true);
    };
    let (lo, hi) = e.range();
    let is_match = app.find.cache.contains(e.story, lo, hi);
    if !is_match {
        return step(app, true);
    }
    let text = app.find.replacement.clone();
    let len = text.chars().count();
    if app.exec(Command::ReplaceText {
        story: e.story,
        start: lo,
        end: hi,
        text,
    }) && let Some(v) = app.view_mut()
    {
        v.edit = Some(TextEdit::new(e.frame, e.story, lo + len));
    }
    refresh(app);
    step(app, true);
}

/// Replace every match (in the remembered selection if "selection only").
pub fn replace_all(app: &mut PubrsApp) {
    let Some(doc_id) = app.doc_id() else { return };
    let f = &app.find;
    let scope = if f.selection_only { f.scope } else { None };
    let req = TextReplaceAll {
        doc_id: Some(doc_id),
        frame_id: None,
        story_id: scope.map(|s| s.0),
        query: f.query.clone(),
        replacement: f.replacement.clone(),
        case_sensitive: f.case_sensitive,
        whole_word: f.whole_word,
        start: scope.map(|s| s.1),
        end: scope.map(|s| s.2),
    };
    let r = app.session.text_replace_all(req);
    if let Some(out) = app.report(r) {
        app.find.last_replaced = Some(out.count);
        app.notify(match out.count {
            0 => "No matches to replace".to_string(),
            1 => "Replaced 1 occurrence".to_string(),
            n => format!("Replaced {n} occurrences"),
        });
        // Any selection may now point past the end of its story.
        if let Some(v) = app.view_mut() {
            v.edit = None;
        }
    }
}

enum Act {
    Next,
    Previous,
    Replace,
    ReplaceAll,
    Close,
}

/// The controls shared by the bar and the window.
fn controls(app: &mut PubrsApp, ui: &mut egui::Ui, replace: bool) -> Option<Act> {
    refresh(app);
    let count = app.find.match_count();
    let has_query = !app.find.query.is_empty();
    let mut act = None;
    let mut query = app.find.query.clone();
    let mut replacement = app.find.replacement.clone();
    let (mut case, mut whole, mut sel_only) = (
        app.find.case_sensitive,
        app.find.whole_word,
        app.find.selection_only,
    );
    let can_scope = selection_scope(app).is_some() || app.find.scope.is_some();
    let mut focused = false;

    let id = egui::Id::new(QUERY_ID);
    ui.label("Find");
    let r = ui.add(
        egui::TextEdit::singleline(&mut query)
            .id(id)
            .hint_text("Find in document")
            .desired_width(200.0),
    );
    crate::dialogs::mark(ui, "find-query", &r);
    if app.find.focus {
        app.find.focus = false;
        r.request_focus();
        let mut st = egui::TextEdit::load_state(ui.ctx(), id).unwrap_or_default();
        st.cursor.set_char_range(Some(egui::text::CCursorRange::two(
            egui::text::CCursor::new(0),
            egui::text::CCursor::new(query.chars().count()),
        )));
        st.store(ui.ctx(), id);
    }
    focused |= r.has_focus() || r.lost_focus();
    if r.lost_focus() && ui.input(|i| i.key_pressed(Key::Enter)) {
        act = Some(if ui.input(|i| i.modifiers.shift) {
            Act::Previous
        } else {
            Act::Next
        });
        r.request_focus();
    }
    if replace {
        ui.label("Replace with");
        let rr = ui.add(
            egui::TextEdit::singleline(&mut replacement)
                .hint_text("Replacement")
                .desired_width(200.0),
        );
        if rr.lost_focus() && ui.input(|i| i.key_pressed(Key::Enter)) {
            act = Some(Act::Replace);
            rr.request_focus();
        }
        crate::dialogs::mark(ui, "find-replacement", &rr);
        focused |= rr.has_focus() || rr.lost_focus();
    }
    let c = ui.checkbox(&mut case, "Match case");
    crate::dialogs::mark(ui, "find-case", &c);
    let w = ui.checkbox(&mut whole, "Whole word");
    crate::dialogs::mark(ui, "find-whole", &w);
    focused |= c.has_focus() || w.has_focus();
    if replace {
        let s = ui
            .add_enabled(
                can_scope || sel_only,
                egui::Checkbox::new(&mut sel_only, "Selection only"),
            )
            .on_disabled_hover_text("Select some text in a text frame first");
        crate::dialogs::mark(ui, "find-selection-only", &s);
        focused |= s.has_focus();
    }
    let button = |ui: &mut egui::Ui, name: &str, label: &str, enabled: bool, why: &str| {
        let r = ui
            .add_enabled(enabled, egui::Button::new(label))
            .on_disabled_hover_text(why.to_string());
        crate::dialogs::mark(ui, name, &r);
        r
    };
    let none = if has_query {
        "No matches"
    } else {
        "Type something to find"
    };
    let prev = button(ui, "find-previous", "Previous", count > 0, none);
    let next = button(ui, "find-next", "Next", count > 0, none);
    focused |= prev.has_focus() || next.has_focus();
    if prev.clicked() {
        act = Some(Act::Previous);
    }
    if next.clicked() {
        act = Some(Act::Next);
    }
    if replace {
        let rp = button(ui, "find-replace", "Replace", count > 0, none);
        let ra = button(ui, "find-replace-all", "Replace All", count > 0, none);
        focused |= rp.has_focus() || ra.has_focus();
        if rp.clicked() {
            act = Some(Act::Replace);
        }
        if ra.clicked() {
            act = Some(Act::ReplaceAll);
        }
    }
    ui.label(if !has_query {
        String::new()
    } else if count == 0 {
        "No matches".into()
    } else if count == 1 {
        "1 match".into()
    } else {
        format!("{count} matches")
    });
    let x = ui.button("Close").on_hover_text("Close (Esc)");
    focused |= x.has_focus();
    if x.clicked() {
        act = Some(Act::Close);
    }
    if focused && ui.input(|i| i.key_pressed(Key::Escape)) {
        act = Some(Act::Close);
    }

    // Ticking "selection only" remembers the selection it applies to.
    if sel_only && !app.find.selection_only {
        app.find.scope = selection_scope(app);
        app.find.scope_rev = app.session.get(None).map_or(0, |o| o.revision());
        sel_only = app.find.scope.is_some();
    } else if !sel_only {
        app.find.scope = None;
    }
    app.find.query = query;
    app.find.replacement = replacement;
    app.find.case_sensitive = case;
    app.find.whole_word = whole;
    app.find.selection_only = sel_only;
    act
}

fn run(app: &mut PubrsApp, act: Option<Act>) {
    match act {
        Some(Act::Next) => step(app, true),
        Some(Act::Previous) => step(app, false),
        Some(Act::Replace) => replace_current(app),
        Some(Act::ReplaceAll) => replace_all(app),
        Some(Act::Close) => close(app),
        None => {}
    }
}

/// The find bar, a panel above the status bar (call before the canvas).
pub fn bar(app: &mut PubrsApp, ui: &mut egui::Ui) {
    if app.find.mode != Mode::Bar || app.view().is_none() {
        return;
    }
    let mut act = None;
    egui::Panel::bottom("find-bar").show(ui, |ui| {
        ui.horizontal(|ui| act = controls(app, ui, false));
    });
    run(app, act);
}

/// The Find and Replace window.
pub fn replace_window(app: &mut PubrsApp, ctx: &egui::Context) {
    if app.find.mode != Mode::Replace || app.view().is_none() {
        return;
    }
    let mut act = None;
    egui::Window::new("Find and Replace")
        .collapsible(false)
        .resizable(false)
        .anchor(egui::Align2::RIGHT_TOP, [-24.0, 96.0])
        .show(ctx, |ui| {
            ui.vertical(|ui| act = controls(app, ui, true));
        });
    run(app, act);
}

#[cfg(test)]
mod tests {
    use pubrs_api::core::{FrameId, SeqIds};

    use super::*;

    fn story(ids: &mut SeqIds, text: &str) -> (StoryId, Arc<Story>) {
        let id = StoryId::new(ids);
        let mut s = Story::new(id, FrameId::new(ids));
        s.insert_text(0, text, None);
        (id, Arc::new(s))
    }

    #[test]
    fn an_edit_rescans_only_the_edited_story() {
        let mut ids = SeqIds::new(3);
        let doc = DocumentId::new(&mut ids);
        let mut stories = BTreeMap::new();
        for t in ["the cat", "the dog the end", "nothing", "bathe"] {
            let (id, s) = story(&mut ids, t);
            stories.insert(id, s);
        }
        let key = CacheKey {
            doc,
            query: "the".into(),
            case_sensitive: false,
            whole_word: false,
            scope: None,
        };
        let mut c = MatchCache::default();
        c.update(key.clone(), 1, &stories, &BTreeMap::new());
        assert_eq!((c.scans, c.total), (4, 4));
        // Same revision: nothing to do.
        c.update(key.clone(), 1, &stories, &BTreeMap::new());
        assert_eq!(c.scans, 4);
        // Edit one story (a new Arc), others keep theirs.
        let first = *stories.keys().next().expect("stories");
        let mut edited = (*stories[&first]).clone();
        edited.insert_text(0, "the ", None);
        stories.insert(first, Arc::new(edited));
        c.update(key.clone(), 2, &stories, &BTreeMap::new());
        assert_eq!((c.scans, c.total), (5, 5));
        // A deleted story drops out without a scan.
        let (before, gone) = (c.total, c.stories[&first].1.len());
        stories.remove(&first);
        c.update(key, 3, &stories, &BTreeMap::new());
        assert_eq!((c.scans, c.total), (5, before - gone));
    }
}
