// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

// Ported from xEdit: Core/wbImplementation.pas

//! The reference cache file of `TwbFile.BuildOrLoadRef` (`.refcache`), with
//! `SaveRefsToStream` and `LoadRefsFromStream` of `TwbMainRecord`.
//!
//! The file is an LZ4 frame (`TwbCompression.Compress(ctLZ4F)`: independent
//! blocks of up to 4 MB, no checksums) around this stream, little endian:
//! the record count (`Integer`), then for every record of the file in the
//! order of `flRecords` its FormID as the file stores it, the count and the
//! FormIDs of `mrReferences`, the editor ID and the full name (an `Integer`
//! length and as many UTF-16 code units), the base record FormID
//! (`mrBaseRecordID`), whether the grid cell was checked (a `Boolean` byte),
//! if so whether the record has one, and if so the cell (two `Integer`s);
//! for the game master also the name and the short name of every record
//! with an editor ID. Upstream compresses with LZ4 HC at level 12, which the
//! port's LZ4 does not have: the port writes the same stream in a frame of
//! the same format, which upstream reads, and reads upstream's files.
//!
//! The file is named after the CRC32 of the program, the name, extension
//! and CRC32 of the plugin, the code pages of its strings and of the
//! language, and the language, so a cache is only used for the same plugin
//! read by the same program in the same language.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::Ordering;
use std::time::Duration;

use xedit_io::Encoding;

use crate::interface::element::{Container, File, MainRecord};
use crate::interface::form_id::FormID;
use crate::interface::globals::{
    app_name, cache_path, data_path, display_load_order_form_id, display_shorter_names, language,
    no_full_in_short_name, set_display_load_order_form_id, set_display_shorter_names,
};
use crate::interface::sub_record_group::RecordDef;
use crate::interface::types::{ElementType, FileState, KnownSubRecord, PascalEnum};
use crate::localization::encoding_for_language;

use super::{FileImpl, MainRecordImpl};

/// Upstream `wbRefCacheExt`.
pub const REF_CACHE_EXT: &str = ".refcache";

/// Upstream `wbCacheTimeThreshold`: a file whose references took longer to
/// build is cached whatever its record count.
pub const CACHE_TIME_THRESHOLD: Duration = Duration::from_secs(2);

/// Upstream `csDotGhost`.
const DOT_GHOST: &str = ".ghost";

/// Port of the cache path of `xeInit`: `-C:<path>`, else the folder
/// `<AppName>Edit Cache` in the data folder (`FO4Edit Cache`).
pub fn default_cache_path() -> String {
    format!("{}{}Edit Cache\\", data_path(), app_name())
}

/// Port of `wbCRC32App`: the CRC32 of the running program, which names the
/// cache files so that another build does not read them.
pub fn crc32_app() -> u32 {
    static CRC: std::sync::OnceLock<u32> = std::sync::OnceLock::new();
    *CRC.get_or_init(|| {
        std::env::current_exe()
            .ok()
            .and_then(|exe| std::fs::read(exe).ok())
            .map_or(0, |bytes| xedit_io::crc32(&bytes))
    })
}

/// Delphi `TEncoding.CodePage`.
fn code_page(encoding: Encoding) -> u32 {
    match encoding {
        Encoding::Mbcs(code_page) => code_page,
        Encoding::Utf8 => 65001,
    }
}

/// The name of the cache file of `file` in `BuildOrLoadRef`, or `None`
/// without a cache path.
pub fn cache_file_name(file: &FileImpl) -> Option<PathBuf> {
    let path = cache_path();
    if path.is_empty() {
        return None;
    }
    let mut name = crate::delphi::path_file_name(file.file_name()).to_owned();
    if name.len() >= DOT_GHOST.len() && name[name.len() - DOT_GHOST.len()..].eq_ignore_ascii_case(DOT_GHOST) {
        name.truncate(name.len() - DOT_GHOST.len());
    }
    let (stem, extension) = match name.rfind('.') {
        Some(dot) => (&name[..dot], &name[dot + 1..]),
        None => (name.as_str(), ""),
    };
    let language = language();
    let text = format!(
        "{path}{:08X}_{stem}_{extension}_{:08X}_g{}_t{}_l{}_{language}{REF_CACHE_EXT}",
        crc32_app(),
        file.crc32(),
        code_page(file.get_encoding(false)),
        code_page(file.get_encoding(true)),
        code_page(encoding_for_language(&language, false)),
    );
    Some(PathBuf::from(text))
}

/// The values of a record the cache keeps that are read from its elements
/// while its references are built.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RecordCacheData {
    /// `mrBaseRecordID`.
    pub base_record_id: u32,
    /// `mrsGridCellChecked`, with `mrsHasGridCell` and `mrGridCell`.
    pub grid_cell: Option<Option<(i32, i32)>>,
}

impl RecordCacheData {
    /// The values `SaveRefsToStream` writes, as the init of the record left
    /// them (`TwbMainRecord.Init`, `GetBaseRecord`): the base record FormID
    /// as stored for a placed record, or for another record with a base
    /// record subrecord when it links to a record; the grid cell is
    /// checked when the record has the grid cell subrecord or a subrecord
    /// past `QuickInitLimit` (`TwbMainRecord.Init`).
    pub fn read(record: &Arc<MainRecordImpl>) -> Self {
        let Some(def) = record.mr_def.clone() else {
            return Self::default();
        };
        record.do_init();
        let known = def.known_sub_record_signatures();
        let mut result = Self::default();
        if def.get_contains_known_sub_record(KnownSubRecord::ksrBaseRecord)
            && let Some(name) = record.get_record_by_signature(known[KnownSubRecord::ksrBaseRecord.ord()])
        {
            let raw = name
                .as_element_impl()
                .and_then(|name| name.as_data_container())
                .and_then(|data| data.get_data())
                .and_then(|data| data.get(..4))
                .map_or(0, |data| u32::from_le_bytes([data[0], data[1], data[2], data[3]]));
            let links = || {
                name.get_links_to()
                    .is_some_and(|target| target.get_element_type() == ElementType::etMainRecord)
            };
            if def.get_is_reference() || links() {
                result.base_record_id = raw;
            }
        }
        let grid_cell = *record.mr_grid_cell.lock().unwrap();
        if grid_cell.checked {
            result.grid_cell = Some(grid_cell.cell);
        }
        result
    }
}

/// Port of `mrsGridCellChecked` with `mrsHasGridCell` and `mrGridCell`:
/// whether the init of the record decided its grid cell, and the cell.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct GridCellState {
    pub checked: bool,
    pub cell: Option<(i32, i32)>,
}

/// A record as `LoadRefsFromStream` reads it.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CachedRecord {
    pub form_id: FormID,
    pub references: Vec<FormID>,
    pub editor_id: String,
    pub full_name: String,
    pub data: RecordCacheData,
    /// `mrName` and `mrShortName`, kept for the game master.
    pub names: Option<(String, String)>,
}

fn write_i32(out: &mut Vec<u8>, value: i32) {
    out.extend_from_slice(&value.to_le_bytes());
}

fn write_string(out: &mut Vec<u8>, text: &str) {
    let units: Vec<u16> = text.encode_utf16().collect();
    write_i32(out, units.len() as i32);
    for unit in units {
        out.extend_from_slice(&unit.to_le_bytes());
    }
}

/// Port of `SaveRefsToStream` of every record into the uncompressed stream.
pub fn encode(records: &[CachedRecord], save_names: bool) -> Vec<u8> {
    let mut out = Vec::new();
    write_i32(&mut out, records.len() as i32);
    for record in records {
        out.extend_from_slice(&record.form_id.to_cardinal().to_le_bytes());
        write_i32(&mut out, record.references.len() as i32);
        for form_id in &record.references {
            out.extend_from_slice(&form_id.to_cardinal().to_le_bytes());
        }
        write_string(&mut out, &record.editor_id);
        write_string(&mut out, &record.full_name);
        out.extend_from_slice(&record.data.base_record_id.to_le_bytes());
        out.push(u8::from(record.data.grid_cell.is_some()));
        if let Some(cell) = record.data.grid_cell {
            out.push(u8::from(cell.is_some()));
            if let Some((x, y)) = cell {
                write_i32(&mut out, x);
                write_i32(&mut out, y);
            }
        }
        if save_names {
            let (name, short_name) = record.names.clone().unwrap_or_default();
            write_string(&mut out, &name);
            write_string(&mut out, &short_name);
        }
    }
    out
}

/// A reader of the uncompressed stream.
struct Reader<'a> {
    data: &'a [u8],
    position: usize,
}

impl Reader<'_> {
    fn bytes(&mut self, count: usize) -> Result<&[u8], String> {
        let end = self
            .position
            .checked_add(count)
            .filter(|end| *end <= self.data.len())
            .ok_or_else(|| "Stream read error".to_owned())?;
        let bytes = &self.data[self.position..end];
        self.position = end;
        Ok(bytes)
    }

    fn u32(&mut self) -> Result<u32, String> {
        let bytes = self.bytes(4)?;
        Ok(u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]))
    }

    fn i32(&mut self) -> Result<i32, String> {
        Ok(self.u32()? as i32)
    }

    fn bool(&mut self) -> Result<bool, String> {
        Ok(self.bytes(1)?[0] != 0)
    }

    fn count(&mut self) -> Result<usize, String> {
        Ok(usize::try_from(self.i32()?.max(0)).unwrap_or(0))
    }

    fn string(&mut self) -> Result<String, String> {
        let count = self.count()?;
        let bytes = self.bytes(count.checked_mul(2).ok_or("Stream read error")?)?;
        let units: Vec<u16> = bytes
            .as_chunks::<2>()
            .0
            .iter()
            .map(|unit| u16::from_le_bytes(*unit))
            .collect();
        Ok(String::from_utf16_lossy(&units))
    }
}

/// Port of `LoadRefsFromStream` of every record from the uncompressed
/// stream.
pub fn decode(data: &[u8], load_names: bool) -> Result<Vec<CachedRecord>, String> {
    let mut reader = Reader { data, position: 0 };
    let count = reader.count()?;
    let mut records = Vec::with_capacity(count.min(data.len() / 16));
    for _ in 0..count {
        let form_id = FormID::from_cardinal(reader.u32()?);
        let reference_count = reader.count()?;
        let mut references = Vec::with_capacity(reference_count.min(data.len() / 4));
        for _ in 0..reference_count {
            references.push(FormID::from_cardinal(reader.u32()?));
        }
        let editor_id = reader.string()?;
        let full_name = reader.string()?;
        let base_record_id = reader.u32()?;
        let grid_cell = if reader.bool()? {
            Some(if reader.bool()? {
                Some((reader.i32()?, reader.i32()?))
            } else {
                None
            })
        } else {
            None
        };
        let names = if load_names {
            Some((reader.string()?, reader.string()?))
        } else {
            None
        };
        records.push(CachedRecord {
            form_id,
            references,
            editor_id,
            full_name,
            data: RecordCacheData {
                base_record_id,
                grid_cell,
            },
            names,
        });
    }
    Ok(records)
}

/// The LZ4 frame of `TwbCompression.Compress(ctLZ4F)`.
pub fn compress(data: &[u8]) -> std::io::Result<Vec<u8>> {
    use std::io::Write;
    let info = lz4_flex::frame::FrameInfo::new()
        .block_size(lz4_flex::frame::BlockSize::Max4MB)
        .block_mode(lz4_flex::frame::BlockMode::Independent);
    let mut encoder = lz4_flex::frame::FrameEncoder::with_frame_info(info, Vec::new());
    encoder.write_all(data)?;
    encoder.finish().map_err(std::io::Error::other)
}

/// The stream inside an LZ4 frame, of any length
/// (`TwbCompression.Decompress(ctLZ4F)` of a stream).
pub fn decompress(data: &[u8]) -> std::io::Result<Vec<u8>> {
    use std::io::Read;
    let mut out = Vec::new();
    lz4_flex::frame::FrameDecoder::new(data).read_to_end(&mut out)?;
    Ok(out)
}

/// The short name and the name of a record as the GUI shows them
/// (`wbDisplayShorterNames` and `wbDisplayLoadOrderFormID` on, as
/// `TfrmMain` sets them): `GetShortNameInternal(False)` and `GetName`.
fn gui_names(record: &Arc<MainRecordImpl>) -> (String, String) {
    let editor_id = record.get_editor_id();
    let full_name = record.get_full_name();
    let form_id = format!(
        "[{}:{}]",
        record.get_signature(),
        record.get_load_order_form_id().to_string(true)
    );
    let short_name = |for_name: bool| {
        let mut result = editor_id.clone();
        if (!no_full_in_short_name() || for_name) && !full_name.is_empty() {
            if !result.is_empty() {
                result.push(' ');
            }
            result.push('"');
            result.push_str(&full_name.replace('"', "\"\""));
            result.push('"');
        }
        if !result.is_empty() {
            result.push(' ');
        }
        result.push_str(&form_id);
        result
    };
    let mut name = short_name(true);
    if let Some(def) = &record.mr_def {
        let main_record: crate::interface::element::MainRecordRef = record.clone();
        let info = RecordDef::additional_info_for(&**def, &main_record);
        let info = info.trim();
        if !info.is_empty() {
            name = format!("{name} ({info})");
        }
    }
    (name, short_name(false))
}

/// Port of the save half of `BuildOrLoadRef`: the stream of every record
/// of `records` (the file's, in order) with the references and values of
/// `cache`, compressed into `path`.
pub fn save(
    file: &FileImpl,
    path: &Path,
    records: &[Arc<MainRecordImpl>],
    cache: &[(Vec<FormID>, RecordCacheData)],
) -> std::io::Result<()> {
    let save_names = file.get_file_states().contains(FileState::fsIsGameMaster);
    // The names are the GUI's (`TfrmMain` sets `wbDisplayShorterNames` and
    // `wbDisplayLoadOrderFormID`), also in the additional information that
    // names other records. The settings are process wide: the save runs on
    // the calling thread once the workers are done, and restores them.
    let settings = (display_shorter_names(), display_load_order_form_id());
    if save_names {
        set_display_shorter_names(true);
        set_display_load_order_form_id(true);
    }
    let cached: Vec<CachedRecord> = records
        .iter()
        .zip(cache)
        .map(|(record, (references, data))| {
            let editor_id = record.get_editor_id();
            let names = save_names.then(|| {
                if editor_id.is_empty() {
                    (String::new(), String::new())
                } else {
                    gui_names(record)
                }
            });
            CachedRecord {
                form_id: record.mr_struct().form_id,
                references: references.clone(),
                editor_id,
                full_name: record.get_full_name(),
                data: data.clone(),
                names,
            }
        })
        .collect();
    set_display_shorter_names(settings.0);
    set_display_load_order_form_id(settings.1);
    let compressed = compress(&encode(&cached, save_names))?;
    if let Some(folder) = path.parent() {
        std::fs::create_dir_all(folder)?;
    }
    std::fs::write(path, compressed)
}

/// Port of the load half of `BuildOrLoadRef`: the records of the cache file,
/// checked against the records of `file` (upstream asserts the count and
/// every FormID).
pub fn load(file: &FileImpl, path: &Path) -> Result<Vec<CachedRecord>, String> {
    let error = |message: String| format!("{}: {message}", path.display());
    let compressed = std::fs::read(path).map_err(|e| error(e.to_string()))?;
    let data = decompress(&compressed).map_err(|e| error(format!("LZ4F decompression error: {e}")))?;
    let load_names = file.get_file_states().contains(FileState::fsIsGameMaster);
    let cached = decode(&data, load_names).map_err(error)?;
    let records = file.records();
    if cached.len() != records.len() {
        return Err(error(
            "[TwbFile.BuildOrLoadRef] flRecordsCount <> Length(flRecords)".to_owned(),
        ));
    }
    for (cached, record) in cached.iter().zip(&records) {
        if cached.form_id != record.mr_struct().form_id {
            return Err(error(format!(
                "the cached record [{}] is not the record [{}] of the file",
                cached.form_id.to_string(true),
                record.mr_struct().form_id.to_string(true)
            )));
        }
    }
    Ok(cached)
}

impl MainRecordImpl {
    /// The `mrsEditorIDFromCache` and `mrsFullNameFromCache` of
    /// `LoadRefsFromStream`: the names come from the cache until the record
    /// is built.
    pub(crate) fn set_names_from_cache(&self, editor_id: String, full_name: String) {
        if self.mr_names_known.load(Ordering::Acquire) {
            return;
        }
        self.cache_editor_id(editor_id);
        self.set_full_name(full_name);
        self.mr_names_known.store(true, Ordering::Release);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> Vec<CachedRecord> {
        vec![
            CachedRecord {
                form_id: FormID::from_cardinal(0x0001_2345),
                references: vec![FormID::from_cardinal(7), FormID::from_cardinal(0x0100_0800)],
                editor_id: "Edid".to_owned(),
                full_name: "Name \u{e9}".to_owned(),
                data: RecordCacheData {
                    base_record_id: 0x0000_0014,
                    grid_cell: Some(Some((-3, 4))),
                },
                names: Some((
                    "Edid \"Name\" [NPC_:00012345]".to_owned(),
                    "Edid [NPC_:00012345]".to_owned(),
                )),
            },
            CachedRecord {
                form_id: FormID::from_cardinal(0x0001_2346),
                data: RecordCacheData {
                    base_record_id: 0,
                    grid_cell: Some(None),
                },
                names: Some(Default::default()),
                ..Default::default()
            },
        ]
    }

    #[test]
    fn stream_round_trips() {
        let records = sample();
        let stream = encode(&records, true);
        assert_eq!(decode(&stream, true).unwrap(), records);
        // The layout of the first record: FormID, count, references, the
        // names as UTF-16 with their lengths.
        assert_eq!(&stream[..4], &2i32.to_le_bytes());
        assert_eq!(&stream[4..8], &0x0001_2345u32.to_le_bytes());
        assert_eq!(&stream[8..12], &2i32.to_le_bytes());
        assert_eq!(&stream[20..24], &4i32.to_le_bytes());
        assert_eq!(
            &stream[24..32],
            "Edid"
                .encode_utf16()
                .flat_map(u16::to_le_bytes)
                .collect::<Vec<_>>()
                .as_slice()
        );
    }

    #[test]
    fn stream_without_names() {
        let mut records = sample();
        for record in &mut records {
            record.names = None;
        }
        let stream = encode(&records, false);
        assert_eq!(decode(&stream, false).unwrap(), records);
        assert!(decode(&stream[..stream.len() - 1], false).is_err());
    }

    #[test]
    fn frame_round_trips() {
        let stream = encode(&sample(), true);
        let frame = compress(&stream).unwrap();
        // The LZ4 frame magic.
        assert_eq!(&frame[..4], &[0x04, 0x22, 0x4D, 0x18]);
        assert_eq!(decompress(&frame).unwrap(), stream);
    }
}
