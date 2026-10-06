// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

// Ported from xEdit: Core/wbBSA.pas

//! The resource containers, upstream `wbContainerHandler`: the archives and
//! the data folder, searched in the order they were added. The read side is
//! ported (`AddFolder`, `AddBSA`, `ContainerExists`, `OpenResource`); the
//! resource listings and the texture helpers are not.

use std::path::{Path, PathBuf};
use std::sync::{Arc, RwLock};

use xedit_io::{Archive, ArchiveError};

/// Upstream `IwbResourceContainer`: a folder or an archive.
enum Container {
    /// `TwbFolder`: the files below a folder.
    Folder(PathBuf),
    /// `TwbBSAFile`: the files of an archive.
    Archive(Archive),
}

impl Container {
    /// Upstream `Name`: the full path of the folder or archive.
    fn name(&self) -> String {
        match self {
            Container::Folder(path) => path.display().to_string(),
            Container::Archive(archive) => archive.path().to_owned(),
        }
    }

    /// Port of `OpenResource` and `GetData` of the resource: the contents of
    /// the file `path` (relative to the data folder) when the container has it.
    fn open_resource(&self, path: &str) -> Result<Option<Vec<u8>>, ArchiveError> {
        match self {
            Container::Folder(folder) => {
                let file = folder.join(path.replace('\\', "/"));
                if file.is_file() {
                    std::fs::read(&file)
                        .map(Some)
                        .map_err(|error| ArchiveError(format!("[{}] {error}", file.display())))
                } else {
                    Ok(None)
                }
            }
            Container::Archive(archive) => archive.read(path),
        }
    }
}

static CONTAINERS: RwLock<Vec<Arc<Container>>> = RwLock::new(Vec::new());

/// Port of `TwbContainerHandler.ContainerExists`: whether a container of
/// that full path was added, ignoring case.
pub fn container_exists(name: &str) -> bool {
    CONTAINERS
        .read()
        .unwrap()
        .iter()
        .any(|container| container.name().eq_ignore_ascii_case(name))
}

/// Port of `TwbContainerHandler.AddFolder`: the files below `path` as the
/// last container.
pub fn add_folder(path: &Path) {
    if container_exists(&path.display().to_string()) {
        return;
    }
    CONTAINERS
        .write()
        .unwrap()
        .push(Arc::new(Container::Folder(path.to_path_buf())));
}

/// Port of `TwbContainerHandler.AddBSA`: the files of the archive as the
/// last container.
pub fn add_archive(path: &Path) -> Result<(), ArchiveError> {
    if container_exists(&path.display().to_string()) {
        return Ok(());
    }
    let archive = Archive::open(path)?;
    CONTAINERS.write().unwrap().push(Arc::new(Container::Archive(archive)));
    Ok(())
}

/// Forgets every container, for the tests and a new session.
pub fn clear_containers() {
    CONTAINERS.write().unwrap().clear();
}

/// Port of `TwbContainerHandler.OpenResource`: the contents of the file
/// `path` (relative to the data folder) from every container that has it,
/// in the order the containers were added. A container that fails to read
/// the file is skipped like a missing resource.
pub fn open_resource(path: &str) -> Vec<Vec<u8>> {
    let containers = CONTAINERS.read().unwrap().clone();
    containers
        .iter()
        .filter_map(|container| container.open_resource(path).ok().flatten())
        .collect()
}

/// `OpenResource` taking the last resource, which is the one the loaders
/// use: the container added last wins.
pub fn open_resource_last(path: &str) -> Option<Vec<u8>> {
    open_resource(path).pop()
}
