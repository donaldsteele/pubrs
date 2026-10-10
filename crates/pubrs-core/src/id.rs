//! Stable object identifiers.
//!
//! Every addressable object (page, frame, story, style, asset, guide, master)
//! carries a UUID that never changes for the life of the object. Clients
//! (GUI, MCP, scripts) address objects by these ids, never by index.

use std::fmt;
use std::str::FromStr;

use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// A source of fresh UUIDs.
///
/// Commands carry the ids of anything they create, so applying a command is
/// deterministic. Ids are allocated by whoever builds the command.
pub trait IdSource {
    fn next_uuid(&mut self) -> Uuid;
}

/// Random v4 UUIDs (the normal choice for interactive sessions).
#[cfg(feature = "random-ids")]
#[derive(Debug, Default, Clone, Copy)]
pub struct RandomIds;

#[cfg(feature = "random-ids")]
impl IdSource for RandomIds {
    fn next_uuid(&mut self) -> Uuid {
        Uuid::new_v4()
    }
}

/// Deterministic ids: a 64-bit seed and a counter packed into a UUIDv8.
///
/// Used by tests, golden files and built-in templates so that output is
/// reproducible on every OS.
#[derive(Debug, Clone)]
pub struct SeqIds {
    seed: u64,
    counter: u64,
}

impl SeqIds {
    pub fn new(seed: u64) -> Self {
        Self { seed, counter: 0 }
    }
}

impl IdSource for SeqIds {
    fn next_uuid(&mut self) -> Uuid {
        self.counter += 1;
        let mut bytes = [0u8; 16];
        bytes[..8].copy_from_slice(&self.seed.to_be_bytes());
        bytes[8..].copy_from_slice(&self.counter.to_be_bytes());
        uuid::Builder::from_custom_bytes(bytes).into_uuid()
    }
}

macro_rules! typed_id {
    ($(#[$meta:meta])* $name:ident) => {
        $(#[$meta])*
        #[derive(Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
        #[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
        #[serde(transparent)]
        pub struct $name(pub Uuid);

        impl $name {
            pub fn new(ids: &mut dyn IdSource) -> Self {
                Self(ids.next_uuid())
            }

            pub fn uuid(&self) -> Uuid {
                self.0
            }
        }

        impl fmt::Debug for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                write!(f, "{}({})", stringify!($name), self.0)
            }
        }

        impl fmt::Display for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                fmt::Display::fmt(&self.0, f)
            }
        }

        impl FromStr for $name {
            type Err = uuid::Error;
            fn from_str(s: &str) -> Result<Self, Self::Err> {
                Uuid::parse_str(s).map(Self)
            }
        }
    };
}

typed_id!(
    /// Identifies a document within a session.
    DocumentId
);
typed_id!(
    /// Identifies a page.
    PageId
);
typed_id!(
    /// Identifies a master page.
    MasterId
);
typed_id!(
    /// Identifies a frame (text, image, shape or group).
    FrameId
);
typed_id!(
    /// Identifies a story: the text that flows through a chain of linked text frames.
    StoryId
);
typed_id!(
    /// Identifies a paragraph or character style.
    StyleId
);
typed_id!(
    /// Identifies an embedded asset (image or SVG).
    AssetId
);
typed_id!(
    /// Identifies a ruler guide.
    GuideId
);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn seq_ids_are_deterministic_and_unique() {
        let mut a = SeqIds::new(7);
        let mut b = SeqIds::new(7);
        let x = FrameId::new(&mut a);
        let y = FrameId::new(&mut a);
        assert_ne!(x, y);
        assert_eq!(x, FrameId::new(&mut b));
    }

    #[test]
    fn ids_round_trip_through_strings() {
        let id = PageId::new(&mut SeqIds::new(1));
        let parsed: PageId = id.to_string().parse().unwrap();
        assert_eq!(id, parsed);
        let json = serde_json::to_string(&id).unwrap();
        assert_eq!(json, format!("\"{id}\""));
        assert_eq!(serde_json::from_str::<PageId>(&json).unwrap(), id);
    }
}
