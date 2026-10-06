// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

// Ported from xEdit: Core/wbInterface.pas

//! The constructors and the helpers that the generated builder functions in
//! [`super::builders`] call, with the parameters of the upstream constructors
//! in the upstream order.
//!
//! The generated code passes every interface value as an `Option`, because
//! the definitions pass `nil` members on purpose. The constructors here drop
//! `None` members where upstream skips unassigned members, and panic where
//! upstream would fail on a `nil`.

use std::collections::HashMap;
use std::sync::{Arc, RwLock};

use super::array::{ArrayDef, ArrayDefArgs};
use super::byte_array::{ByteArrayDef, CountCallback};
use super::def::{
    AfterLoadCallback, AfterSetCallback, Def, DontShowCallback, EmptyDef, GetConflictPriority, NamedDefArgs, ValueDef,
};
use super::element::{DataPtr, ElementArg, ElementRef};
use super::enum_def::{EnumClass, EnumDef, SparseName};
use super::flags::FlagsDef;
use super::float::{FloatDef, FloatDefArgs, FloatKind, FloatNormalizer};
use super::form_id_formater::FormIDDefFormater;
use super::formaters::{
    CallbackDef, DivDef, DivFDef, DumpIntegerDefFormater, IntToStrCallback, IntegerDefFormaterUnion,
    IntegerDefFormaterUnionDecider, MulDef, Str4, StrToIntCallback,
};
use super::globals::{hide_never_show, report_mode};
use super::guid::GuidDef;
use super::integer::{IntegerDef, IntegerDefFormater};
use super::len_string::LenStringDef;
use super::main_record::{AddInfoCallback, MainRecordDef, MainRecordDefArgs, add_ref_record_def};
use super::misc::{int_to_hex64, str_to_int_def};
use super::resolvable::{RecursiveDef, UnionDecider, UnionDef};
use super::string::{StringClass, StringDef};
use super::struct_def::{StructDef, StructDefArgs};
use super::sub_record::{RecordMemberDef, SubRecordDef};
use super::sub_record_group::{
    IsSortedCallback, RUnionDecider, SubRecordArrayDef, SubRecordStructDef, SubRecordUnionDef,
};
use super::types::{CallbackType, ConflictPriority, ElementType, IntType, KnownSubRecordSignatures, Signature, VarRec};
use crate::delphi::single_same_value;

/// Upstream `wbRadiansToDegreesScale`, a variable that nothing changes.
pub fn radians_to_degrees_scale() -> f64 {
    180.0 / std::f64::consts::PI
}

#[allow(clippy::too_many_arguments)]
fn named(
    priority: ConflictPriority,
    required: bool,
    name: &str,
    after_load: Option<AfterLoadCallback>,
    after_set: Option<AfterSetCallback>,
    dont_show: Option<DontShowCallback>,
    get_cp: Option<GetConflictPriority>,
    terminator: bool,
) -> NamedDefArgs {
    NamedDefArgs {
        priority,
        required,
        name: name.to_owned(),
        after_load,
        after_set,
        dont_show,
        get_cp,
        terminator,
    }
}

/// The members that are assigned, as the upstream constructors keep them.
fn assigned<T: ?Sized>(members: &[Option<Arc<T>>]) -> Vec<Arc<T>> {
    members.iter().flatten().cloned().collect()
}

fn strings(names: &[&str]) -> Vec<String> {
    names.iter().map(|name| (*name).to_owned()).collect()
}

/// The sparse names of an enumeration from the `array of const` argument:
/// an index and a name, with a summary when `has_summary`.
fn sparse_names(has_summary: bool, sparse: &[VarRec]) -> Vec<SparseName> {
    let step = if has_summary { 3 } else { 2 };
    assert!(
        sparse.len().is_multiple_of(step),
        "sparse names come in groups of {step}"
    );
    sparse
        .chunks(step)
        .map(|group| {
            let VarRec::Int(index) = &group[0] else {
                panic!("the index of a sparse name is an integer")
            };
            let VarRec::Str(name) = &group[1] else {
                panic!("the name of a sparse name is a string")
            };
            let summary = match group.get(2) {
                Some(VarRec::Str(summary)) => summary.as_str(),
                Some(_) => panic!("the summary of a sparse name is a string"),
                None => "",
            };
            SparseName::with_summary(*index, name, summary)
        })
        .collect()
}

fn not_ported(class: &str, reason: &str) -> ! {
    panic!("{class} is not ported: {reason}")
}

// ----- constructors -----

#[allow(clippy::too_many_arguments)]
pub fn twb_array_def_create_count(
    a_priority: ConflictPriority,
    a_required: bool,
    a_name: &str,
    a_element: Option<Arc<dyn ValueDef>>,
    a_count: i32,
    a_labels: &[&str],
    a_sorted: bool,
    a_after_load: Option<AfterLoadCallback>,
    a_after_set: Option<AfterSetCallback>,
    a_dont_show: Option<DontShowCallback>,
    a_get_cp: Option<GetConflictPriority>,
    a_can_add_to: bool,
    a_terminator: bool,
    a_terminated: bool,
) -> Option<Arc<ArrayDef>> {
    Some(ArrayDef::create(
        named(
            a_priority,
            a_required,
            a_name,
            a_after_load,
            a_after_set,
            a_dont_show,
            a_get_cp,
            a_terminator,
        ),
        ArrayDefArgs {
            element: a_element.expect("the element of an array definition"),
            count: a_count,
            count_callback: None,
            labels: strings(a_labels),
            sorted: a_sorted,
            can_add_to: a_can_add_to,
            terminated: a_terminated,
        },
    ))
}

#[allow(clippy::too_many_arguments)]
pub fn twb_array_def_create_count_callback(
    a_priority: ConflictPriority,
    a_required: bool,
    a_name: &str,
    a_element: Option<Arc<dyn ValueDef>>,
    a_count_callback: Option<CountCallback>,
    a_labels: &[&str],
    a_sorted: bool,
    a_after_load: Option<AfterLoadCallback>,
    a_after_set: Option<AfterSetCallback>,
    a_dont_show: Option<DontShowCallback>,
    a_get_cp: Option<GetConflictPriority>,
    a_can_add_to: bool,
    a_terminator: bool,
    a_terminated: bool,
) -> Option<Arc<ArrayDef>> {
    Some(ArrayDef::create(
        named(
            a_priority,
            a_required,
            a_name,
            a_after_load,
            a_after_set,
            a_dont_show,
            a_get_cp,
            a_terminator,
        ),
        ArrayDefArgs {
            element: a_element.expect("the element of an array definition"),
            count: 0,
            count_callback: a_count_callback,
            labels: strings(a_labels),
            sorted: a_sorted,
            can_add_to: a_can_add_to,
            terminated: a_terminated,
        },
    ))
}

#[allow(clippy::too_many_arguments)]
pub fn twb_byte_array_def_create(
    a_priority: ConflictPriority,
    a_required: bool,
    a_name: &str,
    a_size: i64,
    a_dont_show: Option<DontShowCallback>,
    a_count_callback: Option<CountCallback>,
    a_get_cp: Option<GetConflictPriority>,
    a_terminator: bool,
) -> Option<Arc<ByteArrayDef>> {
    Some(ByteArrayDef::create(
        named(
            a_priority,
            a_required,
            a_name,
            None,
            None,
            a_dont_show,
            a_get_cp,
            a_terminator,
        ),
        a_size,
        a_count_callback,
    ))
}

pub fn twb_callback_def_create(
    a_to_str: Option<IntToStrCallback>,
    a_to_int: Option<StrToIntCallback>,
) -> Option<Arc<CallbackDef>> {
    Some(CallbackDef::create(
        a_to_str.expect("the to-string callback of a callback formater"),
        a_to_int,
    ))
}

pub fn twb_char4_create() -> Option<Arc<dyn IntegerDefFormater>> {
    not_ported("TwbChar4", "only Oblivion uses it")
}

pub fn twb_data6_key2_enum_def_create(
    a_has_summary: bool,
    a_names: &[&str],
    a_sparse_names: &[VarRec],
) -> Option<Arc<EnumDef>> {
    Some(EnumDef::create(
        EnumClass::Data6Key2,
        a_has_summary,
        a_names,
        &sparse_names(a_has_summary, a_sparse_names),
    ))
}

pub fn twb_div_def_create(a_value: i32, a_precision: i32) -> Option<Arc<DivDef>> {
    Some(DivDef::create(a_value, a_precision))
}

pub fn twb_div_f_def_create(a_value: f64, a_precision: i32) -> Option<Arc<DivFDef>> {
    Some(DivFDef::create(a_value, a_precision))
}

pub fn twb_dump_integer_def_formater_create() -> Option<Arc<DumpIntegerDefFormater>> {
    Some(DumpIntegerDefFormater::create())
}

#[allow(clippy::too_many_arguments)]
pub fn twb_empty_def_create(
    a_priority: ConflictPriority,
    a_required: bool,
    a_name: &str,
    a_after_load: Option<AfterLoadCallback>,
    a_after_set: Option<AfterSetCallback>,
    a_dont_show: Option<DontShowCallback>,
    a_sorted: bool,
    a_get_cp: Option<GetConflictPriority>,
) -> Option<Arc<EmptyDef>> {
    Some(EmptyDef::create(
        named(
            a_priority,
            a_required,
            a_name,
            a_after_load,
            a_after_set,
            a_dont_show,
            a_get_cp,
            false,
        ),
        a_sorted,
    ))
}

pub fn twb_enum_def_create(a_has_summary: bool, a_names: &[&str], a_sparse_names: &[VarRec]) -> Option<Arc<EnumDef>> {
    Some(EnumDef::create(
        EnumClass::Enum,
        a_has_summary,
        a_names,
        &sparse_names(a_has_summary, a_sparse_names),
    ))
}

#[allow(clippy::too_many_arguments)]
pub fn twb_flags_def_create(
    a_has_summary: bool,
    a_base_flags_def: Option<Arc<FlagsDef>>,
    a_names: &[&str],
    a_dont_shows: &[Option<DontShowCallback>],
    a_unknown_is_unused: bool,
    a_ignore_mask: i64,
    a_get_c_ps: &[Option<GetConflictPriority>],
) -> Option<Arc<FlagsDef>> {
    Some(FlagsDef::create(
        a_has_summary,
        a_base_flags_def,
        a_names,
        a_dont_shows,
        a_unknown_is_unused,
        a_ignore_mask,
        a_get_c_ps,
    ))
}

#[allow(clippy::too_many_arguments)]
pub fn twb_float_def_create(
    a_priority: ConflictPriority,
    a_required: bool,
    a_name: &str,
    a_after_load: Option<AfterLoadCallback>,
    a_after_set: Option<AfterSetCallback>,
    a_scale: f64,
    a_digits: i32,
    a_dont_show: Option<DontShowCallback>,
    a_normalizer: Option<FloatNormalizer>,
    a_default: f64,
    a_get_cp: Option<GetConflictPriority>,
    a_kind: FloatKind,
    a_terminator: bool,
) -> Option<Arc<FloatDef>> {
    Some(FloatDef::create(
        named(
            a_priority,
            a_required,
            a_name,
            a_after_load,
            a_after_set,
            a_dont_show,
            a_get_cp,
            a_terminator,
        ),
        FloatDefArgs {
            scale: a_scale,
            digits: a_digits,
            normalizer: a_normalizer,
            default: a_default,
            kind: a_kind,
        },
    ))
}

pub fn twb_form_id_checked_create(
    a_valid_refs: &[Signature],
    a_valid_flst_refs: &[Signature],
    a_persistent: bool,
    a_no_reach: bool,
) -> Option<Arc<FormIDDefFormater>> {
    Some(FormIDDefFormater::create_checked(
        a_valid_refs,
        a_valid_flst_refs,
        a_persistent,
        a_no_reach,
    ))
}

pub fn twb_form_id_checked_st_create(
    a_valid_refs: &[Signature],
    a_valid_flst_refs: &[Signature],
    a_persistent: bool,
    a_no_reach: bool,
) -> Option<Arc<FormIDDefFormater>> {
    // Upstream only ever creates the ST variant without FLST references and reachable.
    assert!(
        a_valid_flst_refs.is_empty() && !a_no_reach,
        "TwbFormIDCheckedST with FLST references or no reach"
    );
    Some(FormIDDefFormater::create_checked_st(a_valid_refs, a_persistent))
}

pub fn twb_form_id_def_formater_create() -> Option<Arc<FormIDDefFormater>> {
    Some(FormIDDefFormater::create())
}

#[allow(clippy::too_many_arguments)]
pub fn twb_guid_def_create(
    a_priority: ConflictPriority,
    a_required: bool,
    a_name: &str,
    a_after_load: Option<AfterLoadCallback>,
    a_after_set: Option<AfterSetCallback>,
    a_dont_show: Option<DontShowCallback>,
    a_get_cp: Option<GetConflictPriority>,
    a_terminator: bool,
) -> Option<Arc<GuidDef>> {
    Some(GuidDef::create(named(
        a_priority,
        a_required,
        a_name,
        a_after_load,
        a_after_set,
        a_dont_show,
        a_get_cp,
        a_terminator,
    )))
}

#[allow(clippy::too_many_arguments)]
pub fn twb_integer_def_create(
    a_priority: ConflictPriority,
    a_required: bool,
    a_name: &str,
    a_int_type: IntType,
    a_formater: Option<Arc<dyn IntegerDefFormater>>,
    a_dont_show: Option<DontShowCallback>,
    a_after_set: Option<AfterSetCallback>,
    a_default: i64,
    a_get_cp: Option<GetConflictPriority>,
    a_terminator: bool,
) -> Option<Arc<IntegerDef>> {
    Some(IntegerDef::create(
        named(
            a_priority,
            a_required,
            a_name,
            None,
            a_after_set,
            a_dont_show,
            a_get_cp,
            a_terminator,
        ),
        a_int_type,
        a_formater,
        a_default,
    ))
}

pub fn twb_integer_def_formater_union_create(
    a_decider: Option<IntegerDefFormaterUnionDecider>,
    a_members: &[Option<Arc<dyn IntegerDefFormater>>],
) -> Option<Arc<IntegerDefFormaterUnion>> {
    let members: Vec<Arc<dyn IntegerDefFormater>> = a_members
        .iter()
        .map(|member| member.clone().expect("a member of a formater union"))
        .collect();
    Some(IntegerDefFormaterUnion::create(
        a_decider.expect("the decider of a formater union"),
        &members,
    ))
}

pub fn twb_key2_data6_enum_def_create(
    a_has_summary: bool,
    a_names: &[&str],
    a_sparse_names: &[VarRec],
) -> Option<Arc<EnumDef>> {
    Some(EnumDef::create(
        EnumClass::Key2Data6,
        a_has_summary,
        a_names,
        &sparse_names(a_has_summary, a_sparse_names),
    ))
}

#[allow(clippy::too_many_arguments)]
fn string_def(
    class: StringClass,
    a_priority: ConflictPriority,
    a_required: bool,
    a_name: &str,
    a_size: i32,
    a_after_load: Option<AfterLoadCallback>,
    a_after_set: Option<AfterSetCallback>,
    a_dont_show: Option<DontShowCallback>,
    a_get_cp: Option<GetConflictPriority>,
    a_terminator: bool,
    a_forward: bool,
) -> Option<Arc<StringDef>> {
    Some(StringDef::create(
        class,
        named(
            a_priority,
            a_required,
            a_name,
            a_after_load,
            a_after_set,
            a_dont_show,
            a_get_cp,
            a_terminator,
        ),
        a_size,
        a_forward,
    ))
}

macro_rules! string_constructor {
    ($name:ident, $class:expr) => {
        #[allow(clippy::too_many_arguments)]
        pub fn $name(
            a_priority: ConflictPriority,
            a_required: bool,
            a_name: &str,
            a_size: i32,
            a_after_load: Option<AfterLoadCallback>,
            a_after_set: Option<AfterSetCallback>,
            a_dont_show: Option<DontShowCallback>,
            a_get_cp: Option<GetConflictPriority>,
            a_terminator: bool,
            a_forward: bool,
        ) -> Option<Arc<StringDef>> {
            string_def(
                $class,
                a_priority,
                a_required,
                a_name,
                a_size,
                a_after_load,
                a_after_set,
                a_dont_show,
                a_get_cp,
                a_terminator,
                a_forward,
            )
        }
    };
}

string_constructor!(twb_string_def_create, StringClass::String);
string_constructor!(twb_string_lc_def_create, StringClass::LC);
string_constructor!(twb_string_kc_def_create, StringClass::KC);
string_constructor!(twb_string_script_def_create, StringClass::Script);
string_constructor!(twb_l_string_def_create, StringClass::LString);
string_constructor!(twb_l_string_kc_def_create, StringClass::LStringKC);

#[allow(clippy::too_many_arguments)]
pub fn twb_string_mgef_code_def_create(
    _a_priority: ConflictPriority,
    _a_required: bool,
    _a_name: &str,
    _a_size: i32,
    _a_after_load: Option<AfterLoadCallback>,
    _a_after_set: Option<AfterSetCallback>,
    _a_dont_show: Option<DontShowCallback>,
    _a_get_cp: Option<GetConflictPriority>,
    _a_terminator: bool,
    _a_forward: bool,
) -> Option<Arc<StringDef>> {
    not_ported("TwbStringMgefCodeDef", "only Oblivion uses it")
}

#[allow(clippy::too_many_arguments)]
pub fn twb_len_string_def_create(
    a_priority: ConflictPriority,
    a_required: bool,
    a_name: &str,
    a_prefix: i32,
    a_after_load: Option<AfterLoadCallback>,
    a_after_set: Option<AfterSetCallback>,
    a_dont_show: Option<DontShowCallback>,
    a_get_cp: Option<GetConflictPriority>,
    a_terminator: bool,
) -> Option<Arc<LenStringDef>> {
    Some(LenStringDef::create(
        named(
            a_priority,
            a_required,
            a_name,
            a_after_load,
            a_after_set,
            a_dont_show,
            a_get_cp,
            a_terminator,
        ),
        a_prefix,
    ))
}

pub fn twb_mul_def_create(a_value: i32) -> Option<Arc<MulDef>> {
    Some(MulDef::create(a_value))
}

#[allow(clippy::too_many_arguments)]
pub fn twb_recursive_def_create(
    a_priority: ConflictPriority,
    a_required: bool,
    a_name: &str,
    a_levels_up: i32,
    a_dont_show: Option<DontShowCallback>,
    a_after_set: Option<AfterSetCallback>,
    a_get_cp: Option<GetConflictPriority>,
) -> Option<Arc<RecursiveDef>> {
    Some(RecursiveDef::create(
        named(
            a_priority,
            a_required,
            a_name,
            None,
            a_after_set,
            a_dont_show,
            a_get_cp,
            false,
        ),
        a_levels_up,
    ))
}

pub fn twb_ref_id_create() -> Option<Arc<dyn IntegerDefFormater>> {
    not_ported("TwbRefID", "only save files use it")
}

pub fn twb_str4_create() -> Option<Arc<Str4>> {
    Some(Str4::create())
}

/// The compressed structures of save files share one signature upstream.
#[allow(clippy::too_many_arguments)]
fn struct_c_def(class: &str) -> ! {
    not_ported(class, "only save files use it")
}

macro_rules! struct_c_constructor {
    ($name:ident, $class:literal) => {
        #[allow(clippy::too_many_arguments)]
        pub fn $name(
            _a_priority: ConflictPriority,
            _a_required: bool,
            _a_name: &str,
            _a_members: &[Option<Arc<dyn ValueDef>>],
            _a_sort_key: &[i32],
            _a_ex_sort_key: &[i32],
            _a_optional_from_element: i32,
            _a_dont_show: Option<DontShowCallback>,
            _a_after_load: Option<AfterLoadCallback>,
            _a_after_set: Option<AfterSetCallback>,
            _a_size_call_back: Option<SizeCallback>,
            _a_get_chapter_type: Option<GetChapterTypeCallback>,
            _a_get_chapter_type_name: Option<GetChapterTypeNameCallback>,
            _a_get_chapter_name: Option<GetChapterNameCallback>,
            _a_get_cp: Option<GetConflictPriority>,
        ) -> Option<Arc<StructDef>> {
            struct_c_def($class)
        }
    };
}

/// Upstream `TwbSizeCallback`, for the save file structures that are not ported.
pub type SizeCallback = Arc<dyn Fn(ElementArg) -> i64 + Send + Sync>;
/// Upstream `TwbGetChapterTypeCallback`, for the save file structures that are not ported.
pub type GetChapterTypeCallback = Arc<dyn Fn(ElementArg) -> i32 + Send + Sync>;
/// Upstream `TwbGetChapterTypeNameCallback`, for the save file structures that are not ported.
pub type GetChapterTypeNameCallback = Arc<dyn Fn(ElementArg) -> String + Send + Sync>;
/// Upstream `TwbGetChapterNameCallback`, for the save file structures that are not ported.
pub type GetChapterNameCallback = Arc<dyn Fn(ElementArg) -> String + Send + Sync>;

struct_c_constructor!(twb_struct_c_def_create, "TwbStructCDef");
struct_c_constructor!(twb_struct_z_def_create, "TwbStructZDef");
struct_c_constructor!(twb_struct_lz_def_create, "TwbStructLZDef");

#[allow(clippy::too_many_arguments)]
pub fn twb_struct_def_create(
    a_priority: ConflictPriority,
    a_required: bool,
    a_name: &str,
    a_members: &[Option<Arc<dyn ValueDef>>],
    a_sort_key: &[i32],
    a_ex_sort_key: &[i32],
    a_element_map: &[i32],
    a_optional_from_element: i32,
    a_dont_show: Option<DontShowCallback>,
    a_after_load: Option<AfterLoadCallback>,
    a_after_set: Option<AfterSetCallback>,
    a_get_cp: Option<GetConflictPriority>,
) -> Option<Arc<StructDef>> {
    Some(StructDef::create(
        named(
            a_priority,
            a_required,
            a_name,
            a_after_load,
            a_after_set,
            a_dont_show,
            a_get_cp,
            false,
        ),
        StructDefArgs {
            members: assigned(a_members),
            sort_key: a_sort_key.to_vec(),
            ex_sort_key: a_ex_sort_key.to_vec(),
            element_map: a_element_map.iter().map(|&index| index as u32).collect(),
            optional_from_element: a_optional_from_element,
        },
    ))
}

#[allow(clippy::too_many_arguments)]
pub fn twb_sub_record_array_def_create(
    a_priority: ConflictPriority,
    a_required: bool,
    a_name: &str,
    a_element: Option<Arc<dyn RecordMemberDef>>,
    a_count: i32,
    a_sorted: bool,
    a_after_load: Option<AfterLoadCallback>,
    a_after_set: Option<AfterSetCallback>,
    a_dont_show: Option<DontShowCallback>,
    a_is_sorted: Option<IsSortedCallback>,
    a_get_cp: Option<GetConflictPriority>,
) -> Option<Arc<SubRecordArrayDef>> {
    Some(SubRecordArrayDef::create(
        named(
            a_priority,
            a_required,
            a_name,
            a_after_load,
            a_after_set,
            a_dont_show,
            a_get_cp,
            false,
        ),
        a_element.expect("the element of a subrecord array definition"),
        a_count,
        a_sorted,
        a_is_sorted,
    ))
}

#[allow(clippy::too_many_arguments)]
pub fn twb_sub_record_def_create_signature(
    a_priority: ConflictPriority,
    a_required: bool,
    a_signature: Signature,
    a_name: &str,
    a_value: Option<Arc<dyn ValueDef>>,
    a_after_load: Option<AfterLoadCallback>,
    a_after_set: Option<AfterSetCallback>,
    a_size_match: bool,
    a_dont_show: Option<DontShowCallback>,
    a_get_cp: Option<GetConflictPriority>,
) -> Option<Arc<SubRecordDef>> {
    twb_sub_record_def_create_signatures(
        a_priority,
        a_required,
        &[a_signature],
        a_name,
        a_value,
        a_after_load,
        a_after_set,
        a_size_match,
        a_dont_show,
        a_get_cp,
    )
}

#[allow(clippy::too_many_arguments)]
pub fn twb_sub_record_def_create_signatures(
    a_priority: ConflictPriority,
    a_required: bool,
    a_signatures: &[Signature],
    a_name: &str,
    a_value: Option<Arc<dyn ValueDef>>,
    a_after_load: Option<AfterLoadCallback>,
    a_after_set: Option<AfterSetCallback>,
    a_size_match: bool,
    a_dont_show: Option<DontShowCallback>,
    a_get_cp: Option<GetConflictPriority>,
) -> Option<Arc<SubRecordDef>> {
    Some(SubRecordDef::create(
        named(
            a_priority,
            a_required,
            a_name,
            a_after_load,
            a_after_set,
            a_dont_show,
            a_get_cp,
            false,
        ),
        a_signatures,
        a_value,
        a_size_match,
    ))
}

#[allow(clippy::too_many_arguments)]
pub fn twb_sub_record_struct_def_create(
    a_priority: ConflictPriority,
    a_required: bool,
    a_name: &str,
    a_members: &[Option<Arc<dyn RecordMemberDef>>],
    a_skip_sigs: &[Signature],
    a_dont_show: Option<DontShowCallback>,
    a_allow_unordered: bool,
    a_after_load: Option<AfterLoadCallback>,
    a_after_set: Option<AfterSetCallback>,
    a_get_cp: Option<GetConflictPriority>,
) -> Option<Arc<SubRecordStructDef>> {
    Some(SubRecordStructDef::create(
        named(
            a_priority,
            a_required,
            a_name,
            a_after_load,
            a_after_set,
            a_dont_show,
            a_get_cp,
            false,
        ),
        assigned(a_members),
        a_skip_sigs,
        a_allow_unordered,
    ))
}

#[allow(clippy::too_many_arguments)]
pub fn twb_sub_record_struct_sk_def_create(
    a_priority: ConflictPriority,
    a_required: bool,
    a_name: &str,
    a_members: &[Option<Arc<dyn RecordMemberDef>>],
    a_skip_sigs: &[Signature],
    a_sort_key: &[i32],
    a_ex_sort_key: &[i32],
    a_dont_show: Option<DontShowCallback>,
    a_allow_unordered: bool,
    a_after_load: Option<AfterLoadCallback>,
    a_after_set: Option<AfterSetCallback>,
    a_get_cp: Option<GetConflictPriority>,
) -> Option<Arc<SubRecordStructDef>> {
    Some(SubRecordStructDef::create_sk(
        named(
            a_priority,
            a_required,
            a_name,
            a_after_load,
            a_after_set,
            a_dont_show,
            a_get_cp,
            false,
        ),
        assigned(a_members),
        a_skip_sigs,
        a_sort_key,
        a_ex_sort_key,
        a_allow_unordered,
    ))
}

#[allow(clippy::too_many_arguments)]
pub fn twb_sub_record_union_def_create(
    a_priority: ConflictPriority,
    a_required: bool,
    a_name: &str,
    a_members: &[Option<Arc<dyn RecordMemberDef>>],
    a_skip_sigs: &[Signature],
    a_dont_show: Option<DontShowCallback>,
    a_get_cp: Option<GetConflictPriority>,
    a_decider: Option<RUnionDecider>,
) -> Option<Arc<SubRecordUnionDef>> {
    // Upstream does not skip unassigned members here.
    let members = a_members
        .iter()
        .map(|member| member.clone().expect("a member of a subrecord union"))
        .collect();
    Some(SubRecordUnionDef::create(
        named(a_priority, a_required, a_name, None, None, a_dont_show, a_get_cp, false),
        members,
        a_skip_sigs,
        a_decider,
    ))
}

#[allow(clippy::too_many_arguments)]
pub fn twb_union_def_create(
    a_priority: ConflictPriority,
    a_required: bool,
    a_name: &str,
    a_decider: Option<UnionDecider>,
    a_members: &[Option<Arc<dyn ValueDef>>],
    a_dont_show: Option<DontShowCallback>,
    a_after_set: Option<AfterSetCallback>,
    a_get_cp: Option<GetConflictPriority>,
) -> Option<Arc<UnionDef>> {
    Some(UnionDef::create(
        named(
            a_priority,
            a_required,
            a_name,
            None,
            a_after_set,
            a_dont_show,
            a_get_cp,
            false,
        ),
        a_decider.expect("the decider of a union"),
        assigned(a_members),
    ))
}

// ----- builders that are written by hand -----

/// Upstream `wbRecordDefs` and `wbRecordDefHashMap`: the record definitions
/// by signature.
static RECORD_DEFS: RwLock<Vec<(Signature, Arc<MainRecordDef>)>> = RwLock::new(Vec::new());
static RECORD_DEF_INDEX: RwLock<Option<HashMap<u32, usize>>> = RwLock::new(None);

/// Upstream `wbRecordDefs`, in the order of definition.
pub fn record_defs() -> Vec<(Signature, Arc<MainRecordDef>)> {
    RECORD_DEFS.read().unwrap().clone()
}

static RECORDS_INIT: RwLock<bool> = RwLock::new(false);

/// Port of `wbInitRecords`: runs `InitFromParent` over every record
/// definition and the main record header once, after the definitions of
/// the game are complete, so that the flags that inherit up and down the
/// definitions are in place. Upstream runs it before the first file loads.
pub fn init_records() {
    let mut done = RECORDS_INIT.write().unwrap();
    if *done {
        return;
    }
    *done = true;
    for _looped in [false, true] {
        for (_, def) in record_defs() {
            def.init_from_parent();
        }
        if let Some(header) = super::main_record::main_record_header() {
            header.init_from_parent();
        }
    }
}

/// Upstream `wbFindRecordDef`: the definition of the records with `signature`.
pub fn find_record_def(signature: Signature) -> Option<Arc<MainRecordDef>> {
    let index = *RECORD_DEF_INDEX.read().unwrap().as_ref()?.get(&signature.to_int())?;
    Some(RECORD_DEFS.read().unwrap()[index].1.clone())
}

/// Empties upstream `wbRecordDefs`, for the tests.
pub fn clear_record_defs() {
    *RECORDS_INIT.write().unwrap() = false;
    RECORD_DEFS.write().unwrap().clear();
    *RECORD_DEF_INDEX.write().unwrap() = None;
}

/// Upstream `wbRecord` with `aIsReference`: the overload that the others
/// call. It registers the definition.
///
/// Panics with the upstream message on a second definition of a signature.
#[allow(clippy::too_many_arguments)]
pub fn wb_record_is_reference(
    a_signature: Signature,
    a_name: &str,
    a_known_s_rs: Option<&'static KnownSubRecordSignatures>,
    a_record_flags: Option<Arc<dyn IntegerDefFormater>>,
    a_members: &[Option<Arc<dyn RecordMemberDef>>],
    a_allow_unordered: bool,
    a_add_info_callback: Option<AddInfoCallback>,
    a_priority: ConflictPriority,
    a_required: bool,
    a_after_load: Option<AfterLoadCallback>,
    a_after_set: Option<AfterSetCallback>,
    a_is_reference: bool,
) -> Option<Arc<MainRecordDef>> {
    let mut defs = RECORD_DEFS.write().unwrap();
    let mut index = RECORD_DEF_INDEX.write().unwrap();
    let index = index.get_or_insert_with(HashMap::new);
    assert!(
        !index.contains_key(&a_signature.to_int()),
        "Duplicated record definition for signature {a_signature}"
    );
    let def = MainRecordDef::create(MainRecordDefArgs {
        priority: a_priority,
        required: a_required,
        signature: a_signature,
        name: a_name.to_owned(),
        known_srs: a_known_s_rs.copied(),
        record_flags: a_record_flags,
        members: assigned(a_members),
        allow_unordered: a_allow_unordered,
        add_info_callback: a_add_info_callback,
        after_load: a_after_load,
        after_set: a_after_set,
        is_reference: a_is_reference,
    })
    .unwrap_or_else(|error| panic!("{error}"));
    index.insert(a_signature.to_int(), defs.len());
    defs.push((a_signature, def.clone()));
    Some(def)
}

/// Upstream `wbRefRecord` with record flags.
#[allow(clippy::too_many_arguments)]
pub fn wb_ref_record_record_flags(
    a_signature: Signature,
    a_name: &str,
    a_record_flags: Option<Arc<dyn IntegerDefFormater>>,
    a_members: &[Option<Arc<dyn RecordMemberDef>>],
    a_allow_unordered: bool,
    a_add_info_callback: Option<AddInfoCallback>,
    a_priority: ConflictPriority,
    a_required: bool,
    a_after_load: Option<AfterLoadCallback>,
    a_after_set: Option<AfterSetCallback>,
) -> Option<Arc<MainRecordDef>> {
    let result = wb_record_is_reference(
        a_signature,
        a_name,
        None,
        a_record_flags,
        a_members,
        a_allow_unordered,
        a_add_info_callback,
        a_priority,
        a_required,
        a_after_load,
        a_after_set,
        true,
    );
    if let Some(def) = &result {
        add_ref_record_def(def.clone());
    }
    result
}

static FORM_ID: RwLock<Option<Arc<FormIDDefFormater>>> = RwLock::new(None);

/// Upstream `wbFormID` without arguments: one shared formater, or a new one
/// in report mode.
pub fn wb_form_id() -> Option<Arc<FormIDDefFormater>> {
    if report_mode() {
        return Some(FormIDDefFormater::create());
    }
    let mut shared = FORM_ID.write().unwrap();
    Some(shared.get_or_insert_with(FormIDDefFormater::create).clone())
}

/// Upstream `wbRefID` without arguments.
pub fn wb_ref_id() -> Option<Arc<dyn IntegerDefFormater>> {
    not_ported("TwbRefID", "only save files use it")
}

/// Upstream `wbNeverShow`.
pub fn wb_never_show(_a_element: ElementArg) -> bool {
    hide_never_show()
}

/// Upstream `wbNextObjectIDToString`.
pub fn wb_next_object_id_to_string(a_int: i64, _a_element: ElementArg, a_type: CallbackType) -> String {
    match a_type {
        CallbackType::ctToStr | CallbackType::ctToSortKey => int_to_hex64(a_int, 8),
        CallbackType::ctToEditValue => format!("${}", int_to_hex64(a_int, 8)),
        _ => String::new(),
    }
}

/// Upstream `wbNextObjectIDToInt`. The `?` form needs the file of the
/// element and belongs to the write path.
pub fn wb_next_object_id_to_int(a_string: &str, _a_element: ElementArg) -> i64 {
    let s = a_string.trim();
    if s.is_empty() {
        return 2048;
    }
    if s == "?" {
        unimplemented!("wbNextObjectIDToInt with '?' needs the high object ID of the file: write path");
    }
    i64::from(str_to_int_def(s, 2048))
}

/// Upstream `wbVCI1ToStrBeforeFO4`: the version control info of a record header.
pub fn wb_vci1_to_str_before_fo4(
    a_value: &mut String,
    a_base_ptr: DataPtr,
    _a_element: ElementArg,
    a_type: CallbackType,
) {
    if a_type != CallbackType::ctToStr {
        return;
    }
    let Some(bytes) = a_base_ptr.and_then(|data| data.get(..4)) else {
        return;
    };
    let mut c = u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]);
    if c == 0 {
        *a_value = "None".to_owned();
        return;
    }
    let day = c & 0xFF;
    c >>= 8;
    let mut year = i64::from(c & 0xFF);
    c >>= 8;
    year -= 1;
    let month = year % 12 + 1;
    year = year / 12 + 2003;
    let user = c & 0xFF;
    c >>= 8;
    let index = c & 0xFF;
    *a_value = format!("{year:04}-{month:02}-{day:02} User: {user} Index: {index}");
}

/// Upstream `wbVCI1ToStrAfterFO4`.
pub fn wb_vci1_to_str_after_fo4(
    a_value: &mut String,
    a_base_ptr: DataPtr,
    _a_element: ElementArg,
    a_type: CallbackType,
) {
    if a_type != CallbackType::ctToStr {
        return;
    }
    let Some(bytes) = a_base_ptr.and_then(|data| data.get(..4)) else {
        return;
    };
    let mut c = u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]);
    if c == 0 {
        *a_value = "None".to_owned();
        return;
    }
    let day = c & 0x1F;
    c >>= 5;
    let month = c & 0x0F;
    c >>= 4;
    let year = (c & 0x7F) + 2000;
    c >>= 7;
    let user = c & 0xFF;
    c >>= 8;
    let index = c & 0xFF;
    *a_value = format!("{year:04}-{month:02}-{day:02} User: {user} Index: {index}");
}

/// Upstream `wbFlagsList`: the 32 flag names of the record flags from sparse
/// enumeration names, with `Deleted` and `Ignored` at their fixed positions.
pub fn wb_flags_list(a_flags: &[VarRec], a_deleted: bool, a_unknowns: bool) -> Vec<String> {
    let e = EnumDef::create(EnumClass::Enum, false, &[], &sparse_names(false, a_flags));
    (0..32)
        .map(|i| {
            if i == 12 {
                "Ignored".to_owned()
            } else if a_deleted && i == 5 {
                "Deleted".to_owned()
            } else {
                let s = IntegerDefFormater::to_string(&*e, i64::from(i), None, false);
                if !s.starts_with('<') {
                    s
                } else if a_unknowns {
                    format!("Unknown {i}")
                } else {
                    String::new()
                }
            }
        })
        .collect()
}

/// Upstream `wbTimeStampToString`: the date in a file header.
pub fn wb_time_stamp_to_string(
    a_value: &mut String,
    a_base_ptr: DataPtr,
    _a_element: ElementArg,
    a_type: CallbackType,
) {
    if a_type != CallbackType::ctToStr {
        return;
    }
    let Some(bytes) = a_base_ptr.and_then(|data| data.get(..2)) else {
        return;
    };
    let mut c = u32::from(u16::from_le_bytes([bytes[0], bytes[1]]));
    if c == 0 {
        *a_value = "None".to_owned();
        return;
    }
    let day = c & 0x1F;
    c >>= 5;
    let month = c & 0x0F;
    c >>= 4;
    let year = (c & 0x7F) + 2000;
    *a_value = format!("{year:04}-{month:02}-{day:02}");
}

/// Upstream `wbSparseFlags`: the flag names from sparse enumeration names.
pub fn wb_sparse_flags(a_flags: &[VarRec], a_unknowns: bool, a_size: u32) -> Vec<String> {
    let e = EnumDef::create(EnumClass::Enum, false, &[], &sparse_names(false, a_flags));
    (0..a_size)
        .map(|i| {
            let s = IntegerDefFormater::to_string(&*e, i64::from(i), None, false);
            if !s.starts_with('<') {
                s
            } else if a_unknowns {
                format!("Unknown {i}")
            } else {
                String::new()
            }
        })
        .collect()
}

/// Upstream `GetContainerFromUnion`: the container that a union or value
/// element belongs to, or the element itself when it is a container.
pub fn get_container_from_union(element: &ElementRef) -> Option<ElementRef> {
    let mut result = match element.get_element_type() {
        ElementType::etUnion | ElementType::etValue => element.get_container(),
        _ => Some(element.clone()),
    };
    while let Some(current) = &result
        && current.get_element_type() == ElementType::etUnion
    {
        result = current.get_container();
    }
    result.filter(|container| container.as_container().is_some())
}

/// Upstream `GetContainerRefFromUnionOrValue`.
pub fn get_container_ref_from_union_or_value(element: &ElementRef) -> Option<ElementRef> {
    let is_container = |candidate: &ElementRef| candidate.as_container().is_some();
    let mut result = match element.get_element_type() {
        ElementType::etUnion | ElementType::etValue => element.get_container().filter(is_container),
        _ => Some(element.clone()).filter(is_container),
    };
    while let Some(current) = result.clone()
        && current.get_element_type() == ElementType::etUnion
    {
        result = current.get_container().filter(is_container);
    }
    result
}

/// Upstream `TwoPi`: twice the single-precision `OnePi`.
///
/// UPSTREAM-QUIRK: `OnePi` is declared as `Single = 3.1415927`, so angles
/// normalized by adding or subtracting `TwoPi` carry its error.
pub const TWO_PI: f64 = 2.0 * (std::f32::consts::PI as f64);

/// Upstream `wbNormalizeRadians`: the angle in `0..2π`.
pub fn wb_normalize_radians(_a_element: ElementArg, a_float: f64) -> f64 {
    let mut result = a_float;
    if (result / TWO_PI).abs() > 100.0 {
        result -= result.signum() * TWO_PI * ((result / TWO_PI).abs() - 100.0).trunc();
        if (result / TWO_PI).abs() > 101.0 {
            return f64::NAN;
        }
    }
    while result < 0.0 {
        result += TWO_PI;
    }
    while result > TWO_PI {
        result -= TWO_PI;
    }
    if single_same_value(result, 0.0) || result < 0.0 {
        result = 0.0;
    }
    if single_same_value(result, TWO_PI) || result > TWO_PI {
        result = 0.0;
    }
    result
}

#[cfg(test)]
mod tests {
    use super::super::builders::*;
    use super::super::def::NamedDef;
    use super::super::globals::test_lock;
    use super::super::sub_record_group::RecordDef;
    use super::super::types::{ConflictPriority, IntType};
    use super::*;

    #[test]
    fn generated_builders_define_a_record() {
        let _guard = test_lock();
        clear_record_defs();
        let edid = Signature::new(b"EDID");
        let data = Signature::new(b"DATA");
        let record = wb_record(
            Signature::new(b"GMST"),
            "Game Setting",
            &[
                wb_string_signature(edid, "Editor ID", 0, ConflictPriority::cpNormal, true, None, None, None)
                    .map(|def| def as Arc<dyn RecordMemberDef>),
                None,
                wb_struct_signature(
                    data,
                    "Data",
                    &[
                        wb_integer(
                            "Value",
                            IntType::itS32,
                            None,
                            ConflictPriority::cpNormal,
                            false,
                            None,
                            None,
                            0,
                            None,
                        )
                        .map(|def| def as Arc<dyn ValueDef>),
                        wb_float("Float", ConflictPriority::cpNormal, false, None, None, None, 0.0, None)
                            .map(|def| def as Arc<dyn ValueDef>),
                    ],
                    ConflictPriority::cpNormal,
                    false,
                    None,
                    -1,
                    None,
                    None,
                    None,
                )
                .map(|def| def as Arc<dyn RecordMemberDef>),
            ],
            false,
            None,
            ConflictPriority::cpNormal,
            false,
            None,
            None,
        )
        .unwrap();
        assert_eq!(record.get_name(), "Game Setting");
        // The unassigned member is skipped, as upstream.
        assert_eq!(record.get_member_count(), 2);
        assert!(find_record_def(Signature::new(b"GMST")).is_some());
        assert!(find_record_def(Signature::new(b"NONE")).is_none());
        clear_record_defs();
    }

    #[test]
    fn unknown_and_unused_builders() {
        let unknown = wb_unknown(ConflictPriority::cpNormal, false, None, None).unwrap();
        assert_eq!(unknown.get_name(), "Unknown");
        let unused = wb_unused(false).unwrap();
        assert_eq!(unused.get_name(), "Unused");
    }

    #[test]
    fn normalize_radians() {
        let two_pi = TWO_PI;
        assert_eq!(wb_normalize_radians(None, -1.0), two_pi - 1.0);
        assert_eq!(wb_normalize_radians(None, two_pi), 0.0);
        assert!((wb_normalize_radians(None, 1000.5 * two_pi) - two_pi / 2.0).abs() < 1e-9);
    }
}
