//! Page tools: page_add, page_delete, page_reorder, page_list, page_info,
//! master_create, master_apply, master_delete, guide_add, guide_remove.

use pubrs_core::{
    Axis, Command, DocumentId, Guide, GuideId, MasterId, MasterPage, Owner, Page, PageId, Size,
};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use super::{FrameSummary, require_confirm, summarize};
use crate::error::{ApiError, ApiResult};
use crate::session::{Changed, Session};

#[derive(Debug, Clone, Default, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct PageAdd {
    /// Document id (default: the most recently used document).
    #[serde(default)]
    pub doc_id: Option<DocumentId>,
    /// Insert position, 0-based (default: after the last page).
    #[serde(default)]
    pub index: Option<usize>,
    /// Master page to apply to the new page.
    #[serde(default)]
    pub master_id: Option<MasterId>,
}

#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct PageAddOut {
    pub page_id: PageId,
    /// 0-based position of the new page.
    pub index: usize,
    #[serde(flatten)]
    pub change: Changed,
}

#[derive(Debug, Clone, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct PageDelete {
    /// Document id (default: the most recently used document).
    #[serde(default)]
    pub doc_id: Option<DocumentId>,
    pub page_id: PageId,
    /// Must be true: the page and all its frames are deleted.
    #[serde(default)]
    pub confirm: bool,
}

#[derive(Debug, Clone, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct PageReorder {
    /// Document id (default: the most recently used document).
    #[serde(default)]
    pub doc_id: Option<DocumentId>,
    pub page_id: PageId,
    /// New 0-based position.
    pub index: usize,
}

#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct PageEntry {
    pub page_id: PageId,
    /// 0-based position (page number - 1).
    pub index: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub master_id: Option<MasterId>,
    pub frame_count: usize,
}

#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct PageListOut {
    pub pages: Vec<PageEntry>,
    /// Trim size shared by all pages (points).
    pub page_size: Size,
    pub revision: u64,
}

#[derive(Debug, Clone, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct PageRef {
    /// Document id (default: the most recently used document).
    #[serde(default)]
    pub doc_id: Option<DocumentId>,
    pub page_id: PageId,
}

#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct PageInfoOut {
    pub page_id: PageId,
    pub index: usize,
    pub page_size: Size,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub master_id: Option<MasterId>,
    /// Frames on the page, back to front (groups include their members).
    pub frames: Vec<FrameSummary>,
    pub guides: Vec<Guide>,
    pub revision: u64,
}

#[derive(Debug, Clone, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct MasterCreate {
    /// Document id (default: the most recently used document).
    #[serde(default)]
    pub doc_id: Option<DocumentId>,
    /// Display name, e.g. "Footer".
    pub name: String,
}

#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct MasterCreateOut {
    /// Add frames to it with frame_add_* and `master_id`.
    pub master_id: MasterId,
    #[serde(flatten)]
    pub change: Changed,
}

#[derive(Debug, Clone, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct MasterApply {
    /// Document id (default: the most recently used document).
    #[serde(default)]
    pub doc_id: Option<DocumentId>,
    /// Master to apply, or null to remove the master from the pages.
    pub master_id: Option<MasterId>,
    /// Pages to change (default: every page).
    #[serde(default)]
    pub page_ids: Option<Vec<PageId>>,
}

#[derive(Debug, Clone, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct MasterDelete {
    /// Document id (default: the most recently used document).
    #[serde(default)]
    pub doc_id: Option<DocumentId>,
    pub master_id: MasterId,
    /// Must be true: the master and its frames are deleted.
    #[serde(default)]
    pub confirm: bool,
}

#[derive(Debug, Clone, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct GuideAdd {
    /// Document id (default: the most recently used document).
    #[serde(default)]
    pub doc_id: Option<DocumentId>,
    /// Page to add the guide to (or give master_id).
    #[serde(default)]
    pub page_id: Option<PageId>,
    #[serde(default)]
    pub master_id: Option<MasterId>,
    /// "horizontal" (position is y) or "vertical" (position is x).
    pub axis: Axis,
    /// Points from the page's top (horizontal) or left (vertical) edge.
    pub position: f64,
}

#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct GuideAddOut {
    pub guide_id: GuideId,
    #[serde(flatten)]
    pub change: Changed,
}

#[derive(Debug, Clone, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct GuideMove {
    /// Document id (default: the most recently used document).
    #[serde(default)]
    pub doc_id: Option<DocumentId>,
    /// Page or master owning the guide (optional: found by guide_id).
    #[serde(default)]
    pub page_id: Option<PageId>,
    #[serde(default)]
    pub master_id: Option<MasterId>,
    pub guide_id: GuideId,
    /// New position in points from the page's top (horizontal) or left (vertical) edge.
    pub position: f64,
}

#[derive(Debug, Clone, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct GuideRemove {
    /// Document id (default: the most recently used document).
    #[serde(default)]
    pub doc_id: Option<DocumentId>,
    #[serde(default)]
    pub page_id: Option<PageId>,
    #[serde(default)]
    pub master_id: Option<MasterId>,
    pub guide_id: GuideId,
}

impl Session {
    pub fn page_add(&mut self, req: PageAdd) -> ApiResult<PageAddOut> {
        let id = PageId::new(&mut *self.ids);
        let mut page = Page::new(id);
        page.master = req.master_id;
        let change = self.exec(
            req.doc_id,
            Command::AddPage {
                page,
                index: req.index,
            },
        )?;
        let index = self.require_page(Some(change.doc_id), id)?;
        Ok(PageAddOut {
            page_id: id,
            index,
            change,
        })
    }

    pub fn page_delete(&mut self, req: PageDelete) -> ApiResult<Changed> {
        require_confirm(req.confirm, "deleting a page and its frames")?;
        self.exec(req.doc_id, Command::DeletePage { page: req.page_id })
    }

    pub fn page_reorder(&mut self, req: PageReorder) -> ApiResult<Changed> {
        self.require_page(req.doc_id, req.page_id)?;
        self.exec(
            req.doc_id,
            Command::MovePage {
                page: req.page_id,
                index: req.index,
            },
        )
    }

    pub fn page_list(&mut self, req: super::doc::DocRef) -> ApiResult<PageListOut> {
        let open = self.get(req.doc_id)?;
        let doc = open.doc();
        Ok(PageListOut {
            pages: doc
                .pages
                .iter()
                .enumerate()
                .map(|(index, p)| PageEntry {
                    page_id: p.id,
                    index,
                    master_id: p.master,
                    frame_count: doc.all_frames_of(Owner::Page(p.id)).len(),
                })
                .collect(),
            page_size: doc.setup.size,
            revision: open.revision(),
        })
    }

    pub fn page_info(&mut self, req: PageRef) -> ApiResult<PageInfoOut> {
        let index = self.require_page(req.doc_id, req.page_id)?;
        let open = self.get(req.doc_id)?;
        let doc = open.doc();
        let page = &doc.pages[index];
        Ok(PageInfoOut {
            page_id: page.id,
            index,
            page_size: doc.setup.size,
            master_id: page.master,
            frames: doc
                .all_frames_of(Owner::Page(page.id))
                .iter()
                .filter_map(|f| doc.frame(*f))
                .map(|f| summarize(doc, f))
                .collect(),
            guides: page.guides.clone(),
            revision: open.revision(),
        })
    }

    pub fn master_create(&mut self, req: MasterCreate) -> ApiResult<MasterCreateOut> {
        if req.name.trim().is_empty() {
            return Err(ApiError::invalid(
                "master name is empty",
                "give a name like \"Footer\"",
            ));
        }
        let id = MasterId::new(&mut *self.ids);
        let change = self.exec(
            req.doc_id,
            Command::AddMaster {
                master: MasterPage {
                    id,
                    name: req.name,
                    frames: vec![],
                    guides: vec![],
                },
            },
        )?;
        let mut change = change;
        change.affected_ids.push(id.to_string());
        Ok(MasterCreateOut {
            master_id: id,
            change,
        })
    }

    pub fn master_apply(&mut self, req: MasterApply) -> ApiResult<Changed> {
        let doc = self.get(req.doc_id)?.doc();
        let pages = req
            .page_ids
            .unwrap_or_else(|| doc.pages.iter().map(|p| p.id).collect());
        let commands = pages
            .into_iter()
            .map(|page| Command::SetPageMaster {
                page,
                master: req.master_id,
            })
            .collect();
        self.exec(
            req.doc_id,
            Command::Batch {
                label: "apply master".into(),
                commands,
            },
        )
    }

    pub fn master_delete(&mut self, req: MasterDelete) -> ApiResult<Changed> {
        require_confirm(req.confirm, "deleting a master page and its frames")?;
        self.exec(
            req.doc_id,
            Command::DeleteMaster {
                master: req.master_id,
            },
        )
    }

    pub fn guide_add(&mut self, req: GuideAdd) -> ApiResult<GuideAddOut> {
        let owner = self.owner_for(req.doc_id, req.page_id, req.master_id)?;
        let id = GuideId::new(&mut *self.ids);
        let mut change = self.exec(
            req.doc_id,
            Command::AddGuide {
                owner,
                guide: Guide {
                    id,
                    axis: req.axis,
                    position: req.position,
                },
            },
        )?;
        change.affected_ids.push(id.to_string());
        Ok(GuideAddOut {
            guide_id: id,
            change,
        })
    }

    fn guide_owner(
        &self,
        doc_id: Option<DocumentId>,
        page_id: Option<PageId>,
        master_id: Option<MasterId>,
        guide_id: GuideId,
    ) -> ApiResult<Owner> {
        match (page_id, master_id) {
            (None, None) => {
                let doc = self.get(doc_id)?.doc();
                let p = doc
                    .pages
                    .iter()
                    .find(|p| p.guides.iter().any(|g| g.id == guide_id))
                    .map(|p| Owner::Page(p.id));
                let m = doc
                    .masters
                    .iter()
                    .find(|m| m.guides.iter().any(|g| g.id == guide_id))
                    .map(|m| Owner::Master(m.id));
                p.or(m).ok_or_else(|| {
                    ApiError::not_found("guide", &guide_id.to_string(), "see page_info for guides")
                })
            }
            (p, m) => self.owner_for(doc_id, p, m),
        }
    }

    pub fn guide_move(&mut self, req: GuideMove) -> ApiResult<Changed> {
        let owner = self.guide_owner(req.doc_id, req.page_id, req.master_id, req.guide_id)?;
        self.exec(
            req.doc_id,
            Command::MoveGuide {
                owner,
                guide: req.guide_id,
                position: req.position,
            },
        )
    }

    pub fn guide_remove(&mut self, req: GuideRemove) -> ApiResult<Changed> {
        let owner = match (req.page_id, req.master_id) {
            (None, None) => {
                let doc = self.get(req.doc_id)?.doc();
                let p = doc
                    .pages
                    .iter()
                    .find(|p| p.guides.iter().any(|g| g.id == req.guide_id))
                    .map(|p| Owner::Page(p.id));
                let m = doc
                    .masters
                    .iter()
                    .find(|m| m.guides.iter().any(|g| g.id == req.guide_id))
                    .map(|m| Owner::Master(m.id));
                p.or(m).ok_or_else(|| {
                    ApiError::not_found(
                        "guide",
                        &req.guide_id.to_string(),
                        "see page_info for guides",
                    )
                })?
            }
            (p, m) => self.owner_for(req.doc_id, p, m)?,
        };
        self.exec(
            req.doc_id,
            Command::RemoveGuide {
                owner,
                guide: req.guide_id,
            },
        )
    }
}
