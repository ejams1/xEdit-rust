// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

// Ported from xEdit: Core/wbInterface.pas

//! `TwbMainRecordDef`.
//!
//! Not ported yet: the index keys (`SetBuildIndexKeys`), `SetEditorID` and `Report`.

use std::collections::BTreeMap;
use std::sync::atomic::{AtomicU8, Ordering};
use std::sync::{Arc, OnceLock, RwLock, Weak};

use super::def::{
    AfterLoadCallback, AfterSetCallback, Def, DefBase, DefCell, DefKind, DefRef, NamedDef, NamedDefArgs, NamedDefBase,
    ValueDef, set_parent,
};
use super::element::{ElementArg, ElementRef, MainRecordRef};
use super::form_id::FormID;
use super::form_id_formater::FormIDDefFormater;
use super::globals::{copy_is_running, report_mode, report_required};
use super::integer::{IntegerDef, IntegerDefFormater};
use super::struct_def::{StructDef, set_array_entry};
use super::sub_record::{RecordMemberDef, SignatureDef, signature_def_get_full_name};
use super::sub_record_group::{RecordDef, struct_keys_to_summary};
use super::types::{
    CallbackType, ConflictPriority, DefType, KNOWN_SUB_RECORD_SIGNATURES, KnownSubRecord, KnownSubRecordSignatures,
    PascalEnum, Signature,
};

pub type AddInfoCallback = Arc<dyn Fn(&MainRecordRef) -> String + Send + Sync>;
pub type MainRecordGetFormIDCallback = Arc<dyn Fn(&MainRecordRef) -> Option<FormID> + Send + Sync>;
pub type MainRecordIdentityCallback = Arc<dyn Fn(&MainRecordRef) -> String + Send + Sync>;
/// Gives the editor ID from the subrecord that holds it.
pub type MainRecordGetEditorIDCallback = Arc<dyn Fn(&ElementRef) -> String + Send + Sync>;
/// Gives the grid cell from the subrecord that holds it.
pub type MainRecordGetGridCellCallback = Arc<dyn Fn(&ElementRef) -> Option<GridCell> + Send + Sync>;

/// Upstream `TwbGridCell`: the position of a cell in the grid of a worldspace.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct GridCell {
    pub x: i32,
    pub y: i32,
}

static MAIN_RECORD_HEADER: RwLock<Option<Arc<dyn ValueDef>>> = RwLock::new(None);
static RECORD_FLAGS: RwLock<Option<Arc<IntegerDef>>> = RwLock::new(None);
static REF_RECORD_DEFS: RwLock<Vec<Arc<MainRecordDef>>> = RwLock::new(Vec::new());

/// Sets upstream `wbMainRecordHeader`: the structure of the header of every main record.
pub fn set_main_record_header(header: Option<Arc<dyn ValueDef>>) {
    *MAIN_RECORD_HEADER.write().unwrap() = header;
}

/// Upstream `wbMainRecordHeader`.
pub fn main_record_header() -> Option<Arc<dyn ValueDef>> {
    MAIN_RECORD_HEADER.read().unwrap().clone()
}

/// Sets upstream `wbRecordFlags`: the record flags member of the header.
pub fn set_record_flags(flags: Option<Arc<IntegerDef>>) {
    *RECORD_FLAGS.write().unwrap() = flags;
}

/// Upstream `wbRecordFlags`.
pub fn record_flags() -> Option<Arc<IntegerDef>> {
    RECORD_FLAGS.read().unwrap().clone()
}

/// Upstream `wbRefRecordDefs`: the definitions of the reference records.
pub fn ref_record_defs() -> Vec<Arc<MainRecordDef>> {
    REF_RECORD_DEFS.read().unwrap().clone()
}

/// Adds a definition to upstream `wbRefRecordDefs`.
pub fn add_ref_record_def(def: Arc<MainRecordDef>) {
    REF_RECORD_DEFS.write().unwrap().push(def);
}

/// Empties upstream `wbRefRecordDefs`.
pub fn clear_ref_record_defs() {
    REF_RECORD_DEFS.write().unwrap().clear();
}

/// The constructor arguments of `TwbMainRecordDef`.
pub struct MainRecordDefArgs {
    pub priority: ConflictPriority,
    pub required: bool,
    pub signature: Signature,
    pub name: String,
    /// The signatures of the subrecords with a known role. `None` selects
    /// upstream `wbKnownSubRecordSignatures`.
    pub known_srs: Option<KnownSubRecordSignatures>,
    pub record_flags: Option<Arc<dyn IntegerDefFormater>>,
    pub members: Vec<Arc<dyn RecordMemberDef>>,
    pub allow_unordered: bool,
    pub add_info_callback: Option<AddInfoCallback>,
    pub after_load: Option<AfterLoadCallback>,
    pub after_set: Option<AfterSetCallback>,
    pub is_reference: bool,
}

/// A failure of `TwbMainRecordDef.Create`. The message is the upstream exception message.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("{0}")]
pub struct MainRecordDefError(pub String);

/// Upstream `TwbMainRecordDef`: the definition of a record type.
pub struct MainRecordDef {
    self_ref: Weak<MainRecordDef>,
    def: DefBase,
    nd: NamedDefBase,
    so_signature: Signature,
    rec_record_flags: Option<Arc<dyn IntegerDefFormater>>,
    rec_record_header_struct: Option<Arc<StructDef>>,
    rec_members: Vec<Arc<dyn RecordMemberDef>>,
    /// Signature to the index of its member. For a signature that several
    /// members have, the last one, as the sorted list upstream finds it.
    rec_signatures: BTreeMap<Signature, usize>,
    rec_add_info_callback: DefCell<AddInfoCallback>,
    rec_quick_init_limit: i32,
    rec_allow_unordered: bool,
    rec_is_reference: bool,
    rec_can_be_partial: bool,
    rec_base_record_form_id: Option<Arc<FormIDDefFormater>>,
    rec_references: OnceLock<Vec<Signature>>,
    rec_known_srs: KnownSubRecordSignatures,
    rec_known_sr_members: [i32; 5],
    rec_get_form_id_callback: DefCell<MainRecordGetFormIDCallback>,
    rec_identity_callback: DefCell<MainRecordIdentityCallback>,
    rec_get_editor_id_callback: DefCell<MainRecordGetEditorIDCallback>,
    rec_get_grid_cell_callback: DefCell<MainRecordGetGridCellCallback>,
    rec_form_id_base: AtomicU8,
    rec_form_id_name_base: AtomicU8,
    rec_summary_key: DefCell<Vec<i32>>,
    rec_summary_prefix: DefCell<Vec<String>>,
    rec_summary_suffix: DefCell<Vec<String>>,
    rec_summary_max_depth: DefCell<Vec<i32>>,
    rec_summary_delimiter: DefCell<String>,
    rec_ignore_list: DefCell<Vec<Signature>>,
}

impl MainRecordDef {
    /// Port of `TwbMainRecordDef.Create`.
    pub fn create(args: MainRecordDefArgs) -> Result<Arc<Self>, MainRecordDefError> {
        let known_srs = args.known_srs.unwrap_or(KNOWN_SUB_RECORD_SIGNATURES);
        let known = |role: KnownSubRecord| known_srs[role.ord()];
        let name = if args.name.is_empty() {
            args.signature.to_string()
        } else {
            args.name
        };
        let (def, nd) = NamedDefBase::create(NamedDefArgs {
            priority: args.priority,
            required: args.required,
            name,
            after_load: args.after_load,
            after_set: args.after_set,
            dont_show: None,
            get_cp: None,
            terminator: false,
        });
        let rec_can_be_partial = args
            .record_flags
            .as_ref()
            .and_then(|flags| flags.as_flags_def())
            .is_some_and(|flags| flags.find_flag("Partial Form").is_some());
        let mut error = None;
        let this = Arc::new_cyclic(|self_ref: &Weak<Self>| {
            let parent: Weak<dyn Def> = self_ref.clone();
            // Each record definition has its own copy of the header, with its
            // own record flags.
            let mut rec_record_header_struct = None;
            if let Some(flags) = &args.record_flags
                && let Some(global_flags) = record_flags()
                && let Some(header) = main_record_header()
            {
                let header = set_parent(header, &parent, true)
                    .into_struct_def()
                    .expect("the main record header is a structure");
                header
                    .get_member_by_name(global_flags.get_name())
                    .and_then(|member| member.as_integer_def())
                    .expect("the main record header has the record flags")
                    .replace_formater(Some(flags.clone()));
                rec_record_header_struct = Some(header);
            }

            let mut rec_members = Vec::with_capacity(args.members.len());
            let mut rec_signatures = BTreeMap::new();
            let mut rec_quick_init_limit = -1;
            let mut rec_known_sr_members = [-1; 5];
            let mut rec_base_record_form_id = None;
            for (index, member) in args.members.into_iter().enumerate() {
                let member = set_parent(member, &parent, false);
                for sig_index in 0..member.get_signature_count() {
                    let sig = member.get_signature(sig_index);
                    if sig == known(KnownSubRecord::ksrEditorID)
                        || sig == known(KnownSubRecord::ksrFullName)
                        || sig == known(KnownSubRecord::ksrGridCell)
                        || (sig == known(KnownSubRecord::ksrBaseRecord) && args.is_reference)
                    {
                        rec_quick_init_limit = index as i32;
                        for role in KnownSubRecord::ALL {
                            if sig == known(*role) {
                                rec_known_sr_members[role.ord()] = index as i32;
                            }
                        }
                        if sig == known(KnownSubRecord::ksrBaseRecord) {
                            // The base record is a subrecord with a checked FormID.
                            let formater = member
                                .as_sub_record_def()
                                .and_then(|sub_record| sub_record.get_value())
                                .and_then(|value| {
                                    let integer = value.as_integer_def()?;
                                    assert!(!integer.get_formater_can_change());
                                    integer.get_formater(None)
                                })
                                .and_then(|formater| formater.into_form_id_def_formater())
                                .filter(|formater| formater.class() != super::form_id_formater::FormIDClass::FormID);
                            assert!(formater.is_some(), "the base record of a reference is a checked FormID");
                            rec_base_record_form_id = formater;
                        }
                    }
                    if args.allow_unordered && rec_signatures.contains_key(&sig) {
                        error = Some(MainRecordDefError(format!(
                            "Duplicate definition {sig} in allow unordered record {}",
                            args.signature
                        )));
                    }
                    rec_signatures.insert(sig, index);
                }
                rec_members.push(member);
            }
            if args.is_reference && rec_base_record_form_id.is_none() {
                error = Some(MainRecordDefError(
                    "Reference MainRecord must have BaseRecordFormID".to_owned(),
                ));
            }
            Self {
                self_ref: self_ref.clone(),
                def,
                nd,
                so_signature: args.signature,
                rec_record_flags: args.record_flags,
                rec_record_header_struct,
                rec_members,
                rec_signatures,
                rec_add_info_callback: DefCell::new(args.add_info_callback),
                rec_quick_init_limit,
                rec_allow_unordered: args.allow_unordered,
                rec_is_reference: args.is_reference,
                rec_can_be_partial,
                rec_base_record_form_id,
                rec_references: OnceLock::new(),
                rec_known_srs: known_srs,
                rec_known_sr_members,
                rec_get_form_id_callback: DefCell::default(),
                rec_identity_callback: DefCell::default(),
                rec_get_editor_id_callback: DefCell::default(),
                rec_get_grid_cell_callback: DefCell::default(),
                rec_form_id_base: AtomicU8::new(0),
                rec_form_id_name_base: AtomicU8::new(0),
                rec_summary_key: DefCell::default(),
                rec_summary_prefix: DefCell::default(),
                rec_summary_suffix: DefCell::default(),
                rec_summary_max_depth: DefCell::default(),
                rec_summary_delimiter: DefCell::new(Some(" ".to_owned())),
                rec_ignore_list: DefCell::default(),
            }
        });
        if let Some(error) = error {
            return Err(error);
        }
        DefBase::after_construction(&*this);
        Ok(this)
    }

    /// Port of `TwbMainRecordDef.Clone` and `AfterClone`.
    // UPSTREAM-QUIRK: AfterClone does not copy the summary depths and the
    // ignore list.
    pub fn clone_from(source: &Self) -> Arc<Self> {
        let named = NamedDefBase::clone_args(source);
        let this = Self::create(MainRecordDefArgs {
            priority: named.priority,
            required: named.required,
            signature: source.so_signature,
            name: named.name,
            known_srs: Some(source.rec_known_srs),
            record_flags: source.rec_record_flags.clone(),
            members: source.rec_members.clone(),
            allow_unordered: source.rec_allow_unordered,
            add_info_callback: source.rec_add_info_callback.load().as_deref().cloned(),
            after_load: named.after_load,
            after_set: named.after_set,
            is_reference: source.rec_is_reference,
        })
        .expect("a definition that was created once can be created again");
        NamedDefBase::after_clone(&*this, source);
        this.rec_get_form_id_callback.assign(&source.rec_get_form_id_callback);
        this.rec_identity_callback.assign(&source.rec_identity_callback);
        this.rec_get_editor_id_callback
            .assign(&source.rec_get_editor_id_callback);
        this.rec_get_grid_cell_callback
            .assign(&source.rec_get_grid_cell_callback);
        this.rec_form_id_base
            .store(source.rec_form_id_base.load(Ordering::Relaxed), Ordering::Relaxed);
        this.rec_form_id_name_base
            .store(source.rec_form_id_name_base.load(Ordering::Relaxed), Ordering::Relaxed);
        this.rec_summary_key.assign(&source.rec_summary_key);
        this.rec_summary_prefix.assign(&source.rec_summary_prefix);
        this.rec_summary_suffix.assign(&source.rec_summary_suffix);
        this.rec_summary_delimiter.assign(&source.rec_summary_delimiter);
        this
    }

    pub fn get_is_reference(&self) -> bool {
        self.rec_is_reference
    }

    /// The index of the last member that the quick initialisation of a record needs.
    pub fn get_quick_init_limit(&self) -> i32 {
        self.rec_quick_init_limit
    }

    pub fn get_contains_known_sub_record(&self, known: KnownSubRecord) -> bool {
        self.rec_known_sr_members[known.ord()] >= 0
    }

    pub fn get_known_sub_record_member_index(&self, known: KnownSubRecord) -> i32 {
        self.rec_known_sr_members[known.ord()]
    }

    pub fn get_known_sub_record_member(&self, known: KnownSubRecord) -> Option<Arc<dyn RecordMemberDef>> {
        usize::try_from(self.rec_known_sr_members[known.ord()])
            .ok()
            .map(|index| self.rec_members[index].clone())
    }

    pub fn known_sub_record_signatures(&self) -> &KnownSubRecordSignatures {
        &self.rec_known_srs
    }

    pub fn get_can_be_partial(&self) -> bool {
        self.rec_can_be_partial
    }

    /// The structure of the header of records with this definition.
    pub fn get_record_header_struct(&self) -> Option<Arc<StructDef>> {
        match &self.rec_record_header_struct {
            Some(header) => Some(header.clone()),
            None => main_record_header().and_then(|header| header.into_struct_def()),
        }
    }

    pub fn get_base_signature_count(&self) -> i32 {
        self.rec_base_record_form_id
            .as_ref()
            .map_or(0, |formater| formater.get_signature_count())
    }

    /// Upstream `GetBaseSignature`. Fails for a definition that is not a reference.
    pub fn get_base_signature(&self, index: usize) -> Signature {
        self.rec_base_record_form_id
            .as_ref()
            .expect("Invalid index")
            .get_signature(index)
    }

    /// Whether a record with `signature` can be the base of this reference.
    pub fn is_valid_base_signature(&self, signature: Signature) -> bool {
        self.rec_base_record_form_id
            .as_ref()
            .is_some_and(|formater| formater.is_valid(signature))
    }

    /// Port of `recBuildReferences`: the signatures of the reference records
    /// that can have this record as their base, sorted.
    fn rec_references(&self) -> &Vec<Signature> {
        self.rec_references.get_or_init(|| {
            let mut references: Vec<Signature> = ref_record_defs()
                .iter()
                .filter(|def| def.is_valid_base_signature(self.so_signature))
                .map(|def| def.get_default_signature())
                .collect();
            references.sort();
            references.dedup();
            references
        })
    }

    pub fn get_reference_signature_count(&self) -> i32 {
        self.rec_references().len() as i32
    }

    pub fn get_reference_signature(&self, index: usize) -> Signature {
        self.rec_references()[index]
    }

    pub fn is_valid_reference_signature(&self, signature: Signature) -> bool {
        self.rec_references().binary_search(&signature).is_ok()
    }

    /// Upstream `GetFormIDBase`. Fails for a definition without a FormID base.
    pub fn get_form_id_base(&self) -> u8 {
        let result = self.rec_form_id_base.load(Ordering::Relaxed);
        assert!(result >= 1, "{} has no FormID Base", self.get_name());
        result
    }

    pub fn get_form_id_name_base(&self) -> u8 {
        match self.rec_form_id_name_base.load(Ordering::Relaxed) {
            0 => self.get_form_id_base(),
            base => base,
        }
    }

    /// Upstream `GetFormID`: the FormID from the callback, for records that do
    /// not store it in their header.
    pub fn get_form_id(&self, main_record: &MainRecordRef) -> Option<FormID> {
        self.rec_get_form_id_callback
            .load()
            .as_deref()
            .and_then(|callback| callback(main_record))
    }

    pub fn get_identity(&self, main_record: &MainRecordRef) -> String {
        match self.rec_identity_callback.load().as_deref() {
            Some(callback) => callback(main_record),
            None => main_record.get_editor_id(),
        }
    }

    /// Upstream `GetEditorID`: the editor ID from the subrecord that holds it.
    pub fn get_editor_id(&self, sub_record: &ElementRef) -> String {
        match self.rec_get_editor_id_callback.load().as_deref() {
            Some(callback) => callback(sub_record),
            None => sub_record.get_edit_value(),
        }
    }

    /// Upstream `GetGridCell`: the grid cell from the subrecord that holds it.
    pub fn get_grid_cell(&self, sub_record: &ElementRef) -> Option<GridCell> {
        if let Some(callback) = self.rec_get_grid_cell_callback.load().as_deref() {
            return callback(sub_record);
        }
        // Upstream fails for a subrecord without the values X and Y.
        let container = sub_record.as_container()?;
        let read = |path: &str| container.get_element_native_value(path).as_ordinal().unwrap_or(0) as i32;
        Some(GridCell {
            x: read("X"),
            y: read("Y"),
        })
    }

    /// Upstream `ShouldIgnore`: whether subrecords with this signature are not
    /// part of the record.
    pub fn should_ignore(&self, signature: Signature) -> bool {
        self.rec_ignore_list
            .load()
            .as_deref()
            .is_some_and(|list| list.contains(&signature))
    }

    /// Port of `ToSummary`: a short text for the record `main_record`.
    pub fn to_summary(&self, depth: i32, main_record: ElementArg, links_to: &mut Option<ElementRef>) -> String {
        let mut result = String::new();
        if let Some(to_str) = self.nd.nd_to_str.load().as_deref() {
            to_str(&mut result, None, main_record, CallbackType::ctToSummary);
        }
        if result.is_empty() {
            struct_keys_to_summary(
                depth,
                &mut result,
                main_record,
                &self.rec_members,
                &Self::loaded(&self.rec_summary_key),
                &Self::loaded(&self.rec_summary_prefix),
                &Self::loaded(&self.rec_summary_suffix),
                &Self::loaded(&self.rec_summary_max_depth),
                &Self::loaded(&self.rec_summary_delimiter),
                links_to,
            );
        }
        if links_to.is_none()
            && !result.is_empty()
            && let Some(main_record) = main_record
        {
            *links_to = main_record.get_links_to();
        }
        result
    }

    fn loaded<T: Clone + Default>(cell: &DefCell<T>) -> T {
        cell.load().as_deref().cloned().unwrap_or_default()
    }

    fn unlocked(self: Arc<Self>) -> Arc<Self> {
        if self.def.def_is_locked() {
            Self::clone_from(&self)
        } else {
            self
        }
    }

    pub fn set_add_info(self: Arc<Self>, add_info: Option<AddInfoCallback>) -> Arc<Self> {
        let this = self.unlocked();
        this.rec_add_info_callback.set(add_info);
        this
    }

    pub fn set_form_id_base(self: Arc<Self>, base: u8) -> Arc<Self> {
        let this = self.unlocked();
        this.rec_form_id_base.store(base, Ordering::Relaxed);
        this
    }

    pub fn set_form_id_name_base(self: Arc<Self>, base: u8) -> Arc<Self> {
        let this = self.unlocked();
        this.rec_form_id_name_base.store(base, Ordering::Relaxed);
        this
    }

    pub fn set_get_form_id_callback(self: Arc<Self>, callback: Option<MainRecordGetFormIDCallback>) -> Arc<Self> {
        let this = self.unlocked();
        this.rec_get_form_id_callback.set(callback);
        this
    }

    pub fn set_identity_callback(self: Arc<Self>, callback: Option<MainRecordIdentityCallback>) -> Arc<Self> {
        let this = self.unlocked();
        this.rec_identity_callback.set(callback);
        this
    }

    pub fn set_get_editor_id_callback(self: Arc<Self>, callback: Option<MainRecordGetEditorIDCallback>) -> Arc<Self> {
        let this = self.unlocked();
        this.rec_get_editor_id_callback.set(callback);
        this
    }

    pub fn set_get_grid_cell_callback(self: Arc<Self>, callback: Option<MainRecordGetGridCellCallback>) -> Arc<Self> {
        let this = self.unlocked();
        this.rec_get_grid_cell_callback.set(callback);
        this
    }

    pub fn set_summary_key(self: Arc<Self>, summary_key: &[i32]) -> Arc<Self> {
        let this = self.unlocked();
        this.rec_summary_key.set(Some(summary_key.to_vec()));
        this
    }

    pub fn set_summary_member_prefix_suffix(self: Arc<Self>, index: usize, prefix: &str, suffix: &str) -> Arc<Self> {
        let this = self.unlocked();
        assert!(
            index < this.rec_members.len(),
            "[TwbMainRecordDef.SetSummaryMemberPrefixSuffix] not InRange(aIndex, Low(recMembers), High(recMembers))"
        );
        let mut prefixes = Self::loaded(&this.rec_summary_prefix);
        set_array_entry(&mut prefixes, index, prefix.to_owned());
        this.rec_summary_prefix.set(Some(prefixes));
        let mut suffixes = Self::loaded(&this.rec_summary_suffix);
        set_array_entry(&mut suffixes, index, suffix.to_owned());
        this.rec_summary_suffix.set(Some(suffixes));
        this
    }

    pub fn set_summary_member_max_depth(self: Arc<Self>, index: usize, max_depth: i32) -> Arc<Self> {
        let this = self.unlocked();
        assert!(
            index < this.rec_members.len(),
            "[TwbMainRecordDef.SetSummaryMemberMaxDepth] not InRange(aIndex, Low(recMembers), High(recMembers))"
        );
        let mut max_depths = Self::loaded(&this.rec_summary_max_depth);
        set_array_entry(&mut max_depths, index, max_depth);
        this.rec_summary_max_depth.set(Some(max_depths));
        this
    }

    pub fn set_summary_delimiter(self: Arc<Self>, delimiter: &str) -> Arc<Self> {
        let this = self.unlocked();
        this.rec_summary_delimiter.set(Some(delimiter.to_owned()));
        this
    }

    pub fn set_ignore_list(self: Arc<Self>, signatures: &[Signature]) -> Arc<Self> {
        let this = self.unlocked();
        this.rec_ignore_list.set(Some(signatures.to_vec()));
        this
    }
}

impl Def for MainRecordDef {
    fn def_base(&self) -> &DefBase {
        &self.def
    }

    fn as_dyn_def(&self) -> &dyn Def {
        self
    }

    fn def_ref(&self) -> DefRef {
        self.self_ref.upgrade().expect("a definition is alive while it is used")
    }

    fn duplicate(&self) -> DefRef {
        Self::clone_from(self)
    }

    fn as_named_def(&self) -> Option<&dyn NamedDef> {
        Some(self)
    }

    fn as_signature_def(&self) -> Option<&dyn SignatureDef> {
        Some(self)
    }

    fn as_record_def(&self) -> Option<&dyn RecordDef> {
        Some(self)
    }

    fn as_main_record_def(&self) -> Option<&MainRecordDef> {
        Some(self)
    }

    fn get_def_type(&self) -> DefType {
        DefType::dtRecord
    }

    fn get_def_type_name(&self) -> String {
        "Record".to_owned()
    }

    fn get_child_pos(&self, child: &dyn Def) -> i32 {
        self.rec_members
            .iter()
            .position(|member| child.equals(Some(member.as_dyn_def())))
            .map_or(-1, |index| index as i32)
    }

    fn init_from_parent_do_children(&self) {
        for member in &self.rec_members {
            member.init_from_parent();
        }
        if let Some(header) = &self.rec_record_header_struct {
            header.init_from_parent();
        }
    }
}

impl NamedDef for MainRecordDef {
    fn named_def_base(&self) -> &NamedDefBase {
        &self.nd
    }

    fn get_full_name(&self) -> String {
        signature_def_get_full_name(self)
    }

    fn after_load(&self, element: &ElementRef) {
        self.used(None, "");
        if let Some(after_load) = self.nd.nd_after_load.load().as_deref() {
            after_load(element);
        }
        if report_mode()
            && report_required()
            && let Some(container) = element.as_container()
        {
            for member in &self.rec_members {
                if member.is_not_required() {
                    continue;
                }
                let found = (0..container.get_element_count()).any(|j| {
                    container.get_element(j).is_some_and(|child| {
                        let def = child.get_def();
                        let value_def = child.get_value_def();
                        member.equals(def.as_deref().map(|def| def.as_dyn_def()))
                            || member.equals(value_def.as_deref().map(|def| def.as_dyn_def()))
                    })
                });
                member.possibly_required();
                if !found {
                    member.not_required();
                }
            }
        }
    }
}

impl SignatureDef for MainRecordDef {
    fn get_default_signature(&self) -> Signature {
        self.so_signature
    }

    fn get_signature(&self, _index: i32) -> Signature {
        self.so_signature
    }

    fn get_signature_count(&self) -> i32 {
        1
    }
}

impl RecordDef for MainRecordDef {
    fn contains_member_for(&self, _container: ElementArg, signature: Signature, _data_container: ElementArg) -> bool {
        self.rec_signatures.contains_key(&signature)
    }

    fn get_member_for(
        &self,
        _container: ElementArg,
        signature: Signature,
        _data_container: ElementArg,
    ) -> Option<Arc<dyn RecordMemberDef>> {
        self.rec_signatures
            .get(&signature)
            .map(|&index| self.rec_members[index].clone())
    }

    fn get_member_index_for(&self, _container: ElementArg, signature: Signature, _data_container: ElementArg) -> i32 {
        self.rec_signatures.get(&signature).map_or(-1, |&index| index as i32)
    }

    fn allow_unordered(&self) -> bool {
        self.rec_allow_unordered
    }

    fn additional_info_for(&self, main_record: &MainRecordRef) -> String {
        match self.rec_add_info_callback.load().as_deref() {
            Some(callback) if copy_is_running() == 0 => callback(main_record),
            _ => String::new(),
        }
    }

    fn get_member(&self, index: usize) -> Arc<dyn RecordMemberDef> {
        self.rec_members[index].clone()
    }

    fn get_member_count(&self) -> i32 {
        self.rec_members.len() as i32
    }

    fn get_skip_signature(&self, _signature: Signature) -> bool {
        false
    }
}

impl DefKind for MainRecordDef {
    fn duplicate_same(&self) -> Arc<Self> {
        Self::clone_from(self)
    }
}

#[cfg(test)]
mod tests {
    use super::super::def::NamedDefSetters;
    use super::super::flags::FlagsDef;
    use super::super::globals::test_lock;
    use super::super::struct_def::StructDefArgs;
    use super::super::sub_record::SubRecordDef;
    use super::super::types::IntType;
    use super::*;

    const MISC: Signature = Signature::new(b"MISC");
    const REFR: Signature = Signature::new(b"REFR");
    const EDID: Signature = Signature::new(b"EDID");
    const FULL: Signature = Signature::new(b"FULL");
    const DATA: Signature = Signature::new(b"DATA");
    const NAME: Signature = Signature::new(b"NAME");

    fn named(name: &str) -> NamedDefArgs {
        NamedDefArgs {
            priority: ConflictPriority::cpNormal,
            required: false,
            name: name.to_owned(),
            after_load: None,
            after_set: None,
            dont_show: None,
            get_cp: None,
            terminator: false,
        }
    }

    fn sub(signature: Signature, name: &str) -> Arc<dyn RecordMemberDef> {
        SubRecordDef::create(named(name), &[signature], None, false)
    }

    fn record(signature: Signature, name: &str, members: Vec<Arc<dyn RecordMemberDef>>) -> MainRecordDefArgs {
        MainRecordDefArgs {
            priority: ConflictPriority::cpNormal,
            required: false,
            signature,
            name: name.to_owned(),
            known_srs: None,
            record_flags: None,
            members,
            allow_unordered: false,
            add_info_callback: None,
            after_load: None,
            after_set: None,
            is_reference: false,
        }
    }

    #[test]
    fn members_and_known_subrecords() {
        let _guard = test_lock();
        let def = MainRecordDef::create(record(
            MISC,
            "Misc. Item",
            vec![
                sub(EDID, "Editor ID"),
                sub(DATA, "Data"),
                sub(FULL, "Name"),
                sub(DATA, "More Data"),
            ],
        ))
        .unwrap();
        assert_eq!(def.get_full_name(), "MISC - Misc. Item");
        assert_eq!(def.get_def_type_name(), "Record");
        assert_eq!(def.get_member_count(), 4);
        assert_eq!(def.get_member_index_for(None, EDID, None), 0);
        // The last member with a signature is the one that is found for it.
        assert_eq!(def.get_member_index_for(None, DATA, None), 3);
        assert_eq!(def.get_member_index_for(None, NAME, None), -1);
        assert!(def.contains_member_for(None, FULL, None));
        assert_eq!(def.get_quick_init_limit(), 2);
        assert!(def.get_contains_known_sub_record(KnownSubRecord::ksrEditorID));
        assert_eq!(def.get_known_sub_record_member_index(KnownSubRecord::ksrFullName), 2);
        assert!(!def.get_contains_known_sub_record(KnownSubRecord::ksrGridCell));
        assert_eq!(
            def.get_known_sub_record_member(KnownSubRecord::ksrFullName)
                .unwrap()
                .get_name(),
            "Name"
        );
        assert_eq!(def.get_child_pos(def.get_member(1).as_dyn_def()), 1);
        assert_eq!(
            def.get_member(1).get_full_path(),
            "MISC - Misc. Item \\ [1] DATA - Data"
        );
        assert!(!def.get_is_reference() && !def.get_can_be_partial());
        assert!(!def.allow_unordered() && !def.get_skip_signature(DATA));
        assert_eq!(def.to_summary(0, None, &mut None), "");
        assert_eq!(def.get_base_signature_count(), 0);
    }

    #[test]
    fn unordered_records_reject_duplicates() {
        let _guard = test_lock();
        let mut args = record(MISC, "Misc. Item", vec![sub(DATA, "Data"), sub(DATA, "More Data")]);
        args.allow_unordered = true;
        assert_eq!(
            MainRecordDef::create(args).err().unwrap().to_string(),
            "Duplicate definition DATA in allow unordered record MISC"
        );
        let mut args = record(REFR, "Placed Object", vec![sub(EDID, "Editor ID")]);
        args.is_reference = true;
        assert_eq!(
            MainRecordDef::create(args).err().unwrap().to_string(),
            "Reference MainRecord must have BaseRecordFormID"
        );
    }

    #[test]
    fn references_and_their_bases() {
        let _guard = test_lock();
        clear_ref_record_defs();
        let base: Arc<dyn ValueDef> = IntegerDef::create(
            named(""),
            IntType::itU32,
            Some(FormIDDefFormater::create_checked(&[MISC], &[], false, false)),
            0,
        );
        let name: Arc<dyn RecordMemberDef> = SubRecordDef::create(named("Base"), &[NAME], Some(base), false);
        let mut args = record(REFR, "Placed Object", vec![sub(EDID, "Editor ID"), name]);
        args.is_reference = true;
        let reference = MainRecordDef::create(args).unwrap();
        add_ref_record_def(reference.clone());
        assert!(reference.get_is_reference());
        assert!(reference.is_valid_base_signature(MISC) && !reference.is_valid_base_signature(REFR));
        assert_eq!(reference.get_base_signature_count(), 1);
        assert_eq!(reference.get_base_signature(0), MISC);
        assert_eq!(
            reference.get_known_sub_record_member_index(KnownSubRecord::ksrBaseRecord),
            1
        );

        let misc = MainRecordDef::create(record(MISC, "Misc. Item", vec![sub(EDID, "Editor ID")])).unwrap();
        assert_eq!(misc.get_reference_signature_count(), 1);
        assert_eq!(misc.get_reference_signature(0), REFR);
        assert!(misc.is_valid_reference_signature(REFR));
        clear_ref_record_defs();
    }

    #[test]
    fn header_with_own_record_flags() {
        let _guard = test_lock();
        let flags_member = IntegerDef::create(named("Record Flags"), IntType::itU32, None, 0);
        let header: Arc<dyn ValueDef> = StructDef::create(
            named("Record Header"),
            StructDefArgs {
                members: vec![
                    IntegerDef::create(named("Data Size"), IntType::itU32, None, 0),
                    flags_member.clone(),
                ],
                ..StructDefArgs::default()
            },
        );
        set_record_flags(Some(flags_member));
        set_main_record_header(Some(header.clone()));
        let flags: Arc<dyn IntegerDefFormater> =
            FlagsDef::create(false, None, &["ESM", "Partial Form"], &[], false, 0, &[]);
        let mut args = record(MISC, "Misc. Item", vec![sub(EDID, "Editor ID")]);
        args.record_flags = Some(flags);
        let def = MainRecordDef::create(args).unwrap();
        assert!(def.get_can_be_partial());
        let own_header = def.get_record_header_struct().unwrap();
        assert_ne!(own_header.get_def_id(), header.get_def_id());
        assert_eq!(own_header.get_path(), "Misc. Item \\ Record Header");
        let own_flags = own_header.get_member_by_name("Record Flags").unwrap();
        assert_eq!(own_flags.to_string(Some(&[3, 0, 0, 0]), None), "ESM, Partial Form");
        // The shared header keeps its plain record flags.
        let shared = header.clone().into_struct_def().unwrap();
        assert_eq!(
            shared
                .get_member_by_name("Record Flags")
                .unwrap()
                .to_string(Some(&[3, 0, 0, 0]), None),
            "3"
        );
        let plain = MainRecordDef::create(record(MISC, "Misc. Item", vec![])).unwrap();
        assert_eq!(
            plain.get_record_header_struct().unwrap().get_def_id(),
            header.get_def_id()
        );
        set_record_flags(None);
        set_main_record_header(None);
    }

    #[test]
    fn setters_and_clone() {
        let _guard = test_lock();
        let def = MainRecordDef::create(record(MISC, "", vec![sub(EDID, "Editor ID"), sub(DATA, "Data")]))
            .unwrap()
            .set_form_id_base(3)
            .set_summary_key(&[1])
            .set_summary_member_prefix_suffix(1, "(", ")")
            .set_ignore_list(&[NAME])
            .set_summary_name("Item");
        assert_eq!(def.get_name(), "MISC");
        assert_eq!(def.get_form_id_base(), 3);
        assert_eq!(def.get_form_id_name_base(), 3);
        assert!(def.should_ignore(NAME) && !def.should_ignore(DATA));
        let copy = MainRecordDef::clone_from(&def);
        assert_eq!(copy.get_form_id_base(), 3);
        assert_eq!(MainRecordDef::loaded(&copy.rec_summary_key), [1]);
        assert_eq!(copy.get_summary_name(), "Item");
        // The ignore list is not copied.
        assert!(!copy.should_ignore(NAME));
        assert_eq!(copy.get_member_count(), 2);
    }
}
