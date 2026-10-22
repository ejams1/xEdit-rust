// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

// Ported from xEdit: Core/wbImplementation.pas (TwbTemplateElement,
// TwbMultipleElements, wbMultipleElements, TwbElement.GetAssignTemplates)
// and Core/wbInterface.pas (GetAssignTemplates of TwbDef, TwbSubRecordDef,
// TwbSubRecordArrayDef, TwbSubRecordStructDef, TwbSubRecordUnionDef and
// TwbFlagsDef)

//! The sources of an assign that are not elements of a file: a template
//! element (`TwbTemplateElement`, an element of type `etTemplate` that is
//! only a definition, which the assign creates the element of without
//! taking any data from it), the templates an element offers at an index
//! (`GetAssignTemplates`: what the script functions `AssignTemplateByIndex`,
//! `AssignTemplateByName`, `AssignTemplateCount` and `TemplateAssign` read),
//! and a list of elements assigned one by one (`TwbMultipleElements`,
//! `wbMultipleElements`).
//!
//! UPSTREAM-QUIRK: nothing in the 4.1.5q GUI or script API makes an
//! `IwbMultipleElements` (`wbMultipleElements` is exported by
//! `wbImplementation.pas` and called nowhere); the port keeps it for the
//! operation layer, where the assign branches of the subrecords take it.

use std::sync::Arc;

use crate::interface::def::{Def, NamedDef, ValueDef};
use crate::interface::element::{Element, ElementRef, FileRef, MainRecordRef};
use crate::interface::form_id::FormID;
use crate::interface::misc::Variant;
use crate::interface::types::{ASSIGN_ADD, ASSIGN_THIS, ConflictPriority, DefFlag, ElementType, TriBool};

/// Port of `TwbTemplateElement`: an element that is only a definition.
pub struct TemplateElement {
    def: Arc<dyn NamedDef>,
}

impl TemplateElement {
    /// Port of `TwbTemplateElement.Create(aDef)`: no container.
    pub fn new(def: Arc<dyn NamedDef>) -> Arc<Self> {
        Arc::new(TemplateElement { def })
    }
}

impl Element for TemplateElement {
    fn get_element_id(&self) -> usize {
        std::ptr::from_ref(self) as *const () as usize
    }

    /// Port of `TwbTemplateElement.GetName`.
    fn get_name(&self) -> String {
        self.def.get_name().to_owned()
    }

    /// The name of the template element without building it: the name of
    /// its definition.
    fn name_matches(&self, name: &str) -> bool {
        crate::interface::element::name_equals_ignore_case(self.def.get_name(), name)
    }

    /// `TwbElement.GetFullPath` of an element without a container.
    fn get_full_path(&self) -> String {
        format!(" \\ {}", self.get_name())
    }

    fn get_data_size(&self) -> i32 {
        0
    }

    fn get_masters_updated(&self) -> bool {
        false
    }

    fn add_referenced_from_id(&self, _form_id: FormID) {}

    fn get_edit_value(&self) -> String {
        String::new()
    }

    fn get_links_to(&self) -> Option<ElementRef> {
        None
    }

    /// Port of `TwbTemplateElement.GetElementType`.
    fn get_element_type(&self) -> ElementType {
        ElementType::etTemplate
    }

    /// Port of `TwbTemplateElement.GetDef`.
    fn get_def(&self) -> Option<Arc<dyn NamedDef>> {
        Some(self.def.clone())
    }

    /// Port of `TwbTemplateElement.GetValueDef`: the definition, when it is
    /// a value definition.
    fn get_value_def(&self) -> Option<Arc<dyn ValueDef>> {
        (self.def.clone() as Arc<dyn Def>).into_value_def()
    }

    fn get_native_value(&self) -> Variant {
        Variant::Empty
    }

    fn get_container(&self) -> Option<ElementRef> {
        None
    }

    fn get_memory_order(&self) -> i32 {
        i32::MIN
    }

    fn get_file(&self) -> Option<FileRef> {
        None
    }

    fn get_containing_main_record(&self) -> Option<MainRecordRef> {
        None
    }

    fn get_path(&self) -> String {
        self.get_name()
    }

    fn get_localized(&self) -> TriBool {
        TriBool::tbUnknown
    }

    fn get_conflict_priority(&self) -> ConflictPriority {
        ConflictPriority::cpNormal
    }

    fn get_dont_show(&self) -> bool {
        false
    }
}

/// Port of `TwbMultipleElements`: elements assigned as one source, each in
/// turn (`AssignInternal` of a subrecord array or of a subrecord with an
/// array value), and accepted when one of them is (`CanAssignInternal`).
pub struct MultipleElements {
    elements: Vec<ElementRef>,
}

impl MultipleElements {
    /// The elements of the source.
    pub fn elements(&self) -> &[ElementRef] {
        &self.elements
    }
}

/// Port of `wbMultipleElements`.
pub fn multiple_elements(elements: Vec<ElementRef>) -> Arc<MultipleElements> {
    Arc::new(MultipleElements { elements })
}

/// `Supports(aElement, IwbMultipleElements)`.
pub fn as_multiple_elements(element: &ElementRef) -> Option<&MultipleElements> {
    element.as_multiple_elements()
}

impl Element for MultipleElements {
    fn get_element_id(&self) -> usize {
        std::ptr::from_ref(self) as *const () as usize
    }

    fn get_name(&self) -> String {
        String::new()
    }

    fn get_full_path(&self) -> String {
        " \\ ".to_owned()
    }

    fn get_data_size(&self) -> i32 {
        0
    }

    fn get_masters_updated(&self) -> bool {
        false
    }

    fn add_referenced_from_id(&self, _form_id: FormID) {}

    fn get_edit_value(&self) -> String {
        String::new()
    }

    fn get_links_to(&self) -> Option<ElementRef> {
        None
    }

    /// UPSTREAM-QUIRK: `TwbMultipleElements` does not override the
    /// abstract `GetElementType` (an assertion upstream); nothing asks it
    /// on the paths that take one, so the port says `etValue`.
    fn get_element_type(&self) -> ElementType {
        ElementType::etValue
    }

    fn as_multiple_elements(&self) -> Option<&MultipleElements> {
        Some(self)
    }

    fn get_def(&self) -> Option<Arc<dyn NamedDef>> {
        None
    }

    fn get_value_def(&self) -> Option<Arc<dyn ValueDef>> {
        None
    }

    fn get_native_value(&self) -> Variant {
        Variant::Empty
    }

    fn get_container(&self) -> Option<ElementRef> {
        None
    }

    fn get_memory_order(&self) -> i32 {
        i32::MIN
    }

    fn get_file(&self) -> Option<FileRef> {
        None
    }

    fn get_containing_main_record(&self) -> Option<MainRecordRef> {
        None
    }

    fn get_path(&self) -> String {
        String::new()
    }

    fn get_localized(&self) -> TriBool {
        TriBool::tbUnknown
    }

    fn get_conflict_priority(&self) -> ConflictPriority {
        ConflictPriority::cpNormal
    }

    fn get_dont_show(&self) -> bool {
        false
    }
}

/// Port of the `GetAssignTemplates(aContainer, aIndex)` of the definitions:
/// the definitions an assign at `index` of an element with the definition
/// can create (`TwbDef`: the definition itself for `wbAssignThis`; a
/// subrecord array: its element's for `wbAssignAdd`; a subrecord structure:
/// its member's at a member index; a union: the decided member, or every
/// member, for `wbAssignThis`). None for a `dfDontAssign` definition.
pub fn def_assign_templates(
    def: &Arc<dyn NamedDef>,
    container: Option<&ElementRef>,
    index: i32,
) -> Vec<Arc<dyn NamedDef>> {
    if def.def_base().def_flags.contains(DefFlag::dfDontAssign) {
        return Vec::new();
    }
    if let Some(array) = def.as_sub_record_array_def() {
        if index == ASSIGN_ADD {
            let element = array.get_element().clone() as Arc<dyn NamedDef>;
            return def_assign_templates(&element, container, ASSIGN_THIS);
        }
    } else if let Some(structure) = def.as_sub_record_struct_def() {
        use crate::interface::sub_record_group::RecordDef;
        if index >= 0 && index < structure.get_member_count() {
            let member = structure.get_member(index as usize) as Arc<dyn NamedDef>;
            return def_assign_templates(&member, container, ASSIGN_THIS);
        }
    } else if let Some(union) = def.as_sub_record_union_def() {
        use crate::interface::sub_record_group::RecordDef;
        if index != ASSIGN_THIS {
            return Vec::new();
        }
        if let Some(decided) = union.decide(container) {
            return usize::try_from(decided)
                .ok()
                .filter(|&decided| decided < union.get_member_count() as usize)
                .map(|decided| vec![union.get_member(decided) as Arc<dyn NamedDef>])
                .unwrap_or_default();
        }
        return (0..union.get_member_count() as usize)
            .map(|member| union.get_member(member) as Arc<dyn NamedDef>)
            .collect();
    }
    // `TwbDef.GetAssignTemplates` (and the inherited ones of the subrecord
    // and the flags definitions, whose own bodies are commented out).
    if index == ASSIGN_THIS {
        vec![def.clone()]
    } else {
        Vec::new()
    }
}

/// Port of `TwbElement.GetAssignTemplates`: the templates of the definition,
/// else of the value definition when it is another one.
pub fn assign_templates(element: &ElementRef, index: i32) -> Vec<ElementRef> {
    let container = element.as_container().is_some().then(|| element.clone());
    if let Some(def) = element.get_def() {
        let templates = def_assign_templates(&def, container.as_ref(), index);
        if !templates.is_empty() {
            return templates
                .into_iter()
                .map(|def| TemplateElement::new(def) as ElementRef)
                .collect();
        }
    }
    let def = element.get_def();
    if let Some(value_def) = element.get_value_def() {
        let same = def.as_ref().is_some_and(|def| value_def.equals(Some(def.as_dyn_def())));
        if !same {
            let value_def = value_def as Arc<dyn NamedDef>;
            let templates = def_assign_templates(&value_def, container.as_ref(), index);
            return templates
                .into_iter()
                .map(|def| TemplateElement::new(def) as ElementRef)
                .collect();
        }
    }
    Vec::new()
}

/// Port of `SetToDefaultIfAsCreatedEmpty`: a container still holding only
/// the element it was created with (`csAsCreatedEmpty`) takes its default;
/// any other element is left.
pub fn set_to_default_if_as_created_empty(element: &ElementRef) -> Result<(), crate::interface::misc::EditError> {
    let as_created_empty = element
        .as_element_impl()
        .and_then(|element| element.container_base())
        .is_some_and(|base| base.cnt_as_created_empty.load(std::sync::atomic::Ordering::Relaxed));
    if as_created_empty {
        element.set_to_default()?;
    }
    Ok(())
}
