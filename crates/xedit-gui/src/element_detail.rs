// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

// Ported from xEdit: xEdit/xeElementDetailForm.pas and
// xeElementDetailForm.dfm (an empty `TForm1` in 4.1.5q: the form exists,
// has no controls and nothing opens it), xEdit/xeMainForm.pas
// (vstViewEditing, vstViewGetEditText, vstViewNewText, vstViewDblClick,
// mniViewEditClick, vstCreateEditor and the `wbConvertIntFormID` branch)

//! The element detail editor: the in-place editor of the view tree
//! (`vstViewEditing`/`vstViewNewText`/`vstViewGetEditText`) and the "Edit
//! Value" dialog (`mniViewEditClick`), shown as an inspector pane tied to
//! the selection of the view grid.
//!
//! 4.1.5q's `xeElementDetailForm` is an empty placeholder, so the rules the
//! pane implements are the ones the view applies:
//!
//! * only an editable element is edited (`vstViewEditing`: `IsEditable`),
//!   and only while editing is allowed (`wbEditAllowed`);
//! * the text an editor starts with is the element's `EditValue`
//!   (`vstViewGetEditText`), and committing it assigns `EditValue`
//!   (`vstViewNewText`);
//! * a double click on an integer, flag or float element opens the in-place
//!   editor; every other type opens the multiline viewer
//!   (`vstViewDblClick`);
//! * "Edit Value" shows a checkbox per flag for a flags value
//!   (`IwbIntegerDef.Formater[Element] as IwbFlagsDef`), the localization
//!   editor for a translatable string of a localized plugin, and a text
//!   prompt otherwise (`mniViewEditClick`);
//! * a FormID typed as a decimal number becomes eight hexadecimal digits,
//!   and a `0x` prefix is dropped (`wbConvertIntFormID`).
//!
//! Every commit is `elements.set`, the command that ports
//! `Element.EditValue := ...`; the pane adds no editing path of its own
//! (design rule 1). [`ElementDetailPane::validate`] is the same quick check
//! the in-place editors make before they close, and the command's `dry_run`
//! is the authoritative answer the pane also uses on Apply.

use crate::command::CommandBridge;
use eframe::egui;
use serde_json::Value;
use xedit_session::CommandError;
use xedit_session::commands::{
    ElementDetail, ElementNode, ElementsGetRequest, ElementsSetRequest, ElementsSetResponse,
};

use crate::panes::RecordSelection;

/// The value types the in-place editors choose between
/// (`vstCreateEditor`'s `etDefault`/`etComboBox`/`etCheckComboBox` and
/// `vstViewDblClick`'s `dtInteger`/`dtFlag`/`dtFloat` test).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ValueType {
    /// A container: shown, never edited in place.
    Container,
    Text,
    Integer,
    Float,
    /// A flags value (`wbFlagsAsArray` gives each set flag a child).
    Flags,
    /// A value that holds a FormID: `wbConvertIntFormID` applies.
    FormId,
    /// A byte array, the GUID editor's type.
    Bytes,
}

impl ValueType {
    pub fn name(self) -> &'static str {
        match self {
            ValueType::Container => "container",
            ValueType::Text => "text",
            ValueType::Integer => "integer",
            ValueType::Float => "float",
            ValueType::Flags => "flags",
            ValueType::FormId => "FormID",
            ValueType::Bytes => "byte array",
        }
    }

    /// `vstViewDblClick`: these three open the in-place editor; every other
    /// type opens the multiline viewer (`xeViewElementsForm`, step 6).
    pub fn edits_in_place(self) -> bool {
        matches!(self, ValueType::Integer | ValueType::Float | ValueType::Flags)
    }
}

/// Classifies an element from the node `records.get`/`elements.get` return.
///
/// The node carries the display value and the native value, which is the
/// `Variant` `Element.NativeValue` gives: a boolean for a flag, a number for
/// an integer or a float, a string for text and a file FormID, and
/// hexadecimal bytes for a byte array.
pub fn value_type(node: &ElementNode) -> ValueType {
    match &node.native {
        Value::Bool(_) => ValueType::Flags,
        Value::Number(number) => {
            // A FormID's native value is its cardinal (`500` for
            // `000001F4`); the display value carries the link.
            if looks_like_form_id(&node.value) {
                ValueType::FormId
            } else if number.is_f64() {
                ValueType::Float
            } else {
                ValueType::Integer
            }
        }
        Value::String(text) => {
            if text.len() > 8 && text.bytes().all(|byte| matches!(byte, b'0' | b'1')) {
                ValueType::Flags
            } else if looks_like_form_id(text) || looks_like_form_id(&node.value) {
                ValueType::FormId
            } else {
                ValueType::Text
            }
        }
        Value::Array(_) => ValueType::Bytes,
        // A container has no native value; a leaf without one is text.
        _ if node.children.is_empty() => ValueType::Text,
        _ => ValueType::Container,
    }
}

/// Whether `text` is eight hexadecimal digits, the load order FormID shape,
/// or a display value that carries one (`Name [WEAP:00012EB7]`).
pub fn looks_like_form_id(text: &str) -> bool {
    let text = text.trim();
    if text.len() == 8 && text.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return true;
    }
    // `Name [SIGN:00123456]` of `wbDisplayShorterNames`.
    text.rsplit_once('[')
        .and_then(|(_, tail)| tail.split_once(':'))
        .is_some_and(|(_, tail)| {
            let digits: String = tail.chars().take(8).collect();
            digits.len() == 8 && digits.bytes().all(|byte| byte.is_ascii_hexdigit())
        })
}

/// `wbConvertIntFormID`: a bare decimal FormID becomes eight hexadecimal
/// digits and a `0x` prefix is dropped. Everything else is unchanged.
pub fn convert_int_form_id(text: &str) -> String {
    let trimmed = text.trim();
    if trimmed.starts_with('0') {
        // `0` and `0x...` are already the hexadecimal forms.
        return trimmed.strip_prefix("0x").unwrap_or(trimmed).to_owned();
    }
    if let Ok(value) = trimmed.parse::<i32>() {
        return format!("{value:08X}");
    }
    trimmed.to_owned()
}

/// The quick check [`ElementDetailPane`] makes before it calls
/// `elements.set`: the editor's own type rules. Returns the text to commit.
///
/// The command is still the authority -- the pane applies the same text
/// with `dry_run` first -- so this only spares a round trip for the obvious
/// mistakes the in-place editors also catch.
pub fn validate(kind: ValueType, text: &str) -> Result<String, String> {
    match kind {
        ValueType::Container => Err("a container has no value to edit".to_owned()),
        ValueType::Text => Ok(text.to_owned()),
        ValueType::Integer => {
            let trimmed = text.trim();
            let digits = trimmed
                .strip_prefix("0x")
                .or_else(|| trimmed.strip_prefix("0X"))
                .or_else(|| trimmed.strip_prefix('$'))
                .unwrap_or(trimmed);
            if digits.is_empty()
                || !digits
                    .strip_prefix('-')
                    .unwrap_or(digits)
                    .bytes()
                    .all(|byte| byte.is_ascii_hexdigit() || byte == b'_')
            {
                return Err(format!("{text:?} is not an integer"));
            }
            Ok(text.to_owned())
        }
        ValueType::Float => {
            if text.trim().parse::<f64>().is_err() {
                return Err(format!("{text:?} is not a number"));
            }
            Ok(text.to_owned())
        }
        ValueType::Flags => {
            // `TwbFlags.GetEditValue`: one character per flag, `0` or `1`.
            let trimmed = text.trim();
            if trimmed.is_empty() || !trimmed.bytes().all(|byte| matches!(byte, b'0' | b'1')) {
                return Err(format!("{text:?} is not a flags value (0 and 1)"));
            }
            Ok(trimmed.to_owned())
        }
        ValueType::FormId => {
            let converted = convert_int_form_id(text);
            if !looks_like_form_id(&converted) {
                return Err(format!("{text:?} is not a FormID"));
            }
            Ok(converted)
        }
        ValueType::Bytes => {
            let trimmed = text.trim();
            let compact: String = trimmed.chars().filter(|c| !c.is_whitespace()).collect();
            if compact.is_empty() || !compact.len().is_multiple_of(2) || !compact.bytes().all(|b| b.is_ascii_hexdigit())
            {
                return Err(format!("{text:?} is not a byte array (hexadecimal digits)"));
            }
            Ok(trimmed.to_owned())
        }
    }
}

/// The element the inspector shows: the record and the element's path in
/// it, as the view grid's selection names them.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SelectedElement {
    pub record: RecordSelection,
    pub path: String,
}

/// The inspector pane of `xeElementDetailForm`.
#[derive(Default)]
pub struct ElementDetailPane {
    pub selection: Option<SelectedElement>,
    /// `elements.get` of [`Self::selection`].
    pub detail: Option<ElementDetail>,
    /// The text of the in-place editor.
    pub edit_text: String,
    /// Why the last load, validation or commit failed.
    pub error: Option<String>,
    /// The last successful change, for the status line.
    pub last_change: Option<ElementsSetResponse>,
    /// A load is due on the next frame (the selection changed).
    pub needs_load: bool,
}

impl ElementDetailPane {
    pub fn new() -> Self {
        Self::default()
    }

    /// Points the pane at a new selection; the next [`Self::load`] reads it.
    /// The same selection keeps the element and the edit in progress.
    pub fn select(&mut self, selection: Option<SelectedElement>) {
        if self.selection == selection {
            return;
        }
        self.selection = selection;
        self.detail = None;
        self.edit_text.clear();
        self.error = None;
        self.last_change = None;
        self.needs_load = true;
    }

    /// `elements.get` for the selection.
    pub fn load(&mut self, bridge: &mut CommandBridge) -> Result<(), CommandError> {
        let Some(selection) = self.selection.clone() else {
            self.detail = None;
            self.needs_load = false;
            return Ok(());
        };
        let request = ElementsGetRequest {
            form_id: selection.record.form_id.clone(),
            path: selection.path.clone(),
            file: selection.record.file.clone(),
            depth: None,
        };
        match bridge.request::<_, ElementDetail>("elements.get", &request) {
            Ok(detail) => {
                self.edit_text = edit_text_of(&detail.node);
                self.detail = Some(detail);
                self.error = None;
                self.needs_load = false;
                Ok(())
            }
            Err(error) => {
                self.error = Some(error.message.clone());
                self.detail = None;
                self.needs_load = false;
                Err(error)
            }
        }
    }

    /// The type of the selected element.
    pub fn value_type(&self) -> Option<ValueType> {
        self.detail.as_ref().map(|detail| value_type(&detail.node))
    }

    /// Applies the edit text through `elements.set`, after the quick check.
    /// `dry_run` only asks the command what it would do.
    pub fn commit(&mut self, bridge: &mut CommandBridge, dry_run: bool) -> Result<bool, CommandError> {
        let Some(selection) = self.selection.clone() else {
            return Ok(false);
        };
        let Some(detail) = &self.detail else {
            return Ok(false);
        };
        let kind = value_type(&detail.node);
        let text = match validate(kind, &self.edit_text) {
            Ok(text) => text,
            Err(message) => {
                self.error = Some(message.clone());
                return Err(CommandError::new("invalid_value", message));
            }
        };
        let request = ElementsSetRequest {
            form_id: selection.record.form_id.clone(),
            // The path the selection names, not the display `FullPath` of the
            // response (`[00] Plugin \ [1] DATA`), which does not resolve.
            path: selection.path.clone(),
            file: selection.record.file.clone(),
            value: Value::String(text),
            dry_run,
        };
        match bridge.request::<_, ElementsSetResponse>("elements.set", &request) {
            Ok(response) => {
                self.error = None;
                if !dry_run {
                    self.edit_text = edit_text_of(&response.new);
                    // The element may have been added: the node is the one
                    // the command now reads back.
                    if let Some(detail) = &mut self.detail {
                        detail.node = response.new.clone();
                    }
                }
                self.last_change = Some(response);
                Ok(true)
            }
            Err(error) => {
                self.error = Some(error.message.clone());
                Err(error)
            }
        }
    }

    /// `null` through `elements.set`: the element goes back to its default.
    pub fn set_to_default(&mut self, bridge: &mut CommandBridge) -> Result<bool, CommandError> {
        let Some(selection) = self.selection.clone() else {
            return Ok(false);
        };
        if self.detail.is_none() {
            return Ok(false);
        }
        let request = ElementsSetRequest {
            form_id: selection.record.form_id.clone(),
            path: selection.path.clone(),
            file: selection.record.file.clone(),
            value: Value::Null,
            dry_run: false,
        };
        match bridge.request::<_, ElementsSetResponse>("elements.set", &request) {
            Ok(response) => {
                self.error = None;
                self.edit_text = edit_text_of(&response.new);
                self.last_change = Some(response);
                Ok(true)
            }
            Err(error) => {
                self.error = Some(error.message.clone());
                Err(error)
            }
        }
    }

    /// Sets one flag of a flags value: `elements.set` names the flag below
    /// the value (`ACBS\Flags\Female`).
    pub fn set_flag(&mut self, bridge: &mut CommandBridge, flag: &str, on: bool) -> Result<(), CommandError> {
        let Some(selection) = self.selection.clone() else {
            return Ok(());
        };
        if self.detail.is_none() {
            return Ok(());
        }
        let path = format!("{}\\{}", selection.path, flag);
        let request = ElementsSetRequest {
            form_id: selection.record.form_id.clone(),
            path,
            file: selection.record.file.clone(),
            value: Value::Bool(on),
            dry_run: false,
        };
        match bridge.request::<_, ElementsSetResponse>("elements.set", &request) {
            Ok(response) => {
                self.error = None;
                self.last_change = Some(response);
                self.needs_load = true;
                Ok(())
            }
            Err(error) => {
                self.error = Some(error.message.clone());
                Err(error)
            }
        }
    }

    /// Draws the inspector. Returns whether the edit text was applied this
    /// frame.
    pub fn show(&mut self, ui: &mut egui::Ui, bridge: &mut CommandBridge, edit_allowed: bool) -> bool {
        if self.needs_load {
            let _ = self.load(bridge);
        }
        let mut applied = false;
        ui.horizontal(|ui| {
            ui.heading("Element Detail");
            if self.detail.is_some()
                && let Some(selection) = &self.selection
            {
                ui.label(format!("{} in {}", selection.path, selection.record.form_id));
            }
        });
        let Some(detail) = self.detail.as_ref() else {
            ui.label("No element is selected.");
            if let Some(error) = &self.error {
                ui.colored_label(ui.visuals().error_fg_color, error);
            }
            return false;
        };
        let kind = value_type(&detail.node);
        let name = match &detail.node.display_name {
            Some(display_name) if !display_name.is_empty() => display_name.clone(),
            _ => detail.node.name.clone(),
        };
        let value = detail.node.value.clone();
        let summary = detail.node.summary.clone();
        let flags: Vec<(String, bool)> = detail
            .node
            .children
            .iter()
            .map(|child| (child.name.clone(), child.native == Value::Bool(true)))
            .collect();
        ui.label(format!("{name}  ({})", kind.name()));
        ui.label(format!("Value: {value}"));
        if !summary.is_empty() {
            ui.label(format!("Summary: {summary}"));
        }
        if kind == ValueType::Container {
            ui.label(format!("{} child elements", detail.node.children.len()));
            return false;
        }
        ui.horizontal(|ui| {
            ui.label("Edit:");
            ui.add(
                egui::TextEdit::singleline(&mut self.edit_text)
                    .desired_width(f32::INFINITY)
                    .hint_text("the value xEdit's in-place editor commits"),
            );
        });
        let mut apply = false;
        ui.horizontal(|ui| {
            let can_edit = edit_allowed;
            if ui
                .add_enabled(can_edit, egui::Button::new("Apply"))
                .on_disabled_hover_text("editing is off (--view)")
                .clicked()
            {
                apply = true;
            }
            if ui
                .add_enabled(can_edit, egui::Button::new("Set to default"))
                .on_disabled_hover_text("editing is off (--view)")
                .clicked()
                && let Err(error) = self.set_to_default(bridge)
            {
                self.error = Some(error.message);
            }
            if ui.button("Reload").clicked() {
                self.needs_load = true;
            }
        });
        if apply {
            applied = self.commit(bridge, false).unwrap_or(false);
        }
        if kind == ValueType::Flags && !flags.is_empty() {
            ui.separator();
            ui.label("Flags (wbFlagsAsArray):");
            for (flag, checked) in flags {
                let mut checked = checked;
                if ui
                    .add_enabled(edit_allowed, egui::Checkbox::new(&mut checked, &flag))
                    .changed()
                    && let Err(error) = self.set_flag(bridge, &flag, checked)
                {
                    self.error = Some(error.message);
                }
            }
        }
        if let Some(error) = &self.error {
            ui.colored_label(ui.visuals().error_fg_color, error);
        }
        applied
    }
}

/// The text the in-place editor starts with: `vstViewGetEditText` reads
/// `EditValue`; the element detail the command returns carries the display
/// value, which is the same text for every type but a named reference.
fn edit_text_of(node: &ElementNode) -> String {
    node.value.clone()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn form_ids_convert_as_wb_convert_int_form_id_does() {
        assert_eq!(convert_int_form_id("1"), "00000001");
        assert_eq!(convert_int_form_id("123456"), "0001E240");
        assert_eq!(convert_int_form_id("0x1A2B"), "1A2B");
        assert_eq!(convert_int_form_id("0ABC"), "0ABC");
        assert_eq!(convert_int_form_id(" 10 "), "0000000A");
        assert_eq!(convert_int_form_id("Iron Sword"), "Iron Sword");
    }

    #[test]
    fn a_form_id_is_eight_hex_digits_or_a_display_name() {
        assert!(looks_like_form_id("00012EB7"));
        assert!(looks_like_form_id("Iron Sword [WEAP:00012EB7]"));
        assert!(!looks_like_form_id("Iron Sword"));
        assert!(!looks_like_form_id("00012EB"));
    }

    #[test]
    fn the_editor_type_rules() {
        assert_eq!(validate(ValueType::Integer, "42").unwrap(), "42");
        assert!(validate(ValueType::Integer, "4x2").is_err());
        assert!(validate(ValueType::Float, "1.5").is_ok());
        assert!(validate(ValueType::Float, "1,5").is_err());
        assert!(validate(ValueType::Flags, "0000000000000001").is_ok());
        assert!(validate(ValueType::Flags, "0000000100000000").is_ok());
        assert!(validate(ValueType::Flags, "abc").is_err());
        assert_eq!(validate(ValueType::FormId, "123456").unwrap(), "0001E240");
        assert!(validate(ValueType::FormId, "Null").is_err());
        assert!(validate(ValueType::Bytes, "01 02 03").is_ok());
        assert!(validate(ValueType::Bytes, "012").is_err());
        assert!(validate(ValueType::Container, "x").is_err());
        assert_eq!(validate(ValueType::Text, "anything").unwrap(), "anything");
    }

    #[test]
    fn the_value_type_follows_the_native_variant() {
        let node = |native: Value, value: &str, children: Vec<ElementNode>| ElementNode {
            name: "X".to_owned(),
            display_name: None,
            signature: None,
            value: value.to_owned(),
            summary: String::new(),
            native,
            editable: false,
            edit_value: None,
            children,
        };
        assert_eq!(value_type(&node(Value::Bool(true), "", vec![])), ValueType::Flags);
        assert_eq!(value_type(&node(Value::from(3), "", vec![])), ValueType::Integer);
        assert_eq!(value_type(&node(Value::from(1.5), "", vec![])), ValueType::Float);
        assert_eq!(
            value_type(&node(Value::from("00012EB7"), "Iron Sword", vec![])),
            ValueType::FormId
        );
        assert_eq!(value_type(&node(Value::from("text"), "text", vec![])), ValueType::Text);
        let child = node(Value::from(1), "1", vec![]);
        assert_eq!(value_type(&node(Value::Null, "", vec![child])), ValueType::Container);
    }
}
