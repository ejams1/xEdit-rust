// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! A folder in the system temporary directory that is removed when its
//! guard is dropped.
//!
//! The tests of every crate make their scratch folders through
//! [`TempDir`]: the folder goes when the test ends, whether it passed or
//! panicked (a panic unwinds through the guard's `Drop`). Declare the guard
//! before anything that holds a file in the folder open (a session, a
//! mapped file), so that it drops last.

use std::ops::Deref;
use std::path::{Path, PathBuf};

/// A folder `std::env::temp_dir()/<name>`, created empty by [`TempDir::new`]
/// and removed with everything in it on drop.
#[derive(Debug)]
pub struct TempDir {
    path: PathBuf,
}

impl TempDir {
    /// Creates the folder `name` in the temporary directory. A folder of
    /// that name left by an earlier run is removed first. The name should
    /// carry the process id (and the test's name) so that tests running at
    /// the same time do not share a folder.
    ///
    /// # Panics
    ///
    /// When the folder cannot be created.
    pub fn new(name: impl AsRef<str>) -> TempDir {
        let path = std::env::temp_dir().join(name.as_ref());
        let _ = std::fs::remove_dir_all(&path);
        std::fs::create_dir_all(&path).unwrap_or_else(|error| panic!("creating {}: {error}", path.display()));
        TempDir { path }
    }

    /// The folder.
    pub fn path(&self) -> &Path {
        &self.path
    }
}

impl Deref for TempDir {
    type Target = Path;

    fn deref(&self) -> &Path {
        &self.path
    }
}

impl AsRef<Path> for TempDir {
    fn as_ref(&self) -> &Path {
        &self.path
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.path);
    }
}

#[cfg(test)]
mod tests {
    use super::TempDir;

    #[test]
    fn the_folder_goes_with_the_guard() {
        let path = {
            let dir = TempDir::new(format!("xedit-temp-guard-{}", std::process::id()));
            std::fs::create_dir_all(dir.join("a").join("b")).unwrap();
            std::fs::write(dir.join("a").join("b").join("c.txt"), b"x").unwrap();
            dir.to_path_buf()
        };
        assert!(!path.exists());
    }

    #[test]
    fn the_folder_goes_when_the_test_panics() {
        let name = format!("xedit-temp-guard-panic-{}", std::process::id());
        let path = std::env::temp_dir().join(&name);
        let result = std::panic::catch_unwind(|| {
            let dir = TempDir::new(&name);
            std::fs::write(dir.join("c.txt"), b"x").unwrap();
            panic!("the test fails");
        });
        assert!(result.is_err());
        assert!(!path.exists());
    }
}
