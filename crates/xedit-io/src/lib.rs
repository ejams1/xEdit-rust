// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Memory-mapped files, compression and string encodings.

pub mod archive;
pub mod compression;
pub mod dds;
pub mod encoding;
pub mod hash;
pub mod mapped_file;
pub mod simd;

pub use archive::{Archive, ArchiveError, ArchiveType, MultiSourcePacker};
pub use compression::{CompressionError, CompressionType};
pub use encoding::{Encoding, EncodingError};
pub use hash::crc32;
pub use mapped_file::MappedFile;
