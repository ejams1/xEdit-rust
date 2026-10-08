// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

// Ported from xEdit: Core/wbBSA.pas

//! The resource containers, upstream `wbContainerHandler`: the archives and
//! the data folder, searched in the order they were added: `AddFolder`,
//! `AddBSA`, `ContainerExists`, `OpenResource`, the resource listings of
//! the containers (`ContainerResourceList`, `ContainerResourceDict`,
//! `ResourceExists`, `ResourceCount`, `ResourceCopy`, `OpenResourceData`) and
//! the tables that give a file or folder hash its name (`ResolveFileHash`,
//! `ResolveFolderHash`). The texture helpers belong to the DDS code.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, RwLock};

use xedit_io::archive::{extract_file_ext, extract_file_name, include_trailing_path_delimiter, packer::list_files};
use xedit_io::{Archive, ArchiveError};

use crate::interface::globals::{GameMode, game_mode, game_name};

/// Upstream `IwbResourceContainer`: a folder or an archive.
enum Container {
    /// `TwbFolder`: the files below a folder.
    Folder(PathBuf),
    /// `TwbBSAFile`: the files of an archive.
    Archive(Box<Archive>),
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
    invalidate_cache();
}

/// Port of `TwbContainerHandler.AddBSA`: the files of the archive as the
/// last container.
pub fn add_archive(path: &Path) -> Result<(), ArchiveError> {
    if container_exists(&path.display().to_string()) {
        return Ok(());
    }
    let archive = Archive::open(path)?;
    CONTAINERS
        .write()
        .unwrap()
        .push(Arc::new(Container::Archive(Box::new(archive))));
    invalidate_cache();
    Ok(())
}

/// Forgets every container, for the tests and a new session.
pub fn clear_containers() {
    CONTAINERS.write().unwrap().clear();
    invalidate_cache();
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

impl Container {
    /// Port of `ResourceExists`.
    fn resource_exists(&self, path: &str) -> bool {
        match self {
            Container::Folder(folder) => folder.join(path.replace('\\', "/")).is_file(),
            Container::Archive(archive) => archive.file_exists(path),
        }
    }

    /// Port of `ResourceList`: the names of the files below `folder` (all of
    /// them for an empty folder). A folder container gives the lower-case
    /// paths relative to it, an archive the names it holds.
    fn resource_list(&self, folder: &str) -> Vec<String> {
        match self {
            Container::Folder(root) => {
                let folder = folder.replace('/', "\\");
                let start = root.join(folder.replace('\\', "/"));
                if !start.is_dir() {
                    return Vec::new();
                }
                // `TDirectory.GetFiles(fPath + aFolder, '*.*', soAllDirectories)`
                // names the files with that path in front.
                let prefix = format!(
                    "{}{}",
                    include_trailing_path_delimiter(&root.display().to_string()),
                    folder
                );
                let start_text = if folder.is_empty() {
                    prefix.clone()
                } else {
                    include_trailing_path_delimiter(&prefix)
                };
                let mut files = Vec::new();
                if list_files(&start, &start_text, &mut files).is_err() {
                    return Vec::new();
                }
                let root_length = include_trailing_path_delimiter(&root.display().to_string())
                    .chars()
                    .count();
                files
                    .into_iter()
                    .map(|file| {
                        let relative: String = file.chars().skip(root_length).collect();
                        xedit_io::encoding::lower_case(&relative)
                    })
                    .collect()
            }
            Container::Archive(archive) => archive
                .files_by_folder(folder)
                .into_iter()
                .map(|file| file.name.clone())
                .collect(),
        }
    }
}

/// Upstream `TwbResourceDict`: a set of names.
pub type ResourceDict = HashSet<String>;

/// Port of `ContainerList`: the full path of every container, in the order
/// they were added.
pub fn container_list() -> Vec<String> {
    CONTAINERS
        .read()
        .unwrap()
        .iter()
        .map(|container| container.name())
        .collect()
}

/// Port of `ContainerResourceList`: the names of the files below `folder`
/// of the container named `container_name` (of every container for an empty
/// name), in the order of the containers. Duplicates are not removed.
pub fn container_resource_list(container_name: &str, folder: &str) -> Vec<String> {
    let mut list = Vec::new();
    for container in CONTAINERS.read().unwrap().iter() {
        if container_name.is_empty() || container.name().eq_ignore_ascii_case(container_name) {
            list.extend(container.resource_list(folder));
            if !container_name.is_empty() {
                break;
            }
        }
    }
    list
}

/// Port of `ContainerResourceDict`: as `container_resource_list`, but into a
/// set that keeps the first spelling of a name.
pub fn container_resource_dict(container_name: &str, folder: &str, dict: &mut ResourceDict) {
    dict.extend(container_resource_list(container_name, folder));
}

/// Port of `TwbContainerHandler.ResourceExists`: whether any container has
/// the file.
pub fn resource_exists(path: &str) -> bool {
    CONTAINERS
        .read()
        .unwrap()
        .iter()
        .any(|container| container.resource_exists(path))
}

/// Port of `TwbContainerHandler.ResourceCount`: in how many containers the
/// file is, and the names of those containers.
pub fn resource_count(path: &str) -> (usize, Vec<String>) {
    let containers: Vec<String> = CONTAINERS
        .read()
        .unwrap()
        .iter()
        .filter(|container| container.resource_exists(path))
        .map(|container| container.name())
        .collect();
    (containers.len(), containers)
}

/// Port of `OpenResourceData`: the contents of the file from the container
/// named `container_name` (the last one that has it for an empty name).
pub fn open_resource_data(container_name: &str, path: &str) -> Vec<u8> {
    let containers = CONTAINERS.read().unwrap().clone();
    for container in containers.iter().rev() {
        if !(container_name.is_empty() || container.name().eq_ignore_ascii_case(container_name)) {
            continue;
        }
        if let Ok(Some(data)) = container.open_resource(path) {
            return data;
        }
    }
    Vec::new()
}

/// A failure of `resource_copy`: the upstream message.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("{0}")]
pub struct ResourceError(pub String);

/// Port of `TwbContainerHandler.ResourceCopy`: writes the file from the
/// container named `container_name` (the last one that has it for an empty
/// name) to `path_out`, a file name when it has an extension, else a folder.
pub fn resource_copy(container_name: &str, file_name: &str, path_out: &str) -> Result<(), ResourceError> {
    if path_out.is_empty() {
        return Err(ResourceError("Destination path is not specified".to_owned()));
    }
    let containers: Vec<Arc<Container>> = CONTAINERS
        .read()
        .unwrap()
        .iter()
        .filter(|container| container.resource_exists(file_name))
        .cloned()
        .collect();
    if containers.is_empty() {
        return Err(ResourceError("Resource doesn't exist".to_owned()));
    }
    let chosen = containers
        .iter()
        .rev()
        .find(|container| container_name.is_empty() || container.name().eq_ignore_ascii_case(container_name))
        .unwrap_or_else(|| containers.last().expect("a container"));

    // A file name is provided instead of a path.
    let target = if Path::new(path_out).extension().is_some() {
        path_out.to_owned()
    } else {
        format!("{}{file_name}", include_trailing_path_delimiter(path_out))
    };
    let directory = crate::delphi::extract_file_path(&target);
    if !directory.is_empty() && !Path::new(&directory).is_dir() && std::fs::create_dir_all(&directory).is_err() {
        return Err(ResourceError(format!(
            "Unable to create destination directory {directory}"
        )));
    }
    let data = chosen
        .open_resource(file_name)
        .ok()
        .flatten()
        .ok_or_else(|| ResourceError("Resource doesn't exist".to_owned()))?;
    std::fs::write(&target, data).map_err(|error| ResourceError(format!("{target}: {error}")))
}

/// Upstream `wbLoaderDone`: the callbacks resolve the hash of a file or a
/// folder to its name only once the load order is loaded.
static LOADER_DONE: AtomicBool = AtomicBool::new(false);

pub fn loader_done() -> bool {
    LOADER_DONE.load(Ordering::Relaxed)
}

pub fn set_loader_done(done: bool) {
    LOADER_DONE.store(done, Ordering::Relaxed);
}

/// `TwbContainerCache`: the names of the files and folders of the
/// containers by their hash.
struct ContainerCache {
    file_hashes: HashMap<i64, String>,
    folder_hashes: HashMap<i64, String>,
}

static CACHE: RwLock<Option<Arc<ContainerCache>>> = RwLock::new(None);

fn invalidate_cache() {
    *CACHE.write().unwrap() = None;
}

/// `TwbHash.BSCRC32` from Skyrim on, `TwbHash.TES4` before.
fn cache_hash(text: &str) -> i64 {
    if game_mode() >= GameMode::gmTES5 {
        i64::from(xedit_io::hash::bscrc32(text))
    } else {
        xedit_io::hash::tes4(text, true) as i64
    }
}

/// Port of `TwbContainerHandler.BuildCache`.
///
/// DEVIATION: upstream writes the seeds it read back to
/// `<game>.HashSeed.txt2` next to the program, without the names the
/// containers have already; the port reads the seeds and writes nothing.
fn build_cache() -> ContainerCache {
    let mut all: ResourceDict = HashSet::new();
    container_resource_dict("", "", &mut all);

    let seed_name = format!(
        "{}{}.HashSeed.txt",
        include_trailing_path_delimiter(&crate::delphi::extract_file_path(&crate::delphi::exe_path())),
        game_name()
    );
    if let Ok(text) = std::fs::read_to_string(&seed_name) {
        for line in text.lines() {
            all.insert(xedit_io::encoding::lower_case(line).replace('/', "\\"));
        }
    }

    let mut files: HashSet<String> = HashSet::new();
    let mut folders: HashSet<String> = HashSet::new();
    let mut cache = ContainerCache {
        file_hashes: HashMap::new(),
        folder_hashes: HashMap::new(),
    };
    for full_name in &all {
        let directory = crate::delphi::extract_file_path(full_name);
        let folder = xedit_io::encoding::lower_case(directory.trim_end_matches('\\')).replace('/', "\\");
        if folders.insert(folder.clone()) {
            cache.folder_hashes.entry(cache_hash(&folder)).or_insert(folder);
        }

        let mut file = xedit_io::encoding::lower_case(extract_file_name(full_name));
        if game_mode() >= GameMode::gmTES5 {
            file = crate::delphi::change_file_ext(&file, "");
        }
        if files.insert(file.clone()) {
            cache
                .file_hashes
                .entry(cache_hash(&file))
                .or_insert_with(|| file.clone());

            if game_mode() < GameMode::gmTES5 && extract_file_ext(&file) == ".dds" {
                file = crate::delphi::change_file_ext(&file, ".ddx");
            }
            // UPSTREAM-QUIRK: this second step is not part of the `if` above
            // in the source (the indentation misleads); it adds the `.ddx`
            // name of a texture before Skyrim, and a name that is there
            // already for the other games.
            if files.insert(file.clone()) {
                cache.file_hashes.entry(cache_hash(&file)).or_insert(file);
            }
        }
    }
    cache
}

fn cache() -> Arc<ContainerCache> {
    if let Some(cache) = CACHE.read().unwrap().as_ref() {
        return cache.clone();
    }
    let built = Arc::new(build_cache());
    *CACHE.write().unwrap() = Some(built.clone());
    built
}

/// Port of `ResolveFileHash`: the name of the file with the hash, or an
/// empty string. For a file hash of a game from Skyrim on, the name has no
/// extension.
pub fn resolve_file_hash(hash: i64) -> String {
    cache().file_hashes.get(&hash).cloned().unwrap_or_default()
}

/// Port of `ResolveFolderHash`: the name of the folder with the hash, or an
/// empty string.
pub fn resolve_folder_hash(hash: i64) -> String {
    cache().folder_hashes.get(&hash).cloned().unwrap_or_default()
}
