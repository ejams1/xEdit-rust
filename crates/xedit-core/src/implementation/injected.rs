// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

// Ported from xEdit: Core/wbImplementation.pas
// (TwbMainRecord.GetInjectionSourceFiles, TwbMainRecord.GetReferencesInjected,
// TwbElement.GetReferencesInjected, TwbMainRecord.RemoveInjected,
// TwbContainer.RemoveInjected, TwbElement.RemoveInjected)

//! The references of a record to injected records: a record of another
//! plugin that uses the FormID space of a master (`GetIsInjected`). A
//! record that refers to an injected record whose file is not one of its
//! own file's masters depends on a plugin it does not name; the GUI's
//! "Cleanup injected records" copies such a record into the plugin that
//! holds the injected records and removes the references from the
//! original.
//!
//! Upstream reads the references of a record from `mrReferences`, which the
//! reference index fills (`BuildRef`); here they are the FormIDs that
//! [`MainRecordImpl::build_ref`] collects, every time they are asked for.
//! Upstream answers `ReferencesInjected` only for a record whose references
//! were built (`csRefsBuild`); the GUI builds them for every record when it
//! loads the plugins, so the port answers it always.

use std::sync::Arc;

use crate::interface::element::{Container, Element};
use crate::interface::types::Signature;

use super::{ElementImpl, FileImpl, MainRecordImpl};

/// The record of a reference of `record` (a file FormID), when it is in
/// another file and is injected there: the file that holds it.
fn injected_source(
    record: &MainRecordImpl,
    file: &Arc<FileImpl>,
    form_id: crate::interface::FormID,
) -> Option<Arc<FileImpl>> {
    let target = file.record_by_form_id(form_id, true, record.get_masters_updated())?;
    let target_file = target.file_impl()?;
    if Arc::ptr_eq(&target_file, file) {
        return None;
    }
    let master = target.master().unwrap_or(target);
    if !master.is_injected() {
        return None;
    }
    Some(target_file)
}

impl MainRecordImpl {
    /// Port of `TwbMainRecord.GetInjectionSourceFiles`: the files of the
    /// injected records this record refers to, in load order.
    pub fn injection_source_files(self: &Arc<Self>) -> Vec<Arc<FileImpl>> {
        let Some(file) = self.file_impl() else {
            return Vec::new();
        };
        let mut result: Vec<Arc<FileImpl>> = Vec::new();
        for form_id in self.collect_references() {
            if let Some(source) = injected_source(self, &file, form_id) {
                result.push(source);
            }
        }
        result.sort_by_key(|file| file.load_order());
        result.dedup_by(|a, b| Arc::ptr_eq(a, b));
        result
    }

    /// Port of `TwbMainRecord.GetReferencesInjected`: the record refers to
    /// an injected record whose file is not a master of the record's file.
    pub fn references_injected(self: &Arc<Self>) -> bool {
        let Some(file) = self.file_impl() else {
            return false;
        };
        let masters = file.masters();
        self.collect_references().into_iter().any(|form_id| {
            injected_source(self, &file, form_id)
                .is_some_and(|source| !masters.iter().any(|master| Arc::ptr_eq(master, &source)))
        })
    }

    /// Port of `TwbMainRecord.RemoveInjected`: the elements that refer to an
    /// injected record of a file that is not a master go (a script of the
    /// older games loses its references and its compiled code). Returns
    /// whether a reference is left that could not be removed; with
    /// `can_remove` the record itself goes instead when it can.
    pub fn remove_injected(self: &Arc<Self>, can_remove: bool) -> bool {
        self.do_init();
        let mut result = false;
        if self.references_injected() {
            if self.get_signature() == Signature::new(b"SCPT") {
                remove_script_references(self);
            } else {
                for element in self.container.elements().into_iter().rev() {
                    let Some(element) = element.as_element_impl() else {
                        continue;
                    };
                    if element.can_contain_form_ids() {
                        result = remove_injected_element(element, true) || result;
                        if result && can_remove {
                            break;
                        }
                    }
                }
            }
        }
        if result && can_remove && self.get_is_removable() {
            result = false;
            self.remove();
        }
        result
    }
}

/// The `SCPT` branch of `TwbMainRecord.RemoveInjected`: the references and
/// the compiled script go, and the source loses everything from its first
/// `begin`.
fn remove_script_references(record: &Arc<MainRecordImpl>) {
    if let Some(element) = record.get_element_by_name("References") {
        element.remove();
    }
    if let Some(element) = record.get_record_by_signature(Signature::new(b"SCDA")) {
        let _ = element.set_edit_value("1D 00 00 00");
    }
    if let Some(element) = record.get_record_by_signature(Signature::new(b"SCHR"))
        && let Some(header) = element.as_container()
    {
        if let Some(count) = header.get_element_by_name("RefCount") {
            let _ = count.set_edit_value("0");
        }
        if let Some(size) = header.get_element_by_name("CompiledSize") {
            let _ = size.set_edit_value("4");
        }
    }
    if let Some(element) = record.get_record_by_signature(Signature::new(b"SCTX")) {
        // `TStringList.Text`: the lines up to the first that starts with
        // `begin`. UPSTREAM-QUIRK: the loop deletes the last line while
        // `i <= Count`, so it keeps the lines before that one.
        let text = element.get_edit_value();
        let lines: Vec<&str> = text.lines().collect();
        if let Some(begin) = lines.iter().position(|line| line.trim().starts_with("begin")) {
            let mut kept = lines.clone();
            while begin <= kept.len() && !kept.is_empty() {
                kept.pop();
            }
            let mut text = kept.join("\r\n");
            if !kept.is_empty() {
                text.push_str("\r\n");
            }
            let _ = element.set_edit_value(&text);
        }
    }
}

/// Port of `TwbElement.GetReferencesInjected`: the element links to an
/// injected record of a file that is not its file or one of its masters.
fn element_references_injected(element: &dyn ElementImpl) -> bool {
    let Some(linked) = element.get_links_to() else {
        return false;
    };
    let Some(record) = linked.as_element_impl().and_then(ElementImpl::main_record_impl) else {
        return false;
    };
    if !record.is_injected() {
        return false;
    }
    let (Some(file), Some(linked_file)) = (element.file_impl(), record.file_impl()) else {
        return false;
    };
    !Arc::ptr_eq(&file, &linked_file) && !file.masters().iter().any(|master| Arc::ptr_eq(master, &linked_file))
}

/// Port of `TwbContainer.RemoveInjected` and `TwbElement.RemoveInjected`.
fn remove_injected_element(element: &dyn ElementImpl, can_remove: bool) -> bool {
    let mut result = element_references_injected(element);
    if !result && let Some(base) = element.container_base() {
        for child in base.elements().into_iter().rev() {
            let Some(child) = child.as_element_impl() else { continue };
            if child.can_contain_form_ids() {
                result = remove_injected_element(child, true) || result;
                if result && can_remove {
                    break;
                }
            }
        }
    }
    if result && can_remove && element.get_is_removable() {
        result = false;
        element.remove();
    }
    result
}
