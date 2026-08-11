//! Distinct coordinate and index types used by the evidence contract.

use std::{fmt, str::FromStr};

use serde::{Deserialize, Deserializer, Serialize, de::Error as _};

/// A rejected one-based coordinate.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CoordinateError {
    name: &'static str,
}

impl fmt::Display for CoordinateError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{} must be one-based", self.name)
    }
}

impl std::error::Error for CoordinateError {}

macro_rules! one_based_coordinate {
    ($(#[$meta:meta])* $name:ident, $label:literal) => {
        $(#[$meta])*
        #[repr(transparent)]
        #[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
        #[serde(transparent)]
        pub struct $name(usize);

        impl $name {
            /// Construct a coordinate, rejecting zero.
            pub const fn new(value: usize) -> Result<Self, CoordinateError> {
                if value == 0 {
                    Err(CoordinateError { name: $label })
                } else {
                    Ok(Self(value))
                }
            }

            /// Return the underlying one-based value.
            #[must_use]
            pub const fn get(self) -> usize {
                self.0
            }
        }

        impl TryFrom<usize> for $name {
            type Error = CoordinateError;

            fn try_from(value: usize) -> Result<Self, Self::Error> {
                Self::new(value)
            }
        }

        impl<'de> Deserialize<'de> for $name {
            fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
            where
                D: Deserializer<'de>,
            {
                let value = usize::deserialize(deserializer)?;
                Self::new(value).map_err(D::Error::custom)
            }
        }

        impl FromStr for $name {
            type Err = String;

            fn from_str(value: &str) -> Result<Self, Self::Err> {
                value
                    .parse::<usize>()
                    .map_err(|error| error.to_string())
                    .and_then(|value| Self::new(value).map_err(|error| error.to_string()))
            }
        }

        impl fmt::Display for $name {
            fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
                self.0.fmt(formatter)
            }
        }

        impl PartialEq<usize> for $name {
            fn eq(&self, other: &usize) -> bool {
                self.0 == *other
            }
        }

        impl PartialEq<$name> for usize {
            fn eq(&self, other: &$name) -> bool {
                *self == other.0
            }
        }
    };
}

macro_rules! zero_based_index {
    ($(#[$meta:meta])* $name:ident) => {
        $(#[$meta])*
        #[repr(transparent)]
        #[derive(
            Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize,
        )]
        #[serde(transparent)]
        pub struct $name(usize);

        impl $name {
            /// Construct an index. Zero is the first valid value.
            #[must_use]
            pub const fn new(value: usize) -> Self {
                Self(value)
            }

            /// Return the underlying zero-based value.
            #[must_use]
            pub const fn get(self) -> usize {
                self.0
            }
        }

        impl From<usize> for $name {
            fn from(value: usize) -> Self {
                Self::new(value)
            }
        }

        impl FromStr for $name {
            type Err = std::num::ParseIntError;

            fn from_str(value: &str) -> Result<Self, Self::Err> {
                value.parse().map(Self::new)
            }
        }

        impl fmt::Display for $name {
            fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
                self.0.fmt(formatter)
            }
        }

        impl PartialEq<usize> for $name {
            fn eq(&self, other: &usize) -> bool {
                self.0 == *other
            }
        }

        impl PartialEq<$name> for usize {
            fn eq(&self, other: &$name) -> bool {
                *self == other.0
            }
        }
    };
}

one_based_coordinate!(
    /// A one-based Markdown source line.
    Line,
    "line"
);
one_based_coordinate!(
    /// A one-based Markdown character column.
    Column,
    "column"
);
one_based_coordinate!(
    /// A one-based physical PDF page.
    Page,
    "page"
);
zero_based_index!(
    /// A zero-based claim index.
    ClaimIndex
);
zero_based_index!(
    /// A zero-based locator index within one claim.
    LocatorIndex
);
