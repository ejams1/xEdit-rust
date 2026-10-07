// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

// Ported from xEdit: Core/wbImplementation.pas

//! Port of the save path of `wbImplementation.pas`: the modified states of
//! `TwbElement`, `PrepareSave`, `WriteToStream` and the CRC32 of a file.
//!
//! State: a file whose records are unmodified, or modified only by what the
//! load does itself (the offsets a worldspace drops), writes the bytes
//! upstream writes. A record written from its elements copies each
//! unmodified subrecord raw; the edits that change element data come with
//! the editing step of phase 3, which also lifts the `Unsupported` errors of
//! `prepare_save` where upstream edits the file header on save.

use std::sync::Arc;
use std::sync::atomic::Ordering;

use crate::interface::element::{Container, Element, ElementRef, File};
use crate::interface::globals::{
    GameMode, allow_esp_masters_on_save, always_save_onam, always_save_onam_force, app_name, clamp_form_id,
    complex_file_file_id, delay_load_records, game_mode, game_name, has_added_optimized_support, header_signature,
    is_fallout3, is_fallout4, is_fallout76, is_light_supported, is_skyrim, is_starfield, red_pill,
    size_of_main_record_struct, tool_name, vwd_as_quest_children, vwd_in_temporary, wb_group_order_count,
};
use crate::interface::types::{ElementType, FileState, Signature};

use super::structs::SubRecordHeaderStruct;
use super::{ElementBase, ElementImpl, FileImpl, GroupRecordImpl, MainRecordImpl};

/// Port of the save states of `TwbElementState`, as bits of
/// `ElementBase::e_states`.
#[allow(non_camel_case_types)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u32)]
pub enum ElementState {
    /// The element or one of its children changed; it is written from its
    /// elements.
    esModified = 1,
    /// The change came from an internal edit (the load itself), so the file
    /// does not count as unsaved.
    esInternalModified = 2,
    /// A change that is not on disk yet.
    esUnsaved = 4,
}

/// Port of `TwbResetModified`: what a save does to the modified states.
#[allow(non_camel_case_types)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ResetModified {
    rmNo,
    rmYes,
    rmSetInternal,
}

/// A save that did not happen. `Refused` carries an upstream exception
/// message: the oracle refuses the same file with the same text.
/// `Unsupported` names what the port cannot do yet.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum SaveError {
    #[error("{0}")]
    Refused(String),
    #[error("{0}")]
    Unsupported(String),
    #[error("{0}")]
    Internal(String),
}

impl ElementBase {
    pub fn has_state(&self, state: ElementState) -> bool {
        self.e_states.load(Ordering::Relaxed) & state as u32 != 0
    }

    pub(crate) fn include_state(&self, state: ElementState) {
        self.e_states.fetch_or(state as u32, Ordering::Relaxed);
    }

    pub(crate) fn exclude_state(&self, state: ElementState) {
        self.e_states.fetch_and(!(state as u32), Ordering::Relaxed);
    }

    /// Port of `TwbElement.ResetModified`, the tail of every
    /// `WriteToStreamInternal`.
    pub(crate) fn reset_modified(&self, reset: ResetModified) {
        self.exclude_state(ElementState::esUnsaved);
        match reset {
            ResetModified::rmNo => {}
            ResetModified::rmYes => {
                self.exclude_state(ElementState::esModified);
                self.exclude_state(ElementState::esInternalModified);
            }
            ResetModified::rmSetInternal => {
                if self.has_state(ElementState::esModified) {
                    self.include_state(ElementState::esInternalModified);
                }
            }
        }
    }
}

/// Port of `TwbElement.SetModified`. The sort keys, the generation counters
/// and the update count of upstream are not ported: nothing reads them yet.
pub(crate) fn element_set_modified(element: &dyn ElementImpl, value: bool) {
    if !value {
        return;
    }
    let base = element.element_base();
    if crate::interface::globals::is_internal_edit() {
        if !base.has_state(ElementState::esModified) {
            base.include_state(ElementState::esInternalModified);
        }
    } else {
        base.exclude_state(ElementState::esInternalModified);
        base.include_state(ElementState::esUnsaved);
    }
    base.include_state(ElementState::esModified);
    element.set_parent_modified();
}

/// Port of `MarkModifiedRecursive`: the elements of the given types below
/// `element` and `element` itself are marked modified; a main record also
/// marks its child group.
pub(crate) fn mark_modified_recursive(element: &dyn ElementImpl, types: &[ElementType]) {
    if !types.contains(&element.get_element_type()) {
        return;
    }
    // `TwbContainer.MarkModifiedRecursive`: `DoInit(False)`, then the elements.
    if let Some(container) = element.as_container() {
        container.get_element_count();
    }
    if let Some(base) = element.container_base() {
        for child in base.elements() {
            if let Some(child) = child.as_element_impl() {
                mark_modified_recursive(child, types);
            }
        }
    }
    // `TwbElement.MarkModifiedRecursive`: every container lets its elements
    // be edited here, so the element is marked.
    element.set_modified(true);
    if let Some(record) = element.main_record_impl()
        && let Some(group) = record.child_group()
    {
        mark_modified_recursive(&*group, types);
    }
}

/// Port of `TwbContainer.WriteToStreamInternal`: `DoInit(True)`, the
/// elements in order, then the states reset.
pub(crate) fn container_write_to_stream(
    element: &dyn ElementImpl,
    out: &mut Vec<u8>,
    reset: ResetModified,
) -> Result<(), SaveError> {
    if let Some(container) = element.as_container() {
        container.get_element_count();
    }
    if let Some(base) = element.container_base() {
        for child in base.elements() {
            child
                .as_element_impl()
                .ok_or_else(|| SaveError::Internal(format!("{} has no write path", child.get_full_path())))?
                .write_to_stream(out, reset)?;
        }
    }
    element.element_base().reset_modified(reset);
    Ok(())
}

/// Port of `TwbContainer.PrepareSave`: with `wbDelayLoadRecords` an
/// unmodified container is left alone; else the elements prepare in reverse
/// order.
pub(crate) fn container_prepare_save(element: &dyn ElementImpl) -> Result<(), SaveError> {
    if delay_load_records() && !element.element_base().has_state(ElementState::esModified) {
        return Ok(());
    }
    if let Some(base) = element.container_base() {
        for child in base.elements().iter().rev() {
            if let Some(child) = child.as_element_impl() {
                child.prepare_save()?;
            }
        }
    }
    Ok(())
}

/// Port of `TwbContainer.GetCountedRecordCount`: the sum over the elements
/// after `DoInit(False)`.
pub(crate) fn container_counted_record_count(element: &dyn ElementImpl) -> u32 {
    if let Some(container) = element.as_container() {
        container.get_element_count();
    }
    element.container_base().map_or(0, |base| {
        base.elements()
            .iter()
            .filter_map(|child| child.as_element_impl().map(ElementImpl::get_counted_record_count))
            .sum()
    })
}

/// Port of `TwbDataContainer.WriteToStreamInternal` for an element whose
/// data is `data`: a modified element writes its elements (its storage is
/// never valid in this version, as after `InvalidateStorage`), an
/// unmodified one copies its data.
pub(crate) fn data_container_write_to_stream(
    element: &dyn ElementImpl,
    data: Option<&[u8]>,
    dont_save: bool,
    out: &mut Vec<u8>,
    reset: ResetModified,
) -> Result<(), SaveError> {
    if dont_save {
        element.element_base().reset_modified(reset);
        return Ok(());
    }
    let old_len = out.len();
    let expected = usize::try_from(element.get_data_size()).unwrap_or(0);
    if element.element_base().has_state(ElementState::esModified) {
        container_write_to_stream(element, out, reset)?;
        if out.len() == old_len
            && let Some(data) = data
        {
            let size = expected.min(data.len());
            out.extend_from_slice(&data[..size]);
        }
    } else {
        if let Some(data) = data
            && expected > 0
        {
            // `WriteBuffer(GetDataBasePtr^, Size)`: the data as loaded.
            let size = expected.min(data.len());
            out.extend_from_slice(&data[..size]);
        }
        element.element_base().reset_modified(reset);
    }
    // `DoCheckSizeAfterWrite` is false for every data container, so a size
    // mismatch is not an error here.
    Ok(())
}

// ----- subrecords -----

impl super::sub_record::SubRecordImpl {
    /// Port of `TwbSubRecord.WriteToStreamInternal`: the header, with an
    /// `XXXX` subrecord before it for a size over 16 bits, then the data.
    /// A subrecord whose header carries size 0 is always rebuilt, because
    /// its real size came from an `XXXX` subrecord that left the record.
    pub(crate) fn write_to_stream_impl(&self, out: &mut Vec<u8>, reset: ResetModified) -> Result<(), SaveError> {
        let header = self.header_struct();
        let modified = self.base.has_state(ElementState::esModified);
        if modified || header.data_size == 0 {
            self.do_init();
            let big_size = u32::try_from(self.get_data_size()).unwrap_or(0);
            let header_bytes = if big_size > u32::from(u16::MAX) && game_mode() != GameMode::gmTES3 {
                out.extend_from_slice(
                    &SubRecordHeaderStruct {
                        signature: Signature::new(b"XXXX"),
                        data_size: 4,
                    }
                    .to_bytes(),
                );
                out.extend_from_slice(&big_size.to_le_bytes());
                SubRecordHeaderStruct {
                    signature: header.signature,
                    data_size: 0,
                }
            } else {
                SubRecordHeaderStruct {
                    signature: header.signature,
                    data_size: big_size,
                }
            };
            out.extend_from_slice(&header_bytes.to_bytes());
            let start = out.len();
            data_container_write_to_stream(self, self.data(), false, out, reset)?;
            let written = out.len() - start;
            if written != big_size as usize {
                return Err(SaveError::Internal(format!(
                    "{}: wrote {written} bytes for a subrecord of {big_size}",
                    self.get_full_path()
                )));
            }
        } else {
            out.extend_from_slice(self.header_bytes());
            let start = out.len();
            data_container_write_to_stream(self, self.data(), false, out, reset)?;
            if out.len() - start != header.data_size as usize {
                return Err(SaveError::Internal(format!(
                    "{}: wrote {} bytes for a subrecord of {}",
                    self.get_full_path(),
                    out.len() - start,
                    header.data_size
                )));
            }
        }
        self.base.reset_modified(reset);
        Ok(())
    }
}

// ----- group records -----

impl GroupRecordImpl {
    /// Port of `TwbGroupRecord.WriteToStreamInternal`: the header, the
    /// elements, and the size patched when the group changed.
    pub(crate) fn write_to_stream_impl(&self, out: &mut Vec<u8>, reset: ResetModified) -> Result<(), SaveError> {
        let start = out.len();
        out.extend_from_slice(&self.gr_struct.to_bytes());
        for child in self.container.elements() {
            child
                .as_element_impl()
                .ok_or_else(|| SaveError::Internal(format!("{} has no write path", child.get_full_path())))?
                .write_to_stream(out, reset)?;
        }
        if self.base.has_state(ElementState::esModified) {
            let size = u32::try_from(out.len() - start)
                .map_err(|_| SaveError::Refused(format!("{} is too large for a group", self.get_name())))?;
            out[start + 4..start + 8].copy_from_slice(&size.to_le_bytes());
        } else if out.len() - start != self.gr_struct.group_size as usize {
            return Err(SaveError::Internal(format!(
                "{}: wrote {} bytes for a group of {}",
                self.get_full_path(),
                out.len() - start,
                self.gr_struct.group_size
            )));
        }
        self.base.reset_modified(reset);
        Ok(())
    }

    /// Port of `TwbGroupRecord.PrepareSave`: a modified group sorts, an
    /// empty group leaves the file.
    pub(crate) fn prepare_save_impl(&self) -> Result<(), SaveError> {
        if self.base.has_state(ElementState::esModified) {
            self.sort();
        }
        container_prepare_save(self)?;
        if self.container.element_count() == 0 {
            self.remove();
        } else if self.base.has_state(ElementState::esModified) {
            self.gr_sorted.store(false, Ordering::Relaxed);
            self.sort();
        }
        Ok(())
    }

    /// Port of `TwbElement.Remove` for a group: the group leaves its
    /// container, which is modified by that.
    fn remove(&self) {
        let Some(this) = self.self_ref.upgrade() else { return };
        let this: ElementRef = this;
        if let Some(container) = self.base.container()
            && let Some(parent) = container.as_element_impl()
        {
            if let Some(base) = parent.container_base() {
                base.remove_element_by_identity(&this);
            }
            parent.set_modified(true);
        }
    }

    /// Port of `TwbGroupRecord.GetCountedRecordCount`: the group itself and
    /// what it holds.
    pub(crate) fn counted_record_count_impl(&self) -> u32 {
        1 + container_counted_record_count(self)
    }
}

// ----- main records -----

/// The signatures a persistent or visible-when-distant cell children group
/// may hold (`TwbMainRecord.PrepareSave`, group types 8 and 10).
const PLACED_SIGNATURES: [&[u8; 4]; 11] = [
    b"REFR", b"ACHR", b"ACRE", b"PGRE", b"PMIS", b"PARW", b"PBEA", b"PFLA", b"PCON", b"PBAR", b"PHZD",
];

impl MainRecordImpl {
    /// The bytes of the record as loaded: the header and the data.
    fn raw_record_bytes(&self) -> Option<&[u8]> {
        let header_size = size_of_main_record_struct() as usize;
        let start = self.dc_data_base.checked_sub(header_size)?;
        self.bytes.as_slice().get(start..self.dc_data_end)
    }

    /// Port of `TwbMainRecord.WriteToStreamInternal`: a modified record is
    /// rebuilt from its elements, compressed again when it is compressed,
    /// with the data size patched into the header; an unmodified one is
    /// copied as loaded.
    pub(crate) fn write_to_stream_impl(
        self: &Arc<Self>,
        out: &mut Vec<u8>,
        reset: ResetModified,
    ) -> Result<(), SaveError> {
        let header_size = size_of_main_record_struct() as usize;
        if self.base.has_state(ElementState::esModified) {
            self.do_init();
            let start = out.len();
            out.extend_from_slice(&self.mr_struct.to_bytes());
            if self.mr_struct.flags.is_compressed() {
                let mut data = Vec::new();
                container_write_to_stream(&**self, &mut data, reset)?;
                let size = u32::try_from(data.len())
                    .map_err(|_| SaveError::Refused(format!("{} is too large for a record", self.get_name())))?;
                out.extend_from_slice(&size.to_le_bytes());
                let compressed = xedit_io::CompressionType::ZLib
                    .compress(&data)
                    .map_err(|error| SaveError::Internal(error.to_string()))?;
                out.extend_from_slice(&compressed);
            } else {
                container_write_to_stream(&**self, out, reset)?;
            }
            let data_size = u32::try_from(out.len() - start - header_size)
                .map_err(|_| SaveError::Refused(format!("{} is too large for a record", self.get_name())))?;
            out[start + 4..start + 8].copy_from_slice(&data_size.to_le_bytes());
        } else {
            let raw = self
                .raw_record_bytes()
                .ok_or_else(|| SaveError::Internal(format!("{} has no data", self.get_full_path())))?;
            out.extend_from_slice(raw);
            if raw.len() != header_size + self.mr_struct.data_size as usize {
                return Err(SaveError::Internal(format!(
                    "{}: {} bytes loaded for a record of {}",
                    self.get_full_path(),
                    raw.len(),
                    self.mr_struct.data_size
                )));
            }
        }
        self.base.reset_modified(reset);
        Ok(())
    }

    /// Port of `TwbMainRecord.PrepareSave`: the record must sit in a group
    /// that may hold it, and a worldspace whose offsets were dropped marks
    /// its children modified so that their groups are sized again.
    pub(crate) fn prepare_save_impl(self: &Arc<Self>) -> Result<(), SaveError> {
        let signature = self.mr_struct.signature;
        let path = || self.get_full_path();
        if signature == header_signature() {
            let in_file = self
                .base
                .container()
                .is_some_and(|container| container.get_element_type() == ElementType::etFile);
            if !in_file {
                return Err(SaveError::Refused(format!(
                    "File Header record \"{}\" must be contained directly in the file.",
                    path()
                )));
            }
            if !self.mr_struct.form_id.is_null() {
                return Err(SaveError::Refused(format!(
                    "File Header record \"{}\" can not have a FormID.",
                    path()
                )));
            }
        } else {
            if self.mr_struct.form_id.is_null() {
                return Err(SaveError::Refused(format!("Record \"{}\" must have a FormID.", path())));
            }
            let group = self
                .base
                .container()
                .and_then(|container| container.as_element_impl()?.group_record_impl())
                .ok_or_else(|| SaveError::Refused(format!("Record \"{}\" is not contained in a group.", path())))?;
            let not_in = || {
                SaveError::Refused(format!(
                    "Record \"{}\" can not be contained in {}",
                    path(),
                    group.get_name()
                ))
            };
            let is = |expected: &[u8; 4]| signature == Signature::new(expected);
            let flags = self.mr_struct.flags;
            match group.group_type() {
                0 => {
                    if group.gr_struct.label_signature() != signature {
                        return Err(not_in());
                    }
                }
                1 => {
                    if !is(b"CELL") && !is(b"ROAD") {
                        return Err(not_in());
                    }
                }
                2 | 4 | 6 => return Err(not_in()),
                3 | 5 => {
                    if !is(b"CELL") {
                        return Err(not_in());
                    }
                }
                7 => {
                    if !is(b"INFO") {
                        return Err(not_in());
                    }
                }
                8 | 10 => {
                    let placed = PLACED_SIGNATURES.iter().any(|candidate| is(candidate));
                    let quest_child = vwd_as_quest_children() && (is(b"DLBR") || is(b"DIAL") || is(b"SCEN"));
                    if !placed && !quest_child {
                        return Err(not_in());
                    }
                    if group.group_type() == 8 {
                        if !flags.is_persistent() {
                            return Err(SaveError::Refused(format!(
                                "Record \"{}\" needs to have it's Persistent flag set to be contained in {}",
                                path(),
                                group.get_name()
                            )));
                        }
                    } else if !vwd_as_quest_children() {
                        if !flags.is_visible_when_distant() {
                            return Err(SaveError::Refused(format!(
                                "Record \"{}\" needs to have it's Visible when Distant flag set to be contained in {}",
                                path(),
                                group.get_name()
                            )));
                        }
                        if flags.is_persistent() {
                            return Err(SaveError::Refused(format!(
                                "Record \"{}\" can not have it's Persistent flag set to be contained in {}",
                                path(),
                                group.get_name()
                            )));
                        }
                    }
                }
                9 => {
                    let temporary = is(b"LAND")
                        || is(b"PGRD")
                        || is(b"NAVM")
                        || PLACED_SIGNATURES.iter().any(|candidate| is(candidate));
                    if !temporary {
                        return Err(not_in());
                    }
                    if flags.is_persistent() {
                        return Err(SaveError::Refused(format!(
                            "Record \"{}\" can not have it's Persistent flag set to be contained in {}",
                            path(),
                            group.get_name()
                        )));
                    }
                    if flags.is_visible_when_distant() && !vwd_in_temporary() {
                        return Err(SaveError::Refused(format!(
                            "Record \"{}\" can not have it's Visible when Distant flag set to be contained in {}",
                            path(),
                            group.get_name()
                        )));
                    }
                }
                _ => {}
            }
        }
        if self.mr_ofst_removed.swap(false, Ordering::Relaxed)
            && let Some(group) = self.child_group()
        {
            mark_modified_recursive(
                &*group,
                &[
                    ElementType::etFile,
                    ElementType::etMainRecord,
                    ElementType::etGroupRecord,
                ],
            );
        }
        container_prepare_save(&**self)
    }
}

// ----- files -----

impl FileImpl {
    /// Port of `TwbFile.GetCRC32`: the CRC32 of the file as loaded, or of
    /// the last save.
    pub fn crc32(&self) -> u32 {
        let known = self.fl_crc32.load(Ordering::Relaxed);
        if known != 0 {
            return known;
        }
        let crc = xedit_io::crc32(self.fl_bytes.as_slice());
        self.fl_crc32.store(crc, Ordering::Relaxed);
        crc
    }

    /// The extension of the file name, lower case, with the dot.
    fn extension(&self) -> String {
        std::path::Path::new(&self.fl_file_name)
            .extension()
            .map(|extension| format!(".{}", extension.to_string_lossy().to_ascii_lowercase()))
            .unwrap_or_default()
    }

    /// Port of `TwbFile.WriteToStream`: `PrepareSave`, the elements, and
    /// the CRC32 of the result. Returns the bytes of the file.
    pub fn write_to_bytes(self: &Arc<Self>, reset: ResetModified) -> Result<Vec<u8>, SaveError> {
        if self.get_file_states().contains(FileState::fsMastersUpdating) {
            return Err(SaveError::Internal(
                "the masters of the file are being updated".to_owned(),
            ));
        }
        self.prepare_save_impl()?;
        let mut out = Vec::with_capacity(self.fl_bytes.as_slice().len());
        container_write_to_stream(&**self, &mut out, reset)?;
        self.fl_crc32.store(xedit_io::crc32(&out), Ordering::Relaxed);
        Ok(out)
    }

    /// The name upstream puts into its messages: `wbAppName + wbToolName`.
    fn app_and_tool_name() -> String {
        format!("{}{}", app_name(), tool_name())
    }

    /// Port of `TwbFile.PrepareSave`. Where upstream edits the header on
    /// save (the record count in `HEDR`, the interior cell count in `INCC`,
    /// the `ONAM` list, a flag that follows the extension, a clamped
    /// FormID), this version checks whether the edit would change anything
    /// and returns `Unsupported` when it would; the editing step of phase 3
    /// replaces those checks with the edits.
    pub(crate) fn prepare_save_impl(self: &Arc<Self>) -> Result<(), SaveError> {
        let name = self.get_name();
        let elements = self.container.elements();
        let Some(first) = elements.first() else {
            return Err(SaveError::Refused(format!("File {name} has no file header")));
        };
        let header = first
            .as_element_impl()
            .and_then(ElementImpl::main_record_impl)
            .ok_or_else(|| {
                SaveError::Refused(format!(
                    "File {name} has invalid record {} as file header.",
                    first.get_name()
                ))
            })?;
        if header.mr_struct.signature != header_signature() {
            return Err(SaveError::Refused(format!(
                "File {name} has invalid record {} with invalid signature as file header.",
                first.get_name()
            )));
        }
        if header.mr_struct.flags.0 & 0x10 != 0 && !has_added_optimized_support() {
            return Err(SaveError::Refused(format!(
                "Modules with the \"Optimized\" file flag set can not be saved in {}",
                Self::app_and_tool_name()
            )));
        }
        let hedr = header
            .get_record_by_signature(Signature::new(b"HEDR"))
            .ok_or_else(|| SaveError::Refused(format!("File {name} has a file header with missing HEDR subrecord")))?;

        let extension = self.extension();
        let flags = header.mr_struct.flags;
        if extension == ".esm" && !flags.is_esm() {
            return Err(SaveError::Unsupported(format!(
                "{name} has the .esm extension without the ESM flag; setting the flag on save is not ported yet"
            )));
        }
        if is_light_supported() && extension == ".esl" && !(flags.is_esm() && flags.is_light()) {
            return Err(SaveError::Unsupported(format!(
                "{name} has the .esl extension without the ESM and Light flags; setting them on save is not ported yet"
            )));
        }
        if !allow_esp_masters_on_save() && self.masters().iter().any(|master| master.extension() == ".esp") {
            return Err(SaveError::Refused(format!(
                "{} modules must never have .esp masters.",
                game_name()
            )));
        }
        if is_starfield() {
            if flags.is_update() && (flags.is_light() || flags.is_medium()) {
                return Err(SaveError::Unsupported(format!(
                    "{name} has the Update flag with the Small or Medium flag; clearing it on save is not ported yet"
                )));
            }
            if extension == ".esp" {
                if flags.is_light() || flags.is_medium() {
                    return Err(SaveError::Refused(
                        "\".esp\" modules must not be small or medium.".to_owned(),
                    ));
                }
                if flags.is_esm() {
                    return Err(SaveError::Unsupported(format!(
                        "{name} is an .esp with the ESM flag; clearing it on save is not ported yet"
                    )));
                }
            }
            if flags.is_light() && flags.is_medium() {
                return Err(SaveError::Refused(
                    "Small or medium flags are mutually exclusive. Modules cannot be both.".to_owned(),
                ));
            }
            if flags.is_update() {
                return Err(SaveError::Refused(
                    "Update is not correctly supported by the game exe and will cause unexpected behavior. Saving update modules is not currently supported."
                        .to_owned(),
                ));
            }
            if flags.is_blueprint() {
                return Err(SaveError::Refused(
                    "Saving blueprint modules is not currently supported.".to_owned(),
                ));
            }
            if self.masters().iter().any(|master| {
                master
                    .header()
                    .is_some_and(|header| header.mr_struct.flags.is_blueprint())
            }) {
                return Err(SaveError::Refused(format!(
                    "{} modules must never have any blueprint masters.",
                    game_name()
                )));
            }
        }

        container_prepare_save(&**self)?;

        let mut seen = vec![false; usize::try_from(wb_group_order_count()).unwrap_or(0)];
        for element in elements.iter().skip(1) {
            let group = element
                .as_element_impl()
                .and_then(ElementImpl::group_record_impl)
                .ok_or_else(|| {
                    SaveError::Refused(format!(
                        "File {name} contains invalid top level record: {}",
                        element.get_name()
                    ))
                })?;
            if group.group_type() != 0 {
                return Err(SaveError::Refused(format!(
                    "File {name} contains invalid top level group type {} for group: {}",
                    group.group_type(),
                    element.get_name()
                )));
            }
            let sort_order = group.base.e_sort_order.load(Ordering::Relaxed);
            if sort_order < 0 {
                return Err(SaveError::Refused(format!(
                    "File {name} contains top level group without known sort order: {}",
                    element.get_name()
                )));
            }
            let Some(slot) = seen.get_mut(sort_order as usize) else {
                return Err(SaveError::Refused(format!(
                    "File {name} contains top level group with invalid sort order: {}",
                    element.get_name()
                )));
            };
            if *slot {
                return Err(SaveError::Refused(format!(
                    "File {name} contains duplicated top level group: {}",
                    element.get_name()
                )));
            }
            *slot = true;
            // Every worldspace is initialized, so that its offsets are
            // dropped and its child groups sorted.
            if group.gr_struct.label_signature() == Signature::new(b"WRLD") {
                for child in group.container.elements() {
                    if let Some(record) = child.as_element_impl().and_then(ElementImpl::main_record_impl) {
                        record.get_element_count();
                    }
                }
            }
        }
        // The groups after the header in sort order; the sort is stable.
        if elements.len() > 2 {
            let header_ref = elements[0].clone();
            self.container.sort_by(|a, b| {
                let order = |element: &ElementRef| {
                    if Arc::ptr_eq(element, &header_ref) {
                        i32::MIN
                    } else {
                        element.as_element_impl().map_or(i32::MAX, |element| {
                            element.element_base().e_sort_order.load(Ordering::Relaxed)
                        })
                    }
                };
                order(a).cmp(&order(b))
            });
        }

        let record_count = self.get_counted_record_count();
        if record_count < 1 {
            return Err(SaveError::Refused(format!("File {name} has an invalid record count")));
        }
        let counted = (record_count - 1).to_string();
        let stored = hedr
            .as_container()
            .and_then(|hedr| hedr.get_element(1))
            .map(|count| count.get_edit_value())
            .unwrap_or_default();
        if stored != counted {
            return Err(SaveError::Unsupported(format!(
                "{name}: HEDR holds {stored} records and the file has {counted}; updating the count on save is not ported yet"
            )));
        }

        if is_skyrim() || is_fallout3() || is_fallout4() || is_fallout76() || is_starfield() {
            if !is_fallout3() {
                let cells = self
                    .records()
                    .iter()
                    .filter(|record| record.mr_struct.signature == Signature::new(b"CELL"))
                    .filter(|record| {
                        let interior = record
                            .get_element_native_value("DATA")
                            .as_ordinal()
                            .is_some_and(|flags| flags & 1 == 1);
                        // The records are released again as the dump does.
                        super::trim_initialized_records(64, None);
                        interior
                    })
                    .count()
                    .to_string();
                let stored = header
                    .get_record_by_signature(Signature::new(b"INCC"))
                    .map(|incc| incc.get_edit_value());
                if stored.as_deref() != Some(cells.as_str()) {
                    return Err(SaveError::Unsupported(format!(
                        "{name}: INCC holds {} interior cells and the file has {cells}; updating the count on save is not ported yet",
                        stored.unwrap_or_else(|| "no".to_owned())
                    )));
                }
            }
            if header.get_record_by_signature(Signature::new(b"ONAM")).is_some() {
                return Err(SaveError::Unsupported(format!(
                    "{name}: the file header holds ONAM entries, which upstream rebuilds on save; that is not ported yet"
                )));
            }
            if always_save_onam() || always_save_onam_force() || flags.is_esm() || extension == ".esm" {
                let master_count = self.master_count();
                let needs_onam = self.records().iter().any(|record| {
                    let form_id = record.get_fixed_form_id();
                    i32::from(form_id.file_id().full_slot()) < master_count
                        && onam_signature(record.mr_struct.signature)
                        && !record.mr_struct.flags.is_persistent()
                });
                if needs_onam {
                    return Err(SaveError::Unsupported(format!(
                        "{name}: upstream writes ONAM entries for its overridden placed records on save; that is not ported yet"
                    )));
                }
            }
        }

        if clamp_form_id() || self.get_file_states().contains(FileState::fsIsDeltaPatch) {
            let mut index = self.master_count();
            if self.get_file_states().contains(FileState::fsIsDeltaPatch) {
                index -= 1;
            }
            if game_mode() != GameMode::gmTES3 && !complex_file_file_id() {
                let records = self.records();
                let clamped = records
                    .iter()
                    .find(|record| i32::from(record.mr_struct.form_id.file_id().full_slot()) > index);
                if let Some(record) = clamped {
                    return Err(SaveError::Unsupported(format!(
                        "{name}: {} has a FormID beyond the masters, which upstream clamps on save; that is not ported yet",
                        record.get_name()
                    )));
                }
            }
        }

        if complex_file_file_id() {
            if !red_pill() {
                for master in self.masters() {
                    if master.header().is_some_and(|header| header.mr_struct.flags.is_update()) {
                        return Err(SaveError::Refused(format!(
                            "Modules with Update flagged modules as masters can't be saved in {}",
                            Self::app_and_tool_name()
                        )));
                    }
                }
                if flags.is_update() {
                    return Err(SaveError::Refused(format!(
                        "Update flagged files can't be saved in {}",
                        Self::app_and_tool_name()
                    )));
                }
            }
        } else {
            let own = self.get_file_file_id();
            let own_records = || {
                self.records()
                    .into_iter()
                    .filter(move |record| record.get_fixed_form_id().file_id() == own)
            };
            if flags.is_light()
                && let Some(record) =
                    own_records().find(|record| record.get_fixed_form_id().to_cardinal() & 0x00FF_F000 != 0)
            {
                return Err(SaveError::Refused(format!(
                    "Record {} has invalid ObjectID {:06X} for a Light module. You will not be able to save this file with Light flag active",
                    record.get_name(),
                    record.get_fixed_form_id().to_cardinal() & 0x00FF_FFFF
                )));
            }
            if flags.is_medium()
                && let Some(record) =
                    own_records().find(|record| record.get_fixed_form_id().to_cardinal() & 0x00FF_0000 != 0)
            {
                return Err(SaveError::Refused(format!(
                    "Record {} has invalid ObjectID {:06X} for a Medium module. You will not be able to save this file with Medium flag active",
                    record.get_name(),
                    record.get_fixed_form_id().to_cardinal() & 0x00FF_FFFF
                )));
            }
            if flags.is_update() {
                if own.full_slot() <= 0 {
                    return Err(SaveError::Refused(format!(
                        "File {name} is an update module with no masters. You will not be able to save this file with Update flag active"
                    )));
                }
                if let Some(record) = own_records().next() {
                    return Err(SaveError::Refused(format!(
                        "Record {} has invalid ObjectID {:06X} for an update module. You will not be able to save this file with Update flag active",
                        record.get_name(),
                        record.mr_struct.form_id.to_cardinal() & 0x00FF_FFFF
                    )));
                }
            }
        }
        Ok(())
    }
}

/// The signatures `TwbFile.PrepareSave` writes `ONAM` entries for.
fn onam_signature(signature: Signature) -> bool {
    let placed = matches!(
        &signature.0,
        b"NAVM"
            | b"LAND"
            | b"REFR"
            | b"PGRE"
            | b"PMIS"
            | b"ACHR"
            | b"ACRE"
            | b"PARW"
            | b"PBEA"
            | b"PFLA"
            | b"PCON"
            | b"PBAR"
            | b"PHZD"
    );
    placed || ((is_fallout4() || is_starfield()) && matches!(&signature.0, b"SCEN" | b"DLBR" | b"DIAL" | b"INFO"))
}
