// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Memory-mapped files, compression and string encodings.

pub mod compression;
pub mod encoding;
pub mod mapped_file;

pub use compression::{CompressionError, CompressionType};
pub use encoding::{Encoding, EncodingError};
pub use mapped_file::MappedFile;
