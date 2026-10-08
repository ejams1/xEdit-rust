// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

// Ported from xEdit: Core/wbBSArchive.pas (TwbSplitPacker,
// TwbMultiSourcePacker), BSArch.dpr (DoPack, DoUnpack)

//! Packing many files from folders, files and archives into one or several
//! archives, and unpacking an archive into a folder, on all the CPUs.
//!
//! Upstream packs on a pool of threads that pick up work as they come, so
//! the order in which data reaches an archive depends on the timing. The
//! output of `BSArch -mt:no` does not, and the port reproduces that output
//! on every thread count: the work that can run apart (reading a source,
//! hashing and compressing it) runs on the pool and the results are
//! consumed in the order the single thread of upstream would have reached
//! them, so splitting an archive, sharing identical data and the placement
//! of the data of each file are decided in that order.

use std::collections::HashMap;
use std::path::Path;
use std::sync::{Condvar, Mutex};

use super::asset::{do_not_compress, do_not_pack, extract_file_name, get_asset_name, include_trailing_path_delimiter};
use super::write::PackFile;
use super::{Archive, ArchiveError, ArchiveType, AssetType, Target, cannot_open, is_archive};
use crate::compression::CompressionType;
use crate::encoding::lower_case;
use crate::hash::{LookupHash, lookup_hash_text};

mod process;

pub use process::unpack_archive;

/// A failure while packing: the message BSArch prints.
pub type PackerError = ArchiveError;

/// The data one file hands to the archive: its chunks, each ready to be
/// stored and with the size it has unpacked.
struct LoadedChunk {
    data: Vec<u8>,
    uncompressed_size: i32,
    /// The lookup hash of `data`, when the packer shares data.
    hash: Option<LookupHash>,
}

/// What loading a file gives (`TPackedFile` after `LoadFile`).
struct Loaded {
    chunks: Vec<LoadedChunk>,
    /// The size and lookup hash of the whole source data, to tell identical files.
    share: Option<(i32, LookupHash)>,
}

impl PackedFile {
    /// The entry of the files list `CreateArchive` takes, with the index of
    /// the file in the packer as its object.
    fn pack_file(&self, index: usize) -> PackFile {
        PackFile {
            name: self.file_name.clone(),
            file_object: index,
            compress: self.compress,
        }
    }
}

impl Loaded {
    fn size(&self) -> usize {
        self.chunks.iter().map(|chunk| chunk.data.len()).sum()
    }
}

/// `TPackedFile`: a file to pack.
struct PackedFile {
    file_name: String,
    /// The index of the source of the data in `source_files` (`FileObject`).
    source: usize,
    compress: bool,
    /// The archive the file went to (`ArchiveIndex`).
    archive_index: usize,
    /// The loaded chunks until the file is written (`Chunks`).
    loaded: Option<Loaded>,
}

/// The sources of the data (`GetSourceFileData`), shared by the pool.
struct Sources<'a> {
    files: &'a [SourceFile],
    archives: &'a [Archive],
}

impl Sources<'_> {
    /// Port of `TwbMultiSourcePacker.GetSourceFileData`.
    fn data(&self, file_object: usize) -> Result<Vec<u8>, String> {
        let source = &self.files[file_object];
        match source.source_entry {
            Some((archive, entry)) => {
                let archive = &self.archives[archive];
                let file = &archive.files()[entry];
                archive.unpack_entry(file).map_err(|error| {
                    format!(
                        "Error reading source \"{}\\{}\": {}",
                        archive.file_name(),
                        file.name,
                        error.0
                    )
                })
            }
            None => std::fs::read(&source.source_file_name).map_err(|error| {
                format!(
                    "Error reading source \"{}\": {}",
                    source.source_file_name,
                    cannot_open(&source.source_file_name, &error).0
                )
            }),
        }
    }
}

/// `fSourceFiles[i]`.
struct SourceFile {
    asset_name: String,
    hash: LookupHash,
    source_file_name: String,
    /// The archive and the file of it that holds the data.
    source_entry: Option<(usize, usize)>,
    compress: bool,
}

/// Port of `TPath.MatchesPattern` without case: `*` and `?` wildcards.
fn matches_pattern(text: &str, pattern: &str) -> bool {
    let text: Vec<char> = lower_case(text).chars().collect();
    let pattern: Vec<char> = lower_case(pattern).chars().collect();
    let (mut t, mut p) = (0usize, 0usize);
    let (mut star, mut mark) = (None, 0usize);
    while t < text.len() {
        if p < pattern.len() && (pattern[p] == '?' || pattern[p] == text[t]) {
            t += 1;
            p += 1;
        } else if p < pattern.len() && pattern[p] == '*' {
            star = Some(p);
            mark = t;
            p += 1;
        } else if let Some(s) = star {
            p = s + 1;
            mark += 1;
            t = mark;
        } else {
            return false;
        }
    }
    while p < pattern.len() && pattern[p] == '*' {
        p += 1;
    }
    p == pattern.len()
}

/// The name of a directory entry as NTFS orders it: by the upper case of
/// each UTF-16 unit.
fn ntfs_sort_key(name: &str) -> Vec<u16> {
    name.encode_utf16()
        .map(|unit| match char::from_u32(u32::from(unit)) {
            Some(c) => {
                let mut upper = c.to_uppercase();
                match (upper.next(), upper.next()) {
                    (Some(single), None) if (single as u32) <= 0xFFFF => single as u32 as u16,
                    _ => unit,
                }
            }
            None => unit,
        })
        .collect()
}

/// The files below a folder with the text that `TDirectory.GetFiles(path,
/// '*.*', soAllDirectories)` gives for them: `prefix` with the path from the
/// folder, in the order of an NTFS directory listing, a folder's files
/// listed where the folder is met.
fn list_files(folder: &Path, prefix: &str, out: &mut Vec<String>) -> std::io::Result<()> {
    let mut entries: Vec<(String, bool)> = Vec::new();
    for entry in std::fs::read_dir(folder)? {
        let entry = entry?;
        let name = entry.file_name().to_string_lossy().into_owned();
        let kind = entry.file_type()?;
        let is_dir = if kind.is_symlink() {
            entry.path().is_dir()
        } else {
            kind.is_dir()
        };
        entries.push((name, is_dir));
    }
    entries.sort_by_cached_key(|(name, _)| ntfs_sort_key(name));
    for (name, is_dir) in entries {
        let text = format!("{prefix}{name}");
        if is_dir {
            list_files(
                &folder.join(&name),
                &format!("{text}{}", std::path::MAIN_SEPARATOR),
                out,
            )?;
        } else {
            out.push(text);
        }
    }
    Ok(())
}

/// How many threads work at once: `threads`, or all the CPUs for 0.
fn thread_count(threads: usize) -> usize {
    if threads == 0 {
        std::thread::available_parallelism().map_or(1, usize::from)
    } else {
        threads
    }
}

/// The state shared by the producers of `Prefetch`.
struct PrefetchState<T> {
    next_to_start: usize,
    consumed: usize,
    in_flight: usize,
    results: HashMap<usize, (Result<T, String>, usize)>,
    stop: bool,
}

/// Results of `prepare(0)`, `prepare(1)`, ... computed on a pool of threads
/// and handed out in index order. Only a few results wait unconsumed at
/// any time, and their total size stays about `byte_limit`.
struct Prefetch<'a, T> {
    count: usize,
    state: &'a Mutex<PrefetchState<T>>,
    changed: &'a Condvar,
}

impl<T> Prefetch<'_, T> {
    /// The result of the next index, waiting for the pool to finish it.
    fn next(&self) -> Option<(usize, Result<T, String>)> {
        let mut state = self.state.lock().expect("prefetch lock");
        if state.consumed >= self.count {
            return None;
        }
        loop {
            let index = state.consumed;
            if let Some((result, size)) = state.results.remove(&index) {
                state.consumed += 1;
                state.in_flight = state.in_flight.saturating_sub(size);
                self.changed.notify_all();
                return Some((index, result));
            }
            state = self.changed.wait(state).expect("prefetch wait");
        }
    }

    fn stop(&self) {
        self.state.lock().expect("prefetch lock").stop = true;
        self.changed.notify_all();
    }
}

/// Runs `consume` with a `Prefetch` of `count` results of `prepare`, and
/// the pool working in the background when `threads` is more than one.
fn with_prefetch<T: Send, R>(
    count: usize,
    threads: usize,
    byte_limit: usize,
    prepare: &(dyn Fn(usize) -> Result<T, String> + Sync),
    size_of: &(dyn Fn(&T) -> usize + Sync),
    consume: impl FnOnce(&dyn Fn() -> Option<(usize, Result<T, String>)>) -> R,
) -> R {
    if threads <= 1 {
        // Everything on the calling thread, in order: upstream's `-mt:no`.
        let next = std::cell::Cell::new(0usize);
        let inline = || {
            let index = next.get();
            if index >= count {
                return None;
            }
            next.set(index + 1);
            Some((index, prepare(index)))
        };
        return consume(&inline);
    }

    let window = threads * 2 + 2;
    let state = Mutex::new(PrefetchState::<T> {
        next_to_start: 0,
        consumed: 0,
        in_flight: 0,
        results: HashMap::new(),
        stop: false,
    });
    let changed = Condvar::new();
    std::thread::scope(|scope| {
        for _ in 0..threads {
            scope.spawn(|| {
                loop {
                    let index = {
                        let mut guard = state.lock().expect("prefetch lock");
                        loop {
                            if guard.stop || guard.next_to_start >= count {
                                return;
                            }
                            let ahead = guard.next_to_start - guard.consumed;
                            let blocked = ahead >= window
                                || (guard.in_flight >= byte_limit && guard.next_to_start != guard.consumed);
                            if !blocked {
                                break;
                            }
                            guard = changed.wait(guard).expect("prefetch wait");
                        }
                        let index = guard.next_to_start;
                        guard.next_to_start += 1;
                        index
                    };
                    let result = prepare(index);
                    let size = result.as_ref().map_or(0, size_of);
                    let mut guard = state.lock().expect("prefetch lock");
                    guard.in_flight += size;
                    guard.results.insert(index, (result, size));
                    changed.notify_all();
                }
            });
        }
        let prefetch = Prefetch {
            count,
            state: &state,
            changed: &changed,
        };
        let result = consume(&|| prefetch.next());
        // Let the pool finish: nothing more is needed.
        prefetch.stop();
        result
    })
}

/// `TwbMultiSourcePacker`: packs the files of folders, single files and
/// archives into one archive or several archives split by size.
pub struct MultiSourcePacker {
    kind: ArchiveType,
    target: Target,
    file_name: String,
    compression_type: CompressionType,
    share_data: bool,
    archive_flags: u32,
    file_flags: u32,
    max_chunk_count: i32,
    single_mip_chunk_x: i32,
    single_mip_chunk_y: i32,
    split_size: i64,
    compress: bool,
    threads: usize,
    filters: Vec<String>,
    source_files: Vec<SourceFile>,
    source_by_hash: HashMap<LookupHash, usize>,
    source_archives: Vec<Archive>,
    // The state of packing.
    files: Vec<PackedFile>,
    archives: Vec<Archive>,
    process_count: usize,
}

impl Default for MultiSourcePacker {
    fn default() -> Self {
        Self::new()
    }
}

impl MultiSourcePacker {
    /// Port of `TwbMultiSourcePacker.Create`.
    pub fn new() -> Self {
        Self {
            kind: ArchiveType::None,
            target: Target::Pc,
            file_name: String::new(),
            compression_type: CompressionType::None,
            share_data: false,
            archive_flags: 0,
            file_flags: 0,
            max_chunk_count: 4,
            single_mip_chunk_x: 512,
            single_mip_chunk_y: 512,
            split_size: 0,
            compress: false,
            threads: 0,
            filters: Vec::new(),
            source_files: Vec::new(),
            source_by_hash: HashMap::new(),
            source_archives: Vec::new(),
            files: Vec::new(),
            archives: Vec::new(),
            process_count: 0,
        }
    }

    // Properties.

    pub fn compression_type(&self) -> CompressionType {
        self.compression_type
    }

    pub fn set_compression_type(&mut self, compression: CompressionType) {
        self.compression_type = compression;
    }

    pub fn compress(&self) -> bool {
        self.compress
    }

    pub fn set_compress(&mut self, compress: bool) {
        self.compress = compress;
    }

    pub fn share_data(&self) -> bool {
        self.share_data
    }

    pub fn set_share_data(&mut self, share: bool) {
        self.share_data = share;
    }

    pub fn split_size(&self) -> i64 {
        self.split_size
    }

    pub fn set_split_size(&mut self, size: i64) {
        self.split_size = size;
    }

    pub fn set_archive_flags(&mut self, flags: u32) {
        self.archive_flags = flags;
    }

    pub fn set_file_flags(&mut self, flags: u32) {
        self.file_flags = flags;
    }

    pub fn set_target(&mut self, target: Target) {
        self.target = target;
    }

    /// The threads that read and compress: 1 for upstream's `-mt:no`, 0 for
    /// one per CPU. The archives are the same for every count.
    pub fn set_threads(&mut self, threads: usize) {
        self.threads = threads;
    }

    /// Port of `SetFilters`: adds the non-empty trimmed masks.
    pub fn set_filters(&mut self, filters: &[String]) {
        for filter in filters {
            if !filter.trim().is_empty() {
                self.filters.push(filter.trim().to_owned());
            }
        }
    }

    pub fn source_files_count(&self) -> usize {
        self.source_files.len()
    }

    /// Upstream `ProcessCount`: how many steps `process` takes.
    pub fn process_count(&self) -> usize {
        self.process_count
    }

    /// The archives created, in order.
    pub fn archives(&self) -> &[Archive] {
        &self.archives
    }

    /// Upstream `MultiThreaded`.
    pub fn multi_threaded(&self) -> bool {
        self.threads != 1
    }

    // Sources.

    /// Port of `TwbMultiSourcePacker.Add`. Returns whether the file was added.
    fn add(
        &mut self,
        asset_name: &str,
        source_file_name: &str,
        source_entry: Option<(usize, usize)>,
        check: bool,
    ) -> bool {
        if !self.filters.is_empty() {
            let name = extract_file_name(asset_name);
            if !self.filters.iter().any(|filter| matches_pattern(name, filter)) {
                return false;
            }
        }

        let hash = lookup_hash_text(asset_name, true);
        let found = if check {
            self.source_by_hash.get(&hash).copied()
        } else {
            None
        };
        let index = match found {
            Some(index) => index,
            None => {
                self.source_files.push(SourceFile {
                    asset_name: asset_name.to_owned(),
                    hash,
                    source_file_name: String::new(),
                    source_entry: None,
                    compress: self.compress && !do_not_compress(asset_name),
                });
                self.source_by_hash.entry(hash).or_insert(self.source_files.len() - 1);
                self.source_files.len() - 1
            }
        };
        self.source_files[index].source_file_name = source_file_name.to_owned();
        self.source_files[index].source_entry = source_entry;
        true
    }

    /// Port of `AddSourceFile`.
    pub fn add_source_file(&mut self, file_name: &str) -> usize {
        usize::from(
            Path::new(file_name).is_file()
                && self.add(&get_asset_name(file_name, "", AssetType::None), file_name, None, true),
        )
    }

    /// Port of `AddSourceFolder`.
    pub fn add_source_folder(&mut self, folder: &str) -> usize {
        let mut result = 0;
        let check = !self.source_files.is_empty();
        let path = include_trailing_path_delimiter(folder);
        let mut files = Vec::new();
        if list_files(Path::new(&path), &path, &mut files).is_err() {
            return 0;
        }
        for file in files {
            if !do_not_pack(&file) && self.add(&get_asset_name(&file, &path, AssetType::None), &file, None, check) {
                result += 1;
            }
        }
        result
    }

    /// Port of `AddSourceArchive`.
    pub fn add_source_archive(&mut self, archive: &str) -> Result<usize, PackerError> {
        if self
            .source_archives
            .iter()
            .any(|source| source.file_name().eq_ignore_ascii_case(archive))
        {
            return Ok(0);
        }
        let mut source = Archive::new();
        source.load_from_file(archive)?;
        let check = !self.source_files.is_empty();
        let archive_index = self.source_archives.len();
        let names: Vec<String> = source.files().iter().map(|file| file.name.clone()).collect();
        self.source_archives.push(source);
        let mut result = 0;
        for (entry, name) in names.iter().enumerate() {
            if self.add(name, "", Some((archive_index, entry)), check) {
                result += 1;
            }
        }
        Ok(result)
    }

    /// Port of `AddSource`: a folder, an archive or a single file.
    pub fn add_source(&mut self, path: &str) -> Result<usize, PackerError> {
        if Path::new(path).is_dir() {
            Ok(self.add_source_folder(path))
        } else if Path::new(path).is_file() {
            if is_archive(path) {
                self.add_source_archive(path)
            } else {
                Ok(self.add_source_file(path))
            }
        } else {
            Ok(0)
        }
    }

    // Creating.

    /// Port of `TwbSplitPacker.NewArchiveName`: the archive name with the
    /// number of the archive in front of the extension from the second on.
    fn new_archive_name(&self) -> String {
        process::numbered_archive_name(&self.file_name, self.archives.len())
    }

    /// Port of `TwbSplitPacker.AddArchive`.
    fn add_archive(&mut self) {
        let mut archive = Archive::new();
        archive.set_target(self.target);
        archive.set_share_data(self.share_data);
        archive.set_archive_flags(self.archive_flags);
        archive.set_file_flags(self.file_flags);
        archive.set_compression_type(self.compression_type);
        archive.set_max_chunk_count(self.max_chunk_count);
        archive.set_single_mip_chunk(self.single_mip_chunk_x, self.single_mip_chunk_y);
        self.archives.push(archive);
    }

    /// Port of `TwbMultiSourcePacker.CreateArchive` and
    /// `TwbSplitPacker.CreateArchive`: lists the files to pack. With no
    /// split size the one archive is started now.
    pub fn create_archive(&mut self, file_name: &str, kind: ArchiveType) -> Result<(), PackerError> {
        if self.source_files.is_empty() {
            return Err(ArchiveError("No source files have been added for packing".to_owned()));
        }
        self.file_name = file_name.to_owned();
        self.kind = kind;
        if self.compression_type == CompressionType::None {
            self.compression_type = kind.default_compression();
        }

        // A small gimmick for Morrowind archives: repacked vanilla ones will be
        // binary identical. Upstream sorts only when it is not multithreaded;
        // the port always does, so that the archive does not depend on the
        // threads.
        let mut order: Vec<usize> = (0..self.source_files.len()).collect();
        if kind == ArchiveType::Tes3 {
            order.sort_by(|&a, &b| {
                self.source_files[a]
                    .asset_name
                    .encode_utf16()
                    .cmp(self.source_files[b].asset_name.encode_utf16())
            });
        }
        self.files = order
            .iter()
            .map(|&index| PackedFile {
                file_name: self.source_files[index].asset_name.clone(),
                source: index,
                compress: self.source_files[index].compress,
                archive_index: 0,
                loaded: None,
            })
            .collect();

        // If no splitting then create a single archive right now.
        if self.split_size == 0 {
            self.add_archive();
            let list: Vec<PackFile> = self
                .files
                .iter()
                .enumerate()
                .map(|(index, file)| file.pack_file(index))
                .collect();
            let name = self.new_archive_name();
            let kind = self.kind;
            self.archives[0].create_archive(&name, kind, &list)?;
            // One tick per file when no splitting.
            self.process_count = self.files.len();
        } else {
            // Two ticks per file when splitting, for separate loading and writing.
            self.process_count = self.files.len() * 2;
        }
        Ok(())
    }
}
