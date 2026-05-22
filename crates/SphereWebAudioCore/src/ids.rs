//! Type-safe ID wrappers for engine entities.
//!
//! All IDs are strings to stay compatible with the TypeScript project state
//! which uses UUIDs / nanoid strings.

use serde::{Deserialize, Serialize};
use std::fmt;

macro_rules! define_id {
    ($name:ident) => {
        #[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
        pub struct $name(pub String);

        impl $name {
            pub fn new(id: impl Into<String>) -> Self {
                Self(id.into())
            }
            pub fn as_str(&self) -> &str {
                &self.0
            }
        }

        impl fmt::Display for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                write!(f, "{}", self.0)
            }
        }

        impl From<&str> for $name {
            fn from(s: &str) -> Self {
                Self(s.to_string())
            }
        }

        impl From<String> for $name {
            fn from(s: String) -> Self {
                Self(s)
            }
        }
    };
}

define_id!(TrackId);
define_id!(ClipId);
define_id!(DeviceId);
define_id!(AssetId);
define_id!(SendId);
