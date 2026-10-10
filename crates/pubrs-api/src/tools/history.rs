//! History tools: undo, redo, history_list.

use pubrs_core::{DocumentId, HistoryItem};
use schemars::JsonSchema;
use serde::Serialize;

use super::doc::DocRef;
use crate::error::ApiResult;
use crate::session::{Session, changed};

#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct UndoOut {
    pub doc_id: DocumentId,
    /// False if there was nothing to undo/redo.
    pub done: bool,
    /// Label of the step undone/redone.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
    pub revision: u64,
    pub affected_ids: Vec<String>,
    pub can_undo: bool,
    pub can_redo: bool,
}

#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct HistoryOut {
    pub doc_id: DocumentId,
    /// Oldest first. Entries with `undone: true` can be redone.
    pub entries: Vec<HistoryItem>,
    pub revision: u64,
}

impl Session {
    fn step(&mut self, req: DocRef, redo: bool) -> ApiResult<UndoOut> {
        let id = self.resolve(req.doc_id)?;
        let open = self.get_mut(Some(id))?;
        let label = {
            let h = open.editor.history();
            if redo {
                h.iter().find(|e| e.undone).map(|e| e.label.clone())
            } else {
                h.iter().rev().find(|e| !e.undone).map(|e| e.label.clone())
            }
        };
        let change = if redo {
            open.editor.redo()
        } else {
            open.editor.undo()
        };
        let revision = open.revision();
        let (can_undo, can_redo) = (open.editor.can_undo(), open.editor.can_redo());
        Ok(UndoOut {
            doc_id: id,
            done: change.is_some(),
            label: change.as_ref().and(label),
            revision,
            affected_ids: change
                .map(|c| changed(id, &c).affected_ids)
                .unwrap_or_default(),
            can_undo,
            can_redo,
        })
    }

    pub fn undo(&mut self, req: DocRef) -> ApiResult<UndoOut> {
        self.step(req, false)
    }

    pub fn redo(&mut self, req: DocRef) -> ApiResult<UndoOut> {
        self.step(req, true)
    }

    pub fn history_list(&mut self, req: DocRef) -> ApiResult<HistoryOut> {
        let open = self.get(req.doc_id)?;
        Ok(HistoryOut {
            doc_id: open.doc_id,
            entries: open.editor.history(),
            revision: open.revision(),
        })
    }
}
