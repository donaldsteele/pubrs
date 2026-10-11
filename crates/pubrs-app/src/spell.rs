//! Spelling in the app (M28): the spell worker, red squiggles under misspelt
//! words, the right-click menu and the Spelling window (Review → Spelling…,
//! F7).
//!
//! * The spell worker is one thread with its own [`SpellChecker`]: the UI
//!   thread never checks a word. Requests are newest-wins (a page check not
//!   yet started is replaced by a newer one); added and ignored words are
//!   applied in order before anything else, and then the last page is
//!   checked again. A panic is caught per job and reported; the app starts
//!   a dead worker again (and replays the ignored words).
//! * The canvas sends the page snapshot it already builds (an `Arc`
//!   document slice) whenever the revision, page or edited story changes.
//!   The worker answers per story with the `Arc<Story>` it checked: the
//!   squiggles of a story are drawn only while that is still the story on
//!   screen, so stale offsets are never drawn. Stories the snapshot shares
//!   with the last check are not looked at again, and in an edited story the
//!   paragraph cache re-checks only the changed paragraph.
//! * Squiggles are drawn for the frames of the page being shown only, line
//!   by line (cost: the lines on screen).

use std::collections::{BTreeMap, HashSet};
use std::path::PathBuf;
use std::sync::mpsc::{Receiver, Sender, TryRecvError, channel};
use std::sync::{Arc, Condvar, Mutex, PoisonError};
use std::thread::JoinHandle;

use eframe::egui::{self, Color32, Key, Modifiers, Pos2, RichText, Stroke};
use pubrs_api::core::{Command, Document, DocumentId, Owner, Story, StoryId, StyleSheet};
use pubrs_api::io::spell::{Issue, Report, SpellChecker, reading_order};
use pubrs_api::layout::DocumentLayout;
use pubrs_api::tools::export::PageSnapshot;

use crate::app::PubrsApp;
use crate::textedit::{self, TextEdit};
use crate::view::PageXf;

/// Suggestions shown in the right-click menu.
const MENU_SUGGESTIONS: usize = 6;
/// Times a dead spell worker is started again.
const MAX_RESTARTS: u32 = 3;
/// Stories remembered by the worker before it starts over.
const MEMO_LIMIT: usize = 20_000;

// ------------------------------------------------------------ worker

/// Check the stories of a page snapshot.
pub struct PageJob {
    pub doc_id: DocumentId,
    pub page_index: usize,
    pub revision: u64,
    pub doc: Arc<Document>,
}

/// Check a whole document (the Spelling window).
pub struct DocJob {
    pub request: u64,
    pub doc_id: DocumentId,
    pub revision: u64,
    pub doc: Arc<Document>,
}

enum WordOp {
    Add(String),
    Ignore(String),
}

/// The misspellings of one story and the story text they belong to.
#[derive(Clone)]
pub struct StoryIssues {
    pub story: Arc<Story>,
    pub issues: Arc<[Issue]>,
}

pub enum SpellResult {
    Page {
        doc_id: DocumentId,
        page_index: usize,
        revision: u64,
        stories: BTreeMap<StoryId, StoryIssues>,
    },
    Document {
        request: u64,
        doc_id: DocumentId,
        revision: u64,
        report: Report,
    },
    Suggestions {
        request: u64,
        suggestions: Vec<String>,
    },
    /// Something the user should hear about (a word could not be saved).
    Notice(String),
    Failed(String),
}

#[derive(Default)]
struct Queue {
    page: Option<PageJob>,
    document: Option<DocJob>,
    suggest: Option<(u64, String, String)>,
    words: Vec<WordOp>,
    quit: bool,
    #[cfg(test)]
    panic: bool,
}

type Shared = Arc<(Mutex<Queue>, Condvar)>;

pub struct SpellWorker {
    shared: Shared,
    results: Receiver<SpellResult>,
    thread: Option<JoinHandle<()>>,
}

/// What [`SpellWorker::poll`] found.
pub struct Polled {
    pub results: Vec<SpellResult>,
    pub stopped: bool,
}

impl SpellWorker {
    /// Start the worker with the user dictionaries in `dir`; it calls
    /// `ctx.request_repaint()` after each result.
    pub fn new(ctx: egui::Context, dir: Option<PathBuf>) -> Self {
        let shared: Shared = Arc::default();
        let (tx, results) = channel();
        let thread = {
            let shared = shared.clone();
            std::thread::Builder::new()
                .name("pubrs-spell".into())
                .spawn(move || run(&shared, &tx, &ctx, dir))
                .map_err(|e| tracing::error!("cannot start the spell worker: {e}"))
                .ok()
        };
        Self {
            shared,
            results,
            thread,
        }
    }

    fn queue(&self) -> std::sync::MutexGuard<'_, Queue> {
        self.shared.0.lock().unwrap_or_else(PoisonError::into_inner)
    }

    fn notify(&self) {
        self.shared.1.notify_one();
    }

    /// Check a page; replaces a page check not yet started.
    pub fn request_page(&self, job: PageJob) {
        self.queue().page = Some(job);
        self.notify();
    }

    /// Check a document; replaces a document check not yet started.
    pub fn request_document(&self, job: DocJob) {
        self.queue().document = Some(job);
        self.notify();
    }

    /// Suggestions for a word; replaces a request not yet started.
    pub fn request_suggestions(&self, request: u64, word: &str, language: &str) {
        self.queue().suggest = Some((request, word.to_string(), language.to_string()));
        self.notify();
    }

    /// Add a word to the user dictionary (saved in the config folder).
    pub fn add_word(&self, word: &str) {
        self.queue().words.push(WordOp::Add(word.to_string()));
        self.notify();
    }

    /// Accept a word for the rest of the session.
    pub fn ignore_word(&self, word: &str) {
        self.queue().words.push(WordOp::Ignore(word.to_string()));
        self.notify();
    }

    /// Results that arrived, oldest first, and whether the thread is gone.
    pub fn poll(&self) -> Polled {
        let mut results = Vec::new();
        loop {
            match self.results.try_recv() {
                Ok(r) => results.push(r),
                Err(TryRecvError::Empty) => {
                    return Polled {
                        results,
                        stopped: false,
                    };
                }
                Err(TryRecvError::Disconnected) => {
                    return Polled {
                        results,
                        stopped: true,
                    };
                }
            }
        }
    }

    /// Make the next job panic (tests).
    #[cfg(test)]
    fn inject_panic(&self) {
        self.queue().panic = true;
        self.notify();
    }
}

impl Drop for SpellWorker {
    fn drop(&mut self) {
        self.queue().quit = true;
        self.shared.1.notify_all();
        if let Some(t) = self.thread.take()
            && t.join().is_err()
        {
            tracing::error!("the spell worker thread had panicked");
        }
    }
}

enum Job {
    Words(Vec<WordOp>),
    Suggest(u64, String, String),
    Page(PageJob),
    Document(DocJob),
    #[cfg(test)]
    Panic,
}

fn panic_message(p: &(dyn std::any::Any + Send)) -> String {
    p.downcast_ref::<&str>()
        .map(|s| (*s).to_string())
        .or_else(|| p.downcast_ref::<String>().cloned())
        .unwrap_or_else(|| "unknown panic".to_string())
}

struct State {
    checker: SpellChecker,
    /// Story → the story checked and its misspellings.
    memo: BTreeMap<StoryId, StoryIssues>,
    /// Styles the memo was made with (they decide paragraph languages).
    styles: Option<StyleSheet>,
    last_page: Option<PageJob>,
}

impl State {
    fn page(&mut self, job: &PageJob) -> SpellResult {
        let doc = &job.doc;
        if self.styles.as_ref() != Some(&doc.styles) || self.memo.len() > MEMO_LIMIT {
            self.memo.clear();
            self.styles = Some(doc.styles.clone());
        }
        let mut stories = BTreeMap::new();
        for (id, story) in &doc.stories {
            let entry = match self.memo.get(id) {
                Some(m) if Arc::ptr_eq(&m.story, story) => m.clone(),
                _ => {
                    let r = self.checker.check_story(doc, *id);
                    let m = StoryIssues {
                        story: story.clone(),
                        issues: r.issues.into(),
                    };
                    self.memo.insert(*id, m.clone());
                    m
                }
            };
            stories.insert(*id, entry);
        }
        SpellResult::Page {
            doc_id: job.doc_id,
            page_index: job.page_index,
            revision: job.revision,
            stories,
        }
    }

    fn words(&mut self, ops: Vec<WordOp>, tx: &Sender<SpellResult>) -> Option<SpellResult> {
        for op in ops {
            match op {
                WordOp::Add(w) => {
                    if let Err(e) = self.checker.add_word(&w) {
                        let _ = tx.send(SpellResult::Notice(format!("Not added: {e}")));
                    }
                }
                WordOp::Ignore(w) => {
                    self.checker.ignore_word(&w);
                }
            }
        }
        self.memo.clear();
        let last = self.last_page.take()?;
        let r = self.page(&last);
        self.last_page = Some(last);
        Some(r)
    }
}

fn run(shared: &Shared, tx: &Sender<SpellResult>, ctx: &egui::Context, dir: Option<PathBuf>) {
    let mut state = State {
        checker: SpellChecker::new(dir),
        memo: BTreeMap::new(),
        styles: None,
        last_page: None,
    };
    for p in state.checker.problems() {
        tracing::warn!("spelling: {p}");
    }
    loop {
        let job = {
            let mut q = shared.0.lock().unwrap_or_else(PoisonError::into_inner);
            loop {
                if q.quit {
                    return;
                }
                #[cfg(test)]
                if std::mem::take(&mut q.panic) {
                    break Job::Panic;
                }
                if !q.words.is_empty() {
                    break Job::Words(std::mem::take(&mut q.words));
                }
                if let Some((id, w, l)) = q.suggest.take() {
                    break Job::Suggest(id, w, l);
                }
                if let Some(p) = q.page.take() {
                    break Job::Page(p);
                }
                if let Some(d) = q.document.take() {
                    break Job::Document(d);
                }
                q = shared.1.wait(q).unwrap_or_else(PoisonError::into_inner);
            }
        };
        let caught = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| match job {
            Job::Words(ops) => state.words(ops, tx),
            Job::Suggest(request, word, language) => Some(SpellResult::Suggestions {
                request,
                suggestions: state.checker.suggest(&word, &language).unwrap_or_default(),
            }),
            Job::Page(p) => {
                let r = state.page(&p);
                state.last_page = Some(p);
                Some(r)
            }
            Job::Document(d) => Some(SpellResult::Document {
                request: d.request,
                doc_id: d.doc_id,
                revision: d.revision,
                report: state.checker.check_document(&d.doc, None),
            }),
            #[cfg(test)]
            Job::Panic => panic!("injected spell panic"),
        }));
        let out = caught.unwrap_or_else(|p| {
            state.memo.clear();
            Some(SpellResult::Failed(format!(
                "the spell checker panicked: {}",
                panic_message(&*p)
            )))
        });
        if let Some(out) = out {
            if tx.send(out).is_err() {
                return;
            }
            ctx.request_repaint();
        }
    }
}

// ------------------------------------------------------------ UI state

/// The newest page check.
pub struct PageResult {
    pub doc_id: DocumentId,
    pub page_index: usize,
    pub revision: u64,
    pub stories: BTreeMap<StoryId, StoryIssues>,
}

/// The right-click menu on a misspelt word.
#[derive(Debug, Clone)]
pub struct SpellMenu {
    pub anchor: Pos2,
    pub doc_id: DocumentId,
    pub issue: Issue,
    pub request: u64,
    pub suggestions: Option<Vec<String>>,
    /// Opened from the keyboard: focus the first item once it is there.
    pub focus: bool,
}

/// The Spelling window.
#[derive(Debug, Clone)]
pub struct SpellDialog {
    pub doc_id: DocumentId,
    /// Continue at or after this story position.
    from: Option<(StoryId, usize)>,
    /// Occurrences passed over with Next.
    skipped: HashSet<(StoryId, usize, String)>,
    /// The newest whole-document check: (revision, report).
    report: Option<(u64, Report)>,
    /// The document check asked for: (request, revision).
    requested: Option<(u64, u64)>,
    pub current: Option<Issue>,
    pub suggestions: Option<Vec<String>>,
    suggest_request: u64,
    pub choice: usize,
    /// No misspelt words are left.
    pub done: bool,
    /// The window's buttons (they take Enter themselves when focused).
    buttons: Vec<egui::Id>,
}

pub struct SpellState {
    pub worker: SpellWorker,
    dir: Option<PathBuf>,
    restarts: u32,
    /// Words ignored this session, replayed to a restarted worker.
    ignored: Vec<String>,
    /// The page check last asked for: doc, page, revision, edited story.
    requested: Option<(DocumentId, usize, u64, Option<StoryId>)>,
    pub page: Option<PageResult>,
    pub menu: Option<SpellMenu>,
    pub dialog: Option<SpellDialog>,
    next_request: u64,
    /// Squiggles drawn in the last frame (the self-test checks them).
    pub drawn: usize,
}

impl SpellState {
    /// The worker starts with the user dictionaries in `dir`.
    pub fn new(ctx: &egui::Context, dir: Option<PathBuf>) -> Self {
        Self {
            worker: SpellWorker::new(ctx.clone(), dir.clone()),
            dir,
            restarts: 0,
            ignored: Vec::new(),
            requested: None,
            page: None,
            menu: None,
            dialog: None,
            next_request: 0,
            drawn: 0,
        }
    }

    fn request_id(&mut self) -> u64 {
        self.next_request += 1;
        self.next_request
    }

    /// The misspellings of `story` if the newest check is of exactly this
    /// story text.
    pub fn issues(&self, doc_id: DocumentId, id: StoryId, story: &Arc<Story>) -> Option<&[Issue]> {
        let p = self.page.as_ref().filter(|p| p.doc_id == doc_id)?;
        let s = p.stories.get(&id)?;
        Arc::ptr_eq(&s.story, story).then_some(&*s.issues)
    }

    /// The menu or the Spelling window wants the keyboard.
    pub fn wants_keys(&self) -> bool {
        self.menu.is_some() || self.dialog.is_some()
    }

    fn ignore(&mut self, word: &str) {
        self.ignored.push(word.to_string());
        self.worker.ignore_word(word);
    }
}

/// Ask the worker to check the page being shown, when it changed.
pub fn request_page(app: &mut PubrsApp, snap: &PageSnapshot, editing: Option<StoryId>) {
    let key = (snap.doc_id, snap.page_index, snap.revision, editing);
    if app.spell.requested == Some(key) {
        return;
    }
    app.spell.requested = Some(key);
    app.spell.worker.request_page(PageJob {
        doc_id: snap.doc_id,
        page_index: snap.page_index,
        revision: snap.revision,
        doc: snap.doc.clone(),
    });
}

/// Take the worker's results (once a frame).
pub fn poll(app: &mut PubrsApp, ctx: &egui::Context) {
    let polled = app.spell.worker.poll();
    for r in polled.results {
        match r {
            SpellResult::Page {
                doc_id,
                page_index,
                revision,
                stories,
            } => {
                app.spell.page = Some(PageResult {
                    doc_id,
                    page_index,
                    revision,
                    stories,
                });
            }
            SpellResult::Document {
                request,
                doc_id,
                revision,
                report,
            } => {
                if let Some(d) = &mut app.spell.dialog
                    && d.doc_id == doc_id
                    && d.requested.map(|r| r.0) == Some(request)
                {
                    d.report = Some((revision, report));
                }
            }
            SpellResult::Suggestions {
                request,
                suggestions,
            } => {
                if let Some(m) = &mut app.spell.menu
                    && m.request == request
                {
                    m.suggestions = Some(suggestions.clone());
                }
                if let Some(d) = &mut app.spell.dialog
                    && d.suggest_request == request
                {
                    d.suggestions = Some(suggestions);
                    d.choice = 0;
                }
            }
            SpellResult::Notice(m) => app.notify(m),
            SpellResult::Failed(m) => {
                tracing::error!("{m}");
                app.spell.requested = None;
            }
        }
    }
    if polled.stopped {
        let s = &mut app.spell;
        if s.restarts < MAX_RESTARTS {
            s.restarts += 1;
            tracing::warn!("the spell worker stopped; starting it again");
            s.worker = SpellWorker::new(ctx.clone(), s.dir.clone());
            for w in &s.ignored {
                s.worker.ignore_word(w);
            }
            s.requested = None;
            if let Some(d) = &mut s.dialog {
                d.requested = None;
            }
        }
    }
}

// ------------------------------------------------------------ squiggles

/// A wavy line from `a` to `b` (screen points): two points from crest to
/// trough, two points a half wave, whole device pixels thick (HiDPI-sharp).
fn squiggle(painter: &egui::Painter, a: Pos2, b: Pos2, ppp: f32, color: Color32) {
    let len = a.distance(b);
    if len < 1.0 {
        return;
    }
    let snap = |v: f32| (v * ppp).round().max(1.0) / ppp;
    let (half, amp, width) = (snap(2.0), snap(1.0), snap(1.0));
    let d = (b - a) / len;
    let n = egui::vec2(-d.y, d.x);
    let steps = (len / half).ceil() as usize;
    let pts: Vec<Pos2> = (0..=steps)
        .map(|i| {
            let t = (i as f32 * half).min(len);
            let s = if i % 2 == 0 { -amp } else { amp };
            a + d * t + n * s
        })
        .collect();
    painter.add(egui::Shape::line(pts, Stroke::new(width, color)));
}

/// Draw the squiggles of the frames on the page being shown.
#[allow(clippy::too_many_arguments)]
pub fn draw_squiggles(
    app: &mut PubrsApp,
    painter: &egui::Painter,
    doc_id: DocumentId,
    doc: &Document,
    layout: &DocumentLayout,
    owner: Owner,
    xf: PageXf,
    ppp: f32,
    color: Color32,
) {
    let mut drawn = 0;
    for (id, story) in &doc.stories {
        let Some(issues) = app.spell.issues(doc_id, *id, story) else {
            continue;
        };
        if issues.is_empty() {
            continue;
        }
        for f in &story.frames {
            if doc.owner_of(*f) != Some(owner) {
                continue;
            }
            for (a, b) in
                textedit::underline_segments(doc, layout, *id, *f, issues, |i| (i.start, i.end))
            {
                squiggle(painter, xf.to_screen(a), xf.to_screen(b), ppp, color);
                drawn += 1;
            }
        }
    }
    app.spell.drawn = drawn;
}

// ------------------------------------------------------------ context menu

/// Open the menu for the misspelt word at `offset` of `story`, if there is
/// one. `doc` is the snapshot on screen.
pub fn open_menu(
    app: &mut PubrsApp,
    doc_id: DocumentId,
    doc: &Document,
    story: StoryId,
    offset: usize,
    anchor: Pos2,
    keyboard: bool,
) -> bool {
    let Some(s) = doc.stories.get(&story) else {
        return false;
    };
    let Some(issue) = app
        .spell
        .issues(doc_id, story, s)
        .and_then(|v| v.iter().find(|i| i.start <= offset && offset <= i.end))
        .cloned()
    else {
        return false;
    };
    let request = app.spell.request_id();
    app.spell
        .worker
        .request_suggestions(request, &issue.word, &issue.language);
    app.spell.menu = Some(SpellMenu {
        anchor,
        doc_id,
        issue,
        request,
        suggestions: None,
        focus: keyboard,
    });
    true
}

/// The text now at an issue's range is still its word.
fn still_there(app: &PubrsApp, doc_id: DocumentId, i: &Issue) -> bool {
    app.session
        .get(Some(doc_id))
        .ok()
        .and_then(|o| o.doc().story(i.story).map(Story::plain_text))
        .is_some_and(|t| {
            t.chars()
                .skip(i.start)
                .take(i.end - i.start)
                .collect::<String>()
                == i.word
        })
}

/// Replace a misspelt word (one undo step) and put the caret after it.
fn replace(app: &mut PubrsApp, doc_id: DocumentId, i: &Issue, with: &str) -> bool {
    if app.doc_id() != Some(doc_id) || !still_there(app, doc_id, i) {
        app.notify("The text changed: check the spelling again");
        return false;
    }
    let ok = app.exec(Command::ReplaceText {
        story: i.story,
        start: i.start,
        end: i.end,
        text: with.to_string(),
    });
    if ok
        && let Some(v) = app.view_mut()
        && let Some(e) = &mut v.edit
        && e.story == i.story
    {
        e.set(i.start + with.chars().count(), false);
    }
    ok
}

enum MenuAct {
    Replace(String),
    Add,
    Ignore,
}

/// Show the right-click menu while it is open.
pub fn context_menu(app: &mut PubrsApp, ui: &egui::Ui) {
    let Some(menu) = app.spell.menu.clone() else {
        return;
    };
    if app.doc_id() != Some(menu.doc_id) {
        app.spell.menu = None;
        return;
    }
    let mut open = true;
    let mut act = None;
    let mut table_act = None;
    let table_menu = app.table_menu.clone();
    let mut focused = false;
    egui::Popup::new(
        egui::Id::new("spell-menu"),
        ui.ctx().clone(),
        menu.anchor,
        ui.layer_id(),
    )
    .kind(egui::PopupKind::Menu)
    .layout(egui::Layout::top_down_justified(egui::Align::Min))
    .close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside)
    .open_bool(&mut open)
    .show(|ui| {
        ui.set_min_width(170.0);
        let mut first = None;
        match &menu.suggestions {
            None => {
                ui.weak("Looking up suggestions…");
            }
            Some(v) if v.is_empty() => {
                ui.weak("No suggestions");
            }
            Some(v) => {
                for (k, s) in v.iter().take(MENU_SUGGESTIONS).enumerate() {
                    let r = ui.button(RichText::new(s).strong());
                    crate::dialogs::mark(ui, &format!("spell-suggestion-{k}"), &r);
                    first.get_or_insert(r.id);
                    if r.clicked() {
                        act = Some(MenuAct::Replace(s.clone()));
                    }
                }
            }
        }
        ui.separator();
        let r = ui.button("Add to Dictionary");
        crate::dialogs::mark(ui, "spell-add", &r);
        if menu.suggestions.as_ref().is_some_and(Vec::is_empty) {
            first.get_or_insert(r.id);
        }
        if r.clicked() {
            act = Some(MenuAct::Add);
        }
        let r = ui.button("Ignore");
        crate::dialogs::mark(ui, "spell-ignore", &r);
        if r.clicked() {
            act = Some(MenuAct::Ignore);
        }
        // A right-click in a table cell lists the table items after these.
        if let Some(tm) = &table_menu {
            ui.separator();
            table_act = crate::tables::items_ui(ui, tm);
        }
        if menu.focus
            && let Some(id) = first
        {
            ui.memory_mut(|m| m.request_focus(id));
            focused = true;
        }
    });
    let word = menu.issue.word.clone();
    match act {
        Some(MenuAct::Replace(s)) => {
            replace(app, menu.doc_id, &menu.issue, &s);
            open = false;
        }
        Some(MenuAct::Add) => {
            app.spell.worker.add_word(&word);
            app.notify(format!("Added \u{201c}{word}\u{201d} to the dictionary"));
            open = false;
        }
        Some(MenuAct::Ignore) => {
            app.spell.ignore(&word);
            open = false;
        }
        None => {}
    }
    if let (Some(tm), Some(a)) = (table_menu, table_act) {
        crate::tables::apply(app, &tm, a);
        open = false;
    }
    if !open {
        app.table_menu = None;
    }
    match (&mut app.spell.menu, open) {
        (Some(m), true) => m.focus &= !focused,
        (m, false) => *m = None,
        _ => {}
    }
}

// ------------------------------------------------------------ Spelling window

/// Review → Spelling…: check from the caret (or the start) to the end and
/// round again.
pub fn open_dialog(app: &mut PubrsApp) {
    let Some(doc_id) = app.doc_id() else { return };
    if app
        .spell
        .dialog
        .as_ref()
        .is_some_and(|d| d.doc_id == doc_id)
    {
        return;
    }
    let from = app
        .view()
        .and_then(|v| v.edit.as_ref())
        .map(|e| (e.story, e.range().0));
    app.spell.menu = None;
    app.spell.dialog = Some(SpellDialog {
        doc_id,
        from,
        skipped: HashSet::new(),
        report: None,
        requested: None,
        current: None,
        suggestions: None,
        suggest_request: 0,
        choice: 0,
        done: false,
        buttons: Vec::new(),
    });
}

/// Story order for comparing positions.
fn position(order: &[StoryId], story: StoryId, offset: usize) -> (usize, usize) {
    (
        order.iter().position(|s| *s == story).unwrap_or(usize::MAX),
        offset,
    )
}

/// Select an issue in the text: its page, its frame, the word.
fn select(app: &mut PubrsApp, i: &Issue) {
    let Some(doc_id) = app.doc_id() else { return };
    let Ok(layout) = app.session.layout(Some(doc_id)) else {
        return;
    };
    let Some(doc) = app.doc() else { return };
    let Some(story) = doc.story(i.story) else {
        return;
    };
    let sl = layout.stories.get(&i.story);
    let frame = story.frames.iter().copied().find(|f| {
        sl.and_then(|s| s.frames.get(f))
            .is_some_and(|ft| ft.chars.contains(&i.start))
    });
    let Some(frame) = frame else { return };
    let page = match doc.owner_of(frame) {
        Some(Owner::Page(p)) => doc.pages.iter().position(|q| q.id == p),
        _ => None,
    };
    let rect = doc.frame(frame).map(|f| f.rect);
    if let Some(v) = app.view_mut() {
        if let Some(page) = page {
            v.page = page;
        }
        v.selection = vec![frame];
        v.edit = Some(TextEdit {
            frame,
            story: i.story,
            caret: i.end,
            anchor: i.start,
            ..TextEdit::new(frame, i.story, i.end)
        });
        v.scroll_to = rect;
    }
}

/// Keep the window's check current and pick the next issue.
fn advance(app: &mut PubrsApp) {
    let Some(doc_id) = app.spell.dialog.as_ref().map(|d| d.doc_id) else {
        return;
    };
    let Ok(open) = app.session.get(Some(doc_id)) else {
        app.spell.dialog = None;
        return;
    };
    let rev = open.revision();
    let d = app.spell.dialog.as_ref().expect("checked above");
    let current = d.report.as_ref().is_some_and(|(r, _)| *r == rev);
    if !current {
        if d.requested.map(|r| r.1) != Some(rev) {
            let doc = Arc::new(open.doc().clone());
            let request = app.spell.request_id();
            app.spell.worker.request_document(DocJob {
                request,
                doc_id,
                revision: rev,
                doc,
            });
            if let Some(d) = &mut app.spell.dialog {
                d.requested = Some((request, rev));
            }
        }
        return;
    }
    if d.current.is_some() || d.done {
        return;
    }
    let order = reading_order(open.doc());
    let d = app.spell.dialog.as_mut().expect("checked above");
    let Some((_, report)) = &d.report else { return };
    let from = d.from.map_or((0, 0), |(s, o)| position(&order, s, o));
    let usable = |i: &&Issue| !d.skipped.contains(&(i.story, i.start, i.word.clone()));
    let next = report
        .issues
        .iter()
        .filter(usable)
        .find(|i| position(&order, i.story, i.start) >= from)
        .or_else(|| report.issues.iter().find(usable))
        .cloned();
    match next {
        Some(i) => {
            d.current = Some(i.clone());
            d.suggestions = None;
            d.choice = 0;
            let request = app.spell.request_id();
            app.spell
                .worker
                .request_suggestions(request, &i.word, &i.language);
            if let Some(d) = &mut app.spell.dialog {
                d.suggest_request = request;
            }
            select(app, &i);
        }
        None => d.done = true,
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum DialogAct {
    Change,
    ChangeAll,
    Ignore,
    Add,
    Next,
    Close,
}

/// Move on after an action: check again from `from`.
fn moved(app: &mut PubrsApp, from: (StoryId, usize), recheck: bool) {
    if let Some(d) = &mut app.spell.dialog {
        d.from = Some(from);
        d.current = None;
        d.suggestions = None;
        if recheck {
            d.report = None;
            d.requested = None;
        }
    }
}

fn dialog_act(app: &mut PubrsApp, act: DialogAct) {
    let Some(d) = app.spell.dialog.clone() else {
        return;
    };
    if act == DialogAct::Close {
        app.spell.dialog = None;
        return;
    }
    let Some(i) = d.current.clone() else { return };
    let chosen = d
        .suggestions
        .as_ref()
        .and_then(|s| s.get(d.choice))
        .cloned();
    match act {
        DialogAct::Change => {
            let Some(with) = chosen else { return };
            if replace(app, d.doc_id, &i, &with) {
                moved(app, (i.story, i.start + with.chars().count()), false);
            } else {
                moved(app, (i.story, i.start), true);
            }
        }
        DialogAct::ChangeAll => {
            let Some(with) = chosen else { return };
            let Some((_, report)) = &d.report else { return };
            let mut all: Vec<&Issue> = report.issues.iter().filter(|j| j.word == i.word).collect();
            all.sort_by_key(|j| std::cmp::Reverse((j.story, j.start)));
            let commands = all
                .iter()
                .map(|j| Command::ReplaceText {
                    story: j.story,
                    start: j.start,
                    end: j.end,
                    text: with.clone(),
                })
                .collect();
            if still_there(app, d.doc_id, &i) {
                app.exec(Command::Batch {
                    label: "change all".into(),
                    commands,
                });
            }
            moved(app, (i.story, i.start), true);
        }
        DialogAct::Ignore => {
            app.spell.ignore(&i.word);
            moved(app, (i.story, i.start), true);
        }
        DialogAct::Add => {
            app.spell.worker.add_word(&i.word);
            moved(app, (i.story, i.start), true);
        }
        DialogAct::Next => {
            if let Some(d) = &mut app.spell.dialog {
                d.skipped.insert((i.story, i.start, i.word.clone()));
            }
            moved(app, (i.story, i.end), false);
        }
        DialogAct::Close => {}
    }
}

/// The window's keys, read before the canvas: Enter changes (unless a
/// button has focus), Up/Down choose a suggestion, Escape closes.
pub fn dialog_keys(app: &mut PubrsApp, ctx: &egui::Context) {
    let Some(d) = &app.spell.dialog else { return };
    if ctx.text_edit_focused() {
        return;
    }
    let on_button = ctx
        .memory(|m| m.focused())
        .is_some_and(|f| d.buttons.contains(&f));
    let n = d.suggestions.as_ref().map_or(0, Vec::len);
    let (enter, esc, up, down) = ctx.input_mut(|i| {
        (
            !on_button && i.consume_key(Modifiers::NONE, Key::Enter),
            i.consume_key(Modifiers::NONE, Key::Escape),
            i.consume_key(Modifiers::NONE, Key::ArrowUp),
            i.consume_key(Modifiers::NONE, Key::ArrowDown),
        )
    });
    if let Some(d) = &mut app.spell.dialog {
        if up {
            d.choice = d.choice.saturating_sub(1);
        }
        if down && d.choice + 1 < n {
            d.choice += 1;
        }
    }
    if esc {
        dialog_act(app, DialogAct::Close);
    } else if enter {
        let done = app.spell.dialog.as_ref().is_some_and(|d| d.done);
        dialog_act(
            app,
            if done {
                DialogAct::Close
            } else {
                DialogAct::Change
            },
        );
    }
}

/// The Spelling window (non-modal, like Find and Replace).
pub fn dialog_window(app: &mut PubrsApp, ctx: &egui::Context) {
    if app.spell.dialog.is_none() {
        return;
    }
    if app.spell.dialog.as_ref().map(|d| d.doc_id) != app.doc_id() {
        app.spell.dialog = None;
        return;
    }
    advance(app);
    let Some(mut d) = app.spell.dialog.clone() else {
        return;
    };
    let mut act = None;
    let mut buttons = Vec::new();
    egui::Window::new("Spelling")
        .collapsible(false)
        .resizable(false)
        .anchor(egui::Align2::RIGHT_TOP, [-24.0, 96.0])
        .show(ctx, |ui| {
            ui.set_min_width(300.0);
            let mut button = |ui: &mut egui::Ui, label: &str, name: &str, enabled: bool| {
                let r = ui.add_enabled(enabled, egui::Button::new(label));
                crate::dialogs::mark(ui, name, &r);
                buttons.push(r.id);
                r.clicked()
            };
            if d.done {
                ui.label("The spelling check is complete.");
                if button(ui, "Close", "spell-close", true) {
                    act = Some(DialogAct::Close);
                }
                return;
            }
            let Some(i) = d.current.clone() else {
                ui.weak("Checking…");
                if button(ui, "Close", "spell-close", true) {
                    act = Some(DialogAct::Close);
                }
                return;
            };
            ui.label("Not in dictionary:");
            ui.label(RichText::new(&i.word).strong().size(16.0));
            ui.weak(format!("Language: {}", i.language));
            ui.add_space(4.0);
            ui.label("Suggestions:");
            let has = d.suggestions.as_ref().is_some_and(|s| !s.is_empty());
            egui::Frame::group(ui.style()).show(ui, |ui| {
                ui.set_min_height(90.0);
                ui.set_min_width(280.0);
                match &d.suggestions {
                    None => {
                        ui.weak("Looking up…");
                    }
                    Some(s) if s.is_empty() => {
                        ui.weak("(no suggestions)");
                    }
                    Some(s) => {
                        for (k, w) in s.iter().take(8).enumerate() {
                            let r = ui.selectable_label(d.choice == k, w);
                            crate::dialogs::mark(ui, &format!("spell-choice-{k}"), &r);
                            if r.clicked() {
                                d.choice = k;
                            }
                            if r.double_clicked() {
                                act = Some(DialogAct::Change);
                            }
                        }
                    }
                }
            });
            ui.add_space(4.0);
            ui.horizontal(|ui| {
                if button(ui, "Change", "spell-change", has) {
                    act = Some(DialogAct::Change);
                }
                if button(ui, "Change All", "spell-change-all", has) {
                    act = Some(DialogAct::ChangeAll);
                }
            });
            ui.horizontal(|ui| {
                if button(ui, "Ignore", "spell-ignore-word", true) {
                    act = Some(DialogAct::Ignore);
                }
                if button(ui, "Add", "spell-add-word", true) {
                    act = Some(DialogAct::Add);
                }
                if button(ui, "Next", "spell-next", true) {
                    act = Some(DialogAct::Next);
                }
                if button(ui, "Close", "spell-close", true) {
                    act = Some(DialogAct::Close);
                }
            });
            ui.weak("Enter: Change · Up/Down: choose · Esc: close");
        });
    d.buttons = buttons;
    if let Some(cur) = &mut app.spell.dialog {
        cur.choice = d.choice;
        cur.buttons = d.buttons;
    }
    if let Some(a) = act {
        dialog_act(app, a);
    }
}

#[cfg(test)]
mod tests {
    use std::time::{Duration, Instant};

    use pubrs_api::core::{DocBuilder, PageSetup, Rect, SeqIds};
    use pubrs_api::{Session, SessionConfig};

    use super::*;

    fn wait(w: &SpellWorker) -> SpellResult {
        let t0 = Instant::now();
        loop {
            let mut p = w.poll();
            assert!(!p.stopped, "the spell worker died");
            if let Some(r) = p.results.pop() {
                return r;
            }
            assert!(t0.elapsed() < Duration::from_secs(60), "no result");
            std::thread::sleep(Duration::from_millis(2));
        }
    }

    fn page_job(s: &mut Session, id: DocumentId) -> PageJob {
        let snap = s.page_snapshot(Some(id), 0, &[], None).expect("snapshot");
        PageJob {
            doc_id: id,
            page_index: 0,
            revision: snap.revision,
            doc: snap.doc,
        }
    }

    /// A panic is reported and the worker keeps checking; an ignored word
    /// re-checks the last page without it.
    #[test]
    fn the_worker_survives_a_panic_and_applies_ignored_words() {
        let w = SpellWorker::new(egui::Context::default(), None);
        w.inject_panic();
        match wait(&w) {
            SpellResult::Failed(m) => assert!(m.contains("injected spell panic"), "{m}"),
            _ => panic!("expected a reported failure"),
        }
        let mut b = DocBuilder::new(SeqIds::new(28), PageSetup::default());
        let p = b.page(0);
        b.text(
            p,
            Rect::new(72.0, 72.0, 300.0, 100.0),
            "Teh oven is hot, zorbly hot.",
        )
        .expect("text");
        let mut s = Session::new(SessionConfig {
            software_rendering: true,
            ..Default::default()
        });
        let id = s.insert(b.finish(), None);
        w.request_page(page_job(&mut s, id));
        let words = |r: SpellResult| -> Vec<String> {
            match r {
                SpellResult::Page { stories, .. } => stories
                    .values()
                    .flat_map(|s| s.issues.iter().map(|i| i.word.clone()))
                    .collect(),
                _ => panic!("expected a page result"),
            }
        };
        assert_eq!(words(wait(&w)), ["Teh", "zorbly"]);
        w.ignore_word("zorbly");
        assert_eq!(words(wait(&w)), ["Teh"]);
        w.request_suggestions(7, "Teh", "en-US");
        match wait(&w) {
            SpellResult::Suggestions {
                request,
                suggestions,
            } => {
                assert_eq!(request, 7);
                assert!(suggestions.iter().any(|s| s.eq_ignore_ascii_case("the")));
            }
            _ => panic!("expected suggestions"),
        }
    }
}
