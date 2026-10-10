//! The editor: a document plus its undo/redo history and revision counter.

use serde::{Deserialize, Serialize};

use crate::command::{CmdResult, Command, CommandError};
use crate::model::Document;
use crate::patch::{Patch, Touched};

/// Default maximum number of undo steps kept.
pub const DEFAULT_HISTORY_LIMIT: usize = 500;

/// One undoable step.
#[derive(Debug, Clone)]
pub struct HistoryEntry {
    /// The command as issued.
    pub command: Command,
    /// Turns the state before into the state after.
    pub forward: Patch,
    /// Turns the state after back into the state before (the inverse).
    pub backward: Patch,
    /// Merge key for coalescing consecutive edits (e.g. typing).
    pub coalesce: Option<String>,
}

/// Result of a successful change.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct Change {
    /// Document revision after the change. Increases by one per change.
    pub revision: u64,
    /// What changed.
    pub touched: Touched,
}

/// An entry in the history list.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct HistoryItem {
    /// Stable command name, e.g. "insert_text" or "batch".
    pub name: String,
    pub label: String,
    /// True for entries that can be redone (undone and not yet re-applied).
    pub undone: bool,
}

/// A document with undo/redo.
#[derive(Debug, Clone)]
pub struct Editor {
    doc: Document,
    undo: Vec<HistoryEntry>,
    redo: Vec<HistoryEntry>,
    revision: u64,
    saved_revision: Option<u64>,
    limit: usize,
}

impl Editor {
    pub fn new(doc: Document) -> Self {
        Self {
            doc,
            undo: Vec::new(),
            redo: Vec::new(),
            revision: 0,
            saved_revision: Some(0),
            limit: DEFAULT_HISTORY_LIMIT,
        }
    }

    pub fn with_limit(mut self, limit: usize) -> Self {
        self.limit = limit.max(1);
        self
    }

    pub fn doc(&self) -> &Document {
        &self.doc
    }

    pub fn revision(&self) -> u64 {
        self.revision
    }

    /// True if the document changed since [`Editor::mark_saved`].
    pub fn is_dirty(&self) -> bool {
        self.saved_revision != Some(self.revision)
    }

    pub fn mark_saved(&mut self) {
        self.saved_revision = Some(self.revision);
    }

    /// Mark as modified with no matching saved state (e.g. after recovery).
    pub fn mark_unsaved(&mut self) {
        self.saved_revision = None;
    }

    pub fn can_undo(&self) -> bool {
        !self.undo.is_empty()
    }

    pub fn can_redo(&self) -> bool {
        !self.redo.is_empty()
    }

    /// Apply a command atomically. On error the document is unchanged.
    pub fn execute(&mut self, command: Command) -> CmdResult<Change> {
        self.execute_inner(command, None)
    }

    /// Like [`Editor::execute`], but merges into the previous history entry if
    /// it was executed with the same `key` (used to make typing one undo step).
    pub fn execute_coalesced(&mut self, command: Command, key: &str) -> CmdResult<Change> {
        self.execute_inner(command, Some(key.to_string()))
    }

    fn execute_inner(&mut self, command: Command, key: Option<String>) -> CmdResult<Change> {
        let before = self.doc.clone();
        if let Err(e) = command.apply(&mut self.doc) {
            self.doc = before;
            return Err(e);
        }
        let forward = Patch::diff(&before, &self.doc);
        if forward.is_empty() {
            return Ok(Change {
                revision: self.revision,
                touched: Touched::default(),
            });
        }
        let touched = forward.touched();
        let mergeable = key.is_some()
            && self.redo.is_empty()
            && self.undo.last().is_some_and(|e| e.coalesce == key);
        if mergeable {
            let last = self.undo.last_mut().expect("checked");
            let mut start = before.clone();
            last.backward.apply(&mut start);
            last.forward = Patch::diff(&start, &self.doc);
            last.backward = Patch::diff(&self.doc, &start);
            last.command = Command::Batch {
                label: last.command.label(),
                commands: match std::mem::replace(
                    &mut last.command,
                    Command::Patch(Patch::default()),
                ) {
                    Command::Batch { commands, .. } => {
                        let mut c = commands;
                        c.push(command);
                        c
                    }
                    other => vec![other, command],
                },
            };
        } else {
            let backward = Patch::diff(&self.doc, &before);
            self.undo.push(HistoryEntry {
                command,
                forward,
                backward,
                coalesce: key,
            });
            if self.undo.len() > self.limit {
                self.undo.remove(0);
            }
        }
        self.redo.clear();
        self.revision += 1;
        Ok(Change {
            revision: self.revision,
            touched,
        })
    }

    /// Undo the last step. Returns `None` if there is nothing to undo.
    pub fn undo(&mut self) -> Option<Change> {
        let entry = self.undo.pop()?;
        entry.backward.apply(&mut self.doc);
        let touched = entry.backward.touched();
        self.redo.push(entry);
        self.revision += 1;
        Some(Change {
            revision: self.revision,
            touched,
        })
    }

    /// Redo the last undone step. Returns `None` if there is nothing to redo.
    pub fn redo(&mut self) -> Option<Change> {
        let entry = self.redo.pop()?;
        entry.forward.apply(&mut self.doc);
        let touched = entry.forward.touched();
        self.undo.push(entry);
        self.revision += 1;
        Some(Change {
            revision: self.revision,
            touched,
        })
    }

    /// History, oldest first: done entries then undone (redoable) ones.
    pub fn history(&self) -> Vec<HistoryItem> {
        let done = self.undo.iter().map(|e| HistoryItem {
            name: e.command.name().to_string(),
            label: e.command.label(),
            undone: false,
        });
        let undone = self.redo.iter().rev().map(|e| HistoryItem {
            name: e.command.name().to_string(),
            label: e.command.label(),
            undone: true,
        });
        done.chain(undone).collect()
    }

    /// The commands of all applied (not undone) steps, oldest first.
    pub fn applied_commands(&self) -> impl Iterator<Item = &Command> {
        self.undo.iter().map(|e| &e.command)
    }

    /// Replace the document wholesale, clearing history (e.g. after a reload).
    pub fn reset(&mut self, doc: Document) {
        self.doc = doc;
        self.undo.clear();
        self.redo.clear();
        self.revision += 1;
    }

    /// Validate a command against the current document without applying it.
    pub fn check(&self, command: &Command) -> Result<(), CommandError> {
        let mut scratch = self.doc.clone();
        command.apply(&mut scratch)
    }
}
