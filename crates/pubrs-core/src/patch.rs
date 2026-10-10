//! Patches: the generic, serializable difference between two documents.
//!
//! The editor records every command as a forward and a backward patch, so
//! undo and redo never re-run command logic and are exact by construction.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::bytes::Bytes;
use crate::id::{AssetId, FrameId, PageId, StoryId};
use crate::model::{
    Asset, DocMeta, Document, Frame, GridSettings, MasterPage, Page, PageSetup, Story,
};
use crate::style::StyleSheet;

/// An embedded asset with its bytes.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct AssetEntry {
    pub asset: Asset,
    pub data: Bytes,
}

/// A set of replacements that turns one document state into another.
/// Map entries set to `null` remove the object.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct Patch {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub meta: Option<DocMeta>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub setup: Option<PageSetup>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub grid: Option<GridSettings>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pages: Option<Vec<Page>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub masters: Option<Vec<MasterPage>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub styles: Option<StyleSheet>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub frames: BTreeMap<FrameId, Option<Frame>>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub stories: BTreeMap<StoryId, Option<Story>>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub assets: BTreeMap<AssetId, Option<AssetEntry>>,
}

fn diff_map<K: Ord + Copy, V: Clone + PartialEq>(
    from: &BTreeMap<K, V>,
    to: &BTreeMap<K, V>,
) -> BTreeMap<K, Option<V>> {
    let mut out = BTreeMap::new();
    for (k, v) in to {
        if from.get(k) != Some(v) {
            out.insert(*k, Some(v.clone()));
        }
    }
    for k in from.keys() {
        if !to.contains_key(k) {
            out.insert(*k, None);
        }
    }
    out
}

fn changed<T: Clone + PartialEq>(from: &T, to: &T) -> Option<T> {
    (from != to).then(|| to.clone())
}

impl Patch {
    /// The patch that turns `from` into `to`.
    pub fn diff(from: &Document, to: &Document) -> Patch {
        let mut assets = BTreeMap::new();
        let keys: std::collections::BTreeSet<AssetId> = from
            .assets
            .keys()
            .chain(to.assets.keys())
            .chain(from.blobs.keys())
            .chain(to.blobs.keys())
            .copied()
            .collect();
        for k in keys {
            let a = (from.assets.get(&k), from.blobs.get(&k));
            let b = (to.assets.get(&k), to.blobs.get(&k));
            if a != b {
                let entry = match b {
                    (Some(asset), Some(data)) => Some(AssetEntry {
                        asset: asset.clone(),
                        data: Bytes(data.clone()),
                    }),
                    _ => None,
                };
                assets.insert(k, entry);
            }
        }
        Patch {
            meta: changed(&from.meta, &to.meta),
            setup: changed(&from.setup, &to.setup),
            grid: changed(&from.grid, &to.grid),
            pages: changed(&from.pages, &to.pages),
            masters: changed(&from.masters, &to.masters),
            styles: changed(&from.styles, &to.styles),
            frames: diff_map(&from.frames, &to.frames),
            stories: diff_map(&from.stories, &to.stories),
            assets,
        }
    }

    pub fn is_empty(&self) -> bool {
        *self == Patch::default()
    }

    /// Apply the patch. Patches produced by [`Patch::diff`] always apply cleanly.
    pub fn apply(&self, doc: &mut Document) {
        if let Some(v) = &self.meta {
            doc.meta = v.clone();
        }
        if let Some(v) = &self.setup {
            doc.setup = *v;
        }
        if let Some(v) = &self.grid {
            doc.grid = *v;
        }
        if let Some(v) = &self.pages {
            doc.pages = v.clone();
        }
        if let Some(v) = &self.masters {
            doc.masters = v.clone();
        }
        if let Some(v) = &self.styles {
            doc.styles = v.clone();
        }
        for (k, v) in &self.frames {
            match v {
                Some(f) => doc.frames.insert(*k, f.clone()),
                None => doc.frames.remove(k),
            };
        }
        for (k, v) in &self.stories {
            match v {
                Some(s) => doc.stories.insert(*k, s.clone()),
                None => doc.stories.remove(k),
            };
        }
        for (k, v) in &self.assets {
            match v {
                Some(e) => {
                    doc.assets.insert(*k, e.asset.clone());
                    doc.blobs.insert(*k, e.data.0.clone());
                }
                None => {
                    doc.assets.remove(k);
                    doc.blobs.remove(k);
                }
            }
        }
    }

    /// Ids of the objects this patch creates, changes or removes.
    pub fn touched(&self) -> Touched {
        let mut pages = Vec::new();
        if let Some(ps) = &self.pages {
            pages.extend(ps.iter().map(|p| p.id));
        }
        Touched {
            frames: self.frames.keys().copied().collect(),
            stories: self.stories.keys().copied().collect(),
            pages,
            assets: self.assets.keys().copied().collect(),
            styles: self.styles.is_some(),
            setup: self.setup.is_some() || self.meta.is_some() || self.grid.is_some(),
        }
    }
}

/// Summary of what a change touched.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct Touched {
    pub frames: Vec<FrameId>,
    pub stories: Vec<StoryId>,
    /// Present when the page list changed (all page ids listed).
    pub pages: Vec<PageId>,
    pub assets: Vec<AssetId>,
    pub styles: bool,
    pub setup: bool,
}
