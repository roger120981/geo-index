#![doc = include_str!("../README.md")]
#![warn(missing_docs)]

mod error;
pub mod indices;
pub mod kdtree;
pub mod rtree;
mod r#type;

pub use error::GeoIndexError;
pub use r#type::{CoordType, IndexableNum};

#[cfg(test)]
pub(crate) mod test;
