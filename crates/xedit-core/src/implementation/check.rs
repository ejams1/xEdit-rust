// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

// Ported from xEdit: Core/wbImplementation.pas (TwbMainRecord.GetCheck,
// TwbSubRecord.GetCheck, TwbSubRecordArray.GetCheck,
// TwbSubRecordStruct.GetCheck, TwbValueBase.GetCheck)

//! The `Check` of the element classes, which "Check for Errors" asks of
//! every element: the main record's own checks (new records in an update
//! module, object IDs beyond a light or medium module's range, a FormID
//! that differs from its fixed FormID, the members of a deleted record or a
//! partial form, the required members it lacks) and, for every element
//! with data, the `Check` of its value definition; each ends with the
//! `ToStr` callback of the definition asked with `ctCheck`. The definition
//! side (`TwbIntegerDef.Check` and the rest) is in `crate::interface`.

use std::sync::Arc;

use crate::interface::def::NamedDef;
use crate::interface::element::{Container, ElementRef, MainRecord};
use crate::interface::globals::is_morrowind;
use crate::interface::sub_record::RecordMemberDef;
use crate::interface::sub_record_group::RecordDef;
use crate::interface::types::{CallbackType, DefFlag, KnownSubRecord, PascalEnum};

use super::MainRecordImpl;

/// Whether two definitions are the same object (`Def.Equals`).
fn same_def(a: &Arc<dyn NamedDef>, b: &Arc<dyn RecordMemberDef>) -> bool {
    std::ptr::addr_eq(Arc::as_ptr(a), Arc::as_ptr(b))
}

/// Delphi's `SetLength(Result, Length(Result) - 2)` after a list of
/// `name, ` items: the last separator goes (an empty list stays empty).
fn drop_last_separator(result: &mut String) {
    let len = result.chars().count();
    *result = result.chars().take(len.saturating_sub(2)).collect();
}

impl MainRecordImpl {
    /// Port of `TwbMainRecord.GetCheck`.
    pub(super) fn check(self: &Arc<Self>) -> String {
        let mut result = String::new();
        let Some(mr_def) = &self.mr_def else {
            return result;
        };
        // `recSkipped`: upstream drops a record skipped for its duplicate
        // FormID from the tree; the port keeps it and checks nothing.
        if self.is_skipped_duplicate() {
            return result;
        }

        if let Some(file) = self.file_impl() {
            if file.get_is_update() {
                let form_id = self.mr_struct().form_id;
                if file.is_new_record(form_id.file_id()) {
                    result = "An update module can not contain new records.".to_owned();
                }
            }
            if result.is_empty() {
                let form_id = self.mr_struct().form_id;
                let fixed_form_id = self.get_fixed_form_id();
                let own = fixed_form_id.file_id() == file.get_file_file_id();
                if file.get_is_light() && form_id.object_id() > 0xFFF && own {
                    result = format!(
                        "ObjectID {:06X} is invalid for a light module.",
                        form_id.to_cardinal() & 0x00FF_FFFF
                    );
                } else if file.get_is_medium() && form_id.object_id() > 0xFFFF && own {
                    result = format!(
                        "ObjectID {:06X} is invalid for a medium module.",
                        form_id.to_cardinal() & 0x00FF_FFFF
                    );
                } else if form_id != fixed_form_id && !is_morrowind() {
                    result = format!(
                        "Warning: internal file FormID is a HITME: {} (should be {} )",
                        form_id.to_string(true),
                        fixed_form_id.to_string(true)
                    );
                }
            }
            if !result.is_empty() {
                return result;
            }
        }

        self.do_init();
        let elements = self.container.elements();
        let additional = usize::try_from(self.get_additional_element_count()).unwrap_or(0);

        if self.mr_struct().flags.is_deleted() {
            let base_record = mr_def.known_sub_record_signatures()[KnownSubRecord::ksrBaseRecord.ord()];
            for element in elements.iter().skip(additional) {
                let Some(def) = element.get_def() else { continue };
                if mr_def.get_is_reference()
                    && def
                        .as_signature_def()
                        .is_some_and(|signature| signature.get_default_signature() == base_record)
                {
                    continue;
                }
                result.push_str(def.get_name());
                result.push_str(", ");
            }
            drop_last_separator(&mut result);
            if !result.is_empty() {
                result = format!("Record marked as deleted, but contains: {result}");
            }
            return result;
        }

        if self.get_is_partial_form() {
            let editor_id = mr_def.known_sub_record_signatures()[KnownSubRecord::ksrEditorID.ord()];
            for element in elements.iter().skip(additional) {
                let Some(def) = element.get_def() else { continue };
                if def
                    .as_signature_def()
                    .is_some_and(|signature| signature.get_default_signature() == editor_id)
                {
                    continue;
                }
                if def.def_base().def_flags.contains(DefFlag::dfDontSave) {
                    continue;
                }
                result.push_str(def.get_name());
                result.push_str(", ");
            }
            drop_last_separator(&mut result);
            if !result.is_empty() {
                result = format!("Record is Partial Form, but contains: {result}");
            }
            return result;
        }

        let member_count = usize::try_from(mr_def.get_member_count()).unwrap_or(0);
        let mut required_count: i32 = (0..member_count)
            .filter(|&index| mr_def.get_member(index).get_required())
            .count() as i32;
        for element in &elements {
            if let Some(def) = element.get_def()
                && def.get_required()
            {
                required_count -= 1;
            }
        }
        if required_count > 0 {
            result = "Missing required members: ".to_owned();
            for index in 0..member_count {
                let member = mr_def.get_member(index);
                if !member.get_required() {
                    continue;
                }
                let found = elements
                    .iter()
                    .any(|element| element.get_def().is_some_and(|def| same_def(&def, &member)));
                if !found {
                    result.push_str(&member.get_full_path());
                    result.push_str(", ");
                }
            }
            drop_last_separator(&mut result);
        }

        let self_ref: ElementRef = self.clone();
        mr_def.call_to_str(&mut result, Some(&self_ref), CallbackType::ctCheck);
        result
    }
}

/// Port of `TwbSubRecordArray.GetCheck` and `TwbSubRecordStruct.GetCheck`:
/// the `ToStr` callback of the definition with `ctCheck`.
pub(super) fn record_member_check(def: &dyn NamedDef, element: Option<&ElementRef>) -> String {
    let mut result = String::new();
    def.call_to_str(&mut result, element, CallbackType::ctCheck);
    result
}

/// The elements a "Check for Errors" walk visits below `element`, in the
/// order `CheckForErrorsLinear` visits them (`Container.Elements[i]`).
pub fn check_children(element: &ElementRef) -> Vec<ElementRef> {
    match element.as_container() {
        Some(container) => (0..container.get_element_count())
            .filter_map(|index| container.get_element(index))
            .collect(),
        None => Vec::new(),
    }
}
