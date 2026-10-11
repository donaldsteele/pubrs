//! The Design Checker (Review → Design Checker, ADR 0027): a panel listing
//! print-readiness issues by page, kept up to date as the user edits.
//!
//! Cost model (LD3): nothing happens on the keystroke. The panel looks at the
//! document revision once per frame; a change starts a 400 ms timer (and asks
//! for a repaint after it), and only when the document has been quiet for that
//! long are the pages whose content changed checked again. A page's content is
//! its frames and their stories, compared by `Arc` pointer first (a command
//! replaces only the frames and stories it touches, ADR 0012), so a one-letter
//! edit on a 20-page document checks one page. The story-level warnings
//! (overflow, fonts, glyphs, hyphenation) come from the cached layout and are
//! always rebuilt. The work runs on the UI thread because it needs the
//! session's layout; the panel is not asked for when it is closed.

use std::collections::BTreeMap;
use std::sync::Arc;
use std::time::{Duration, Instant};

use eframe::egui::{self, Color32, RichText};
use pubrs_api::Session;
use pubrs_api::core::{
    Asset, AssetId, Document, DocumentId, Frame, FrameContent, Owner, PageId, PageSetup, Story,
    StyleSheet, Swatch, Warning, WarningCode,
};
use pubrs_api::tools::query::ValidateIn;

use crate::app::PubrsApp;
use crate::dialogs::mark;

/// How long the document must be quiet before the panel is refreshed.
pub const DEBOUNCE: Duration = Duration::from_millis(400);

/// What a commercial printer's preflight rejects: the export asks about
/// these before it saves.
pub const BLOCKING: [WarningCode; 3] = [
    WarningCode::MissingFont,
    WarningCode::RgbInCmykJob,
    WarningCode::LowResolutionImage,
];

/// The frames of a page and the stories they show.
struct PageKey {
    frames: Vec<Arc<Frame>>,
    stories: Vec<Arc<Story>>,
}

impl PageKey {
    fn of(doc: &Document, page: PageId) -> Self {
        let mut frames = Vec::new();
        let mut stories = Vec::new();
        // The master's frames lie beneath the page's (contrast backdrops).
        let master = doc
            .pages
            .iter()
            .find(|p| p.id == page)
            .and_then(|p| p.master);
        let ids = master
            .into_iter()
            .flat_map(|m| doc.all_frames_of(Owner::Master(m)))
            .chain(doc.all_frames_of(Owner::Page(page)));
        for id in ids {
            let Some(f) = doc.frames.get(&id) else {
                continue;
            };
            frames.push(f.clone());
            match &f.content {
                FrameContent::Text(t) => stories.extend(doc.stories.get(&t.story).cloned()),
                FrameContent::Table(t) => {
                    for c in &t.cells {
                        stories.extend(doc.stories.get(&c.story).cloned());
                    }
                }
                _ => {}
            }
        }
        Self { frames, stories }
    }

    fn same(&self, other: &Self) -> bool {
        same_arcs(&self.frames, &other.frames) && same_arcs(&self.stories, &other.stories)
    }
}

fn same_arcs<T: PartialEq>(a: &[Arc<T>], b: &[Arc<T>]) -> bool {
    a.len() == b.len() && a.iter().zip(b).all(|(x, y)| Arc::ptr_eq(x, y) || x == y)
}

/// What every page's warnings depend on besides its own frames.
struct Env {
    setup: PageSetup,
    swatches: Arc<Vec<Swatch>>,
    styles: StyleSheet,
    assets: BTreeMap<AssetId, Asset>,
}

impl Env {
    fn of(doc: &Document) -> Self {
        Self {
            setup: doc.setup.clone(),
            swatches: doc.swatches.clone(),
            styles: doc.styles.clone(),
            assets: doc.assets.clone(),
        }
    }

    fn same(&self, o: &Env) -> bool {
        self.setup == o.setup
            && (Arc::ptr_eq(&self.swatches, &o.swatches) || self.swatches == o.swatches)
            && self.styles == o.styles
            && self.assets == o.assets
    }
}

struct PageEntry {
    id: PageId,
    key: PageKey,
    warnings: Vec<Warning>,
}

struct Cache {
    doc_id: DocumentId,
    commercial: bool,
    env: Env,
    pages: Vec<PageEntry>,
    /// Story-level warnings and undecodable images.
    document: Vec<Warning>,
}

/// The panel's state.
pub struct CheckerState {
    pub open: bool,
    /// The "Commercial printing checks" box.
    pub commercial: bool,
    /// Quiet time before a refresh (400 ms; the self-test may shorten it).
    pub debounce: Duration,
    /// Document and revision the cache was last compared with.
    seen: Option<(DocumentId, u64)>,
    changed_at: Option<Instant>,
    /// Refresh at the next poll without waiting (just opened, or the box
    /// was toggled).
    force: bool,
    /// A forced refresh was asked for and the frame that will run it has not
    /// been requested yet (see [`CheckerState::take_wake`]).
    wake: bool,
    cache: Option<Cache>,
    pub error: Option<String>,
    /// After a failed refresh: the time to try again.
    retry_at: Option<Instant>,
    /// Pages checked by the last refresh, and in all (for tests and logs).
    pub last_pages_checked: usize,
    pub pages_checked: usize,
    pub refreshes: usize,
}

impl Default for CheckerState {
    fn default() -> Self {
        Self {
            open: false,
            commercial: false,
            debounce: DEBOUNCE,
            seen: None,
            changed_at: None,
            force: false,
            wake: false,
            cache: None,
            error: None,
            retry_at: None,
            last_pages_checked: 0,
            pages_checked: 0,
            refreshes: 0,
        }
    }
}

impl CheckerState {
    /// Show the panel; the next poll checks without waiting.
    pub fn show(&mut self) {
        self.open = true;
        self.force_refresh();
    }

    /// Refresh at the next poll without waiting, and ask for one frame to do it.
    pub fn force_refresh(&mut self) {
        self.force = true;
        self.wake = true;
    }

    /// True once per [`CheckerState::force_refresh`]: the caller requests a repaint.
    pub fn take_wake(&mut self) -> bool {
        std::mem::take(&mut self.wake)
    }

    /// Whether a change is waiting for the debounce.
    pub fn pending(&self) -> bool {
        self.changed_at.is_some()
    }

    /// Every issue: the pages in order, then the document's.
    pub fn issues(&self) -> Vec<&Warning> {
        match &self.cache {
            Some(c) => c
                .pages
                .iter()
                .flat_map(|p| p.warnings.iter())
                .chain(c.document.iter())
                .collect(),
            None => Vec::new(),
        }
    }

    pub fn has_results(&self) -> bool {
        self.cache.is_some()
    }

    /// Look at the document and, if it has been quiet for the debounce
    /// time, check what changed. Returns how long to wait for the next
    /// look when a change is pending.
    pub fn poll(
        &mut self,
        session: &mut Session,
        doc: Option<DocumentId>,
        now: Instant,
    ) -> Option<Duration> {
        if !self.open {
            return None;
        }
        let Some(id) = doc else {
            self.cache = None;
            self.seen = None;
            self.changed_at = None;
            return None;
        };
        let Ok(rev) = session.get(Some(id)).map(|o| o.revision()) else {
            return None;
        };
        let fresh = self.cache.as_ref().is_none_or(|c| c.doc_id != id);
        if let Some(t) = self.retry_at {
            if now < t {
                return Some(t - now);
            }
            self.retry_at = None;
        }
        if fresh || self.force {
            self.force = false;
            if self.refresh(session, id) {
                self.seen = Some((id, rev));
                self.changed_at = None;
            } else {
                // Try again after the debounce, without waiting for an edit.
                self.retry_at = Some(now + self.debounce);
                return Some(self.debounce);
            }
            return None;
        }
        if self.seen != Some((id, rev)) {
            self.seen = Some((id, rev));
            self.changed_at = Some(now);
        }
        let t = self.changed_at?;
        let waited = now.saturating_duration_since(t);
        if waited >= self.debounce {
            if self.refresh(session, id) {
                self.changed_at = None;
                None
            } else {
                self.changed_at = None;
                self.seen = None;
                self.retry_at = Some(now + self.debounce);
                Some(self.debounce)
            }
        } else {
            Some(self.debounce - waited)
        }
    }

    /// Check the pages that changed since the last time (all of them for a
    /// new document, a new setup, other colours, fonts or styles, or the
    /// commercial box), and the document's story-level warnings.
    fn refresh(&mut self, session: &mut Session, id: DocumentId) -> bool {
        let (keys, env) = match session.get(Some(id)) {
            Ok(o) => {
                let doc = o.doc();
                (
                    doc.pages
                        .iter()
                        .map(|p| (p.id, PageKey::of(doc, p.id)))
                        .collect::<Vec<_>>(),
                    Env::of(doc),
                )
            }
            Err(_) => return false,
        };
        let old = self.cache.take();
        let reuse = old
            .as_ref()
            .filter(|c| c.doc_id == id && c.commercial == self.commercial && c.env.same(&env));
        let dirty: Vec<PageId> = keys
            .iter()
            .filter(|(pid, key)| {
                reuse
                    .and_then(|c| c.pages.iter().find(|e| e.id == *pid))
                    .is_none_or(|e| !e.key.same(key))
            })
            .map(|(pid, _)| *pid)
            .collect();
        let parts = match session.validate_parts(Some(id), self.commercial, Some(&dirty)) {
            Ok(p) => p,
            Err(e) => {
                self.error = Some(format!("{} ({})", e.message, e.hint));
                self.cache = old;
                return false;
            }
        };
        self.error = None;
        let mut fresh: BTreeMap<PageId, Vec<Warning>> = parts.pages.into_iter().collect();
        let mut old_pages: BTreeMap<PageId, Vec<Warning>> = old
            .filter(|c| c.doc_id == id && c.commercial == self.commercial && c.env.same(&env))
            .map(|c| c.pages.into_iter().map(|e| (e.id, e.warnings)).collect())
            .unwrap_or_default();
        let pages = keys
            .into_iter()
            .map(|(pid, key)| PageEntry {
                id: pid,
                warnings: fresh
                    .remove(&pid)
                    .or_else(|| old_pages.remove(&pid))
                    .unwrap_or_default(),
                key,
            })
            .collect();
        self.last_pages_checked = dirty.len();
        self.pages_checked += dirty.len();
        self.refreshes += 1;
        self.cache = Some(Cache {
            doc_id: id,
            commercial: self.commercial,
            env,
            pages,
            document: parts.document,
        });
        true
    }
}

/// The name a code has in the JSON and in the self-test's control names.
pub fn code_name(code: WarningCode) -> String {
    serde_json::to_value(code)
        .ok()
        .and_then(|v| v.as_str().map(str::to_owned))
        .unwrap_or_default()
}

/// Problems the printer will notice, as opposed to things worth a look.
fn is_error(code: WarningCode) -> bool {
    matches!(
        code,
        WarningCode::Overflow
            | WarningCode::MissingFont
            | WarningCode::MissingGlyphs
            | WarningCode::LowResolutionImage
            | WarningCode::OffPage
            | WarningCode::BeyondBleed
            | WarningCode::RgbInCmykJob
    )
}

/// Go to an issue's object: its page, selected and scrolled into view.
/// An issue with no object only changes the page, if it has one.
pub fn goto(app: &mut PubrsApp, w: &Warning) {
    // The list can be a debounce behind the document.
    if let Some(f) = w.frame
        && app.doc().is_some_and(|d| d.frame(f).is_none())
    {
        app.notify("That object no longer exists");
        app.checker.force_refresh();
        return;
    }
    let page = w.page.and_then(|p| app.doc().and_then(|d| d.page_index(p)));
    let rect = w
        .frame
        .and_then(|f| app.doc().and_then(|d| d.frame(f)).map(|f| f.rect));
    let Some(v) = app.view_mut() else { return };
    if let Some(p) = page {
        v.page = p;
    }
    if let Some(f) = w.frame {
        v.selection = vec![f];
        v.edit = None;
        v.table_sel = None;
        v.scroll_to = rect;
    }
}

/// Run the checker once, now, for a commercial job and keep what blocks
/// the export (missing fonts, RGB colour, low-resolution pictures).
/// `None` if the check itself failed (the error is shown): the export must
/// not go on as if the document were clean.
pub fn blocking_issues(app: &mut PubrsApp) -> Option<Vec<Warning>> {
    let id = app.doc_id()?;
    let r = app.session.validate(ValidateIn {
        doc_id: Some(id),
        commercial: true,
    });
    app.report(r).map(|v| {
        v.warnings
            .into_iter()
            .filter(|w| BLOCKING.contains(&w.code))
            .collect()
    })
}

/// "Fix first" in the export dialog: the panel opens with the commercial
/// checks on, and the first blocking issue is selected.
pub fn fix_first(app: &mut PubrsApp, first: Option<&Warning>) {
    app.checker.commercial = true;
    app.checker.show();
    if let Some(w) = first {
        goto(app, w);
    }
}

/// Look at the document once per frame (see the module comment).
pub fn tick(app: &mut PubrsApp, ctx: &egui::Context) {
    let doc = app.doc_id();
    match app.checker.tick(&mut app.session, doc, Instant::now()) {
        Repaint::No => {}
        Repaint::Now => ctx.request_repaint(),
        Repaint::After(d) => ctx.request_repaint_after(d),
    }
}

/// What the frame should ask egui for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Repaint {
    /// Nothing: an idle panel does not redraw.
    No,
    /// A refresh just ran: draw its result.
    Now,
    /// A change is waiting for the debounce.
    After(Duration),
}

impl CheckerState {
    /// [`CheckerState::poll`], saying what to repaint: only the frame a
    /// refresh ran on, and the wake-up for a pending change.
    pub fn tick(
        &mut self,
        session: &mut Session,
        doc: Option<DocumentId>,
        now: Instant,
    ) -> Repaint {
        let before = self.refreshes;
        match self.poll(session, doc, now) {
            Some(d) => Repaint::After(d),
            None if self.refreshes != before => Repaint::Now,
            None => Repaint::No,
        }
    }
}

/// The bottom panel (between the pages and the properties panel, so the page
/// keeps its width; the list is wide rather than tall).
pub fn panel(app: &mut PubrsApp, ui: &mut egui::Ui) {
    if !app.checker.open || app.view().is_none() {
        return;
    }
    let mut go: Option<Warning> = None;
    let mut close = false;
    let mut toggled = false;
    egui::Panel::bottom("checker")
        .resizable(true)
        .default_size(200.0)
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.heading("Design Checker");
                let (n, errors) = {
                    let issues = app.checker.issues();
                    (
                        issues.len(),
                        issues.iter().filter(|w| is_error(w.code)).count(),
                    )
                };
                let text = if !app.checker.has_results() {
                    "Checking…".to_string()
                } else if n == 0 {
                    "No problems found. Ready to print.".to_string()
                } else {
                    format!(
                        "{n} issue{} ({errors} problem{}, {} to check)",
                        if n == 1 { "" } else { "s" },
                        if errors == 1 { "" } else { "s" },
                        n - errors
                    )
                };
                let colour = if n == 0 && app.checker.has_results() {
                    Color32::DARK_GREEN
                } else {
                    ui.visuals().text_color()
                };
                let l = ui.label(RichText::new(text).color(colour));
                mark(ui, "checker-summary", &l);
                if app.checker.pending() {
                    ui.weak("updating…");
                }
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    let b = ui.button("Close").on_hover_text("Hide the Design Checker");
                    mark(ui, "checker-close", &b);
                    close = b.clicked();
                    let r = ui
                        .checkbox(&mut app.checker.commercial, "Commercial printing checks")
                        .on_hover_text(
                            "Also flag RGB colours, which a CMYK press job should not use. \
                             Turns off the desktop-printer margin check.",
                        );
                    mark(ui, "checker-commercial", &r);
                    toggled = r.changed();
                });
            });
            if let Some(e) = &app.checker.error {
                ui.colored_label(Color32::from_rgb(180, 30, 30), e);
            }
            ui.separator();
            let doc = app.doc();
            let mut groups: BTreeMap<Option<usize>, Vec<&Warning>> = BTreeMap::new();
            for w in app.checker.issues() {
                let page = w
                    .page
                    .and_then(|p| doc.and_then(|d| d.page_index(p)))
                    .map(|i| i + 1);
                groups.entry(page.map(|p| p - 1)).or_default().push(w);
            }
            egui::ScrollArea::vertical()
                .auto_shrink([false, false])
                .show(ui, |ui| {
                    // Pages first, in order; "Publication" (no page) last.
                    let order = groups
                        .keys()
                        .copied()
                        .filter(Option::is_some)
                        .chain(groups.contains_key(&None).then_some(None));
                    for key in order {
                        let list = &groups[&key];
                        let title = match key {
                            Some(i) => format!("Page {} ({})", i + 1, list.len()),
                            None => format!("Publication ({})", list.len()),
                        };
                        egui::CollapsingHeader::new(RichText::new(title).strong())
                            .id_salt(("checker-group", key))
                            .default_open(true)
                            .show(ui, |ui| {
                                for w in list {
                                    if issue_row(ui, w) {
                                        go = Some((*w).clone());
                                    }
                                }
                            });
                    }
                });
        });
    if toggled {
        app.checker.force_refresh();
    }
    if close {
        app.checker.open = false;
    }
    if let Some(w) = go {
        goto(app, &w);
    }
}

/// One issue; true when it was clicked.
fn issue_row(ui: &mut egui::Ui, w: &Warning) -> bool {
    let mut clicked = false;
    ui.horizontal_top(|ui| {
        let (label, colour) = if is_error(w.code) {
            ("⚠ Problem", Color32::from_rgb(180, 30, 30))
        } else {
            ("⚠ Check", Color32::from_rgb(180, 83, 9))
        };
        ui.label(RichText::new(label).color(colour));
        ui.vertical(|ui| {
            let r = ui
                .add(
                    egui::Label::new(RichText::new(&w.message).strong())
                        .sense(egui::Sense::click())
                        .wrap(),
                )
                .on_hover_text(if w.frame.is_some() {
                    "Click to go to the object"
                } else {
                    "Click to go to the page"
                });
            mark(ui, &format!("issue-{}", code_name(w.code)), &r);
            clicked = r.clicked();
            ui.add(egui::Label::new(RichText::new(&w.hint).small().weak()).wrap());
        });
    });
    ui.add_space(2.0);
    clicked
}

#[cfg(test)]
mod tests {
    use super::*;
    use pubrs_api::SessionConfig;
    use pubrs_api::core::{Command, DocBuilder, PageSetup, Rect, SeqIds};

    /// A document of `pages` pages, each with a text frame; the first
    /// page's story id is returned for edits.
    fn doc(pages: usize) -> (Session, DocumentId, pubrs_api::core::StoryId) {
        let mut b = DocBuilder::new(SeqIds::new(5), PageSetup::default());
        let mut first = None;
        for i in 0..pages {
            let p = if i == 0 {
                b.page(0)
            } else {
                b.add_page().unwrap()
            };
            let f = b
                .text(p, Rect::new(72.0, 72.0, 300.0, 60.0), &format!("Page {i}"))
                .unwrap();
            first.get_or_insert(b.story_of(f));
        }
        let mut s = Session::new(SessionConfig {
            software_rendering: true,
            ..Default::default()
        });
        let id = s.insert(b.finish(), None);
        (s, id, first.unwrap())
    }

    #[test]
    fn a_one_letter_edit_checks_one_page_after_the_debounce() {
        let (mut s, id, story) = doc(20);
        let mut c = CheckerState::default();
        let t0 = Instant::now();
        // Closed: no work at all.
        assert_eq!(c.poll(&mut s, Some(id), t0), None);
        assert_eq!(c.refreshes, 0);
        c.show();
        assert_eq!(c.poll(&mut s, Some(id), t0), None);
        assert_eq!((c.refreshes, c.last_pages_checked), (1, 20));
        assert!(c.issues().is_empty());

        // Nothing changed: nothing is checked.
        assert_eq!(c.poll(&mut s, Some(id), t0 + Duration::from_secs(5)), None);
        assert_eq!(c.refreshes, 1);

        // One letter: the timer starts, nothing is checked until it ends.
        s.exec(
            Some(id),
            Command::InsertText {
                story,
                at: 0,
                text: "x".into(),
                format: None,
            },
        )
        .unwrap();
        let t1 = t0 + Duration::from_secs(10);
        let wait = c.poll(&mut s, Some(id), t1).expect("a wait");
        assert!(wait <= DEBOUNCE && c.pending());
        assert_eq!(c.refreshes, 1);
        // A second edit inside the window restarts the timer.
        s.exec(
            Some(id),
            Command::InsertText {
                story,
                at: 1,
                text: "y".into(),
                format: None,
            },
        )
        .unwrap();
        let t2 = t1 + Duration::from_millis(300);
        assert!(c.poll(&mut s, Some(id), t2).is_some());
        assert_eq!(c.refreshes, 1);
        // Quiet for the debounce time: one page is checked, not 20.
        let t3 = t2 + DEBOUNCE + Duration::from_millis(1);
        assert_eq!(c.poll(&mut s, Some(id), t3), None);
        assert_eq!((c.refreshes, c.last_pages_checked), (2, 1));
        assert_eq!(c.pages_checked, 21);
        assert!(!c.pending());
    }

    #[test]
    fn an_idle_open_panel_asks_for_no_repaints() {
        let (mut s, id, story) = doc(3);
        let mut c = CheckerState::default();
        let t0 = Instant::now();
        c.show();
        // The frame the first check runs on draws its result, once.
        assert_eq!(c.tick(&mut s, Some(id), t0), Repaint::Now);
        let mut asked = 0;
        for i in 1..50u64 {
            if c.tick(&mut s, Some(id), t0 + Duration::from_millis(50 * i)) != Repaint::No {
                asked += 1;
            }
        }
        assert_eq!(asked, 0, "idle frames must not request repaints");
        // An edit: one wake-up is asked for, then the refresh frame, then silence.
        s.exec(
            Some(id),
            Command::InsertText {
                story,
                at: 0,
                text: "x".into(),
                format: None,
            },
        )
        .unwrap();
        let t1 = t0 + Duration::from_secs(10);
        assert!(matches!(c.tick(&mut s, Some(id), t1), Repaint::After(_)));
        assert_eq!(
            c.tick(&mut s, Some(id), t1 + DEBOUNCE + Duration::from_millis(1)),
            Repaint::Now
        );
        for i in 1..20u64 {
            let t = t1 + DEBOUNCE + Duration::from_millis(50 * i);
            assert_eq!(c.tick(&mut s, Some(id), t), Repaint::No);
        }
        // A forced refresh (the commercial box) asks for exactly one frame.
        let t2 = t1 + Duration::from_secs(5);
        c.force_refresh();
        assert!(c.take_wake());
        assert!(!c.take_wake());
        assert_eq!(c.tick(&mut s, Some(id), t2), Repaint::Now);
        for i in 1..20u64 {
            let t = t2 + Duration::from_millis(50 * i);
            assert_eq!(c.tick(&mut s, Some(id), t), Repaint::No);
            assert!(!c.take_wake());
        }
    }

    #[test]
    fn edits_that_cause_issues_show_up_and_commercial_rechecks_everything() {
        let (mut s, id, story) = doc(3);
        let mut c = CheckerState::default();
        let t0 = Instant::now();
        c.show();
        c.poll(&mut s, Some(id), t0);
        // Tiny text on page 1.
        s.exec(
            Some(id),
            Command::ApplyCharProps {
                story,
                start: 0,
                end: 3,
                props: pubrs_api::core::CharProps {
                    font_size: Some(3.0),
                    ..Default::default()
                },
            },
        )
        .unwrap();
        c.poll(&mut s, Some(id), t0 + Duration::from_secs(1));
        c.poll(&mut s, Some(id), t0 + Duration::from_secs(2));
        let codes: Vec<_> = c.issues().iter().map(|w| w.code).collect();
        assert_eq!(codes, vec![WarningCode::TextTooSmall]);
        assert_eq!(c.last_pages_checked, 1);
        // The box forces a full pass at the next look.
        c.commercial = true;
        c.force = true;
        c.poll(&mut s, Some(id), t0 + Duration::from_secs(3));
        assert_eq!(c.last_pages_checked, 3);
    }
}
