//! Patches: the generic, serializable difference between two documents.
//!
//! The editor records every command as a forward and a backward patch, so
//! undo and redo never re-run command logic and are exact by construction.

use std::collections::BTreeMap;
use std::sync::Arc;

use serde::{Deserialize, Serialize};

use crate::bytes::Bytes;
use crate::id::{AssetId, FrameId, PageId, StoryId};
use crate::model::{
    Asset, DocMeta, Document, Frame, GridSettings, MasterPage, MergeSource, Page, PageSetup, Story,
};
use crate::style::StyleSheet;
use crate::swatch::Palette;

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
    pub pages: Option<Arc<Vec<Page>>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub masters: Option<Arc<Vec<MasterPage>>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub styles: Option<StyleSheet>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub palette: Option<Palette>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub merge: Option<MergeSlot>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub frames: BTreeMap<FrameId, Option<Arc<Frame>>>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub stories: BTreeMap<StoryId, Option<Arc<Story>>>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub assets: BTreeMap<AssetId, Option<AssetEntry>>,
}

/// The merge data of a document (none = no data), so a patch can also clear it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct MergeSlot(pub Option<MergeSource>);

type ArcMap<K, V> = BTreeMap<K, Option<Arc<V>>>;

fn same<T: PartialEq>(a: &Arc<T>, b: &Arc<T>) -> bool {
    Arc::ptr_eq(a, b) || **a == **b
}

/// Forward (`from` -> `to`) and backward (`to` -> `from`) map patches in one
/// merged walk. Shared (`Arc::ptr_eq`) entries are skipped without comparing.
fn diff_map_pair<K: Ord + Copy, V: PartialEq>(
    from: &BTreeMap<K, Arc<V>>,
    to: &BTreeMap<K, Arc<V>>,
) -> (ArcMap<K, V>, ArcMap<K, V>) {
    let mut fwd = BTreeMap::new();
    let mut bwd = BTreeMap::new();
    let mut a = from.iter().peekable();
    let mut b = to.iter().peekable();
    loop {
        let ka = a.peek().map(|(k, _)| **k);
        let kb = b.peek().map(|(k, _)| **k);
        let order = match (ka, kb) {
            (None, None) => break,
            (Some(_), None) => std::cmp::Ordering::Less,
            (None, Some(_)) => std::cmp::Ordering::Greater,
            (Some(x), Some(y)) => x.cmp(&y),
        };
        match order {
            std::cmp::Ordering::Less => {
                if let Some((k, v)) = a.next() {
                    fwd.insert(*k, None);
                    bwd.insert(*k, Some(v.clone()));
                }
            }
            std::cmp::Ordering::Greater => {
                if let Some((k, v)) = b.next() {
                    fwd.insert(*k, Some(v.clone()));
                    bwd.insert(*k, None);
                }
            }
            std::cmp::Ordering::Equal => {
                if let (Some((k, va)), Some((_, vb))) = (a.next(), b.next())
                    && !same(va, vb)
                {
                    fwd.insert(*k, Some(vb.clone()));
                    bwd.insert(*k, Some(va.clone()));
                }
            }
        }
    }
    (fwd, bwd)
}

fn changed_pair<T: Clone + PartialEq>(from: &T, to: &T) -> (Option<T>, Option<T>) {
    if from == to {
        (None, None)
    } else {
        (Some(to.clone()), Some(from.clone()))
    }
}

fn changed_arc_pair<T: PartialEq>(from: &Arc<T>, to: &Arc<T>) -> (Option<Arc<T>>, Option<Arc<T>>) {
    if same(from, to) {
        (None, None)
    } else {
        (Some(to.clone()), Some(from.clone()))
    }
}

fn overlay<K: Ord + Copy, V>(base: &mut BTreeMap<K, V>, top: BTreeMap<K, V>) {
    base.extend(top);
}

fn same_opt<T: PartialEq>(a: &Option<Arc<T>>, b: &Option<Arc<T>>) -> bool {
    match (a, b) {
        (None, None) => true,
        (Some(a), Some(b)) => same(a, b),
        _ => false,
    }
}

/// Merge a later step into an accumulated pair so the result equals the pair
/// `diff_pair(start, end)` would produce, without walking the document.
fn compose_map<K: Ord + Copy, V: PartialEq>(
    fwd: &mut BTreeMap<K, Option<V>>,
    bwd: &mut BTreeMap<K, Option<V>>,
    step_fwd: BTreeMap<K, Option<V>>,
    step_bwd: BTreeMap<K, Option<V>>,
    eq: impl Fn(&Option<V>, &Option<V>) -> bool,
) {
    overlay(fwd, step_fwd);
    for (k, v) in step_bwd {
        bwd.entry(k).or_insert(v);
    }
    let redundant: Vec<K> = fwd
        .iter()
        .filter(|(k, v)| bwd.get(k).is_some_and(|b| eq(v, b)))
        .map(|(k, _)| *k)
        .collect();
    for k in redundant {
        fwd.remove(&k);
        bwd.remove(&k);
    }
}

fn compose_opt<T: PartialEq>(
    fwd: &mut Option<T>,
    bwd: &mut Option<T>,
    step_fwd: Option<T>,
    step_bwd: Option<T>,
    eq: impl Fn(&T, &T) -> bool,
) {
    if step_fwd.is_some() {
        *fwd = step_fwd;
    }
    if bwd.is_none() {
        *bwd = step_bwd;
    }
    if let (Some(f), Some(b)) = (fwd.as_ref(), bwd.as_ref())
        && eq(f, b)
    {
        *fwd = None;
        *bwd = None;
    }
}

impl Patch {
    /// The patch that turns `from` into `to`.
    pub fn diff(from: &Document, to: &Document) -> Patch {
        Self::diff_pair(from, to).0
    }

    /// Both `from -> to` and `to -> from` in a single pass over the documents.
    pub fn diff_pair(from: &Document, to: &Document) -> (Patch, Patch) {
        let mut fa = BTreeMap::new();
        let mut ba = BTreeMap::new();
        let keys: std::collections::BTreeSet<AssetId> = from
            .assets
            .keys()
            .chain(to.assets.keys())
            .chain(from.blobs.keys())
            .chain(to.blobs.keys())
            .copied()
            .collect();
        let entry = |doc: &Document, k: &AssetId| match (doc.assets.get(k), doc.blobs.get(k)) {
            (Some(asset), Some(data)) => Some(AssetEntry {
                asset: asset.clone(),
                data: Bytes(data.clone()),
            }),
            _ => None,
        };
        for k in keys {
            let a = (from.assets.get(&k), from.blobs.get(&k));
            let b = (to.assets.get(&k), to.blobs.get(&k));
            if a != b {
                fa.insert(k, entry(to, &k));
                ba.insert(k, entry(from, &k));
            }
        }
        let (mf, mb) = changed_pair(&from.meta, &to.meta);
        let (sf, sb) = changed_pair(&from.setup, &to.setup);
        let (gf, gb) = changed_pair(&from.grid, &to.grid);
        let (pf, pb) = changed_arc_pair(&from.pages, &to.pages);
        let (xf, xb) = changed_arc_pair(&from.masters, &to.masters);
        let (yf, yb) = changed_pair(&from.styles, &to.styles);
        let (cf, cb) = if same(&from.swatches, &to.swatches) && from.scheme == to.scheme {
            (None, None)
        } else {
            let pal = |d: &Document| Palette {
                swatches: d.swatches.clone(),
                scheme: d.scheme.clone(),
            };
            (Some(pal(to)), Some(pal(from)))
        };
        let (gmf, gmb) = if from.merge == to.merge {
            (None, None)
        } else {
            (
                Some(MergeSlot(to.merge.clone())),
                Some(MergeSlot(from.merge.clone())),
            )
        };
        let (ff, fb) = diff_map_pair(&from.frames, &to.frames);
        let (tf, tb) = diff_map_pair(&from.stories, &to.stories);
        (
            Patch {
                meta: mf,
                setup: sf,
                grid: gf,
                pages: pf,
                masters: xf,
                styles: yf,
                palette: cf,
                merge: gmf,
                frames: ff,
                stories: tf,
                assets: fa,
            },
            Patch {
                meta: mb,
                setup: sb,
                grid: gb,
                pages: pb,
                masters: xb,
                styles: yb,
                palette: cb,
                merge: gmb,
                frames: fb,
                stories: tb,
                assets: ba,
            },
        )
    }

    /// Fold a later step `(step_fwd, step_bwd)` into the accumulated pair
    /// `(self_fwd, self_bwd)`; the result equals the pair diffed from the
    /// start state to the end state.
    pub fn compose_pair(
        (fwd, bwd): (&mut Patch, &mut Patch),
        (step_fwd, step_bwd): (Patch, Patch),
    ) {
        compose_opt(
            &mut fwd.meta,
            &mut bwd.meta,
            step_fwd.meta,
            step_bwd.meta,
            |a, b| a == b,
        );
        compose_opt(
            &mut fwd.setup,
            &mut bwd.setup,
            step_fwd.setup,
            step_bwd.setup,
            |a, b| a == b,
        );
        compose_opt(
            &mut fwd.grid,
            &mut bwd.grid,
            step_fwd.grid,
            step_bwd.grid,
            |a, b| a == b,
        );
        compose_opt(
            &mut fwd.pages,
            &mut bwd.pages,
            step_fwd.pages,
            step_bwd.pages,
            same,
        );
        compose_opt(
            &mut fwd.masters,
            &mut bwd.masters,
            step_fwd.masters,
            step_bwd.masters,
            same,
        );
        compose_opt(
            &mut fwd.styles,
            &mut bwd.styles,
            step_fwd.styles,
            step_bwd.styles,
            |a, b| a == b,
        );
        compose_opt(
            &mut fwd.palette,
            &mut bwd.palette,
            step_fwd.palette,
            step_bwd.palette,
            |a, b| a == b,
        );
        compose_opt(
            &mut fwd.merge,
            &mut bwd.merge,
            step_fwd.merge,
            step_bwd.merge,
            |a, b| a == b,
        );
        compose_map(
            &mut fwd.frames,
            &mut bwd.frames,
            step_fwd.frames,
            step_bwd.frames,
            same_opt,
        );
        compose_map(
            &mut fwd.stories,
            &mut bwd.stories,
            step_fwd.stories,
            step_bwd.stories,
            same_opt,
        );
        compose_map(
            &mut fwd.assets,
            &mut bwd.assets,
            step_fwd.assets,
            step_bwd.assets,
            |a, b| a == b,
        );
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
            doc.setup = v.clone();
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
        if let Some(v) = &self.palette {
            doc.swatches = v.swatches.clone();
            doc.scheme = v.scheme.clone();
        }
        if let Some(v) = &self.merge {
            doc.merge.clone_from(&v.0);
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
            swatches: self.palette.is_some(),
            merge: self.merge.is_some(),
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
    /// Pages added, removed or changed (frames or guides on them, order).
    pub pages: Vec<PageId>,
    pub assets: Vec<AssetId>,
    pub styles: bool,
    /// The swatch list or colour scheme changed (every page repaints).
    #[serde(default)]
    pub swatches: bool,
    /// The merge data or its preview state changed.
    #[serde(default)]
    pub merge: bool,
    pub setup: bool,
}
