// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

// Ported from xEdit: Core/wbBSA.pas

//! The resource containers, upstream `wbContainerHandler`: the archives and
//! the data folder, searched in the order they were added.
//!
//! This is the one resource handler of the process. Everything that reads a
//! file "from the game" goes through it, as in upstream: the string tables
//! of localized plugins (`localization::LocalizationHandlerImpl::load_for_file`),
//! the script functions of `xejviScriptAdapterResource.pas`
//! (`ResourceContainerList`, `ResourceExists`, `ResourceOpenData`, ...) and
//! the `LoadFromResource` of the data format classes, the asset lookups of
//! LOD generation and of the Starfield definitions, and the names that file
//! and folder hashes display (`ResolveFileHash`, `ResolveFolderHash`). The
//! loaders fill it: `xedit_session::resources` ports the archive discovery
//! of the GUI's `TLoaderThread` (the game ini, `HasBSAs` of each plugin,
//! `wbLoadBSAs`) and of xDump; a caller that wants other containers calls
//! [`clear_containers`], [`add_archive`] and [`add_folder`] in the order
//! upstream would.
//!
//! Precedence is upstream's: [`open_resource`] returns the file of every
//! container that has it in the order they were added, the loaders take the
//! last one ([`open_resource_last`], [`open_resource_data`] with no
//! container name), so the data folder, added last, wins over every archive
//! and a later archive over an earlier one. The listings
//! ([`container_resource_list`]) enumerate the containers in that order and
//! each container in its own order: an archive in the order of its file
//! table, a folder in the order of a directory walk.
//!
//! The functions are free functions over one process-wide list, as
//! `wbContainerHandler` is one global; they are safe to call from several
//! threads (the list is behind a lock and a container is never changed once
//! added).

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, RwLock};

use xedit_io::archive::{extract_file_ext, extract_file_name, include_trailing_path_delimiter, packer::list_files};
use xedit_io::{Archive, ArchiveError};

use crate::delphi_dictionary::StringDictionary;
use crate::interface::globals::{GameMode, game_mode, game_name};

/// Upstream `IwbResourceContainer`: a folder or an archive.
enum Container {
    /// `TwbFolder`: the files below a folder. The text is the path with a
    /// trailing separator (`IncludeTrailingPathDelimiter` of the constructor),
    /// which is its name.
    Folder(String),
    /// `TwbBSAFile`: the files of an archive.
    Archive(Box<Archive>),
}

impl Container {
    /// Upstream `Name`: the full path of the folder (with its trailing
    /// separator) or archive.
    fn name(&self) -> String {
        match self {
            Container::Folder(path) => path.clone(),
            Container::Archive(archive) => archive.path().to_owned(),
        }
    }

    /// The path of a file of a folder container: `fPath + aFileName`.
    fn folder_file(folder: &str, path: &str) -> PathBuf {
        PathBuf::from(format!("{folder}{path}").replace('\\', "/"))
    }

    /// Port of `OpenResource` and `GetData` of the resource: the contents of
    /// the file `path` (relative to the data folder) when the container has it.
    fn open_resource(&self, path: &str) -> Result<Option<Vec<u8>>, ArchiveError> {
        match self {
            Container::Folder(folder) => {
                let file = Self::folder_file(folder, path);
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
/// last container, named by the path with a trailing separator. A folder
/// of that name that was added already is kept where it is.
pub fn add_folder(path: &Path) {
    let name = include_trailing_path_delimiter(&path.display().to_string());
    if container_exists(&name) {
        return;
    }
    CONTAINERS.write().unwrap().push(Arc::new(Container::Folder(name)));
    invalidate_cache();
}

/// Port of `TwbContainerHandler.AddBSA` and `AddBA2`: the files of the
/// archive as the last container, named by the path as given. An archive of
/// that name that was added already is kept where it is. The result is the
/// archive's version (`TwbBA2File.GetVersion`, which the loader prints for a
/// BA2), or the error of `TwbBSArchive.LoadFromFile`.
pub fn add_archive(path: &Path) -> Result<u32, ArchiveError> {
    let name = path.display().to_string();
    if let Some(version) = CONTAINERS
        .read()
        .unwrap()
        .iter()
        .find_map(|container| match &**container {
            Container::Archive(archive) if archive.path().eq_ignore_ascii_case(&name) => Some(archive.version()),
            _ => None,
        })
    {
        invalidate_cache();
        return Ok(version);
    }
    let archive = Archive::open(path)?;
    let version = archive.version();
    CONTAINERS
        .write()
        .unwrap()
        .push(Arc::new(Container::Archive(Box::new(archive))));
    invalidate_cache();
    Ok(version)
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
            Container::Folder(folder) => Self::folder_file(folder, path).is_file(),
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
                let start = Self::folder_file(root, &folder);
                if !start.is_dir() {
                    return Vec::new();
                }
                // `TDirectory.GetFiles(fPath + aFolder, '*.*', soAllDirectories)`
                // names the files with that path in front.
                let prefix = format!("{root}{folder}");
                let start_text = if folder.is_empty() {
                    prefix.clone()
                } else {
                    include_trailing_path_delimiter(&prefix)
                };
                let mut files = Vec::new();
                if list_files(&start, &start_text, &mut files).is_err() {
                    return Vec::new();
                }
                let root_length = root.chars().count();
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

/// Whether any container was added: upstream's `Assigned(wbContainerHandler)`.
pub fn has_containers() -> bool {
    !CONTAINERS.read().unwrap().is_empty()
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

/// `_AllCount`: the capacity `BuildCache` gives the dictionary of every
/// name, which decides its bucket order.
const ALL_COUNT: usize = 1_000_000;

/// Port of `TwbContainerHandler.BuildCache`.
///
/// The names of every container go into one dictionary (`ccAll`, a
/// `TDictionary<string, TwbNothing>`: a name in another case is another key),
/// then the lines of `<game>.HashSeed.txt` next to the program from the last
/// one up; the hash tables then take the names in the dictionary's bucket
/// order, the first name of a hash winning (`TryAdd`). That order is
/// [`StringDictionary`]'s, so two names of one hash resolve as upstream
/// resolves them.
fn build_cache() -> ContainerCache {
    let mut all = StringDictionary::with_capacity(ALL_COUNT);
    for name in container_resource_list("", "") {
        all.try_add(&name);
    }

    let seed_name = format!(
        "{}{}.HashSeed.txt",
        include_trailing_path_delimiter(&crate::delphi::extract_file_path(&crate::delphi::exe_path())),
        game_name()
    );
    if let Ok(bytes) = std::fs::read(&seed_name) {
        // `TStringList.LoadFromFile`, then the lines that were added are
        // written back as `.txt2` (`Delete` of the others, `SaveToFile`).
        let text = xedit_io::encoding::string_list_text(&bytes);
        let mut lines: Vec<&str> = text.lines().collect();
        for index in (0..lines.len()).rev() {
            let seed = xedit_io::encoding::lower_case(lines[index]).replace('/', "\\");
            if !all.try_add(&seed) {
                lines.remove(index);
            }
        }
        let mut kept = String::new();
        for line in lines {
            kept.push_str(line);
            kept.push_str("\r\n");
        }
        let _ = std::fs::write(format!("{seed_name}2"), xedit_io::encoding::ansi_bytes(&kept));
    }

    let mut files: HashSet<String> = HashSet::new();
    let mut folders: HashSet<String> = HashSet::new();
    let mut cache = ContainerCache {
        file_hashes: HashMap::new(),
        folder_hashes: HashMap::new(),
    };
    for full_name in all.keys() {
        let directory = crate::delphi::extract_file_path(full_name);
        let folder =
            xedit_io::encoding::lower_case(directory.strip_suffix('\\').unwrap_or(&directory)).replace('/', "\\");
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

/// Port of `TwbContainerHandler.EnsureCache`: builds the tables of the
/// names of the file and folder hashes unless they are built. The GUI's
/// loader calls it after the containers are added when
/// `wbDecodeTextureHashes` is on; [`resolve_file_hash`] and
/// [`resolve_folder_hash`] call it too.
pub fn ensure_cache() {
    let _ = cache();
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::interface::globals::{set_game_mode, set_game_name, test_lock};
    use xedit_io::archive::{ArchiveType, PackFile};

    /// A data folder in the temporary directory with a few files, and a BSA.
    fn setup(tag: &str) -> (PathBuf, PathBuf) {
        let root = std::env::temp_dir().join(format!("xedit-containers-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let data = root.join("Data");
        for (name, text) in [
            ("Meshes/Loose/a.nif", "loose a"),
            ("meshes/b.nif", "loose b"),
            ("Textures/t.dds", "loose texture"),
        ] {
            let path = data.join(name);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(path, text).unwrap();
        }
        let archive = root.join("Test.bsa");
        let mut bsa = Archive::new();
        let files: Vec<PackFile> = ["meshes\\b.nif", "meshes\\packed\\c.nif"]
            .iter()
            .enumerate()
            .map(|(index, name)| PackFile {
                name: (*name).to_owned(),
                file_object: index,
                compress: false,
            })
            .collect();
        bsa.create_archive(&archive.display().to_string(), ArchiveType::Fo3, &files)
            .unwrap();
        bsa.pack("meshes\\b.nif", b"packed b").unwrap();
        bsa.pack("meshes\\packed\\c.nif", b"packed c").unwrap();
        bsa.save().unwrap();
        (data, archive)
    }

    #[test]
    fn lists_counts_and_copies_resources() {
        let _lock = test_lock();
        let (data, archive) = setup("listing");
        clear_containers();
        add_folder(&data);
        add_archive(&archive).unwrap();
        assert_eq!(
            container_list(),
            [data.display().to_string(), archive.display().to_string()]
        );

        // The loose files are listed lower case and relative to the folder, in
        // the order of a directory listing, the archive's as it names them.
        let data_name = data.display().to_string();
        assert_eq!(
            container_resource_list(&data_name, ""),
            ["meshes\\b.nif", "meshes\\loose\\a.nif", "textures\\t.dds"]
        );
        assert_eq!(
            container_resource_list(&data_name, "meshes\\loose"),
            ["meshes\\loose\\a.nif"]
        );
        assert_eq!(
            container_resource_list(&archive.display().to_string(), "meshes\\packed"),
            ["meshes\\packed\\c.nif"]
        );
        let mut all = ResourceDict::new();
        container_resource_dict("", "", &mut all);
        assert_eq!(all.len(), 4, "{all:?}");

        assert!(resource_exists("meshes\\packed\\c.nif"));
        assert!(!resource_exists("meshes\\missing.nif"));
        let (count, containers) = resource_count("Meshes\\B.nif");
        assert_eq!(count, 2);
        assert_eq!(containers.len(), 2);

        // The last container that has the file wins; a name picks a container.
        assert_eq!(open_resource_data("", "meshes\\b.nif"), b"packed b");
        assert_eq!(open_resource_data(&data_name, "meshes\\b.nif"), b"loose b");

        let out = data.with_file_name("copies");
        resource_copy("", "meshes\\packed\\c.nif", &out.display().to_string()).unwrap();
        assert_eq!(std::fs::read(out.join("meshes/packed/c.nif")).unwrap(), b"packed c");
        resource_copy(
            &data_name,
            "textures\\t.dds",
            &out.join("one.dds").display().to_string(),
        )
        .unwrap();
        assert_eq!(std::fs::read(out.join("one.dds")).unwrap(), b"loose texture");
        assert_eq!(
            resource_copy("", "nothing.nif", &out.display().to_string())
                .unwrap_err()
                .0,
            "Resource doesn't exist"
        );
        assert_eq!(
            resource_copy("", "meshes\\b.nif", "").unwrap_err().0,
            "Destination path is not specified"
        );
        clear_containers();
        std::fs::remove_dir_all(data.parent().unwrap()).unwrap();
    }

    #[test]
    fn resolves_the_names_of_file_and_folder_hashes() {
        let _lock = test_lock();
        let (data, archive) = setup("hashes");
        clear_containers();
        add_folder(&data);
        add_archive(&archive).unwrap();

        // From Skyrim on the hashes are those of the BA2 and a file name has no extension.
        set_game_name("Fallout4");
        set_game_mode(GameMode::gmFO4);
        invalidate_cache();
        let folder = i64::from(xedit_io::hash::fo4("meshes\\packed"));
        assert_eq!(resolve_folder_hash(folder), "meshes\\packed");
        assert_eq!(resolve_file_hash(i64::from(xedit_io::hash::fo4("c"))), "c");
        assert_eq!(resolve_file_hash(i64::from(xedit_io::hash::fo4("nothing"))), "");

        // Before Skyrim the BSA hash takes the extension, and a texture is also known as `.ddx`.
        set_game_mode(GameMode::gmFNV);
        invalidate_cache();
        assert_eq!(resolve_file_hash(xedit_io::hash::tes4("c.nif", true) as i64), "c.nif");
        assert_eq!(resolve_file_hash(xedit_io::hash::tes4("t.ddx", true) as i64), "t.ddx");
        assert_eq!(resolve_file_hash(xedit_io::hash::tes4("t.dds", true) as i64), "t.dds");
        assert!(!loader_done());
        clear_containers();
        std::fs::remove_dir_all(data.parent().unwrap()).unwrap();
    }
}
