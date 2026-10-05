// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

// Ported from xEdit: Core/wbInterface.pas

//! `TwbFlagsDef` and `TwbFlagDef`.

use std::sync::atomic::{AtomicI8, AtomicI32, Ordering};
use std::sync::{Arc, OnceLock, Weak};

use super::def::{
    Def, DefBase, DefCell, DefKind, DefRef, DontShowCallback, GetConflictPriority, NamedDef, NamedDefArgs,
    NamedDefBase, ValueDef, ValueDefBase, get_root, set_parent, value_def_plumbing,
};
use super::element::{DataPtr, ElementArg};
use super::formaters::{editable_unless_internal_only, formater_impls, formater_plumbing};
use super::globals::{report_mode, report_unknown_flags, show_flag_enum_value};
use super::integer::{IntegerDefFormater, integer_def_formater_create};
use super::misc::{get_unknown_int_string, str_to_int_def};
use super::types::{CallbackType, ConflictPriority, DefFlag, DefType, EditType, ElementType, IntType};

/// The `(0x00000001)` suffix that `wbShowFlagEnumValue` adds to flag `index`.
fn flag_value_suffix(index: usize) -> String {
    format!(" (0x{:08X})", 1u64 << index)
}

/// Upstream `TwbFlagsDef`: names for the bits of an integer.
pub struct FlagsDef {
    self_ref: Weak<FlagsDef>,
    def: DefBase,
    nd: NamedDefBase,
    flg_base_flags_def: Option<Arc<FlagsDef>>,
    flg_names: Vec<String>,
    flg_summaries: Vec<String>,
    /// Upstream `flgDontShows`. `flgHasDontShows` is "any entry is assigned".
    flg_dont_shows: DefCell<Vec<Option<DontShowCallback>>>,
    flg_unused_mask: i64,
    flg_ignore_mask: i64,
    flg_unknown_is_unused: bool,
    flg_get_cps: Vec<Option<GetConflictPriority>>,
    flg_flag_defs: Vec<OnceLock<Arc<FlagDef>>>,
    /// Upstream `flgDontShowPath` and `flgDontShowInvert`.
    flg_dont_show_path: DefCell<(String, bool)>,
    flg_deleted_index: AtomicI8,
    flg_partial_form_index: AtomicI8,
    /// How often each unnamed flag was seen. Filled in report mode.
    unknown_flags: [AtomicI32; 64],
}

impl FlagsDef {
    /// Port of `TwbFlagsDef.Create`. With `has_summary`, `names` holds pairs of
    /// name and summary.
    pub fn create(
        has_summary: bool,
        base_flags_def: Option<Arc<FlagsDef>>,
        names: &[&str],
        dont_shows: &[Option<DontShowCallback>],
        unknown_is_unused: bool,
        ignore_mask: i64,
        get_cps: &[Option<GetConflictPriority>],
    ) -> Arc<Self> {
        let step_size = if has_summary { 2 } else { 1 };
        assert!(names.len().is_multiple_of(step_size));
        let count = names.len() / step_size;
        let flg_names: Vec<String> = (0..count).map(|i| names[i * step_size].to_owned()).collect();
        let flg_summaries = (0..count)
            .map(|i| {
                let summary = if has_summary { names[i * step_size + 1] } else { "" };
                if summary.is_empty() {
                    flg_names[i].clone()
                } else {
                    summary.to_owned()
                }
            })
            .collect();
        let base_flags_def = base_flags_def.map(|base| {
            let base: DefRef = base;
            get_root(&base)
                .into_flags_def()
                .expect("the root of a flags definition is a flags definition")
        });
        Self::new(
            base_flags_def,
            flg_names,
            flg_summaries,
            dont_shows.to_vec(),
            unknown_is_unused,
            ignore_mask,
            get_cps.to_vec(),
        )
    }

    fn new(
        flg_base_flags_def: Option<Arc<FlagsDef>>,
        flg_names: Vec<String>,
        flg_summaries: Vec<String>,
        dont_shows: Vec<Option<DontShowCallback>>,
        flg_unknown_is_unused: bool,
        flg_ignore_mask: i64,
        flg_get_cps: Vec<Option<GetConflictPriority>>,
    ) -> Arc<Self> {
        let mut flg_unused_mask: i64 = if flg_unknown_is_unused { !0 } else { 0 };
        for (i, name) in flg_names.iter().enumerate() {
            if name.eq_ignore_ascii_case("Unused") {
                flg_unused_mask |= 1i64 << i;
            } else if flg_unknown_is_unused && !name.is_empty() {
                flg_unused_mask &= !(1i64 << i);
            }
        }
        let (def, nd) = integer_def_formater_create("TwbFlagsDef");
        let this = Arc::new_cyclic(|self_ref: &Weak<Self>| Self {
            self_ref: self_ref.clone(),
            def,
            nd,
            flg_base_flags_def,
            flg_flag_defs: flg_names.iter().map(|_| OnceLock::new()).collect(),
            flg_names,
            flg_summaries,
            flg_dont_shows: DefCell::new(Some(dont_shows)),
            flg_unused_mask,
            flg_ignore_mask,
            flg_unknown_is_unused,
            flg_get_cps,
            flg_dont_show_path: DefCell::default(),
            flg_deleted_index: AtomicI8::new(-1),
            flg_partial_form_index: AtomicI8::new(-1),
            unknown_flags: std::array::from_fn(|_| AtomicI32::new(0)),
        });
        DefBase::after_construction(&*this);
        this
    }

    /// Port of `TwbFlagsDef.Clone` and `AfterClone`.
    pub fn clone_from(source: &Self) -> Arc<Self> {
        let this = Self::new(
            source.flg_base_flags_def.clone(),
            source.flg_names.clone(),
            source.flg_summaries.clone(),
            source.dont_shows(),
            source.flg_unknown_is_unused,
            source.flg_ignore_mask,
            source.flg_get_cps.clone(),
        );
        NamedDefBase::after_clone(&*this, source);
        this.flg_dont_show_path.assign(&source.flg_dont_show_path);
        this
    }

    fn dont_shows(&self) -> Vec<Option<DontShowCallback>> {
        self.flg_dont_shows.load().as_deref().cloned().unwrap_or_default()
    }

    fn dont_show_callback(&self, index: usize) -> Option<DontShowCallback> {
        self.flg_dont_shows
            .load()
            .as_deref()
            .and_then(|dont_shows| dont_shows.get(index).cloned().flatten())
    }

    fn is_record_flags(&self) -> bool {
        self.def.def_flags.contains(DefFlag::dfIsRecordFlags)
    }

    /// Upstream `GetBaseFlagsDef`.
    pub fn get_base_flags_def(&self) -> Arc<FlagsDef> {
        match &self.flg_base_flags_def {
            Some(base) => base.clone(),
            None => get_root(&self.def_ref())
                .into_flags_def()
                .expect("the root of a flags definition is a flags definition"),
        }
    }

    /// Upstream `GetFlag`. Fails when `index` is not below `get_flag_count`.
    pub fn get_flag(&self, index: usize, for_summary: bool) -> String {
        if for_summary {
            return self.flg_summaries[index].clone();
        }
        let mut result = self.flg_names[index].clone();
        if show_flag_enum_value() {
            result.push_str(&flag_value_suffix(index));
        }
        result
    }

    pub fn get_flag_count(&self) -> i32 {
        self.flg_names.len() as i32
    }

    pub fn get_flag_ignore_conflict(&self, index: i32) -> bool {
        (0..64).contains(&index) && self.flg_ignore_mask & (1i64 << index) != 0
    }

    /// Upstream `GetFlagDontShow`.
    pub fn get_flag_dont_show(&self, element: ElementArg, index: i32) -> bool {
        let mut result = false;
        if let Some(dont_show) = usize::try_from(index)
            .ok()
            .and_then(|index| self.dont_show_callback(index))
        {
            result = dont_show(element);
        }
        if let Some((path, invert)) = self.flg_dont_show_path.load().as_deref()
            && !path.is_empty()
        {
            let Some(element) = element else {
                return result;
            };
            let parent;
            let container = match element.as_container() {
                Some(container) => container,
                None => {
                    parent = element.get_container();
                    match parent.as_ref().and_then(|parent| parent.as_container()) {
                        Some(container) => container,
                        None => return result,
                    }
                }
            };
            let Some(mask) = container.get_element_native_value(path).as_ordinal() else {
                return result;
            };
            result = (0..64).contains(&index) && mask & (1i64 << index) != 0;
            if *invert {
                result = !result;
            }
        }
        result
    }

    pub fn get_flag_has_dont_show(&self, index: i32) -> bool {
        usize::try_from(index)
            .ok()
            .and_then(|index| self.dont_show_callback(index))
            .is_some()
    }

    /// Upstream `FlagGetCP`.
    pub fn flag_get_cp(&self, element: ElementArg, index: i32, cp: &mut ConflictPriority) {
        if let Some(Some(get_cp)) = usize::try_from(index)
            .ok()
            .and_then(|index| self.flg_get_cps.get(index))
        {
            get_cp(element, cp);
        }
        let deleted = i32::from(self.flg_deleted_index.load(Ordering::Relaxed));
        let partial = i32::from(self.flg_partial_form_index.load(Ordering::Relaxed));
        if self.is_record_flags()
            && index != deleted
            && index != partial
            && let Some(element) = element
        {
            let parent;
            let value = match element.as_container() {
                Some(_) => element.get_native_value(),
                None => {
                    parent = element.get_container();
                    match &parent {
                        Some(parent) if parent.as_container().is_some() => parent.get_native_value(),
                        _ => return,
                    }
                }
            };
            let Some(flags_value) = value.as_ordinal() else {
                return;
            };
            if self.deleted_or_partial(flags_value) {
                *cp = ConflictPriority::cpIgnore;
            }
        }
    }

    fn deleted_or_partial(&self, flags_value: i64) -> bool {
        let is_set = |index: &AtomicI8| {
            let index = index.load(Ordering::Relaxed);
            index >= 0 && flags_value & (1i64 << index) != 0
        };
        is_set(&self.flg_deleted_index) || is_set(&self.flg_partial_form_index)
    }

    pub fn get_flag_has_get_cp(&self, index: i32) -> bool {
        usize::try_from(index)
            .ok()
            .and_then(|index| self.flg_get_cps.get(index))
            .is_some_and(Option::is_some)
            || self.is_record_flags()
    }

    /// Upstream `GetFlagDef`: the definition of one flag, created on first use.
    pub fn get_flag_def(&self, index: i32) -> Option<Arc<FlagDef>> {
        let index = usize::try_from(index).ok()?;
        let slot = self.flg_flag_defs.get(index)?;
        Some(
            slot.get_or_init(|| {
                let flag_def = FlagDef::create(
                    NamedDefArgs {
                        priority: self.def.def_priority,
                        required: false,
                        name: self.flg_names[index].clone(),
                        after_load: None,
                        after_set: None,
                        dont_show: None,
                        get_cp: None,
                        terminator: false,
                    },
                    index as i32,
                );
                let parent: Weak<dyn Def> = self.self_ref.clone();
                set_parent(flag_def, &parent, false)
            })
            .clone(),
        )
    }

    /// Upstream `FindFlag`: by name, by mask (`$4` or `0x4`) or by index.
    pub fn find_flag(&self, name: &str) -> Option<Arc<FlagDef>> {
        if name.is_empty() {
            return None;
        }
        if let Some(index) = self.flg_names.iter().position(|flag| flag.eq_ignore_ascii_case(name)) {
            return self.get_flag_def(index as i32);
        }
        let name = match name.strip_prefix("0x") {
            Some(digits) => format!("${digits}"),
            None => name.to_owned(),
        };
        if name.starts_with('$') {
            let mut mask = i64::from(str_to_int_def(&name, 0));
            let mut index = 0;
            while mask != 0 && index < self.get_flag_count() {
                if mask == 1 {
                    return self.get_flag_def(index);
                }
                index += 1;
                mask = ((mask as u64) >> 1) as i64;
            }
        }
        let index = str_to_int_def(&name, -1);
        if index >= 0 && index < self.get_flag_count() {
            return self.get_flag_def(index);
        }
        None
    }

    /// Port of `SetDontShowMaskPath`.
    pub fn set_dont_show_mask_path(self: Arc<Self>, path: &str, invert: bool) -> Arc<Self> {
        let this = if self.def.def_is_locked() {
            Self::clone_from(&self)
        } else {
            self
        };
        this.flg_dont_show_path.set(Some((path.to_owned(), invert)));
        this
    }

    /// Port of `SetFlagHasDontShow`.
    pub fn set_flag_has_dont_show(self: Arc<Self>, index: i32, dont_show: Option<DontShowCallback>) -> Arc<Self> {
        let index = usize::try_from(index).expect("a non-negative index");
        let this = if self.def.def_is_locked() {
            Self::clone_from(&self)
        } else {
            self
        };
        let mut dont_shows = this.dont_shows();
        if dont_show.is_some() && dont_shows.len() <= index {
            dont_shows.resize(index + 1, None);
        }
        if let Some(slot) = dont_shows.get_mut(index) {
            *slot = dont_show;
        }
        if dont_shows.iter().all(Option::is_none) {
            dont_shows.clear();
        }
        this.flg_dont_shows.set(Some(dont_shows));
        this
    }

    /// Number of times the unnamed flag `index` was shown in report mode.
    pub fn unknown_flag_count(&self, index: usize) -> i32 {
        self.unknown_flags[index].load(Ordering::Relaxed)
    }

    fn name_or_empty(&self, index: usize) -> &str {
        self.flg_names.get(index).map_or("", String::as_str)
    }
}

impl Def for FlagsDef {
    formater_plumbing!();

    fn get_def_type(&self) -> DefType {
        DefType::dtIntegerFormater
    }

    fn get_def_type_name(&self) -> String {
        if self.flg_names.is_empty() {
            "TwbFlagsDef".to_owned()
        } else {
            format!("({})", self.flg_names.join(","))
        }
    }

    fn as_flags_def(&self) -> Option<&FlagsDef> {
        Some(self)
    }

    fn into_flags_def(self: Arc<Self>) -> Option<Arc<FlagsDef>> {
        Some(self)
    }

    fn get_child_pos(&self, child: &dyn Def) -> i32 {
        self.flg_flag_defs
            .iter()
            .position(|slot| slot.get().is_some_and(|flag_def| child.equals(Some(&**flag_def))))
            .map_or(-1, |index| index as i32)
    }

    fn get_conflict_priority(&self, element: ElementArg) -> ConflictPriority {
        let mut result = self.def.def_priority;
        if let Some(get_cp) = &self.def.def_get_cp {
            get_cp(element, &mut result);
        }
        let Some(element) = element else {
            return result;
        };
        if element.get_element_type() == ElementType::etFlag {
            let def = element.get_def();
            if let Some(flag_def) = def.as_deref().and_then(|def| def.as_flag_def()) {
                self.flag_get_cp(Some(element), flag_def.get_flag_index(), &mut result);
            }
            return result;
        }
        if self.is_record_flags()
            && result == ConflictPriority::cpNormal
            && let Some(flags_value) = element.get_native_value().as_ordinal()
            && self.deleted_or_partial(flags_value)
        {
            result = ConflictPriority::cpNormalIgnoreEmpty;
        }
        result
    }

    fn get_conflict_priority_can_change(&self) -> bool {
        self.def.def_get_cp.is_some() || self.is_record_flags()
    }

    fn init_from_parent_do_children(&self) {
        for slot in &self.flg_flag_defs {
            if let Some(flag_def) = slot.get() {
                flag_def.init_from_parent();
            }
        }
        if self.is_record_flags() {
            if let Some(flag_def) = self.find_flag("Deleted") {
                self.flg_deleted_index
                    .store(flag_def.get_flag_index() as i8, Ordering::Relaxed);
            }
            if let Some(flag_def) = self.find_flag("Partial Form") {
                self.flg_partial_form_index
                    .store(flag_def.get_flag_index() as i8, Ordering::Relaxed);
            }
        }
    }
}

formater_impls!(FlagsDef);

impl IntegerDefFormater for FlagsDef {
    fn to_string(&self, int: i64, element: ElementArg, for_summary: bool) -> String {
        let int = int & !self.flg_unused_mask;
        let mut shown = Vec::new();
        for i in 0..64usize {
            if int & (1i64 << i) == 0 {
                continue;
            }
            let mut s = match (self.flg_names.get(i), for_summary) {
                (Some(_), true) => self.flg_summaries[i].clone(),
                (Some(name), false) => name.clone(),
                (None, _) => String::new(),
            };
            if s.is_empty() {
                s = if for_summary {
                    format!("<{i}>")
                } else {
                    get_unknown_int_string(i as i64)
                };
                if report_mode() && report_unknown_flags() {
                    self.unknown_flags[i].fetch_add(1, Ordering::Relaxed);
                }
            }
            if !for_summary && show_flag_enum_value() {
                s.push_str(&flag_value_suffix(i));
            }
            if !self.get_flag_dont_show(element, i as i32) {
                shown.push(s);
            }
        }
        let result = shown.join(", ");
        self.used(element, &result);
        result
    }

    fn to_sort_key(&self, int: i64, element: ElementArg) -> String {
        let int = int & !self.flg_unused_mask;
        (0..64)
            .map(|i| {
                if int & (1i64 << i) != 0 && !self.get_flag_dont_show(element, i) {
                    '1'
                } else {
                    '0'
                }
            })
            .collect()
    }

    fn check(&self, int: i64, _element: ElementArg) -> String {
        if self.flg_unknown_is_unused {
            return String::new();
        }
        (0..64usize)
            .filter(|&i| int & (1i64 << i) != 0 && self.name_or_empty(i).is_empty())
            .map(|i| get_unknown_int_string(i as i64))
            .collect::<Vec<_>>()
            .join(", ")
    }

    fn get_edit_type(&self, _element: ElementArg) -> EditType {
        EditType::etCheckComboBox
    }

    fn get_edit_info(&self, element: ElementArg) -> Vec<String> {
        let mut flag_count = 64;
        if let Some(element) = element {
            let def = element.get_def();
            let value_def = element.get_value_def();
            let integer_def = def
                .as_deref()
                .and_then(|def| def.as_integer_def())
                .or_else(|| value_def.as_deref().and_then(|def| def.as_integer_def()));
            if let Some(integer_def) = integer_def {
                flag_count = match integer_def.get_int_type() {
                    IntType::it0 => 0,
                    IntType::itU8 | IntType::itS8 => 8,
                    IntType::itU16 | IntType::itS16 => 16,
                    IntType::itU32 | IntType::itS32 => 32,
                    _ => 64,
                };
            }
        }
        (0..flag_count)
            .map(|i| {
                let mut s = self.name_or_empty(i).to_owned();
                if s.is_empty() {
                    s = if self.flg_unknown_is_unused {
                        "Unused".to_owned()
                    } else {
                        get_unknown_int_string(i as i64)
                    };
                }
                if self.get_flag_dont_show(element, i as i32) {
                    s = format!("<Unknown: {i}>");
                }
                if show_flag_enum_value() {
                    s.push_str(&flag_value_suffix(i));
                }
                s
            })
            .collect()
    }

    fn to_edit_value(&self, int: i64, _element: ElementArg) -> String {
        let int = int & !self.flg_unused_mask;
        if int == 0 {
            return "0".repeat(64);
        }
        let highest = 63 - int.leading_zeros() as usize;
        (0..=highest)
            .map(|i| if int & (1i64 << i) != 0 { '1' } else { '0' })
            .collect()
    }

    fn get_is_editable(&self, _int: i64, _element: ElementArg) -> bool {
        editable_unless_internal_only(&self.def)
    }

    fn get_requires_key(&self) -> bool {
        true
    }
}

/// Upstream `TwbFlagDef`: one flag of a `TwbFlagsDef`, shown as an element of
/// its own.
pub struct FlagDef {
    self_ref: Weak<FlagDef>,
    def: DefBase,
    nd: NamedDefBase,
    vd: ValueDefBase,
    fd_flag_index: i32,
}

impl FlagDef {
    pub fn create(args: NamedDefArgs, flag_index: i32) -> Arc<Self> {
        let (def, nd) = NamedDefBase::create(args);
        let this = Arc::new_cyclic(|self_ref: &Weak<Self>| Self {
            self_ref: self_ref.clone(),
            def,
            nd,
            vd: ValueDefBase::default(),
            fd_flag_index: flag_index,
        });
        DefBase::after_construction(&*this);
        this
    }

    pub fn clone_from(source: &Self) -> Arc<Self> {
        let this = Self::create(NamedDefBase::clone_args(source), source.fd_flag_index);
        ValueDefBase::after_clone(&*this, source);
        this
    }

    pub fn get_flag_index(&self) -> i32 {
        self.fd_flag_index
    }

    /// Upstream `GetFlagsDef`: the parent. Fails when the flag has no parent.
    pub fn get_flags_def(&self) -> Arc<FlagsDef> {
        self.def
            .def_parent()
            .and_then(|parent| parent.into_flags_def())
            .expect("a flag definition belongs to a flags definition")
    }
}

impl Def for FlagDef {
    value_def_plumbing!(Def);

    fn get_def_type(&self) -> DefType {
        DefType::dtFlag
    }

    fn get_def_type_name(&self) -> String {
        "FlagDef".to_owned()
    }

    fn as_flag_def(&self) -> Option<&FlagDef> {
        Some(self)
    }

    fn get_has_dont_show(&self) -> bool {
        self.get_flags_def().get_flag_has_dont_show(self.fd_flag_index)
    }

    fn get_dont_show(&self, element: ElementArg) -> bool {
        self.get_flags_def().get_flag_dont_show(element, self.fd_flag_index)
    }

    fn get_conflict_priority(&self, element: ElementArg) -> ConflictPriority {
        let flags_def = self.get_flags_def();
        let mut result = if flags_def.get_flag_ignore_conflict(self.fd_flag_index) {
            ConflictPriority::cpIgnore
        } else {
            ConflictPriority::cpNormal
        };
        flags_def.flag_get_cp(element, self.fd_flag_index, &mut result);
        result
    }

    fn get_conflict_priority_can_change(&self) -> bool {
        self.get_flags_def().get_flag_has_get_cp(self.fd_flag_index)
    }
}

impl NamedDef for FlagDef {
    value_def_plumbing!(NamedDef);
}

impl ValueDef for FlagDef {
    value_def_plumbing!(ValueDef);

    /// Upstream asserts that this is never called.
    fn to_string(&self, data: DataPtr, element: ElementArg) -> String {
        debug_assert!(false, "TwbFlagDef.ToString");
        let mut result = String::new();
        if let Some(to_str) = self.nd.nd_to_str.load().as_deref() {
            to_str(&mut result, data, element, CallbackType::ctToStr);
        }
        result
    }

    fn get_size(&self, _data: DataPtr, _element: ElementArg) -> i32 {
        0
    }

    fn get_default_size(&self, _data: DataPtr, _element: ElementArg) -> i32 {
        0
    }

    fn get_can_be_zero_size(&self) -> bool {
        true
    }
}

impl DefKind for FlagDef {
    fn duplicate_same(&self) -> Arc<Self> {
        Self::clone_from(self)
    }
}

#[cfg(test)]
mod tests {
    use super::super::def::DefSetters;
    use super::super::globals::{set_show_flag_enum_value, test_lock};
    use super::*;

    fn sample(unknown_is_unused: bool) -> Arc<FlagsDef> {
        FlagsDef::create(
            false,
            None,
            &["ESM", "", "Unused", "Deleted", "", "Localized"],
            &[],
            unknown_is_unused,
            0b1000,
            &[],
        )
    }

    #[test]
    fn names_of_set_bits() {
        let _guard = test_lock();
        let def = sample(false);
        assert_eq!(def.to_string(0b10_0001, None, false), "ESM, Localized");
        // Bit 2 is named Unused and never shows. Bit 1 has no name.
        assert_eq!(def.to_string(0b111, None, false), "ESM, <Unknown: 1 $1>");
        assert_eq!(def.to_string(0b10, None, true), "<1>");
        assert_eq!(def.to_string(0, None, false), "");
        assert_eq!(def.check(0b1_0010, None), "<Unknown: 1 $1>, <Unknown: 4 $4>");
        assert_eq!(def.check(1, None), "");
        assert_eq!(def.to_edit_value(0b10_0101, None), "100001");
        assert_eq!(def.to_edit_value(0, None), "0".repeat(64));
        let key = def.to_sort_key(0b10_0101, None);
        assert_eq!(key.len(), 64);
        assert!(key.starts_with("100001000"));
        assert!(def.get_requires_key());
        assert_eq!(def.get_def_type_name(), "(ESM,,Unused,Deleted,,Localized)");
        assert_eq!(def.get_edit_info(None).len(), 64);
        assert_eq!(def.get_edit_info(None)[1], "<Unknown: 1 $1>");
    }

    #[test]
    fn unknown_is_unused_hides_unnamed_bits() {
        let _guard = test_lock();
        let def = sample(true);
        assert_eq!(def.to_string(-1, None, false), "ESM, Deleted, Localized");
        assert_eq!(def.check(-1, None), "");
        assert_eq!(def.get_edit_info(None)[1], "Unused");
    }

    #[test]
    fn summaries_and_values() {
        let _guard = test_lock();
        let def = FlagsDef::create(true, None, &["Master", "M", "Light", ""], &[], false, 0, &[]);
        assert_eq!(def.to_string(0b11, None, true), "M, Light");
        assert_eq!(def.get_flag(0, true), "M");
        set_show_flag_enum_value(true);
        assert_eq!(
            def.to_string(0b11, None, false),
            "Master (0x00000001), Light (0x00000002)"
        );
        assert_eq!(def.get_flag(1, false), "Light (0x00000002)");
        let copy = FlagsDef::clone_from(&def);
        assert_eq!(copy.to_string(0b01, None, true), "M");
    }

    #[test]
    fn dont_show_callbacks() {
        let _guard = test_lock();
        let hide: DontShowCallback = Arc::new(|_| true);
        let def = FlagsDef::create(
            false,
            None,
            &["A", "B", "C"],
            &[None, Some(hide.clone())],
            false,
            0,
            &[],
        );
        assert_eq!(def.to_string(0b111, None, false), "A, C");
        assert!(def.get_flag_has_dont_show(1) && !def.get_flag_has_dont_show(0));
        assert!(def.to_sort_key(0b111, None).starts_with("101"));
        let def = def.set_flag_has_dont_show(1, None);
        assert_eq!(def.to_string(0b111, None, false), "A, B, C");
        let def = def.set_flag_has_dont_show(2, Some(hide));
        assert_eq!(def.to_string(0b111, None, false), "A, B");
    }

    #[test]
    fn flag_definitions() {
        let _guard = test_lock();
        let def = sample(false).include_flag(DefFlag::dfIsRecordFlags);
        def.init_from_parent();
        let deleted = def.find_flag("deleted").unwrap();
        assert_eq!(deleted.get_flag_index(), 3);
        assert_eq!(deleted.get_name(), "Deleted");
        assert!(Arc::ptr_eq(&deleted.get_flags_def(), &def));
        assert!(Arc::ptr_eq(&def.get_flag_def(3).unwrap(), &deleted));
        assert_eq!(def.get_child_pos(&*deleted), 3);
        assert_eq!(def.find_flag("$20").unwrap().get_flag_index(), 5);
        assert_eq!(def.find_flag("0x1").unwrap().get_flag_index(), 0);
        assert_eq!(def.find_flag("4").unwrap().get_flag_index(), 4);
        assert!(def.find_flag("9").is_none() && def.find_flag("").is_none());
        assert!(def.get_flag_def(6).is_none());
        // The ignore mask covers bit 3.
        assert_eq!(deleted.get_conflict_priority(None), ConflictPriority::cpIgnore);
        assert_eq!(
            def.get_flag_def(0).unwrap().get_conflict_priority(None),
            ConflictPriority::cpNormal
        );
        assert!(deleted.get_conflict_priority_can_change());
        assert_eq!(deleted.get_def_type(), DefType::dtFlag);
        assert_eq!(deleted.get_path(), "TwbFlagsDef \\ Deleted");
        assert!(Arc::ptr_eq(&def.get_base_flags_def(), &def));
    }
}
