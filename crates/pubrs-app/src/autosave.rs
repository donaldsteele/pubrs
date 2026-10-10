//! Autosave and crash recovery.
//!
//! Every [`Autosave::interval`], each open document with unsaved changes is
//! written to the recovery folder (`<doc>.pubrs` plus a small JSON sidecar
//! naming the original file). Saving or closing a document deletes its
//! recovery files, so files left behind mean pubrs did not exit cleanly and
//! are offered for recovery at the next start.

use std::collections::HashMap;
use std::path::PathBuf;
use std::time::{Duration, Instant, SystemTime};

use pubrs_api::Session;
use pubrs_api::core::DocumentId;
use serde::{Deserialize, Serialize};

use crate::app::PubrsApp;
use crate::view::DocView;

#[derive(Debug, Clone, Serialize, Deserialize)]
struct Sidecar {
    title: String,
    original: Option<PathBuf>,
}

/// A recovery file found at start-up.
#[derive(Debug, Clone, PartialEq)]
pub struct Recoverable {
    pub file: PathBuf,
    pub sidecar: PathBuf,
    pub title: String,
    pub original: Option<PathBuf>,
    /// Human-readable age, e.g. "3 minutes ago".
    pub saved_at: String,
}

pub struct Autosave {
    pub dir: PathBuf,
    pub interval: Duration,
    last: Instant,
    written: HashMap<DocumentId, u64>,
}

impl Autosave {
    pub fn new(dir: PathBuf) -> Self {
        Self {
            dir,
            interval: Duration::from_secs(60),
            last: Instant::now(),
            written: HashMap::new(),
        }
    }

    fn paths(&self, id: DocumentId) -> (PathBuf, PathBuf) {
        (
            self.dir.join(format!("{id}.pubrs")),
            self.dir.join(format!("{id}.json")),
        )
    }

    /// Write recovery copies of changed documents when the interval elapsed.
    pub fn tick(&mut self, session: &Session, views: &[DocView]) {
        if self.last.elapsed() < self.interval {
            return;
        }
        self.last = Instant::now();
        for v in views {
            self.write_if_changed(session, v.doc_id);
        }
    }

    /// Write a recovery copy now if the document changed since the last one.
    pub fn write_if_changed(&mut self, session: &Session, id: DocumentId) -> bool {
        let Ok(open) = session.get(Some(id)) else {
            return false;
        };
        if !open.editor.is_dirty() || self.written.get(&id) == Some(&open.revision()) {
            return false;
        }
        let (file, sidecar) = self.paths(id);
        if let Err(e) = std::fs::create_dir_all(&self.dir) {
            tracing::warn!("autosave: cannot create {}: {e}", self.dir.display());
            return false;
        }
        match pubrs_api::io::save(open.doc(), &file, None) {
            Ok(()) => {
                let meta = Sidecar {
                    title: open.doc().meta.title.clone(),
                    original: open.path.clone(),
                };
                let _ = std::fs::write(
                    &sidecar,
                    serde_json::to_vec_pretty(&meta).unwrap_or_default(),
                );
                self.written.insert(id, open.revision());
                tracing::debug!("autosaved {}", file.display());
                true
            }
            Err(e) => {
                tracing::warn!("autosave failed: {e}");
                false
            }
        }
    }

    /// The document was saved or closed: its recovery copy is no longer needed.
    pub fn forget(&mut self, id: DocumentId) {
        let (file, sidecar) = self.paths(id);
        let _ = std::fs::remove_file(file);
        let _ = std::fs::remove_file(sidecar);
        self.written.remove(&id);
    }

    /// Recovery files left by a previous run.
    pub fn recoverable(&self) -> Vec<Recoverable> {
        let Ok(rd) = std::fs::read_dir(&self.dir) else {
            return Vec::new();
        };
        let mut out: Vec<Recoverable> = rd
            .filter_map(Result::ok)
            .map(|e| e.path())
            .filter(|p| p.extension().is_some_and(|e| e == "pubrs"))
            .map(|file| {
                let sidecar = file.with_extension("json");
                let meta: Option<Sidecar> = std::fs::read(&sidecar)
                    .ok()
                    .and_then(|b| serde_json::from_slice(&b).ok());
                let age = std::fs::metadata(&file)
                    .and_then(|m| m.modified())
                    .ok()
                    .and_then(|t| SystemTime::now().duration_since(t).ok())
                    .map_or("unknown time".to_string(), |d| match d.as_secs() {
                        s if s < 120 => "moments ago".into(),
                        s if s < 7200 => format!("{} minutes ago", s / 60),
                        s => format!("{} hours ago", s / 3600),
                    });
                Recoverable {
                    title: meta
                        .as_ref()
                        .map(|m| m.title.clone())
                        .filter(|t| !t.is_empty())
                        .unwrap_or_else(|| "Untitled".into()),
                    original: meta.and_then(|m| m.original),
                    file,
                    sidecar,
                    saved_at: age,
                }
            })
            .collect();
        out.sort_by(|a, b| a.title.cmp(&b.title));
        out
    }

    pub fn discard(&self, r: &Recoverable) {
        let _ = std::fs::remove_file(&r.file);
        let _ = std::fs::remove_file(&r.sidecar);
    }
}

impl PubrsApp {
    /// Open a recovery copy as an unsaved document pointing at its original file.
    pub fn recover(&mut self, r: &Recoverable) {
        let out = self.session.doc_open(pubrs_api::tools::doc::DocOpen {
            path: r.file.display().to_string(),
        });
        let Some(out) = self.report(out) else { return };
        if let Ok(open) = self.session.get_mut(Some(out.doc_id)) {
            open.path = r.original.clone();
            open.editor.mark_unsaved();
        }
        self.views.push(DocView::new(out.doc_id));
        self.active = Some(self.views.len() - 1);
        self.autosave.discard(r);
        self.notify(format!(
            "Recovered “{}” — save it to keep the changes",
            r.title
        ));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn writes_lists_and_forgets_recovery_files() {
        let dir = std::env::temp_dir().join(format!("pubrs-autosave-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let mut a = Autosave::new(dir.clone());
        let mut s = Session::default();
        let d = s.doc_new(Default::default()).unwrap();
        assert!(
            !a.write_if_changed(&s, d.doc_id),
            "clean documents are not written"
        );
        s.doc_set_bleed(pubrs_api::tools::doc::DocSetBleed {
            doc_id: None,
            bleed: 9.0,
        })
        .unwrap();
        assert!(a.write_if_changed(&s, d.doc_id));
        assert!(
            !a.write_if_changed(&s, d.doc_id),
            "unchanged since last write"
        );
        let found = a.recoverable();
        assert_eq!(found.len(), 1);
        let reopened = pubrs_api::io::load(&found[0].file).unwrap();
        assert_eq!(reopened.setup.bleed, 9.0);
        a.forget(d.doc_id);
        assert!(a.recoverable().is_empty());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
