// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Read-only view of a file.

use std::fs::File;
use std::io;
use std::ops::Deref;
use std::path::Path;

use memmap2::Mmap;

/// The content of a file, memory-mapped when the file is not empty.
pub struct MappedFile {
    /// `None` for an empty file, which cannot be mapped.
    map: Option<Mmap>,
}

impl MappedFile {
    pub fn open(path: &Path) -> io::Result<Self> {
        let file = File::open(path)?;
        if file.metadata()?.len() == 0 {
            return Ok(Self { map: None });
        }
        // SAFETY: The mapping is read-only. Its content is undefined when another
        // process truncates or rewrites the file while it is mapped. xEdit has the
        // same requirement: plugin files must not change while they are loaded.
        let map = unsafe { Mmap::map(&file)? };
        Ok(Self { map: Some(map) })
    }
}

impl Deref for MappedFile {
    type Target = [u8];

    fn deref(&self) -> &[u8] {
        self.map.as_deref().unwrap_or_default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn maps_content_and_empty_files() {
        let dir = std::env::temp_dir().join(format!("xedit-io-map-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("data.bin");
        std::fs::write(&path, b"TES4").unwrap();
        assert_eq!(&*MappedFile::open(&path).unwrap(), b"TES4");
        std::fs::write(&path, b"").unwrap();
        assert!(MappedFile::open(&path).unwrap().is_empty());
        assert!(MappedFile::open(&dir.join("missing")).is_err());
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
