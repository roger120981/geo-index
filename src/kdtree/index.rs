use std::marker::PhantomData;

use bytemuck::cast_slice;

use crate::error::{GeoIndexError, Result};
use crate::indices::Indices;
use crate::kdtree::constants::{KDBUSH_HEADER_SIZE, KDBUSH_MAGIC, KDBUSH_VERSION};
use crate::r#type::IndexableNum;

/// Common metadata to describe a KDTree
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct KDTreeMetadata<N: IndexableNum> {
    pub(crate) node_size: usize,
    pub(crate) num_items: usize,
    phantom: PhantomData<N>,
    pub(crate) indices_byte_size: usize,
    pub(crate) pad_coords_byte_size: usize,
    pub(crate) coords_byte_size: usize,
}

impl<N: IndexableNum> KDTreeMetadata<N> {
    pub(crate) fn new(num_items: u32, node_size: u16) -> Self {
        assert!((2..=65535).contains(&node_size));

        // The public API uses u32 and u16 types but internally we use usize
        let num_items = num_items as usize;
        let node_size = node_size as usize;

        let coords_byte_size = num_items * 2 * N::BYTES_PER_ELEMENT;
        let indices_bytes_per_element = if num_items < 65536 { 2 } else { 4 };
        let indices_byte_size = num_items * indices_bytes_per_element;
        let pad_coords_byte_size = (8 - (indices_byte_size % 8)) % 8;

        Self {
            node_size,
            num_items,
            phantom: PhantomData,
            indices_byte_size,
            pad_coords_byte_size,
            coords_byte_size,
        }
    }

    fn try_new_from_slice(data: &[u8]) -> Result<Self> {
        if data[0] != KDBUSH_MAGIC {
            return Err(GeoIndexError::General(
                "Data not in Kdbush format.".to_string(),
            ));
        }

        let version_and_type = data[1];
        let version = version_and_type >> 4;
        if version != KDBUSH_VERSION {
            return Err(GeoIndexError::General(
                format!("Got v{} data when expected v{}.", version, KDBUSH_VERSION).to_string(),
            ));
        }

        let type_ = version_and_type & 0x0f;
        if type_ != N::TYPE_INDEX {
            return Err(GeoIndexError::General(
                format!(
                    "Got type {} data when expected type {}.",
                    type_,
                    N::TYPE_INDEX
                )
                .to_string(),
            ));
        }

        let node_size: u16 = cast_slice(&data[2..4])[0];
        let num_items: u32 = cast_slice(&data[4..8])[0];

        let node_size = node_size as usize;
        let num_items = num_items as usize;

        let coords_byte_size = num_items * 2 * N::BYTES_PER_ELEMENT;
        let indices_bytes_per_element = if num_items < 65536 { 2 } else { 4 };
        let indices_byte_size = num_items * indices_bytes_per_element;
        let pad_coords_byte_size = (8 - (indices_byte_size % 8)) % 8;

        let data_buffer_length =
            KDBUSH_HEADER_SIZE + coords_byte_size + indices_byte_size + pad_coords_byte_size;
        assert_eq!(data.len(), data_buffer_length);

        Ok(Self {
            node_size,
            num_items,
            phantom: PhantomData,
            indices_byte_size,
            pad_coords_byte_size,
            coords_byte_size,
        })
    }

    pub(crate) fn data_buffer_length(&self) -> usize {
        KDBUSH_HEADER_SIZE
            + self.coords_byte_size
            + self.indices_byte_size
            + self.pad_coords_byte_size
    }

    pub(crate) fn coords_slice<'a>(&self, data: &'a [u8]) -> &'a [N] {
        let coords_byte_start =
            KDBUSH_HEADER_SIZE + self.indices_byte_size + self.pad_coords_byte_size;
        let coords_byte_end = KDBUSH_HEADER_SIZE
            + self.indices_byte_size
            + self.pad_coords_byte_size
            + self.coords_byte_size;
        cast_slice(&data[coords_byte_start..coords_byte_end])
    }

    pub(crate) fn indices_slice<'a>(&self, data: &'a [u8]) -> Indices<'a> {
        let indices_buf = &data[KDBUSH_HEADER_SIZE..KDBUSH_HEADER_SIZE + self.indices_byte_size];

        if self.num_items < 65536 {
            Indices::U16(cast_slice(indices_buf))
        } else {
            Indices::U32(cast_slice(indices_buf))
        }
    }
}

/// An owned KDTree buffer.
///
/// Usually this will be created from scratch via [`KDTreeBuilder`][crate::kdtree::KDTreeBuilder].
#[derive(Debug, Clone, PartialEq)]
pub struct OwnedKDTree<N: IndexableNum> {
    pub(crate) buffer: Vec<u8>,
    pub(crate) metadata: KDTreeMetadata<N>,
}

impl<N: IndexableNum> OwnedKDTree<N> {
    /// Consume this KDTree, returning the underlying buffer.
    pub fn into_inner(self) -> Vec<u8> {
        self.buffer
    }
}

impl<N: IndexableNum> AsRef<[u8]> for OwnedKDTree<N> {
    fn as_ref(&self) -> &[u8] {
        &self.buffer
    }
}

/// A reference on an external KDTree buffer.
///
/// This will often be created from an [`OwnedKDTree`] via its
/// [`as_kdtree_ref`][OwnedKDTree::as_kdtree_ref] method, but it can also be created from any
/// existing data buffer.
#[derive(Debug, Clone, PartialEq)]
pub struct KDTreeRef<'a, N: IndexableNum> {
    pub(crate) coords: &'a [N],
    pub(crate) indices: Indices<'a>,
    pub(crate) metadata: KDTreeMetadata<N>,
}

impl<'a, N: IndexableNum> KDTreeRef<'a, N> {
    /// Construct a new KDTreeRef from an external byte slice.
    ///
    /// This byte slice must conform to the "kdbush ABI", that is, the ABI originally implemented
    /// by the JavaScript [`kdbush` library](https://github.com/mourner/kdbush). You can extract
    /// such a buffer either via [`OwnedKDTree::into_inner`] or from the `.data` attribute of the
    /// JavaScript `KDBush` object.
    pub fn try_new<T: AsRef<[u8]>>(data: &'a T) -> Result<Self> {
        let data = data.as_ref();
        let metadata = KDTreeMetadata::try_new_from_slice(data)?;
        let coords = metadata.coords_slice(data);
        let indices = metadata.indices_slice(data);

        Ok(Self {
            coords,
            indices,
            metadata,
        })
    }
}
