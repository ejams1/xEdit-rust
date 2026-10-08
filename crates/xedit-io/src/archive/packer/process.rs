// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

// Ported from xEdit: Core/wbBSArchive.pas (TwbSplitPacker.Process, LoadFile,
// WriteFile, MakeArchiveForLoaded), BSArch.dpr (DoUnpack)

//! The steps of packing and the unpacking of an archive into a folder.

use std::collections::{HashMap, HashSet, VecDeque};
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

use super::{
    Loaded, LoadedChunk, MultiSourcePacker, PackedFile, Sources, include_trailing_path_delimiter, thread_count,
    with_prefetch,
};
use crate::archive::write::{ChunkSlot, PackFile, Prepared};
use crate::archive::{Archive, ArchiveError, ArchiveType, split_dir_name, texture};
use crate::compression::CompressionType;
use crate::hash::{LookupHash, lookup_hash, lookup_hash_text};

/// The most the pool reads ahead of the writer, in bytes.
const PREFETCH_BYTES: usize = 512 << 20;

/// Port of `NewArchiveName` for the `count`-th archive: the number in front
/// of the extension from the second archive on.
pub(super) fn numbered_archive_name(file_name: &str, count: usize) -> String {
    let number = if count > 1 { count.to_string() } else { String::new() };
    let dot = crate::archive::last_char_pos(file_name, '.');
    if dot != 0 {
        let mut chars: Vec<char> = file_name.chars().collect();
        chars.splice(dot - 1..dot - 1, number.chars());
        chars.into_iter().collect()
    } else {
        format!("{file_name}{number}")
    }
}

/// What `Process` prepares for a file when the archive is not split.
enum Unsplit {
    /// The data of a general file, compressed when the file is.
    Chunk(Prepared),
    /// A DDS file, split into chunks by the archive.
    Raw(Vec<u8>),
}

impl MultiSourcePacker {
    /// Port of `TwbSplitPacker.Process` called `process_count` times:
    /// reads, compresses and writes every file. `progress` is called with
    /// the number of steps done after each one. The error is the message
    /// BSArch prints.
    pub fn process(&mut self, progress: &mut dyn FnMut(usize)) -> Result<(), String> {
        if self.split_size == 0 {
            self.process_unsplit(progress)
        } else {
            self.process_split(progress)
        }
    }

    /// `Process` with no split size: each file is read, compressed and
    /// written in the order of the list.
    fn process_unsplit(&mut self, progress: &mut dyn FnMut(usize)) -> Result<(), String> {
        let Self {
            files,
            archives,
            source_files,
            source_archives,
            share_data,
            threads,
            kind,
            ..
        } = self;
        let archive = archives.first_mut().ok_or("The archive is not created")?;
        let sources = Sources {
            files: source_files,
            archives: source_archives,
        };
        let share = *share_data;
        let compression = archive.compression_type();
        let dds = kind.is_dds();
        // The entry of each file in the archive and whether it is compressed.
        let entries: Vec<(usize, bool)> = files
            .iter()
            .map(|file| {
                let index = archive.by_hash.get(&lookup_hash_text(&file.file_name, true)).copied();
                index.map(|index| (index, archive.files[index].compress))
            })
            .collect::<Option<_>>()
            .ok_or("File to pack not found in archive")?;
        let names: Vec<String> = files.iter().map(|file| file.file_name.clone()).collect();
        let seen: Mutex<HashMap<(u32, LookupHash), usize>> = Mutex::new(HashMap::new());

        let prepare = |i: usize| -> Result<Unsplit, String> {
            let wrap = |message: String| format!("Error processing \"{}\": {message}", names[i]);
            let data = sources.data(files[i].source).map_err(wrap)?;
            if dds {
                return Ok(Unsplit::Raw(data));
            }
            let hash = share.then(|| lookup_hash(&data));
            let size = data.len();
            // Data that an earlier file has is found in the archive when its
            // turn comes, so it need not be compressed.
            let shared = hash.is_some_and(|hash| {
                let mut seen = seen.lock().expect("seen lock");
                match seen.get_mut(&(size as u32, hash)) {
                    Some(first) if *first < i => true,
                    Some(first) => {
                        *first = i;
                        false
                    }
                    None => {
                        seen.insert((size as u32, hash), i);
                        false
                    }
                }
            });
            let stored = if shared {
                None
            } else if entries[i].1 {
                if compression == CompressionType::None {
                    return Err(wrap("Undefined compression type".to_owned()));
                }
                Some(compression.compress(&data).map_err(|error| wrap(error.0))?)
            } else {
                Some(data)
            };
            Ok(Unsplit::Chunk(Prepared {
                data: stored,
                uncompressed_size: size,
                hash,
            }))
        };
        let size_of = |item: &Unsplit| match item {
            Unsplit::Chunk(prepared) => prepared.data.as_ref().map_or(0, Vec::len),
            Unsplit::Raw(data) => data.len(),
        };
        with_prefetch(
            names.len(),
            thread_count(*threads),
            PREFETCH_BYTES,
            &prepare,
            &size_of,
            |next| {
                for tick in 0..names.len() {
                    let (index, result) = next().ok_or("A file is missing")?;
                    let wrap = |message: String| format!("Error processing \"{}\": {message}", names[index]);
                    match result? {
                        Unsplit::Chunk(prepared) => archive
                            .pack_chunk(entries[index].0, ChunkSlot::File, prepared)
                            .map_err(|error| wrap(error.0))?,
                        Unsplit::Raw(data) => archive
                            .pack_entry(entries[index].0, &data)
                            .map_err(|error| wrap(error.0))?,
                    }
                    progress(tick + 1);
                }
                Ok(())
            },
        )
    }

    /// `Process` with a split size: files are loaded until the archive is
    /// full and then written, two writes for each load once files wait to
    /// be written. The steps are those of upstream's single thread; only the
    /// reading and compressing runs ahead.
    fn process_split(&mut self, progress: &mut dyn FnMut(usize)) -> Result<(), String> {
        let file_name = self.file_name.clone();
        let Self {
            files,
            archives,
            source_files,
            source_archives,
            share_data,
            threads,
            kind,
            split_size,
            compression_type,
            target,
            archive_flags,
            file_flags,
            max_chunk_count,
            single_mip_chunk_x,
            single_mip_chunk_y,
            process_count,
            ..
        } = self;
        let sources = Sources {
            files: source_files,
            archives: source_archives,
        };
        let share = *share_data;
        let compression = *compression_type;
        let kind = *kind;
        let count = files.len();
        let names: Vec<String> = files.iter().map(|file| file.file_name.clone()).collect();
        let compress: Vec<bool> = files.iter().map(|file| file.compress).collect();
        let source_of: Vec<usize> = files.iter().map(|file| file.source).collect();
        let texture_config = texture::TextureConfig {
            max_chunk_count: *max_chunk_count,
            single_mip_chunk_x: *single_mip_chunk_x,
            single_mip_chunk_y: *single_mip_chunk_y,
            target: *target,
        };

        // The part of `LoadFile` that needs nothing of the packer's state.
        let prepare = |i: usize| -> Result<Loaded, String> {
            let wrap = |message: String| format!("Error processing \"{}\": {message}", names[i]);
            let buf = sources.data(source_of[i]).map_err(wrap)?;
            if kind.is_dds() {
                let (chunks, share_of) =
                    texture::chunk_dds(&texture_config, buf, compress[i], share).map_err(|error| wrap(error.0))?;
                let chunks = chunks
                    .into_iter()
                    .map(|(data, uncompressed_size)| LoadedChunk {
                        hash: share.then(|| lookup_hash(&data)),
                        data,
                        uncompressed_size,
                    })
                    .collect();
                return Ok(Loaded {
                    chunks,
                    share: share_of,
                });
            }
            let share_of = share.then(|| (buf.len() as i32, lookup_hash(&buf)));
            let uncompressed_size = buf.len() as i32;
            let data = if compress[i] {
                if compression == CompressionType::None {
                    return Err(wrap("Undefined compression type".to_owned()));
                }
                compression.compress(&buf).map_err(|error| wrap(error.0))?
            } else {
                buf
            };
            Ok(Loaded {
                chunks: vec![LoadedChunk {
                    hash: share.then(|| lookup_hash(&data)),
                    data,
                    uncompressed_size,
                }],
                share: share_of,
            })
        };
        let size_of = |loaded: &Loaded| loaded.size();

        let mut loaded_stack: Vec<usize> = Vec::new();
        let mut loaded_shares: HashSet<(i32, LookupHash)> = HashSet::new();
        let mut written: VecDeque<usize> = VecDeque::new();
        let mut loaded_size: i64 = 0;
        let mut pending_next = 0usize;

        with_prefetch(
            count,
            thread_count(*threads),
            PREFETCH_BYTES,
            &prepare,
            &size_of,
            |next| {
                // Port of `MakeArchiveForLoaded`.
                let make_archive = |archives: &mut Vec<Archive>,
                                    files: &mut Vec<PackedFile>,
                                    loaded_stack: &mut Vec<usize>,
                                    loaded_shares: &mut HashSet<(i32, LookupHash)>,
                                    written: &mut VecDeque<usize>|
                 -> Result<(), ArchiveError> {
                    // The most recently loaded file first.
                    let order: Vec<usize> = loaded_stack.iter().rev().copied().collect();
                    let list: Vec<PackFile> = order.iter().map(|&index| files[index].pack_file(index)).collect();
                    for &index in &order {
                        files[index].archive_index = archives.len();
                    }
                    // Prepend the loaded chain to the written one.
                    let mut chain: VecDeque<usize> = order.into_iter().collect();
                    chain.append(written);
                    *written = chain;
                    loaded_stack.clear();
                    loaded_shares.clear();

                    let mut archive = Archive::new();
                    archive.set_target(*target);
                    archive.set_share_data(share);
                    archive.set_archive_flags(*archive_flags);
                    archive.set_file_flags(*file_flags);
                    archive.set_compression_type(compression);
                    archive.set_max_chunk_count(*max_chunk_count);
                    archive.set_single_mip_chunk(*single_mip_chunk_x, *single_mip_chunk_y);
                    // The data comes from the packer.
                    archive.set_packer_assigned(true);
                    archives.push(archive);
                    let name = numbered_archive_name(&file_name, archives.len());
                    archives
                        .last_mut()
                        .expect("the new archive")
                        .create_archive(&name, kind, &list)
                };

                for tick in 0..*process_count {
                    if !written.is_empty() && (pending_next >= count || tick % 3 != 0) {
                        // Write a file.
                        let index = written.pop_front().expect("a file waits to be written");
                        let wrap = |message: String| format!("Error processing \"{}\": {message}", names[index]);
                        let archive = &mut archives[files[index].archive_index];
                        let entry = archive
                            .by_hash
                            .get(&lookup_hash_text(&names[index], true))
                            .copied()
                            .ok_or_else(|| wrap("File to pack not found in archive".to_owned()))?;
                        let loaded = files[index]
                            .loaded
                            .take()
                            .ok_or_else(|| wrap("The file is not loaded".to_owned()))?;
                        if kind.is_dds() {
                            texture::pack_dds_chunks(
                                archive,
                                entry,
                                loaded
                                    .chunks
                                    .into_iter()
                                    .map(|chunk| (chunk.data, chunk.uncompressed_size, chunk.hash))
                                    .collect(),
                            )
                            .map_err(|error| wrap(error.0))?;
                        } else {
                            let chunk = loaded
                                .chunks
                                .into_iter()
                                .next()
                                .ok_or_else(|| wrap("No data".to_owned()))?;
                            archive
                                .pack_chunk(
                                    entry,
                                    ChunkSlot::File,
                                    Prepared {
                                        data: Some(chunk.data),
                                        uncompressed_size: chunk.uncompressed_size as usize,
                                        hash: chunk.hash,
                                    },
                                )
                                .map_err(|error| wrap(error.0))?;
                        }
                    } else if pending_next < count {
                        // Load the next pending file.
                        let (index, result) = next().ok_or("A file is missing")?;
                        pending_next += 1;
                        let wrap = |message: String| format!("Error processing \"{}\": {message}", names[index]);
                        let loaded = result?;
                        let mut size = loaded.size() as i64;
                        // Data identical to a file loaded for the same archive costs nothing.
                        if let Some(shared) = loaded.share
                            && loaded_shares.contains(&shared)
                        {
                            size = 0;
                        }
                        // A rough (over)estimate of the archive size: space for all
                        // the service data per file, and a possible embedded name.
                        size += 200;
                        size += names[index].encode_utf16().count() as i64;
                        if matches!(kind, ArchiveType::Fo3 | ArchiveType::Sse) {
                            size += names[index].encode_utf16().count() as i64;
                        }
                        loaded_size += size;
                        let share_of = loaded.share;
                        files[index].loaded = Some(loaded);

                        let mut current = Some(index);
                        // Time to create a new archive if greater than the split size.
                        if loaded_size > *split_size {
                            if loaded_stack.is_empty() {
                                // Nothing is loaded yet: the file is alone, which only
                                // happens when it is larger than the split size.
                                loaded_stack.push(index);
                                current = None;
                                loaded_size = 0;
                            } else {
                                loaded_size = size;
                            }
                            make_archive(archives, files, &mut loaded_stack, &mut loaded_shares, &mut written)
                                .map_err(|error| wrap(error.0))?;
                        }
                        if let Some(index) = current {
                            // Insert the file into the loaded chain.
                            loaded_stack.push(index);
                            if let Some(shared) = share_of {
                                loaded_shares.insert(shared);
                            }
                            // The final archive if the last pending file is loaded.
                            if pending_next >= count {
                                make_archive(archives, files, &mut loaded_stack, &mut loaded_shares, &mut written)
                                    .map_err(|error| wrap(error.0))?;
                            }
                        }
                    }
                    progress(tick + 1);
                }
                Ok(())
            },
        )
    }

    /// Port of `TwbSplitPacker.Save`: writes the tables of every archive.
    /// Returns the message BSArch prints on a failure.
    pub fn save(&mut self) -> Result<(), String> {
        for archive in &mut self.archives {
            archive
                .save()
                .map_err(|error| format!("Archive saving error: {}", error.0))?;
        }
        Ok(())
    }
}

/// Port of the unpacking of `BSArch.dpr` (`DoUnpack`): creates the folders
/// and writes every file of the archive below `folder`, on `threads` threads
/// (0 for one per CPU). `progress` is called with the number of files done
/// after each one. A file that fails stops the work; the error is the
/// message of the file with the lowest index.
///
/// DEVIATION: a name that would leave the folder (`..` or an absolute path)
/// is refused; upstream writes wherever the name says.
pub fn unpack_archive(
    archive: &Archive,
    folder: &str,
    threads: usize,
    progress: &(dyn Fn(usize) + Sync),
) -> Result<(), String> {
    let folder = include_trailing_path_delimiter(folder);
    let safe = |name: &str| {
        let normalized = name.replace('/', "\\");
        !(normalized.starts_with('\\') || normalized.contains(':') || normalized.split('\\').any(|part| part == ".."))
    };

    // Create the folders.
    for file in archive.files() {
        if !safe(&file.name) {
            return Err(format!(
                "Error processing \"{}\": The name leaves the destination folder",
                file.name
            ));
        }
        let (position, directory, _) = split_dir_name(&file.name);
        if position != 0 {
            let directory = format!("{folder}{directory}");
            std::fs::create_dir_all(&directory).map_err(|_| {
                format!(
                    "Can't create destination folder: {directory}{}",
                    std::path::MAIN_SEPARATOR
                )
            })?;
        }
    }

    let count = archive.count();
    let next = AtomicUsize::new(0);
    let done = AtomicUsize::new(0);
    let stop = AtomicBool::new(false);
    let first_error: Mutex<Option<(usize, String)>> = Mutex::new(None);
    let work = || {
        loop {
            if stop.load(Ordering::Relaxed) {
                return;
            }
            let index = next.fetch_add(1, Ordering::Relaxed);
            if index >= count {
                return;
            }
            let file = &archive.files()[index];
            let result = archive.unpack(&file.name).map_err(|error| error.0).and_then(|data| {
                let path = format!("{folder}{}", file.name);
                std::fs::write(&path, data).map_err(|error| {
                    format!(
                        "Cannot create file \"{path}\". {}",
                        crate::archive::os_error_text(&error)
                    )
                })
            });
            match result {
                Ok(()) => progress(done.fetch_add(1, Ordering::Relaxed) + 1),
                Err(message) => {
                    stop.store(true, Ordering::Relaxed);
                    let mut first = first_error.lock().expect("error lock");
                    if first.as_ref().is_none_or(|(known, _)| index < *known) {
                        *first = Some((index, format!("Error processing \"{}\": {message}", file.name)));
                    }
                }
            }
        }
    };
    let threads = thread_count(threads);
    if threads <= 1 {
        work();
    } else {
        std::thread::scope(|scope| {
            for _ in 0..threads {
                scope.spawn(&work);
            }
        });
    }
    match first_error.into_inner().expect("error lock") {
        Some((_, message)) => Err(message),
        None => Ok(()),
    }
}
