// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

// Ported from xEdit: BSArch.dpr (DoInfo, DoPack, DoUnpack), Core/wbBSArchive.pas

//! The archive commands: `archive.list` reads the tables of a BSA or BA2,
//! `archive.extract` unpacks files into a folder and `archive.pack` packs
//! folders, files and archives into one archive or several. They are the
//! three modes of `BSArch.exe` and need no loaded plugin.

use std::path::Path;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use xedit_io::archive::packer::unpack_archive;
use xedit_io::archive::{ArchiveType, BSA_MAX_OFFSET, FileEntry, format_size};
use xedit_io::{Archive, CompressionType, MultiSourcePacker};

use crate::{CommandError, Registry, Session};

/// One file of an archive.
#[derive(Serialize, JsonSchema)]
pub struct ArchiveFile {
    /// The name in the archive, with `\` separators.
    pub name: String,
    /// The size field of the table: the unpacked size in a BA2 and the size
    /// as stored in a BSA (with an embedded name and the unpacked size
    /// field of a compressed file).
    pub size: u32,
    /// The packed size of a compressed file, 0 for a file stored as it is.
    pub packed_size: u32,
    /// The offset of the data in the archive (in a Morrowind archive
    /// relative to the data section).
    pub offset: i64,
    /// Whether the file is stored compressed.
    pub compressed: bool,
    /// The hash of the folder (BSA: 16 hexadecimal digits, BA2: 8).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub folder_hash: Option<String>,
    /// The hash of the file name.
    pub name_hash: String,
    /// Texture archives: the width in pixels.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub width: Option<u16>,
    /// Texture archives: the height in pixels.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub height: Option<u16>,
    /// Texture archives: the DXGI format of the texture.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub format: Option<String>,
}

fn describe(archive: &Archive, file: &FileEntry) -> ArchiveFile {
    let kind = archive.archive_type();
    let (folder_hash, name_hash) = match kind {
        ArchiveType::Tes3 => (None, format!("{:016X}", file.name_hash64)),
        ArchiveType::Tes4 | ArchiveType::Fo3 | ArchiveType::Sse => (
            Some(format!("{:016X}", file.dir_hash64)),
            format!("{:016X}", file.name_hash64),
        ),
        _ => (
            Some(format!("{:08X}", file.dir_hash32)),
            format!("{:08X}", file.name_hash32),
        ),
    };
    let dds = kind.is_dds();
    ArchiveFile {
        name: file.name.clone(),
        size: if dds {
            file.dds.tex_chunks.iter().map(|chunk| chunk.chunk.size).sum()
        } else {
            file.chunk.size
        },
        packed_size: if dds {
            file.dds.tex_chunks.iter().map(|chunk| chunk.chunk.packed_size).sum()
        } else {
            file.chunk.packed_size
        },
        offset: if dds {
            file.dds.tex_chunks.first().map_or(0, |chunk| chunk.chunk.offset)
        } else {
            file.chunk.offset
        },
        compressed: archive.is_compressed(file),
        folder_hash,
        name_hash,
        width: dds.then_some(file.dds.width),
        height: dds.then_some(file.dds.height),
        format: dds.then(|| file.dxgi_format_name().to_owned()),
    }
}

/// `archive.list`: the request.
#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ArchiveListRequest {
    /// Path of the archive (`.bsa` or `.ba2`).
    pub archive: String,
    /// List the files too (`-list` of BSArch). The files of one page are
    /// returned; `total` counts the matches before paging.
    #[serde(default)]
    pub files: bool,
    /// Only the files below this folder of the archive, such as `meshes\armor`.
    pub folder: Option<String>,
    /// Files to skip.
    #[serde(default)]
    pub offset: usize,
    /// Files to return at most.
    #[serde(default = "default_limit")]
    pub limit: usize,
}

fn default_limit() -> usize {
    1000
}

/// `archive.list`: the response.
#[derive(Serialize, JsonSchema)]
pub struct ArchiveListResponse {
    /// The archive as given.
    pub archive: String,
    /// The format name, such as `Fallout 4` or `Skyrim AE, Skyrim SE`.
    pub format: String,
    /// The version of the header (`0x69`), absent for Morrowind archives.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
    /// The number of files in the archive.
    pub file_count: usize,
    /// How many files are stored compressed.
    pub compressed_files: usize,
    /// The compression of the archive: `None`, `ZLib`, `LZ4` or `LZ4F`.
    pub compression: String,
    /// The archive flags of a BSA as hexadecimal text, for `-af` of a pack.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub archive_flags: Option<String>,
    /// The names of the archive flags that are set.
    pub archive_flag_names: Vec<String>,
    /// The file flags of a BSA as hexadecimal text, for `-ff` of a pack.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub file_flags: Option<String>,
    /// The names of the file flags that are set.
    pub file_flag_names: Vec<String>,
    /// What is wrong with the archive for the game (files that crash it).
    pub warnings: Vec<String>,
    /// The matches of `files` and `folder` before paging.
    pub total: usize,
    /// The files of the page, when `files` is set.
    pub entries: Vec<ArchiveFile>,
}

fn open_archive(path: &str) -> Result<Archive, CommandError> {
    let mut archive = Archive::new();
    archive
        .load_from_file(path)
        .map_err(|error| CommandError::new("archive_failed", error.0))?;
    Ok(archive)
}

fn archive_list(_: &mut Session, request: ArchiveListRequest) -> Result<ArchiveListResponse, CommandError> {
    let archive = open_archive(&request.archive)?;
    let kind = archive.archive_type();
    let bsa = matches!(kind, ArchiveType::Tes4 | ArchiveType::Fo3 | ArchiveType::Sse);
    let matches: Vec<&FileEntry> = match &request.folder {
        Some(folder) => archive.files_by_folder(folder),
        None => archive.files().iter().collect(),
    };
    let entries = if request.files {
        matches
            .iter()
            .skip(request.offset)
            .take(request.limit)
            .map(|file| describe(&archive, file))
            .collect()
    } else {
        Vec::new()
    };
    let names = |names: &[&str], flags: u32| {
        names
            .iter()
            .enumerate()
            .filter(|(i, _)| (flags >> i) & 1 == 1)
            .map(|(_, name)| (*name).to_owned())
            .collect::<Vec<_>>()
    };
    Ok(ArchiveListResponse {
        archive: request.archive,
        format: kind.format_name().to_owned(),
        version: (kind != ArchiveType::Tes3).then(|| format!("0x{:02X}", archive.version())),
        file_count: archive.count(),
        compressed_files: archive
            .files()
            .iter()
            .filter(|file| archive.is_compressed(file))
            .count(),
        compression: archive.compression_type().name().to_owned(),
        archive_flags: bsa.then(|| format!("0x{:04X}", archive.archive_flags())),
        archive_flag_names: if bsa {
            names(&xedit_io::archive::ARCHIVE_FLAG_NAMES, archive.archive_flags())
        } else {
            Vec::new()
        },
        file_flags: bsa.then(|| format!("0x{:04X}", archive.file_flags())),
        file_flag_names: if bsa {
            names(&xedit_io::archive::FILE_FLAG_NAMES, archive.file_flags())
        } else {
            Vec::new()
        },
        warnings: archive.warnings(),
        total: matches.len(),
        entries,
    })
}

/// `archive.extract`: the request.
#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ArchiveExtractRequest {
    /// Path of the archive.
    pub archive: String,
    /// Folder that exists to unpack into. The files go to their paths in the
    /// archive below it; the folder of the archive when omitted.
    pub output: Option<String>,
    /// Threads that decompress and write; 0 (the default) uses every CPU.
    /// The files written are the same for every count.
    #[serde(default)]
    pub threads: usize,
    /// Report the files that would be written, but write nothing.
    #[serde(default)]
    pub dry_run: bool,
}

/// `archive.extract`: the response.
#[derive(Serialize, JsonSchema)]
pub struct ArchiveExtractResponse {
    pub archive: String,
    /// The folder the files go to, with a trailing separator.
    pub output: String,
    /// The number of files in the archive: written, or for a dry run the
    /// files that would be.
    pub files: usize,
    /// Whether this was a dry run.
    pub dry_run: bool,
}

fn archive_extract(_: &mut Session, request: ArchiveExtractRequest) -> Result<ArchiveExtractResponse, CommandError> {
    let archive = open_archive(&request.archive)?;
    let output = match request.output {
        Some(output) => {
            let folder = xedit_io::archive::include_trailing_path_delimiter(&output);
            if !Path::new(&folder).is_dir() {
                return Err(CommandError::new(
                    "invalid_params",
                    format!("Folder does not exist: {folder}"),
                ));
            }
            folder
        }
        None => path_of(&request.archive),
    };
    if !request.dry_run {
        unpack_archive(&archive, &output, request.threads, &|_| {})
            .map_err(|message| CommandError::new("archive_failed", message))?;
    }
    Ok(ArchiveExtractResponse {
        archive: request.archive,
        output,
        files: archive.count(),
        dry_run: request.dry_run,
    })
}

/// Delphi `ExtractFilePath`.
fn path_of(path: &str) -> String {
    path.rfind(['\\', '/', ':'])
        .map_or(String::new(), |at| path[..=at].to_owned())
}

/// `archive.pack`: the request.
#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ArchivePackRequest {
    /// Where to write the archive. A split archive adds a number to the name
    /// from the second archive on (`mod.bsa`, `mod2.bsa`).
    pub archive: String,
    /// Folders, files and archives with the files to pack, later ones
    /// winning on files with the same name in the archive.
    pub sources: Vec<String>,
    /// The archive format: `tes3` (Morrowind), `tes4` (Oblivion), `fo3`,
    /// `fnv` and `tes5` (the same Skyrim LE format), `sse` (Skyrim SE/AE),
    /// `fo4` (Fallout 4 general), `fo4dds`, `sf1` (Starfield general) or
    /// `sf1dds`. The `dds` formats are texture archives: every file must be a
    /// DDS file, which is stored as chunks of mipmaps.
    pub format: String,
    /// Compress the files: `zlib`, `lz4` or `lz4f` (the formats take some of
    /// them); `default` for the default of the format. Sounds, music and
    /// strings are not compressed. Morrowind archives have no compression.
    pub compress: Option<String>,
    /// Split into archives of this many GB (at most 8), 0 for no split.
    /// Default: BSA formats split at 2 GB, BA2 formats do not split.
    pub split: Option<i64>,
    /// Pack only the files whose name matches one of these masks (`*` and `?`).
    #[serde(default)]
    pub filters: Vec<String>,
    /// Let identical files share their data in the archive. Default true.
    #[serde(default = "default_true")]
    pub share: bool,
    /// Threads that read and compress; 0 (the default) uses every CPU. The
    /// archives are the same for every count.
    #[serde(default)]
    pub threads: usize,
    /// Override the archive flags of a BSA with this hexadecimal value.
    pub archive_flags: Option<String>,
    /// Override the file flags of a BSA with this hexadecimal value.
    pub file_flags: Option<String>,
    /// Add the sources and report the files that would be packed, but write
    /// nothing.
    #[serde(default)]
    pub dry_run: bool,
}

fn default_true() -> bool {
    true
}

/// One archive written by `archive.pack`.
#[derive(Serialize, JsonSchema)]
pub struct PackedArchive {
    /// Path of the archive.
    pub file: String,
    /// The size of the file in bytes.
    pub size: i64,
    /// The size as BSArch prints it, such as `3.64 MB`.
    pub size_text: String,
    /// The number of files in the archive.
    pub files: usize,
    /// How many files share their data with an identical file.
    pub shared_files: i32,
    /// The bytes saved by sharing, as upstream counts them.
    pub shared_size: i64,
    /// What is wrong with the archive for the game.
    pub warnings: Vec<String>,
}

/// `archive.pack`: the response.
#[derive(Serialize, JsonSchema)]
pub struct ArchivePackResponse {
    /// The format name.
    pub format: String,
    /// How many files the sources hold after the merge and the filters.
    pub source_files: usize,
    /// How many files each source added, in the order of `sources`.
    pub source_counts: Vec<usize>,
    /// The archives written, empty for a dry run.
    pub archives: Vec<PackedArchive>,
    pub dry_run: bool,
}

fn parse_format(text: &str) -> Result<ArchiveType, CommandError> {
    Ok(match text.to_ascii_lowercase().as_str() {
        "tes3" => ArchiveType::Tes3,
        "tes4" => ArchiveType::Tes4,
        "fo3" | "fnv" | "tes5" => ArchiveType::Fo3,
        "sse" => ArchiveType::Sse,
        "fo4" => ArchiveType::Fo4,
        "fo4dds" => ArchiveType::Fo4Dds,
        "sf1" => ArchiveType::Sf,
        "sf1dds" => ArchiveType::SfDds,
        _ => {
            return Err(CommandError::new(
                "invalid_params",
                format!(
                    "unknown archive format {text}: use tes3, tes4, fo3, fnv, tes5, sse, fo4, fo4dds, sf1 or sf1dds"
                ),
            ));
        }
    })
}

fn hex_value(text: &str) -> Result<u32, CommandError> {
    let digits = if text.len() >= 2 && text[..2].eq_ignore_ascii_case("0x") {
        &text[2..]
    } else {
        text
    };
    u32::from_str_radix(digits, 16)
        .map_err(|_| CommandError::new("invalid_params", format!("{text} is not a hexadecimal number")))
}

fn archive_pack(_: &mut Session, request: ArchivePackRequest) -> Result<ArchivePackResponse, CommandError> {
    let kind = parse_format(&request.format)?;
    if request.sources.is_empty() {
        return Err(CommandError::new(
            "invalid_params",
            "No source files/folders/archives provided for packing",
        ));
    }
    let mut packer = MultiSourcePacker::new();
    if kind != ArchiveType::Tes3
        && let Some(compress) = &request.compress
    {
        if compress.eq_ignore_ascii_case("default") || compress.is_empty() {
            packer.set_compression_type(kind.default_compression());
        } else {
            let compression = CompressionType::type_by_name(compress).unwrap_or(CompressionType::None);
            if compression == CompressionType::None {
                return Err(CommandError::new(
                    "invalid_params",
                    format!("Unknown compression type {compress}"),
                ));
            }
            if !kind.supports_compression(compression) {
                return Err(CommandError::new(
                    "invalid_params",
                    format!("{} archives don't support {compress} compression", kind.format_name()),
                ));
            }
            packer.set_compression_type(compression);
        }
        packer.set_compress(true);
    }
    match request.split {
        Some(split) => packer.set_split_size(split.min(8) * 1024 * 1024 * 1024),
        None if kind < ArchiveType::Fo4 => packer.set_split_size(BSA_MAX_OFFSET),
        None => {}
    }
    packer.set_filters(&request.filters);
    packer.set_share_data(request.share);
    packer.set_threads(request.threads);
    if let Some(flags) = &request.archive_flags {
        packer.set_archive_flags(hex_value(flags)?);
    }
    if let Some(flags) = &request.file_flags {
        packer.set_file_flags(hex_value(flags)?);
    }

    let mut source_counts = Vec::new();
    for source in &request.sources {
        source_counts.push(
            packer
                .add_source(source)
                .map_err(|error| CommandError::new("archive_failed", error.0))?,
        );
    }
    let source_files = packer.source_files_count();
    if source_files == 0 {
        return Err(CommandError::new("invalid_params", "No valid source file(s) found."));
    }
    let mut response = ArchivePackResponse {
        format: kind.format_name().to_owned(),
        source_files,
        source_counts,
        archives: Vec::new(),
        dry_run: request.dry_run,
    };
    if request.dry_run {
        return Ok(response);
    }

    packer
        .create_archive(&request.archive, kind)
        .map_err(|error| CommandError::new("archive_failed", error.0))?;
    packer
        .process(&mut |_| {})
        .map_err(|message| CommandError::new("archive_failed", message))?;
    packer
        .save()
        .map_err(|message| CommandError::new("archive_failed", message))?;
    response.archives = packer
        .archives()
        .iter()
        .map(|archive| PackedArchive {
            file: archive.file_name().to_owned(),
            size: archive.archive_size(),
            size_text: format_size(archive.archive_size()),
            files: archive.count(),
            shared_files: archive.archive_shared_files(),
            shared_size: archive.archive_shared_size(),
            warnings: archive.warnings(),
        })
        .collect();
    Ok(response)
}

pub fn register(registry: &mut Registry) {
    registry.register(
        "archive.list",
        "Read the header and the file table of a BSA or BA2 archive (BSArch's info, -list and -dump).",
        false,
        archive_list,
    );
    registry.register(
        "archive.extract",
        "Unpack the files of a BSA or BA2 archive into a folder (BSArch unpack). Needs --edit unless --dry-run.",
        true,
        archive_extract,
    );
    registry.register(
        "archive.pack",
        "Pack folders, files and archives into a BSA or BA2 archive (BSArch pack), byte for byte what BSArch writes. Needs --edit unless --dry-run.",
        true,
        archive_pack,
    );
}
