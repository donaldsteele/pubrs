//! Font library: bundled OFL fonts plus lazily-registered system fonts.
//!
//! Layout only ever sees fonts registered here. Bundled fonts are always
//! present (and are the generic sans/serif fallbacks), so a document renders
//! the same on any machine; a family that cannot be found is reported as
//! missing and drawn with the fallback.

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;
use std::sync::Arc;

use parley::FontContext;
use parley::fontique::{
    Blob, Collection, CollectionOptions, FallbackKey, GenericFamily, Script, SourceCache,
};

/// Fonts compiled into the binary (SIL OFL 1.1, see `fonts/OFL.txt`).
pub const BUNDLED_FONTS: &[(&str, &[u8])] = &[
    (
        "LiberationSans-Regular.ttf",
        include_bytes!("../fonts/LiberationSans-Regular.ttf"),
    ),
    (
        "LiberationSans-Bold.ttf",
        include_bytes!("../fonts/LiberationSans-Bold.ttf"),
    ),
    (
        "LiberationSans-Italic.ttf",
        include_bytes!("../fonts/LiberationSans-Italic.ttf"),
    ),
    (
        "LiberationSans-BoldItalic.ttf",
        include_bytes!("../fonts/LiberationSans-BoldItalic.ttf"),
    ),
    (
        "LiberationSerif-Regular.ttf",
        include_bytes!("../fonts/LiberationSerif-Regular.ttf"),
    ),
    (
        "LiberationSerif-Bold.ttf",
        include_bytes!("../fonts/LiberationSerif-Bold.ttf"),
    ),
    (
        "LiberationSerif-Italic.ttf",
        include_bytes!("../fonts/LiberationSerif-Italic.ttf"),
    ),
    (
        "LiberationSerif-BoldItalic.ttf",
        include_bytes!("../fonts/LiberationSerif-BoldItalic.ttf"),
    ),
];

/// Family used for text whose font is missing.
pub const FALLBACK_SANS: &str = "Liberation Sans";
/// Generic serif family.
pub const FALLBACK_SERIF: &str = "Liberation Serif";

/// Where a font family comes from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum FontSource {
    Bundled,
    System,
    Added,
}

/// A family known to the library.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FamilyEntry {
    pub name: String,
    pub source: FontSource,
    /// True once the font data is registered with the shaper.
    pub loaded: bool,
}

/// The set of fonts available to layout.
pub struct FontLibrary {
    pub(crate) fcx: FontContext,
    /// Lower-cased name → entry.
    families: BTreeMap<String, FamilyEntry>,
    system: Option<fontdb::Database>,
    loaded_system_faces: BTreeSet<fontdb::ID>,
}

impl std::fmt::Debug for FontLibrary {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("FontLibrary")
            .field("families", &self.families.len())
            .field("system", &self.system.is_some())
            .finish()
    }
}

impl FontLibrary {
    /// A library with no fonts at all.
    pub fn empty() -> Self {
        Self {
            fcx: FontContext {
                collection: Collection::new(CollectionOptions {
                    shared: false,
                    system_fonts: false,
                }),
                source_cache: SourceCache::default(),
            },
            families: BTreeMap::new(),
            system: None,
            loaded_system_faces: BTreeSet::new(),
        }
    }

    /// Bundled fonts only: deterministic on every machine.
    pub fn bundled() -> Self {
        let mut lib = Self::empty();
        for (_, data) in BUNDLED_FONTS {
            lib.register_blob(Blob::new(Arc::new(*data)), FontSource::Bundled);
        }
        lib.set_fallbacks();
        lib
    }

    /// Bundled fonts plus every installed system font (registered on first use).
    pub fn with_system_fonts() -> Self {
        let mut lib = Self::bundled();
        let mut db = fontdb::Database::new();
        db.load_system_fonts();
        for face in db.faces() {
            for (name, _) in &face.families {
                lib.families
                    .entry(name.to_lowercase())
                    .or_insert_with(|| FamilyEntry {
                        name: name.clone(),
                        source: FontSource::System,
                        loaded: false,
                    });
            }
        }
        tracing::info!(faces = db.len(), "system fonts discovered");
        lib.system = Some(db);
        lib
    }

    /// Only the font files given (used by golden tests).
    pub fn from_files(paths: &[&Path]) -> std::io::Result<Self> {
        let mut lib = Self::empty();
        for p in paths {
            let data = std::fs::read(p)?;
            lib.register_blob(Blob::new(Arc::new(data)), FontSource::Added);
        }
        lib.set_fallbacks();
        Ok(lib)
    }

    /// Every `.ttf`/`.otf` in a directory, sorted by file name.
    pub fn from_dir(dir: &Path) -> std::io::Result<Self> {
        let mut files: Vec<_> = std::fs::read_dir(dir)?
            .filter_map(Result::ok)
            .map(|e| e.path())
            .filter(|p| {
                p.extension()
                    .and_then(|e| e.to_str())
                    .is_some_and(|e| matches!(e.to_ascii_lowercase().as_str(), "ttf" | "otf"))
            })
            .collect();
        files.sort();
        let refs: Vec<&Path> = files.iter().map(|p| p.as_path()).collect();
        Self::from_files(&refs)
    }

    /// Register font bytes (a .ttf/.otf/.ttc). Returns the family names added.
    pub fn add_font(&mut self, data: Vec<u8>) -> Vec<String> {
        let names = self.register_blob(Blob::new(Arc::new(data)), FontSource::Added);
        self.set_fallbacks();
        names
    }

    fn register_blob(&mut self, blob: Blob<u8>, source: FontSource) -> Vec<String> {
        let added = self.fcx.collection.register_fonts(blob, None);
        let mut names = Vec::new();
        for (fid, _) in added {
            if let Some(name) = self.fcx.collection.family_name(fid) {
                let name = name.to_string();
                let e = self
                    .families
                    .entry(name.to_lowercase())
                    .or_insert_with(|| FamilyEntry {
                        name: name.clone(),
                        source,
                        loaded: true,
                    });
                e.loaded = true;
                names.push(name);
            }
        }
        names.sort();
        names.dedup();
        names
    }

    fn set_fallbacks(&mut self) {
        let c = &mut self.fcx.collection;
        let first = {
            let mut names: Vec<String> = c.family_names().map(String::from).collect();
            names.sort();
            names.first().and_then(|n| c.family_id(n))
        };
        let sans = c.family_id(FALLBACK_SANS).or(first);
        let serif = c.family_id(FALLBACK_SERIF).or(sans);
        if let Some(sans) = sans {
            c.set_generic_families(GenericFamily::SansSerif, [sans].into_iter());
            c.set_generic_families(GenericFamily::SystemUi, [sans].into_iter());
            c.set_generic_families(GenericFamily::Monospace, [sans].into_iter());
            c.set_fallbacks(
                FallbackKey::new(Script::from_bytes(*b"Latn"), None),
                [sans].into_iter(),
            );
        }
        if let Some(serif) = serif {
            c.set_generic_families(GenericFamily::Serif, [serif].into_iter());
        }
    }

    /// True if the family is registered and usable right now.
    pub fn is_loaded(&self, family: &str) -> bool {
        self.families
            .get(&family.to_lowercase())
            .is_some_and(|e| e.loaded)
    }

    /// Make a family usable, loading it from the system if needed.
    /// Returns false if the family does not exist anywhere.
    pub fn ensure_family(&mut self, family: &str) -> bool {
        let key = family.to_lowercase();
        match self.families.get(&key) {
            Some(e) if e.loaded => return true,
            Some(_) => {}
            None => return false,
        }
        let Some(db) = &self.system else {
            return false;
        };
        let ids: Vec<fontdb::ID> = db
            .faces()
            .filter(|f| f.families.iter().any(|(n, _)| n.to_lowercase() == key))
            .map(|f| f.id)
            .collect();
        let mut blobs = Vec::new();
        for id in ids {
            if self.loaded_system_faces.insert(id)
                && let Some(Some(data)) = db.with_face_data(id, |data, _| Some(data.to_vec()))
            {
                blobs.push(data);
            }
        }
        for data in blobs {
            self.register_blob(Blob::new(Arc::new(data)), FontSource::System);
        }
        let ok = self.is_loaded(family);
        if ok {
            tracing::debug!(family, "loaded system font");
        }
        ok
    }

    /// All families, sorted by name.
    pub fn families(&self) -> Vec<FamilyEntry> {
        let mut v: Vec<FamilyEntry> = self.families.values().cloned().collect();
        v.sort_by_key(|f| f.name.to_lowercase());
        v
    }

    /// The canonical display name for a family, if known.
    pub fn canonical_name(&self, family: &str) -> Option<&str> {
        self.families
            .get(&family.to_lowercase())
            .map(|e| e.name.as_str())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bundled_families_present() {
        let lib = FontLibrary::bundled();
        let names: Vec<String> = lib.families().into_iter().map(|f| f.name).collect();
        assert_eq!(names, vec!["Liberation Sans", "Liberation Serif"]);
        assert!(lib.is_loaded("liberation sans"));
        assert!(!lib.is_loaded("Comic Sans MS"));
    }

    #[test]
    fn missing_family_is_reported() {
        let mut lib = FontLibrary::bundled();
        assert!(!lib.ensure_family("Definitely Not A Font"));
        assert!(lib.ensure_family("Liberation Serif"));
    }
}
