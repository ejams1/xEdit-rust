// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! A compressed structure whose decompression fails (issue #86, the Fallout 4
//! saves): upstream's `DecompressIfNeeded` leaves the structure with nil
//! data pointers, and `StructDoInit` builds its members over them all the
//! same, so every member is there and its value is an error.

use std::sync::Arc;

use xedit_core::implementation::{FileBytes, wb_file_from_bytes};
use xedit_core::interface::globals::test_lock;
use xedit_core::interface::types::FileStates;
use xedit_core::interface::{
    ConflictPriority, DataPtr, ElementArg, File, FormID, IntType, RecordMemberDef, Signature, ValueDef, wb_float,
    wb_integer, wb_record, wb_struct, wb_struct_z, wb_sub_record_signature,
};

fn sub_record(signature: &[u8; 4], data: &[u8]) -> Vec<u8> {
    let mut bytes = signature.to_vec();
    bytes.extend_from_slice(&(data.len() as u16).to_le_bytes());
    bytes.extend_from_slice(data);
    bytes
}

fn main_record(signature: &[u8; 4], form_id: u32, data: &[u8]) -> Vec<u8> {
    let mut bytes = signature.to_vec();
    bytes.extend_from_slice(&(data.len() as u32).to_le_bytes());
    bytes.extend_from_slice(&0u32.to_le_bytes());
    bytes.extend_from_slice(&form_id.to_le_bytes());
    bytes.extend_from_slice(&0u32.to_le_bytes());
    bytes.extend_from_slice(&44u16.to_le_bytes());
    bytes.extend_from_slice(&0u16.to_le_bytes());
    bytes.extend_from_slice(data);
    bytes
}

fn top_group(label: &[u8; 4], records: &[u8]) -> Vec<u8> {
    let mut bytes = b"GRUP".to_vec();
    bytes.extend_from_slice(&((24 + records.len()) as u32).to_le_bytes());
    bytes.extend_from_slice(label);
    bytes.extend_from_slice(&0i32.to_le_bytes());
    bytes.extend_from_slice(&0u64.to_le_bytes());
    bytes.extend_from_slice(records);
    bytes
}

/// The uncompressed size the sizer gives: 8 bytes, where the first four
/// bytes of the compressed data say otherwise, so the zlib decompression
/// fails as it does for the change forms of a Fallout 4 save.
fn sizer(_data: DataPtr, _element: ElementArg, compressed_size: &mut i32) -> u32 {
    *compressed_size = 8;
    8
}

fn define_test_record() {
    let integer = |name: &str| {
        wb_integer(
            name,
            IntType::itU32,
            None,
            ConflictPriority::cpNormal,
            false,
            None,
            None,
            0,
            None,
        )
        .map(|def| def as Arc<dyn ValueDef>)
    };
    let inner = wb_struct(
        "Inner",
        &[
            integer("A"),
            wb_float("B", ConflictPriority::cpNormal, false, None, None, None, 0.0, None)
                .map(|def| def as Arc<dyn ValueDef>),
        ],
        ConflictPriority::cpNormal,
        false,
        None,
        -1,
        None,
        None,
        None,
    );
    let packed = wb_struct_z(
        "Packed",
        Some(Arc::new(sizer)),
        None,
        None,
        None,
        &[inner.map(|def| def as Arc<dyn ValueDef>)],
        ConflictPriority::cpNormal,
        false,
        None,
        -1,
        None,
        None,
        None,
    );
    let outer = wb_struct(
        "Outer",
        &[integer("Before"), packed.map(|def| def as Arc<dyn ValueDef>)],
        ConflictPriority::cpNormal,
        false,
        None,
        -1,
        None,
        None,
        None,
    );
    let data = wb_sub_record_signature(
        Signature::new(b"DATA"),
        "Data",
        outer.map(|def| def as Arc<dyn ValueDef>),
        None,
        None,
        ConflictPriority::cpNormal,
        false,
        false,
        None,
        None,
    );
    wb_record(
        Signature::new(b"ZTST"),
        "Compressed Test",
        &[data.map(|def| def as Arc<dyn RecordMemberDef>)],
        false,
        None,
        ConflictPriority::cpNormal,
        false,
        None,
        None,
    );
}

fn names(element: &xedit_core::interface::ElementRef) -> Vec<String> {
    let Some(container) = element.as_container() else {
        return Vec::new();
    };
    (0..container.get_element_count())
        .filter_map(|index| container.get_element(index))
        .map(|child| format!("{} {:?}", child.get_name(), names(&child)))
        .collect()
}

#[test]
fn a_failed_decompression_leaves_the_members_with_errors() {
    let _guard = test_lock();
    xedit_core::implementation::clear_files_map();
    xedit_core::implementation::reset_load_order_slots();
    xedit_core::interface::clear_files();
    xedit_session::dump::setup_game("sse").unwrap();
    if xedit_core::interface::find_record_def(Signature::new(b"ZTST")).is_none() {
        define_test_record();
    }

    let mut hedr = Vec::new();
    hedr.extend_from_slice(&1.7f32.to_le_bytes());
    hedr.extend_from_slice(&1u32.to_le_bytes());
    hedr.extend_from_slice(&0x801u32.to_le_bytes());
    let mut bytes = main_record(b"TES4", 0, &sub_record(b"HEDR", &hedr));
    // `Before`, then the compressed data: its first four bytes (0x99) are
    // not the uncompressed size (8).
    let mut data = 7u32.to_le_bytes().to_vec();
    data.extend_from_slice(&[0x99, 0, 0, 0, 0x78, 0x9C, 0x01, 0x02]);
    bytes.extend(top_group(
        b"ZTST",
        &main_record(b"ZTST", 0x800, &sub_record(b"DATA", &data)),
    ));
    let file = wb_file_from_bytes("Compressed.esp", i32::MAX, FileStates::empty(), FileBytes::Owned(bytes)).unwrap();

    let record = file
        .get_record_by_form_id(FormID::from_cardinal(0x800), false, false)
        .unwrap()
        .expect("the test record");
    let value = |path: &str| {
        record
            .get_element_by_path(path)
            .map(|element| element.get_value())
            .unwrap_or_else(|| {
                panic!(
                    "{path} is missing from {:?}",
                    names(&(record.clone() as xedit_core::interface::ElementRef))
                )
            })
    };
    assert_eq!(value("DATA\\Outer\\Before"), "7");
    let inner = record
        .get_element_by_path("DATA\\Outer\\Packed\\Inner")
        .expect("the member over no data");
    assert_eq!(inner.as_container().map(|inner| inner.get_element_count()), Some(2));
    assert_eq!(
        value("DATA\\Outer\\Packed\\Inner\\A"),
        "<Error: Expected 4 bytes of data, found 0>"
    );
    assert_eq!(
        value("DATA\\Outer\\Packed\\Inner\\B"),
        "<Error: Expected 4 bytes of data, found 0>"
    );
    xedit_core::implementation::clear_files_map();
}
