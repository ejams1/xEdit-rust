// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

// Ported from xEdit: xEdit/xeMainForm.pas (PerformDrop, mniViewMoveUpClick,
// mniViewMoveDownClick) and xEdit/JvI/xejviScriptAdapterElement.pas
// (ElementAssign, AssignTemplateByName, AssignTemplateCount,
// TemplateAssign, MoveUp, MoveDown) and xejviScriptAdapterContainer.pas
// (InsertElement)

//! The element commands of the view and of the script API that take one
//! element from somewhere else or move one: `elements.assign` (the view's
//! drag and drop, `TwbElement.Assign` with an element of another record or
//! a template of the definitions as its source, at `wbAssignThis`,
//! `wbAssignAdd` or a position, which for an array the view aligned is the
//! aligned position), `elements.templates` (`GetAssignTemplates`) and
//! `elements.move` (Move Up/Move Down, and `InsertElement` at a position).

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use xedit_core::implementation::copy::{add_required_masters_of, missing_masters, required_masters};
use xedit_core::implementation::{moving, template};
use xedit_core::interface::types::{ASSIGN_ADD, ASSIGN_THIS};
use xedit_core::interface::{Element, ElementRef, MainRecordRef};

use crate::commands::{ElementNode, node_of};
use crate::edit::with_edit_allowed;
use crate::{CommandError, Registry, Session};

/// The `aIndex` of an assign: `this` (`wbAssignThis`: the element takes the
/// value of the source), `add` (`wbAssignAdd`: an entry added to an array),
/// or the position of a member of a record or structure or, in an array the
/// view aligned, the aligned position.
#[derive(Clone, Copy, Debug, Deserialize, Serialize, JsonSchema)]
#[serde(untagged)]
pub enum AssignIndex {
    Keyword(AssignKeyword),
    Position(i32),
}

/// The named indexes of an assign.
#[derive(Clone, Copy, Debug, Deserialize, Serialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum AssignKeyword {
    This,
    Add,
}

impl Default for AssignIndex {
    fn default() -> Self {
        AssignIndex::Keyword(AssignKeyword::This)
    }
}

impl AssignIndex {
    fn value(self) -> i32 {
        match self {
            AssignIndex::Keyword(AssignKeyword::This) => ASSIGN_THIS,
            AssignIndex::Keyword(AssignKeyword::Add) => ASSIGN_ADD,
            AssignIndex::Position(position) => position,
        }
    }
}

/// An element of a record, as a source.
#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ElementSource {
    /// Load order FormID of the record as hexadecimal digits.
    pub form_id: String,
    /// Plugin the record is seen from; the last loaded plugin when omitted.
    pub file: Option<String>,
    /// Path of the element inside the record, with `\` between the names;
    /// the record itself when omitted.
    pub path: Option<String>,
}

/// The element a path names in a record (the record for an empty path).
fn element_at(record: &MainRecordRef, path: Option<&str>) -> Result<ElementRef, CommandError> {
    let record_ref: ElementRef = record.clone();
    match path.filter(|path| !path.is_empty()) {
        Some(path) => record.get_element_by_path(path).ok_or_else(|| {
            CommandError::new(
                "unknown_element",
                format!("{} has no element at {path}", record.get_name()),
            )
        }),
        None => Ok(record_ref),
    }
}

/// The missing last element of `path` added to the record (`Add`), when
/// the container above it is there and its definition has the member.
fn add_missing_member(record: &MainRecordRef, path: &str) -> Option<ElementRef> {
    let path = crate::commands::member_path_for_add(path);
    let (parent, name) = match path.rsplit_once('\\') {
        Some((parent, name)) => (element_at(record, Some(parent)).ok()?, name.to_owned()),
        None => (record.clone() as ElementRef, path.clone()),
    };
    let container = parent.as_container()?;
    container.add(&name, true).ok().flatten()
}

/// `elements.assign`: the request.
#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ElementsAssignRequest {
    /// Load order FormID of the record to change as hexadecimal digits.
    pub form_id: String,
    /// Plugin whose own version of the record is changed (`not_in_file`
    /// when it has none); the version the last loaded plugin sees when
    /// omitted.
    pub file: Option<String>,
    /// Path of the element to assign to, with `\` between the names; the
    /// record itself when omitted.
    pub path: Option<String>,
    /// Where the source goes: `"this"` (the default: the element takes the
    /// source's value), `"add"` (a new entry of an array) or a position (a
    /// member of a record or structure; in an array the view aligned,
    /// `records.compare` before, an aligned position, where a new entry is
    /// made when the array has none).
    #[serde(default)]
    pub index: AssignIndex,
    /// The element to assign from, of any loaded record. Its masters are
    /// added to the plugin first, as the view's drop does.
    pub source: Option<ElementSource>,
    /// The name of a template of the definitions to assign from in place of
    /// an element (`elements.templates` lists them): the element of that
    /// definition is made without data. With `"index": "add"` this is the
    /// script function `TemplateAssign`, which sets an element made empty
    /// to its default.
    pub template: Option<String>,
    /// Assign only the members that make the sort key (`aOnlySK`).
    #[serde(default)]
    pub only_sk: bool,
    /// Report whether the assign can be done (`CanAssign`), but change
    /// nothing.
    #[serde(default)]
    pub dry_run: bool,
}

/// `elements.assign`: the response.
#[derive(Serialize, JsonSchema)]
pub struct ElementsAssignResponse {
    /// Full path of the element that took the source; of the target for a
    /// dry run or an assign that gave no element.
    pub path: String,
    /// The element that took the source, when the assign gives one (an
    /// assign of a whole array with `this` gives none, as upstream).
    pub element: Option<ElementNode>,
    /// Whether the target can take the source at the index (`CanAssign`).
    pub can_assign: bool,
    /// The masters the source needs that the plugin lacks, which the
    /// assign adds.
    pub missing_masters: Vec<String>,
    /// The plugin that now holds a change to save.
    pub file: String,
}

fn elements_assign(
    session: &mut Session,
    request: ElementsAssignRequest,
) -> Result<ElementsAssignResponse, CommandError> {
    crate::commands::refuse_in_translate_mode("elements.assign")?;
    if request.source.is_some() && request.template.is_some() {
        return Err(CommandError::new(
            "invalid_params",
            "give a source or a template, not both",
        ));
    }
    let record = session.record_in_file(&request.form_id, request.file.as_deref())?;
    let index = request.index.value();
    let target = match element_at(&record, request.path.as_deref()) {
        Ok(target) => target,
        // A member the record lacks takes the source as the view's drop on
        // its empty row does: the member is added first (`Add`, by the
        // signature of a `SIG - Name` path), then assigned.
        Err(error) if !request.dry_run && index == ASSIGN_THIS && request.source.is_some() => {
            add_missing_member(&record, request.path.as_deref().unwrap_or_default()).ok_or(error)?
        }
        Err(error) => return Err(error),
    };
    let file = record.get_file().map(|file| file.get_name()).unwrap_or_default();
    let mut missing = Vec::new();
    let source = match (&request.source, &request.template) {
        (Some(source), None) => {
            let source_record = session.record(&source.form_id, source.file.as_deref())?;
            let element = element_at(&source_record, source.path.as_deref())?;
            let target_file = session.file(Some(&file))?;
            let required = required_masters(&element, false);
            missing = missing_masters(&required, &target_file)
                .map_err(|error| CommandError::new("edit_failed", error))?
                .iter()
                .map(|master| master.get_name())
                .collect();
            Some((element, Some((required, target_file))))
        }
        (None, Some(name)) => {
            let templates = template::assign_templates(&target, index);
            let found = templates
                .iter()
                .find(|candidate| candidate.get_name().eq_ignore_ascii_case(name))
                .cloned()
                .ok_or_else(|| {
                    let names: Vec<String> = templates.iter().map(|candidate| candidate.get_name()).collect();
                    CommandError::new(
                        "unknown_template",
                        format!(
                            "{} has no template {name} at this index; it has: {}",
                            target.get_full_path(),
                            names.join(", ")
                        ),
                    )
                })?;
            Some((found, None))
        }
        _ => None,
    };
    let source_element = source.as_ref().map(|(element, _)| element.clone());
    let can_assign = with_edit_allowed(|| target.can_assign(index, source_element.as_ref(), false));
    if request.dry_run {
        return Ok(ElementsAssignResponse {
            path: target.get_full_path(),
            element: None,
            can_assign,
            missing_masters: missing,
            file,
        });
    }
    if let Some((_, Some((required, target_file)))) = &source {
        add_required_masters_of(required, target_file).map_err(|error| CommandError::new("edit_failed", error))?;
    }
    // UPSTREAM-QUIRK: `Assign` gives nil for some assigns that changed the
    // target (`wbAssignThis` on an array value, a union member switched),
    // so a result without an element is not an error.
    let assigned = target.assign(index, source_element.as_ref(), request.only_sk);
    // `TemplateAssign`: the element a template made empty takes its default.
    if let Some(assigned) = &assigned
        && request.template.is_some()
        && index == ASSIGN_ADD
    {
        template::set_to_default_if_as_created_empty(assigned)
            .map_err(|error| CommandError::new("edit_failed", error))?;
    }
    Ok(ElementsAssignResponse {
        path: assigned.as_ref().unwrap_or(&target).get_full_path(),
        element: assigned.as_ref().map(|assigned| node_of(assigned, Some(1))),
        can_assign,
        missing_masters: missing,
        file,
    })
}

/// `elements.templates`: the request.
#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ElementsTemplatesRequest {
    /// Load order FormID of the record as hexadecimal digits.
    pub form_id: String,
    /// Plugin the record is seen from; the last loaded plugin when omitted.
    pub file: Option<String>,
    /// Path of the element, with `\` between the names; the record itself
    /// when omitted.
    pub path: Option<String>,
    /// The index the templates are for, as `elements.assign` takes it
    /// (`"add"` gives the entries an array can add).
    #[serde(default)]
    pub index: AssignIndex,
}

/// `elements.templates`: the response.
#[derive(Serialize, JsonSchema)]
pub struct ElementsTemplatesResponse {
    /// Full path of the element.
    pub path: String,
    /// The names of the templates, in the order of the definitions.
    pub templates: Vec<String>,
}

fn elements_templates(
    session: &mut Session,
    request: ElementsTemplatesRequest,
) -> Result<ElementsTemplatesResponse, CommandError> {
    let record = session.record(&request.form_id, request.file.as_deref())?;
    let element = element_at(&record, request.path.as_deref())?;
    let templates = template::assign_templates(&element, request.index.value())
        .iter()
        .map(|template| template.get_name())
        .collect();
    Ok(ElementsTemplatesResponse {
        path: element.get_full_path(),
        templates,
    })
}

/// The ways `elements.move` moves an element.
#[derive(Clone, Copy, Debug, Deserialize, Serialize, JsonSchema, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum MoveDirection {
    Up,
    Down,
}

/// `elements.move`: the request.
#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ElementsMoveRequest {
    /// Load order FormID of the record as hexadecimal digits.
    pub form_id: String,
    /// Plugin whose own version of the record is changed (`not_in_file`
    /// when it has none); the version the last loaded plugin sees when
    /// omitted.
    pub file: Option<String>,
    /// Path of the element to move, with `\` between the names.
    pub path: String,
    /// One place up or down in its container, as the view's Move Up and
    /// Move Down: only an entry of an array that is not sorted moves.
    pub direction: Option<MoveDirection>,
    /// In place of `direction`: the element leaves its container and goes
    /// back in at this position, as `RemoveElement` and `InsertElement` of
    /// the script API do it (the position is kept within the elements).
    pub position: Option<i32>,
    /// Report whether the element can move, but move nothing.
    #[serde(default)]
    pub dry_run: bool,
}

/// `elements.move`: the response.
#[derive(Serialize, JsonSchema)]
pub struct ElementsMoveResponse {
    /// Full path of the element after the move (before it for a dry run).
    pub path: String,
    /// Whether it can move up (`CanMoveUp`), before the move.
    pub can_move_up: bool,
    /// Whether it can move down (`CanMoveDown`), before the move.
    pub can_move_down: bool,
    /// Whether the element moved.
    pub moved: bool,
    /// The plugin that now holds a change to save.
    pub file: String,
}

fn elements_move(session: &mut Session, request: ElementsMoveRequest) -> Result<ElementsMoveResponse, CommandError> {
    crate::commands::refuse_in_translate_mode("elements.move")?;
    let record = session.record_in_file(&request.form_id, request.file.as_deref())?;
    let element = element_at(&record, Some(&request.path))?;
    let file = record.get_file().map(|file| file.get_name()).unwrap_or_default();
    let can_move_up = with_edit_allowed(|| moving::can_move_up(&element));
    let can_move_down = with_edit_allowed(|| moving::can_move_down(&element));
    let (direction, position) = match (request.direction, request.position) {
        (Some(direction), None) => (Some(direction), None),
        (None, Some(position)) => (None, Some(position)),
        _ => {
            return Err(CommandError::new("invalid_params", "give a direction or a position"));
        }
    };
    let moved = match direction {
        Some(MoveDirection::Up) => can_move_up,
        Some(MoveDirection::Down) => can_move_down,
        None => true,
    };
    if request.dry_run {
        return Ok(ElementsMoveResponse {
            path: element.get_full_path(),
            can_move_up,
            can_move_down,
            moved: false,
            file,
        });
    }
    match (direction, position) {
        (Some(MoveDirection::Up), _) => moving::move_up(&element),
        (Some(MoveDirection::Down), _) => moving::move_down(&element),
        (None, Some(position)) => {
            let container = element.get_container().ok_or_else(|| {
                CommandError::new(
                    "invalid_params",
                    format!("{} has no container", element.get_full_path()),
                )
            })?;
            let container_ref = container.as_container().ok_or_else(|| {
                CommandError::new("internal", format!("{} is not a container", container.get_full_path()))
            })?;
            let index = container_ref.get_index_of(&element);
            let removed = container_ref.remove_element_at(index, true).ok_or_else(|| {
                CommandError::new(
                    "not_removable",
                    format!("{} can not be removed", element.get_full_path()),
                )
            })?;
            moving::insert_element(&container, position, &removed)
                .map_err(|error| CommandError::new("edit_failed", error))?;
        }
        (None, None) => {}
    }
    Ok(ElementsMoveResponse {
        path: element.get_full_path(),
        can_move_up,
        can_move_down,
        moved,
        file,
    })
}

/// Adds the element assign and move commands to the registry.
pub fn register(registry: &mut Registry) {
    registry.register(
        "elements.assign",
        "Assign an element of any loaded record, or a template of the definitions, to an element at an index (the view's drop, ElementAssign, TemplateAssign).",
        true,
        elements_assign,
    );
    registry.register(
        "elements.templates",
        "List the templates an element offers for an assign at an index (GetAssignTemplates).",
        false,
        elements_templates,
    );
    registry.register(
        "elements.move",
        "Move an entry of an unsorted array up or down (the view's Move Up and Move Down), or to a position (InsertElement).",
        true,
        elements_move,
    );
}
