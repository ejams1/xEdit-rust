// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

// Ported from xEdit: Core/wbInterface.pas

//! `TwbFormIDDefFormater` with `TwbFormIDChecked` and `TwbFormIDCheckedST`.
//!
//! Not ported yet: `TwbRefID`, which only save files use, the lookup of actor
//! value records (`FindRecordForAVCode`) of Fallout 3 and New Vegas, the report
//! mode statistics and `GetEditInfo`.

use std::sync::{Arc, RwLock, Weak};

use super::def::{Def, DefBase, DefKind, DefRef, NamedDef, NamedDefBase};
use super::element::{
    ElementArg, ElementRef, FileRef, MainRecordRef, get_game_master_file, record_by_load_order_form_id,
};
use super::enum_def::EnumDef;
use super::form_id::{FileID, FormID};
use super::formaters::{editable_unless_internal_only, formater_impls, formater_plumbing};
use super::globals::{disable_form_id_check, display_load_order_form_id};
use super::integer::{IntegerDefFormater, integer_def_formater_create};
use super::misc::int_to_hex64;
use super::string::to_comma_text;
use super::types::{DefFlag, DefType, EditType, ElementType, EnumSet, FileState, Signature};

static ACTOR_VALUE_ENUM: RwLock<Option<Arc<EnumDef>>> = RwLock::new(None);

/// Sets upstream `wbActorValueEnum`, which the game definitions provide.
pub fn set_actor_value_enum(value: Option<Arc<EnumDef>>) {
    *ACTOR_VALUE_ENUM.write().unwrap() = value;
}

/// Upstream `wbActorValueEnum`.
pub fn actor_value_enum() -> Option<Arc<EnumDef>> {
    ACTOR_VALUE_ENUM.read().unwrap().clone()
}

const ACVA: Signature = Signature::new(b"ACVA");
const NULL: Signature = Signature::new(b"NULL");
const TRGT: Signature = Signature::new(b"TRGT");
const FFFF: Signature = Signature::new(b"FFFF");
const PLYR: Signature = Signature::new(b"PLYR");
const FLST: Signature = Signature::new(b"FLST");

/// The upstream FormID formater classes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FormIDClass {
    /// `TwbFormIDDefFormater`: any record.
    FormID,
    /// `TwbFormIDChecked`: records with one of the valid signatures.
    Checked,
    /// `TwbFormIDCheckedST`: checked, and the sort key starts with the position
    /// of the signature in the list of valid signatures.
    CheckedST,
}

impl FormIDClass {
    fn class_name(self) -> &'static str {
        match self {
            FormIDClass::FormID => "TwbFormIDDefFormater",
            FormIDClass::Checked => "TwbFormIDChecked",
            FormIDClass::CheckedST => "TwbFormIDCheckedST",
        }
    }

    fn is_checked(self) -> bool {
        self != FormIDClass::FormID
    }
}

/// Upstream `TwbFormIDDefFormater`: shows an integer as the record it refers to.
pub struct FormIDDefFormater {
    self_ref: Weak<FormIDDefFormater>,
    def: DefBase,
    nd: NamedDefBase,
    class: FormIDClass,
    fidc_valid_refs_arr: Vec<Signature>,
    /// `fidc_valid_refs_arr` sorted and without duplicates, as upstream keeps it
    /// in a sorted string list.
    fidc_valid_refs: Vec<Signature>,
    fidc_valid_flst_refs_arr: Vec<Signature>,
    fidc_valid_flst_refs: Vec<Signature>,
    fidc_persistent: bool,
    fidc_no_reach: bool,
}

fn sorted_set(signatures: &[Signature]) -> Vec<Signature> {
    let mut result = signatures.to_vec();
    result.sort();
    result.dedup();
    result
}

impl FormIDDefFormater {
    /// Port of `TwbFormIDDefFormater.Create`.
    pub fn create() -> Arc<Self> {
        Self::new(FormIDClass::FormID, &[], &[], false, false)
    }

    /// Port of `TwbFormIDChecked.Create`.
    pub fn create_checked(
        valid_refs: &[Signature],
        valid_flst_refs: &[Signature],
        persistent: bool,
        no_reach: bool,
    ) -> Arc<Self> {
        Self::new(FormIDClass::Checked, valid_refs, valid_flst_refs, persistent, no_reach)
    }

    /// Port of `TwbFormIDCheckedST.Create`.
    pub fn create_checked_st(valid_refs: &[Signature], persistent: bool) -> Arc<Self> {
        Self::new(FormIDClass::CheckedST, valid_refs, &[], persistent, false)
    }

    fn new(
        class: FormIDClass,
        valid_refs: &[Signature],
        valid_flst_refs: &[Signature],
        persistent: bool,
        no_reach: bool,
    ) -> Arc<Self> {
        let (def, nd) = integer_def_formater_create(class.class_name());
        let this = Arc::new_cyclic(|self_ref: &Weak<Self>| Self {
            self_ref: self_ref.clone(),
            def,
            nd,
            class,
            fidc_valid_refs_arr: valid_refs.to_vec(),
            fidc_valid_refs: sorted_set(valid_refs),
            fidc_valid_flst_refs_arr: valid_flst_refs.to_vec(),
            fidc_valid_flst_refs: sorted_set(valid_flst_refs),
            fidc_persistent: persistent,
            fidc_no_reach: no_reach,
        });
        DefBase::after_construction(&*this);
        // Port of the override of AfterConstruction.
        this.def.def_flags.include(DefFlag::dfCanContainFormID);
        this
    }

    pub fn clone_from(source: &Self) -> Arc<Self> {
        let this = Self::new(
            source.class,
            &source.fidc_valid_refs_arr,
            &source.fidc_valid_flst_refs_arr,
            source.fidc_persistent,
            source.fidc_no_reach,
        );
        NamedDefBase::after_clone(&*this, source);
        this
    }

    pub fn class(&self) -> FormIDClass {
        self.class
    }

    /// Upstream `IwbFormIDChecked.SignatureCount`: number of valid signatures.
    pub fn get_signature_count(&self) -> i32 {
        self.fidc_valid_refs.len() as i32
    }

    /// Upstream `IwbFormIDChecked.Signatures`, in sorted order.
    pub fn get_signature(&self, index: usize) -> Signature {
        self.fidc_valid_refs[index]
    }

    /// Upstream `IsValid`: whether a record with this signature may be referenced.
    pub fn is_valid(&self, signature: Signature) -> bool {
        if !self.class.is_checked() || disable_form_id_check() {
            return signature != ACVA;
        }
        self.fidc_valid_refs.binary_search(&signature).is_ok()
    }

    /// Upstream `IsValidFlst`: whether a form list with entries of this
    /// signature may be referenced.
    pub fn is_valid_flst(&self, signature: Signature) -> bool {
        !self.class.is_checked()
            || self.fidc_valid_flst_refs.is_empty()
            || self.fidc_valid_flst_refs.binary_search(&signature).is_ok()
            || disable_form_id_check()
    }

    /// Upstream `CheckFlst`: whether a referenced form list holds only valid entries.
    pub fn check_flst(&self, main_record: &MainRecordRef) -> bool {
        if !self.class.is_checked()
            || disable_form_id_check()
            || self.fidc_valid_flst_refs.is_empty()
            || main_record.get_signature() != FLST
        {
            return true;
        }
        let form_ids = main_record.get_element_by_name("FormIDs");
        let Some(container) = form_ids.as_ref().and_then(|element| element.as_container()) else {
            return true;
        };
        for index in 0..container.get_element_count() {
            let target = container.get_element(index).and_then(|entry| entry.get_links_to());
            if let Some(record) = target.as_ref().and_then(|target| target.as_main_record())
                && self
                    .fidc_valid_flst_refs
                    .binary_search(&record.get_signature())
                    .is_err()
            {
                return false;
            }
        }
        true
    }

    /// Upstream `IsValidMainRecord`.
    pub fn is_valid_main_record(&self, main_record: &MainRecordRef) -> bool {
        if !self.class.is_checked() || disable_form_id_check() {
            return true;
        }
        self.is_valid(main_record.get_signature())
            && self.check_flst(main_record)
            && (!self.fidc_persistent || main_record.get_is_persistent())
    }

    /// Upstream `GetExactIdentString`: equal for formaters that accept the same records.
    pub fn get_exact_ident_string(&self) -> String {
        if !self.class.is_checked() {
            return self.class.class_name().to_owned();
        }
        format!(
            "{}|{}|{}",
            self.class.class_name(),
            signatures_comma_text(&self.fidc_valid_refs),
            signatures_comma_text(&self.fidc_valid_flst_refs)
        )
    }

    fn use_load_order(&self) -> bool {
        self.def.def_flags.contains(DefFlag::dfUseLoadOrder)
    }

    /// Looks up the record that a FormID of `file` refers to. FormIDs of the
    /// hardcoded range belong to the game master unless the file may use that
    /// range itself. Changes `form_id` accordingly.
    fn record_for(
        form_id: &mut FormID,
        file: &FileRef,
        masters_updated: bool,
    ) -> Result<Option<MainRecordRef>, String> {
        if form_id.object_id() < 0x800 && !file.get_allow_hardcoded_range_use() {
            form_id.set_file_id(FileID::null());
        }
        if form_id.is_hardcoded() {
            match get_game_master_file() {
                Some(master) => master.get_record_by_form_id(*form_id, true, false),
                // Upstream fails without a game master file.
                None => Ok(None),
            }
        } else {
            file.get_record_by_form_id(*form_id, true, masters_updated)
        }
    }

    /// Upstream `GetMainRecord`: the record that `int` refers to from `element`.
    pub fn get_main_record(&self, int: i64, element: ElementArg) -> Option<MainRecordRef> {
        let form_id = FormID::from_cardinal(int as u32);
        let file = element.and_then(|element| element.get_file());
        if self.use_load_order() {
            return record_by_load_order_form_id(form_id, file.as_ref());
        }
        let element = element?;
        let mut form_id = form_id;
        Self::record_for(&mut form_id, &file?, element.get_masters_updated())
            .ok()
            .flatten()
    }

    /// The error for a FormID that is marked as unmapped but is not usable as one.
    fn unmapped_file_error(form_id: FormID, file: &FileRef) -> Option<String> {
        let special = EnumSet::of(&[FileState::fsIsGameMaster, FileState::fsIsHardcoded]);
        if !(file.get_file_states() & special).is_empty() {
            return None;
        }
        let first_master_is_game_master = file.get_master_count(true) >= 1
            && file
                .get_master(0, true)
                .is_some_and(|master| master.get_file_states().contains(FileState::fsIsGameMaster));
        if first_master_is_game_master {
            None
        } else {
            Some(format!(
                "[{}] <Error: Unmapped FormIDs can only be different from 00000000 in modules which have the game \
                 master as their first master>",
                form_id.to_string(false)
            ))
        }
    }

    /// Port of `TwbFormIDDefFormater.Check`.
    fn base_check(&self, int: i64, element: ElementArg) -> String {
        if self.use_load_order() {
            return String::new();
        }
        let unmapped = self.def.def_flags.contains(DefFlag::dfUnmappedFormID);
        let mut form_id = FormID::from_cardinal(int as u32);
        if let Some(element) = element
            && let Some(file) = element.get_file()
        {
            if unmapped
                && !form_id.is_null()
                && let Some(error) = Self::unmapped_file_error(form_id, &file)
            {
                return error;
            }
            match Self::record_for(&mut form_id, &file, element.get_masters_updated()) {
                Ok(Some(_)) => return String::new(),
                Ok(None) => {}
                Err(message) => return message,
            }
            if display_load_order_form_id() {
                match file.file_form_id_to_load_order_form_id(form_id, element.get_masters_updated()) {
                    Ok(load_order_form_id) => form_id = load_order_form_id,
                    Err(message) => return message,
                }
            }
        }
        if unmapped && form_id.file_id().full_slot() != 0 {
            return format!(
                "[{}] <Error: Unmapped FormIDs must belong to File ID [00]>",
                form_id.to_string(false)
            );
        }
        if form_id.is_hardcoded() {
            String::new()
        } else {
            format!("[{}] <Error: Could not be resolved>", form_id.to_string(false))
        }
    }

    /// Port of `TwbFormIDChecked.Check`.
    fn checked_check(&self, int: i64, element: ElementArg) -> String {
        if disable_form_id_check() || self.is_valid(ACVA) || self.use_load_order() {
            return String::new();
        }
        let expected = signatures_comma_text(&self.fidc_valid_refs);
        let found = |signature: Signature, shown: &str| -> String {
            if self.fidc_valid_refs.binary_search(&signature).is_ok() {
                String::new()
            } else {
                format!("Found a {shown} reference, expected: {expected}")
            }
        };
        let mut form_id = FormID::from_cardinal(int as u32);
        if form_id.is_null() {
            return if self.is_valid(TRGT) && !self.is_valid(NULL) {
                found(TRGT, "TRGT")
            } else {
                found(NULL, "NULL")
            };
        } else if form_id.is_none() {
            return found(FFFF, "None (FFFFFFFF)");
        } else if form_id.is_player() {
            return found(PLYR, "PLYR");
        }
        if let Some(element) = element
            && let Some(file) = element.get_file()
        {
            let masters_updated = element.get_masters_updated();
            let main_record = match Self::record_for(&mut form_id, &file, masters_updated) {
                Ok(main_record) => main_record,
                Err(message) => return message,
            };
            if !form_id.is_hardcoded() && display_load_order_form_id() {
                form_id = match &main_record {
                    Some(main_record) => main_record.get_load_order_form_id(),
                    None => match file.file_form_id_to_load_order_form_id(form_id, masters_updated) {
                        Ok(load_order_form_id) => load_order_form_id,
                        Err(message) => return message,
                    },
                };
            }
            if let Some(main_record) = main_record {
                let signature = main_record.get_signature();
                let result = found(signature, &signature.to_string());
                if !result.is_empty() {
                    return result;
                }
                if self.fidc_persistent && !main_record.get_winning_override().get_is_persistent() {
                    return "Target is not persistent".to_owned();
                }
                if !self.check_flst(&main_record) {
                    return "Referenced FLST contains invalid entry".to_owned();
                }
                return String::new();
            }
        }
        if int >= 0x800 {
            format!("[{}] <Error: Could not be resolved>", form_id.to_string(false))
        } else {
            String::new()
        }
    }
}

/// Port of `TStrings.CommaText` of a list of signatures.
fn signatures_comma_text(signatures: &[Signature]) -> String {
    let items: Vec<String> = signatures.iter().map(Signature::to_string).collect();
    to_comma_text(&items)
}

impl Def for FormIDDefFormater {
    formater_plumbing!();

    fn get_def_type(&self) -> DefType {
        DefType::dtIntegerFormater
    }

    fn get_def_type_name(&self) -> String {
        self.class.class_name().to_owned()
    }

    fn as_form_id_def_formater(&self) -> Option<&FormIDDefFormater> {
        Some(self)
    }

    fn get_no_reach(&self) -> bool {
        self.class.is_checked() && self.fidc_no_reach
    }
}

formater_impls!(FormIDDefFormater);

impl IntegerDefFormater for FormIDDefFormater {
    fn to_string(&self, int: i64, element: ElementArg, for_summary: bool) -> String {
        let mut form_id = FormID::from_cardinal(int as u32);
        if (form_id.is_hardcoded() || form_id.is_none()) && self.is_valid(ACVA) {
            return if int == -1 || int == 0xFF || int == 0xFFFF_FFFF {
                " None [ACVA:000000FF]".to_owned()
            } else if int == 0x48 {
                " Invalid [ACVA:00000048]".to_owned()
            } else {
                // The lookup of an actor value record is not ported yet.
                let name = match actor_value_enum() {
                    Some(enum_def) => IntegerDefFormater::to_string(&*enum_def, int, element, for_summary),
                    None => String::new(),
                };
                format!("{name} [ACVA:{}]", form_id.to_string(false))
            };
        }
        let result = if int == 0 {
            let id = form_id.to_string(false);
            if self.is_valid(TRGT) && !self.is_valid(NULL) {
                if for_summary {
                    "TARGET".to_owned()
                } else {
                    format!("TARGET - Target Reference [{id}]")
                }
            } else if !for_summary {
                format!("NULL - Null Reference [{id}]")
            } else if self.def.def_flags.contains(DefFlag::dfSummaryExcludeNULL) {
                String::new()
            } else {
                "NULL".to_owned()
            }
        } else if form_id.is_none() {
            if for_summary {
                "FFFF".to_owned()
            } else {
                format!("FFFF - None Reference [{}]", form_id.to_string(false))
            }
        } else {
            let resolved = match element {
                Some(element) => match element.get_file() {
                    Some(file) => self.resolved_to_string(&mut form_id, element, &file, for_summary),
                    None => None,
                },
                None => None,
            };
            match resolved {
                Some(text) => text,
                None => {
                    let id = form_id.to_string(false);
                    if form_id.is_hardcoded() {
                        format!("[{id}] <Warning: Could not be resolved, but is possibly hardcoded in the engine>")
                    } else {
                        format!("[{id}] <Error: Could not be resolved>")
                    }
                }
            }
        };
        self.used(element, &result);
        result
    }

    fn to_sort_key(&self, int: i64, element: ElementArg) -> String {
        let mut form_id = FormID::from_cardinal(int as u32);
        let mut main_record = None;
        if !self.use_load_order() && !(form_id.is_hardcoded() || form_id.is_none()) {
            main_record = self.get_main_record(int, element);
            match &main_record {
                Some(main_record) => form_id = main_record.get_load_order_form_id(),
                None => {
                    if display_load_order_form_id()
                        && let Some(element) = element
                        && let Some(file) = element.get_file()
                        && let Ok(load_order_form_id) =
                            file.file_form_id_to_load_order_form_id(form_id, element.get_masters_updated())
                    {
                        form_id = load_order_form_id;
                    }
                }
            }
        }
        let result = form_id.to_string(false);
        if self.class != FormIDClass::CheckedST {
            return result;
        }
        let position = main_record.and_then(|main_record| {
            let signature = main_record.get_signature();
            self.fidc_valid_refs_arr.iter().position(|valid| *valid == signature)
        });
        match position {
            Some(position) => format!("{position:02X}:{result}"),
            None => format!("XX:{result}"),
        }
    }

    fn check(&self, int: i64, element: ElementArg) -> String {
        if self.class.is_checked() {
            self.checked_check(int, element)
        } else {
            self.base_check(int, element)
        }
    }

    fn build_ref(&self, int: i64, element: ElementArg) {
        if self.def.def_flags.contains(DefFlag::dfExcludeFromBuildRef) {
            return;
        }
        if (int < 0x800 || int == 0xFFFF_FFFF) && self.is_valid(ACVA) {
            return;
        }
        if self.use_load_order() {
            return;
        }
        if int != 0
            && let Some(element) = element
        {
            element.add_referenced_from_id(FormID::from_cardinal(int as u32));
        }
    }

    fn get_edit_type(&self, _element: ElementArg) -> EditType {
        EditType::etComboBox
    }

    fn to_edit_value(&self, int: i64, element: ElementArg) -> String {
        if display_load_order_form_id() {
            let result = IntegerDefFormater::to_string(self, int, element, false);
            match result.strip_prefix('<') {
                Some(rest) => rest.to_owned(),
                None => result,
            }
        } else {
            int_to_hex64(int, 8)
        }
    }

    fn get_is_editable(&self, _int: i64, _element: ElementArg) -> bool {
        editable_unless_internal_only(&self.def)
    }

    fn get_links_to(&self, int: i64, element: ElementArg) -> Option<ElementRef> {
        if int == 0 || (int == 0xFFFF_FFFF && self.is_valid(FFFF)) || (int < 0x800 && self.is_valid(ACVA)) {
            return None;
        }
        let form_id = FormID::from_cardinal(int as u32);
        let file = element.and_then(|element| element.get_file());
        if self.use_load_order() {
            let record: ElementRef = record_by_load_order_form_id(form_id, file.as_ref())?;
            return Some(record);
        }
        let element = element?;
        let file = file?;
        let mut form_id = form_id;
        let mut record = Self::record_for(&mut form_id, &file, element.get_masters_updated())
            .ok()
            .flatten()?;
        if record.get_element_type() == ElementType::etMainRecord && record.get_is_partial_form() {
            record = record.get_highest_override_visible_for_file(&file)?;
        }
        let record: ElementRef = record;
        Some(record)
    }
}

impl FormIDDefFormater {
    /// The part of `ToString` that upstream runs inside `try`: the text for a
    /// FormID that resolves to a record, or an error text. `None` when nothing
    /// was found, with `form_id` changed to the one to show.
    fn resolved_to_string(
        &self,
        form_id: &mut FormID,
        element: &ElementRef,
        file: &FileRef,
        for_summary: bool,
    ) -> Option<String> {
        let masters_updated = element.get_masters_updated();
        let error = |form_id: FormID, message: String| format!("[{}] <Error: {message}>", form_id.to_string(false));
        let main_record = if self.use_load_order() {
            // The stored FormID is already a load order FormID.
            record_by_load_order_form_id(*form_id, Some(file))
        } else {
            let main_record = match Self::record_for(form_id, file, masters_updated) {
                Ok(main_record) => main_record,
                Err(message) => return Some(error(*form_id, message)),
            };
            if !form_id.is_hardcoded() && display_load_order_form_id() {
                *form_id = match &main_record {
                    Some(main_record) => main_record.get_load_order_form_id(),
                    None => match file.file_form_id_to_load_order_form_id(*form_id, masters_updated) {
                        Ok(load_order_form_id) => load_order_form_id,
                        Err(message) => return Some(error(*form_id, message)),
                    },
                };
            }
            main_record
        };
        if self.def.def_flags.contains(DefFlag::dfUnmappedFormID) {
            if form_id.file_id().full_slot() != 0 {
                return Some(format!(
                    "[{}] <Error: Unmapped FormIDs must belong to File ID [00]>",
                    form_id.to_string(false)
                ));
            }
            if let Some(error) = Self::unmapped_file_error(*form_id, file) {
                return Some(error);
            }
        }
        let main_record = main_record?;
        if !for_summary {
            return Some(main_record.get_name());
        }
        let is_self = element
            .get_containing_main_record()
            .is_some_and(|containing| containing.get_element_id() == main_record.get_element_id());
        let self_as_short_name = |flags: Option<super::types::DefFlags>| {
            flags.is_some_and(|flags| flags.contains(DefFlag::dfSummarySelfAsShortName))
        };
        if is_self
            && !self_as_short_name(element.get_value_def().map(|def| def.get_def_flags()))
            && !self_as_short_name(main_record.get_def().map(|def| def.get_def_flags()))
        {
            Some("Self".to_owned())
        } else {
            Some(main_record.get_short_name())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::super::globals::{set_disable_form_id_check, test_lock};
    use super::*;

    const WEAP: Signature = Signature::new(b"WEAP");
    const ARMO: Signature = Signature::new(b"ARMO");

    #[test]
    fn special_values() {
        let _guard = test_lock();
        let def = FormIDDefFormater::create();
        assert!(def.get_def_flags().contains(DefFlag::dfCanContainFormID));
        assert_eq!(
            IntegerDefFormater::to_string(&*def, 0, None, false),
            "NULL - Null Reference [00000000]"
        );
        assert_eq!(IntegerDefFormater::to_string(&*def, 0, None, true), "NULL");
        assert_eq!(
            IntegerDefFormater::to_string(&*def, 0xFFFF_FFFF, None, false),
            "FFFF - None Reference [FFFFFFFF]"
        );
        assert_eq!(IntegerDefFormater::to_string(&*def, 0xFFFF_FFFF, None, true), "FFFF");
        assert_eq!(
            IntegerDefFormater::to_string(&*def, 0x0001_2345, None, false),
            "[00012345] <Error: Could not be resolved>"
        );
        assert_eq!(
            IntegerDefFormater::to_string(&*def, 0x14, None, false),
            "[00000014] <Warning: Could not be resolved, but is possibly hardcoded in the engine>"
        );
        assert_eq!(def.to_sort_key(0x0001_2345, None), "00012345");
        assert_eq!(def.to_edit_value(0x0001_2345, None), "00012345");
        assert_eq!(
            IntegerDefFormater::check(&*def, 0x0001_2345, None),
            "[00012345] <Error: Could not be resolved>"
        );
        assert_eq!(IntegerDefFormater::check(&*def, 0x14, None), "");
        assert!(def.get_links_to(0x0001_2345, None).is_none());
        assert_eq!(def.get_name(), "TwbFormIDDefFormater");
        assert_eq!(def.get_edit_type(None), EditType::etComboBox);
    }

    #[test]
    fn summary_can_exclude_null() {
        let _guard = test_lock();
        let def = FormIDDefFormater::create();
        def.include_flag_no_clone(DefFlag::dfSummaryExcludeNULL, true);
        assert_eq!(IntegerDefFormater::to_string(&*def, 0, None, true), "");
    }

    #[test]
    fn checked_signatures() {
        let _guard = test_lock();
        let def = FormIDDefFormater::create_checked(&[WEAP, ARMO, WEAP], &[], false, true);
        assert_eq!(def.get_signature_count(), 2);
        assert_eq!(def.get_signature(0), ARMO);
        assert!(def.is_valid(WEAP) && !def.is_valid(NULL) && !def.is_valid(ACVA));
        assert!(def.is_valid_flst(ARMO) && def.get_no_reach());
        assert_eq!(def.get_exact_ident_string(), "TwbFormIDChecked|ARMO,WEAP|");
        assert_eq!(
            IntegerDefFormater::check(&*def, 0, None),
            "Found a NULL reference, expected: ARMO,WEAP"
        );
        assert_eq!(
            IntegerDefFormater::check(&*def, 0xFFFF_FFFF, None),
            "Found a None (FFFFFFFF) reference, expected: ARMO,WEAP"
        );
        assert_eq!(
            IntegerDefFormater::check(&*def, 0x14, None),
            "Found a PLYR reference, expected: ARMO,WEAP"
        );
        assert_eq!(
            IntegerDefFormater::check(&*def, 0x0001_2345, None),
            "[00012345] <Error: Could not be resolved>"
        );
        let with_null = FormIDDefFormater::create_checked(&[WEAP, NULL], &[], false, false);
        assert_eq!(IntegerDefFormater::check(&*with_null, 0, None), "");
        let target = FormIDDefFormater::create_checked(&[TRGT], &[], false, false);
        assert_eq!(
            IntegerDefFormater::to_string(&*target, 0, None, false),
            "TARGET - Target Reference [00000000]"
        );
        assert_eq!(IntegerDefFormater::to_string(&*target, 0, None, true), "TARGET");

        set_disable_form_id_check(true);
        assert!(def.is_valid(NULL));
        assert_eq!(IntegerDefFormater::check(&*def, 0, None), "");
    }

    #[test]
    fn checked_st_sort_key_and_clone() {
        let _guard = test_lock();
        let def = FormIDDefFormater::create_checked_st(&[WEAP, ARMO], false);
        assert_eq!(def.to_sort_key(0x0001_2345, None), "XX:00012345");
        assert_eq!(def.get_def_type_name(), "TwbFormIDCheckedST");
        let copy = FormIDDefFormater::clone_from(&def);
        assert_eq!(copy.class(), FormIDClass::CheckedST);
        assert_eq!(copy.get_signature_count(), 2);
    }
}
