// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

// Ported from xEdit: the VCL behaviour behind the ten VCL-backed JVCL
// interpreter units and xejviScriptAdapterMiscVcl.pas -- the object model of
// the `Vcl` seam, the dialog functions of JvInterpreter_Dialogs.pas
// (`MessageDlg`, `ShowMessage`, `InputQuery`/`InputBox`, the common dialogs)
// and the form runner of JvInterpreterFm.pas

//! The headless VCL object model and the agent interaction bridge (issue
//! #26).
//!
//! Phase 6 step 9 registered the ten VCL-backed JVCL units and left every
//! window-bound member to the [`Vcl`] seam, whose headless default,
//! [`crate::script_rtl::NoVcl`], refused with `EVclUnavailable`. This module
//! is the seam's implementation: a script that builds a `TForm`, sets
//! control properties, assigns event handlers and shows the form modal runs
//! against real objects here, and a dialog (`MessageDlg`, `InputQuery`, a
//! selection dialog) is answered from the **recorded answers** the caller
//! supplied (`script.run`'s `answers`, the same JSON the parity harness
//! gives the oracle GUI) instead of a window.
//!
//! # The object model
//!
//! A `TForm.Create(nil)` (or of any registered class) builds a
//! [`VclObject`]: the class is the very `ClassDef` the adapter registered
//! (installed through [`Vcl::install_classes`], because `ClassDef::is_a`
//! compares pointers), properties and the window handle are object state
//! (`Caption`, `Left`/`Top`/`Width`/`Height`, `Visible`, `ModalResult`,
//! `Checked`, `ItemIndex`, ...), `Items`/`Lines` answer the one `TStrings`
//! value the RTL units ship (`xedit_script::strings`), `Parent` builds the
//! control tree, and `X.OnX := Handler` stores the script routine in the
//! object's event slot (the evaluator's method-property branch,
//! `JvInterpreter.pas:6916`).
//!
//! What the model does *not* do, recorded rather than faked:
//!
//! * **Event handlers are stored, not fired.** Showing a form or clicking a
//!   button runs no `OnClick`/`OnShow`/`OnKeyDown` script handler (upstream
//!   calls them from its window message loop, which phase 7's real GUI
//!   supplies). The corpus cases chosen for the parity checks close their
//!   forms through `Button.ModalResult` -- the click semantics the VCL
//!   itself performs before any handler -- so no chosen case depends on a
//!   fired handler; a script that does is not yet supported.
//! * **Canvas drawing and painting are presentation.** `TCanvas` and the
//!   paint members keep their state but draw nothing; the ledger marks them
//!   `presentation`.
//! * **`JvInterpreterMakeForm`/`RunForm`/`RunFormModal`/`RunUnit`** compile a
//!   second unit through a fresh interpreter; the model refuses them with
//!   the step-9 `EVclUnavailable` text until the GUI phase wires the form
//!   runner (no corpus script calls them).
//!
//! # The bridge
//!
//! A dialog the script shows builds a [`ScriptInteraction`] question
//! (window class, caption, text, buttons) and consumes the next
//! [`ScriptAnswer`], mirroring the oracle harness's matching rule: the
//! answer's `class` must equal the question's and its `title` must be a
//! substring of the caption. `text` is put into the first edit control
//! (`WM_SETTEXT`), `keys` are posted to the focused control (the arrow keys
//! move a list's item cursor, space toggles a check, the numpad `*` inverts
//! a check list, return and escape click the default and cancel buttons) and
//! `button` is clicked by its caption -- a button whose `ModalResult` is set
//! closes the modal form, the same rule the VCL's `TButton.Click` applies.
//!
//! When no answer remains (or the next one names a different dialog) the
//! model records the question as *pending* and raises
//! `EVclInteractionRequired`/`EVclAnswerMismatch`; `script.run` reports the
//! pending question in its response and the caller re-invokes with
//! `script.resume` and the answer appended. The interpreter has no
//! continuation, so the run starts over -- deterministic scripts reach the
//! same state and continue past the dialog, which is what "resume" means
//! here.

use std::cell::RefCell;
use std::collections::{BTreeMap, HashMap, VecDeque};
use std::rc::Rc;

use xedit_script::adapter::Args;
use xedit_script::values::{ClassDef, ScriptObject, Value, ValueError};

use super::{MemberKind, VCL_ERROR_CLASS, Vcl};
use crate::script::{ScriptAnswer, ScriptInteraction};
use crate::script_rtl::{arg_i32, arg_text};

/// The class of the error a run raises when a dialog has no recorded answer:
/// `pending_interaction` carries the question.
pub const INTERACTION_REQUIRED_CLASS: &str = "EVclInteractionRequired";
/// The class of the error when the next recorded answer names a different
/// dialog than the one shown.
pub const ANSWER_MISMATCH_CLASS: &str = "EVclAnswerMismatch";

/// The `TMsgDlgBtn` labels and their `mr*` results, in the registration
/// order of `JvInterpreter_Dialogs.pas` (`mbYes` .. `mbHelp`).
const MSG_BUTTONS: &[(&str, &str, i32)] = &[
    ("yes", "&Yes", 6),
    ("no", "&No", 7),
    ("ok", "OK", 1),
    ("cancel", "Cancel", 2),
    ("abort", "&Abort", 3),
    ("retry", "&Retry", 4),
    ("ignore", "&Ignore", 5),
    ("all", "&All", 8),
    ("notoall", "No to &All", 9),
    ("yestoall", "Yes to A&ll", 10),
    ("help", "&Help", -1),
];

/// `TMsgDlgType` captions (`GetMessageCaption` of the VCL's message form).
const MSG_TITLES: &[&str] = &["Warning", "Error", "Information", "Confirm", ""];

/// Virtual keys the recorded keys can use (the oracle harness posts the same
/// codes).
const VK_UP: u16 = 0x26;
const VK_DOWN: u16 = 0x28;
const VK_RETURN: u16 = 0x0D;
const VK_ESCAPE: u16 = 0x1B;
const VK_SPACE: u16 = 0x20;
/// The numpad `*`, which inverts a check list's selection (the oracle
/// harness uses it the same way).
const VK_MULTIPLY: u16 = 0x6A;

/// One VCL object of a script: its registered class, its property state, its
/// event slots, its children and its synthetic window handle.
pub struct VclObject {
    /// The synthetic window handle (`Handle`): the object's id, stable for
    /// the object's life.
    pub id: i64,
    class: Rc<ClassDef>,
    class_name: String,
    /// The property state (`Caption`, `Left`, `Visible`, `ModalResult`,
    /// `Checked` for a check box, ...); a write stores the value, a read
    /// gives it back.
    pub props: BTreeMap<String, Value>,
    /// The event slots: an `OnX` property -> the script routine stored by
    /// the evaluator's method-property branch.
    pub events: BTreeMap<String, String>,
    /// The per-item check states of a `TCheckListBox` (grown lazily; an
    /// entry beyond the vector takes the `CheckAll` state).
    pub checks: Vec<bool>,
    /// The child controls, in creation order (the tab order a fresh form
    /// starts with).
    pub children: Vec<i64>,
    pub parent: Option<i64>,
}

impl VclObject {
    fn new(id: i64, class: Rc<ClassDef>, class_name: &str) -> VclObject {
        VclObject {
            id,
            class,
            class_name: class_name.to_owned(),
            props: BTreeMap::new(),
            events: BTreeMap::new(),
            checks: Vec::new(),
            children: Vec::new(),
            parent: None,
        }
    }

    /// `Checked[i]`: an explicit entry, else the `CheckAll` state an earlier
    /// call set, else unchecked.
    fn checked_at(&self, index: usize) -> bool {
        match self.checks.get(index) {
            Some(checked) => *checked,
            None => match self.props.get("__check_all") {
                Some(Value::Bool(state)) => *state,
                _ => false,
            },
        }
    }
}

impl ScriptObject for VclObject {
    fn class(&self) -> Rc<ClassDef> {
        self.class.clone()
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn std::any::Any {
        self
    }

    /// The event slots are the model's: any `OnX` name (upstream's
    /// `tkMethod` RTTI property) is an event; the routine name the
    /// evaluator read from the token is stored, `None` clears it.
    fn set_event(&mut self, name: &str, function: Option<&str>) -> bool {
        if !is_event_name(name) {
            return false;
        }
        match function {
            Some(function) => {
                self.events.insert(name.to_owned(), function.to_owned());
            }
            None => {
                self.events.remove(name);
            }
        }
        true
    }

    fn event_function(&self, name: &str) -> Option<String> {
        self.events.get(name).cloned()
    }
}

/// Whether a member name is an event property: `On` plus an upper-case
/// letter (the shape every VCL event property has; upstream checks the
/// object's RTTI instead).
fn is_event_name(name: &str) -> bool {
    name.len() > 2 && name.starts_with("On") && name.as_bytes()[2].is_ascii_uppercase()
}

/// The headless VCL object model (the [`Vcl`] seam of issue #26).
pub struct VclModel {
    /// The script being run, for the error texts.
    script: String,
    /// The registered classes ([`Vcl::install_classes`]).
    classes: HashMap<String, Rc<ClassDef>>,
    /// Every live object by id; the object value a script holds is the same
    /// cell.
    objects: HashMap<i64, Rc<RefCell<VclObject>>>,
    /// The recorded answers not consumed yet, in order.
    answers: VecDeque<ScriptAnswer>,
    /// The dialog the run stopped on, when it did.
    pending: Option<ScriptInteraction>,
    next_id: i64,
    /// The `Screen` singleton of the `Forms` unit's `Screen` function.
    screen: Option<i64>,
    /// The forms currently shown modal, innermost last (a button's
    /// `ModalResult` closes the innermost).
    modal_stack: Vec<i64>,
}

impl VclModel {
    pub fn new(script: impl Into<String>, answers: Vec<ScriptAnswer>) -> VclModel {
        VclModel {
            script: script.into(),
            classes: HashMap::new(),
            objects: HashMap::new(),
            answers: answers.into(),
            pending: None,
            next_id: 0,
            screen: None,
            modal_stack: Vec::new(),
        }
    }

    /// The pending question, for `script.run`'s response; consumed once.
    pub fn take_pending(&mut self) -> Option<ScriptInteraction> {
        self.pending.take()
    }

    /// The first recorded answer the run never consumed: the run showed no
    /// such dialog, which `script.run` reports as a failure (the oracle
    /// harness fails a run the same way: "the oracle did not show the
    /// expected dialog").
    pub fn leftover(&self) -> Option<ScriptAnswer> {
        self.answers.front().cloned()
    }

    /// The published-property surface upstream reads before its registration
    /// tables: `TJvInterpreterAdapter.GetValue` tries `GetValueRTTI`
    /// (`JvInterpreter.pas:3949`, `:5115`) first, which answers any
    /// published property of the receiver's real class by name (`GetPropInfo`),
    /// whichever class the tables registered it on -- `btn.ModalResult`
    /// reads `TControl`'s published property although the tables carry
    /// `ModalResult` for `TCustomForm` alone. The interpreter calls this once
    /// the adapters answered nothing for an object member without
    /// arguments; a name that is no published property of the model
    /// (`published_name`) falls through to the undeclared-identifier error,
    /// as a nil `PropInfo` does upstream. Returns `false` for a receiver
    /// that is not one of the model's objects.
    pub fn rtti_get(&mut self, identifier: &str, value: &mut Value, args: &Args) -> Result<bool, ValueError> {
        if !args.values.is_empty() || args.indexed {
            return Ok(false);
        }
        let Some(object) = self.model_receiver(args) else {
            return Ok(false);
        };
        let Some(name) = published_name(identifier) else {
            return Ok(false);
        };
        // Events are `tkMethod` properties, which `GetValueRTTI` does not
        // read (its `case` has no arm for them).
        if is_event_name(name) {
            return Ok(false);
        }
        self.dispatch_get(&object, name, value)?;
        Ok(true)
    }

    /// `SetValueRTTI` (`JvInterpreter.pas:4586`, `:5156`): the write half of
    /// [`Self::rtti_get`]. The event (`tkMethod`) branch is the evaluator's
    /// own (`ScriptObject::set_event`), reached before this.
    pub fn rtti_set(&mut self, identifier: &str, value: &Value, args: &Args) -> Result<bool, ValueError> {
        if !args.values.is_empty() || args.indexed {
            return Ok(false);
        }
        let Some(object) = self.model_receiver(args) else {
            return Ok(false);
        };
        let Some(name) = published_name(identifier) else {
            return Ok(false);
        };
        if is_event_name(name) {
            return Ok(false);
        }
        self.dispatch_set(&object, name, value)?;
        Ok(true)
    }

    /// The receiver of `args` when it is one of the model's live objects.
    fn model_receiver(&self, args: &Args) -> Option<Rc<RefCell<VclObject>>> {
        let Some(Value::Object(object)) = &args.obj else {
            return None;
        };
        let id = object.borrow().as_any().downcast_ref::<VclObject>().map(|vcl| vcl.id)?;
        self.objects.get(&id).cloned()
    }

    //=== Registry helpers ====================================================

    fn object(&self, id: i64) -> Result<Rc<RefCell<VclObject>>, ValueError> {
        self.objects
            .get(&id)
            .cloned()
            .ok_or_else(|| self.model_error("the object has been freed".to_owned()))
    }

    fn model_error(&self, message: String) -> ValueError {
        ValueError::external_class("EVclModel", format!("{message} (script \"{}\")", self.script))
    }

    /// The receiver object of a member call, downcast from `Args.Obj`.
    fn receiver(&self, args: &Args) -> Result<Rc<RefCell<VclObject>>, ValueError> {
        let Some(Value::Object(object)) = &args.obj else {
            return Err(self.model_error("the member call has no object receiver".to_owned()));
        };
        let id = {
            let borrowed = object.borrow();
            let Some(vcl) = borrowed.as_any().downcast_ref::<VclObject>() else {
                return Err(self.model_error("the receiver is not a VCL object".to_owned()));
            };
            vcl.id
        };
        self.object(id)
    }

    fn class_of(&self, name: &str) -> Result<Rc<ClassDef>, ValueError> {
        self.classes
            .get(name)
            .cloned()
            .ok_or_else(|| self.model_error(format!("the class {name} is not registered")))
    }

    /// Builds an object of `class_name` (a `Create`/`CreateNew` call) with
    /// an optional owner, and returns its value.
    fn create_object(&mut self, class_name: &str, owner: Option<&Value>) -> Result<Value, ValueError> {
        let class = self.class_of(class_name)?;
        let id = self.next_id;
        self.next_id += 1;
        let object = Rc::new(RefCell::new(VclObject::new(id, class, class_name)));
        let parent = owner.and_then(|value| value.as_object()).and_then(|owner| {
            let borrowed = owner.borrow();
            borrowed.as_any().downcast_ref::<VclObject>().map(|vcl| vcl.id)
        });
        object.borrow_mut().parent = parent;
        self.objects.insert(id, object.clone());
        if let Some(parent) = parent
            && let Some(parent_object) = self.objects.get(&parent).cloned()
        {
            parent_object.borrow_mut().children.push(id);
        }
        Ok(Value::Object(object))
    }

    /// Builds a child object of `class_name` under `parent_id` and returns
    /// its value (the lazily created `Font`, `Canvas`, `EditLabel`).
    fn create_child(&mut self, parent_id: i64, class_name: &str) -> Result<Value, ValueError> {
        let class = self.class_of(class_name)?;
        let id = self.next_id;
        self.next_id += 1;
        let object = Rc::new(RefCell::new(VclObject::new(id, class, class_name)));
        object.borrow_mut().parent = Some(parent_id);
        self.objects.insert(id, object.clone());
        if let Some(parent) = self.objects.get(&parent_id).cloned() {
            parent.borrow_mut().children.push(id);
        }
        Ok(Value::Object(object))
    }

    /// The class chain of an object contains `name` (the `is` check of the
    /// registration dispatch, by name here).
    fn is_a(&self, object: &Rc<RefCell<VclObject>>, name: &str) -> bool {
        let borrowed = object.borrow();
        let mut current = Some(&borrowed.class);
        while let Some(class) = current {
            if class.name.eq_ignore_ascii_case(name) {
                return true;
            }
            current = class.parent.as_ref();
        }
        false
    }

    /// Releases an object and its children (upstream `Free`).
    fn release(&mut self, id: i64) {
        let Some(object) = self.objects.remove(&id) else {
            return;
        };
        let (children, parent) = {
            let borrowed = object.borrow();
            (borrowed.children.clone(), borrowed.parent)
        };
        if let Some(parent) = parent
            && let Some(parent_object) = self.objects.get(&parent).cloned()
        {
            parent_object.borrow_mut().children.retain(|child| *child != id);
        }
        for child in children {
            self.release(child);
        }
    }

    fn parent_of(&self, id: i64) -> Option<i64> {
        self.objects.get(&id).and_then(|object| object.borrow().parent)
    }

    /// Walks up from `id` to the object without a parent (the form that owns
    /// it).
    fn owning_form(&self, id: i64) -> Option<Rc<RefCell<VclObject>>> {
        let mut current = id;
        loop {
            match self.parent_of(current) {
                Some(parent) => current = parent,
                None => return self.objects.get(&current).cloned(),
            }
        }
    }

    fn prop(&self, object: &Rc<RefCell<VclObject>>, name: &str) -> Option<Value> {
        object.borrow().props.get(name).cloned()
    }

    fn prop_str(&self, object: &Rc<RefCell<VclObject>>, name: &str) -> Option<Vec<u8>> {
        match self.prop(object, name) {
            Some(Value::Str(text)) => Some(text),
            _ => None,
        }
    }

    fn prop_i32(&self, object: &Rc<RefCell<VclObject>>, name: &str, default: i32) -> i32 {
        match self.prop(object, name) {
            Some(Value::Integer(value)) => value,
            _ => default,
        }
    }

    fn prop_bool(&self, object: &Rc<RefCell<VclObject>>, name: &str, default: bool) -> bool {
        match self.prop(object, name) {
            Some(Value::Bool(value)) => value,
            _ => default,
        }
    }

    fn set_prop(&self, object: &Rc<RefCell<VclObject>>, name: &str, value: Value) {
        object.borrow_mut().props.insert(name.to_owned(), value);
    }

    /// The id of a `Value::Object` that is one of the model's objects
    /// (returns `None` for any other value).
    fn id_of(value: &Value) -> Option<i64> {
        let Value::Object(object) = value else {
            return None;
        };
        let borrowed = object.borrow();
        borrowed.as_any().downcast_ref::<VclObject>().map(|vcl| vcl.id)
    }

    /// The object's caption (`Caption`, else `Title`, else `Text`): what a
    /// dialog's window title shows.
    fn caption_of(&self, object: &Rc<RefCell<VclObject>>) -> String {
        for name in ["Caption", "Title", "Text"] {
            if let Some(text) = self.prop_str(object, name) {
                return String::from_utf8_lossy(&text).into_owned();
            }
        }
        String::new()
    }

    /// Whether the object is a form (hidden until `Show`/`ShowModal`; the
    /// VCL `Visible` default: false for a form, true for a control).
    fn form_class(&self, object: &Rc<RefCell<VclObject>>) -> bool {
        self.is_a(object, "TCustomForm") || self.is_a(object, "TDataModule")
    }

    fn visible(&self, object: &Rc<RefCell<VclObject>>) -> bool {
        self.prop_bool(object, "Visible", !self.form_class(object))
    }

    /// The first child of `id` whose class chain contains `name`.
    fn first_child_of_class(&self, id: i64, name: &str) -> Option<Rc<RefCell<VclObject>>> {
        let children = self.objects.get(&id)?.borrow().children.clone();
        children.into_iter().find_map(|child| {
            let object = self.objects.get(&child).cloned()?;
            if self.is_a(&object, name) { Some(object) } else { None }
        })
    }

    /// The lazily created `Items`/`Lines` value of a control: the `Classes`
    /// unit's `TStringList` object (the one the host's `TStrings` members
    /// and `GetRecordDefNames` fill and read), or for a menu the root
    /// `TMenuItem` the menu items are added under.
    fn items_of(&mut self, object: &Rc<RefCell<VclObject>>) -> Result<Value, ValueError> {
        if let Some(value) = object.borrow().props.get("Items").cloned() {
            return Ok(value);
        }
        let value = if self.is_a(object, "TMenu") {
            let id = object.borrow().id;
            self.create_child(id, "TMenuItem")?
        } else {
            let class = self.class_of("TStringList")?;
            Value::Object(Rc::new(RefCell::new(crate::script_strings::StringListObject::new(
                class,
            ))))
        };
        self.set_prop(object, "Items", value.clone());
        Ok(value)
    }

    /// The active control of a form: its `ActiveControl`, else the first
    /// child (the tab order a fresh form starts with).
    fn active_control(&self, form_id: i64) -> Option<Rc<RefCell<VclObject>>> {
        let form = self.objects.get(&form_id)?;
        if let Some(Value::Object(active)) = self.prop(form, "ActiveControl")
            && let Some(vcl) = active.borrow().as_any().downcast_ref::<VclObject>()
            && let Some(object) = self.objects.get(&vcl.id).cloned()
        {
            return Some(object);
        }
        let children = form.borrow().children.clone();
        children.into_iter().find_map(|child| self.objects.get(&child).cloned())
    }

    //=== The interaction bridge ==============================================

    /// Answers the next dialog, in the oracle harness's rule: the next
    /// recorded answer's `class` must equal `class` and its `title` must be
    /// a substring of `title`. Records the question and raises when the
    /// answers ran out or the next one names another dialog.
    fn ask(
        &mut self,
        class: &str,
        title: &str,
        text: String,
        buttons: Vec<String>,
        default: Option<String>,
        kind: &str,
    ) -> Result<ScriptAnswer, ValueError> {
        let question = ScriptInteraction {
            kind: kind.to_owned(),
            class: class.to_owned(),
            title: title.to_owned(),
            text,
            buttons,
            default,
        };
        let Some(answer) = self.answers.front() else {
            let detail = if question.text.is_empty() {
                format!("[{}] \"{}\"", question.class, question.title)
            } else {
                format!("[{}] \"{}\": {}", question.class, question.title, question.text)
            };
            self.pending = Some(question);
            return Err(ValueError::external_class(
                INTERACTION_REQUIRED_CLASS,
                format!(
                    "{detail} needs an answer and none is recorded (script \"{}\")",
                    self.script
                ),
            ));
        };
        if answer.class == class && title.contains(answer.title.as_str()) {
            let answer = self.answers.pop_front().expect("checked above");
            Ok(answer)
        } else {
            let message = format!(
                "the script shows [{}] \"{}\" where the next recorded answer is for [{}] \"{}\" (script \"{}\")",
                class, title, answer.class, answer.title, self.script
            );
            self.pending = Some(question);
            Err(ValueError::external_class(ANSWER_MISMATCH_CLASS, message))
        }
    }

    /// `MessageDlg` (`JvInterpreter_Dialogs.pas`): the offered buttons are
    /// the `mb*` set bits in the registration order; the answer names a
    /// button or posts keys (return clicks the first offered button, escape
    /// the cancel one).
    fn message_dlg(&mut self, args: &Args) -> Result<i32, ValueError> {
        let message = arg_text(args, 0)?;
        let dlg_type = arg_i32(args, 1).unwrap_or(2);
        // `V2S(Args.Values[2])`: a `[...]` argument reaches the handler as
        // the open array of its elements (`ReadOpenArray`), each one a bit.
        let bits = match args.values.get(2) {
            Some(value) => xedit_script::values::v2s(value).unwrap_or(1),
            None => 1,
        };
        let offered: Vec<&(&str, &str, i32)> = MSG_BUTTONS
            .iter()
            .enumerate()
            .filter(|(index, _)| bits & (1 << index) != 0)
            .map(|(_, button)| button)
            .collect();
        let buttons: Vec<String> = offered.iter().map(|(_, label, _)| (*label).to_owned()).collect();
        let default = offered.first().map(|(_, label, _)| (*label).to_owned());
        let title = MSG_TITLES.get(dlg_type.max(0) as usize).copied().unwrap_or("");
        let answer = self.ask("#32770", title, message, buttons, default, "message")?;
        self.resolve_button(&answer, &offered, title)
    }

    /// The `mr*` of an answer to a message dialog: the named button, or the
    /// key rules (return -> the first offered button, escape -> cancel).
    fn resolve_button(
        &self,
        answer: &ScriptAnswer,
        offered: &[&(&str, &str, i32)],
        title: &str,
    ) -> Result<i32, ValueError> {
        if let Some(button) = &answer.button {
            let wanted = normalize_button(button);
            for (_, label, result) in offered {
                if normalize_button(label) == wanted {
                    return Ok(*result);
                }
            }
            return Err(self.model_error(format!(
                "the dialog \"{title}\" has no {button} button ({})",
                offered
                    .iter()
                    .map(|(_, label, _)| *label)
                    .collect::<Vec<_>>()
                    .join(", ")
            )));
        }
        if answer.keys.contains(&VK_ESCAPE) {
            for (name, _, result) in offered {
                if *name == "cancel" {
                    return Ok(*result);
                }
            }
            for (name, _, result) in offered {
                if *name == "no" {
                    return Ok(*result);
                }
            }
            return Ok(2);
        }
        if answer.keys.contains(&VK_RETURN)
            && let Some((_, _, result)) = offered.first()
        {
            return Ok(*result);
        }
        Err(self.model_error(format!(
            "the recorded answer for the dialog \"{title}\" names no button and no keys"
        )))
    }

    /// `ShowMessage` (and the position variants): an information box with
    /// `OK`; the result is dropped.
    fn show_message(&mut self, args: &Args) -> Result<(), ValueError> {
        let message = arg_text(args, 0)?;
        let offered: Vec<&(&str, &str, i32)> = MSG_BUTTONS.iter().filter(|(name, _, _)| *name == "ok").collect();
        let buttons = vec!["OK".to_owned()];
        let answer = self.ask(
            "#32770",
            "Information",
            message,
            buttons.clone(),
            buttons.first().cloned(),
            "message",
        )?;
        self.resolve_button(&answer, &offered, "Information")?;
        Ok(())
    }

    /// `InputQuery` (`Dialogs.InputQuery`): a `TInputQueryForm` with an edit
    /// control; the answer's `text` is the new value, `false` when its
    /// button is Cancel or it posts escape.
    fn input_query(&mut self, args: &mut Args) -> Result<bool, ValueError> {
        let caption = arg_text(args, 0)?;
        let prompt = arg_text(args, 1)?;
        let current = arg_text(args, 2)?;
        let answer = self.ask(
            "TInputQueryForm",
            &caption,
            prompt,
            vec!["OK".to_owned(), "Cancel".to_owned()],
            None,
            "input",
        )?;
        if input_cancelled(&answer) {
            return Ok(false);
        }
        let value = answer.text.unwrap_or(current);
        *args.values.get_mut(2).expect("registered with three parameters") = Value::Str(value.into_bytes());
        Ok(true)
    }

    /// `InputBox`: the value of an `InputQuery`, the default on cancel.
    fn input_box(&mut self, args: &Args) -> Result<Vec<u8>, ValueError> {
        let caption = arg_text(args, 0)?;
        let prompt = arg_text(args, 1)?;
        let default = arg_text(args, 2)?;
        let answer = self.ask(
            "TInputQueryForm",
            &caption,
            prompt,
            vec!["OK".to_owned(), "Cancel".to_owned()],
            None,
            "input",
        )?;
        if input_cancelled(&answer) {
            return Ok(default.into_bytes());
        }
        Ok(answer.text.unwrap_or(default).into_bytes())
    }

    /// `CreateMessageDialog`: the form the caller shows itself. The model
    /// builds a form object carrying the message and the button set; its
    /// `ShowModal` is answered like a message dialog.
    fn create_message_dialog(&mut self, args: &Args) -> Result<Value, ValueError> {
        let message = arg_text(args, 0)?;
        let dlg_type = arg_i32(args, 1).unwrap_or(2);
        // `V2S(Args.Values[2])`: a `[...]` argument reaches the handler as
        // the open array of its elements (`ReadOpenArray`), each one a bit.
        let bits = match args.values.get(2) {
            Some(value) => xedit_script::values::v2s(value).unwrap_or(1),
            None => 1,
        };
        let value = self.create_object("TForm", None)?;
        if let Some(id) = Self::id_of(&value) {
            let form = self.object(id)?;
            let title = MSG_TITLES.get(dlg_type.max(0) as usize).copied().unwrap_or("");
            self.set_prop(&form, "Caption", Value::Str(title.as_bytes().to_vec()));
            self.set_prop(&form, "Tag", Value::Integer(bits));
            self.set_prop(&form, "Text", Value::Str(message.into_bytes()));
        }
        Ok(value)
    }

    /// The `Windows.MessageBox` of the `Windows` unit (`AddExtFun`): the
    /// `uType` flags offer the classic buttons; the classic ids come back
    /// (`IDOK` 1, `IDCANCEL` 2, `IDYES` 6, `IDNO` 7).
    fn message_box(&mut self, args: &Args) -> Result<i32, ValueError> {
        // `MessageBox(hWnd, lpText, lpCaption, uType)`.
        let text = args.values.get(1).map(value_text).unwrap_or_default();
        let caption = args.values.get(2).map(value_text).unwrap_or_default();
        let flags = args
            .values
            .get(3)
            .and_then(|value| xedit_script::values::to_i32(value).ok())
            .unwrap_or(0);
        let (labels, ids): (Vec<&str>, Vec<i32>) = match flags & 0xF {
            1 => (vec!["OK", "Cancel"], vec![1, 2]),
            3 => (vec!["&Yes", "&No", "Cancel"], vec![6, 7, 2]),
            4 => (vec!["&Yes", "&No"], vec![6, 7]),
            _ => (vec!["OK"], vec![1]),
        };
        let buttons: Vec<String> = labels.iter().map(|label| (*label).to_owned()).collect();
        let default = buttons.first().cloned();
        let answer = self.ask("#32770", &caption, text, buttons, default, "message")?;
        if let Some(button) = &answer.button {
            let wanted = normalize_button(button);
            for (label, id) in labels.iter().zip(&ids) {
                if normalize_button(label) == wanted {
                    return Ok(*id);
                }
            }
            return Err(self.model_error(format!("the message box has no {button} button")));
        }
        if answer.keys.contains(&VK_ESCAPE) {
            return Ok(ids.last().copied().unwrap_or(1));
        }
        Ok(ids.first().copied().unwrap_or(1))
    }

    //=== Forms ===============================================================

    /// `ShowModal` of the model: consults the next recorded answer, applies
    /// its text/keys/button and returns the form's `ModalResult` when the
    /// answer closed it. A form that stays open is the harness's "dialog
    /// nobody answered".
    fn show_modal(&mut self, form_id: i64) -> Result<i32, ValueError> {
        let form = self.object(form_id)?;
        let class_name = form.borrow().class_name.clone();
        let title = self.caption_of(&form);
        let text = self
            .prop_str(&form, "Text")
            .map(|bytes| String::from_utf8_lossy(&bytes).into_owned())
            .unwrap_or_default();
        let answer = self.ask(&class_name, &title, text, Vec::new(), None, "form")?;
        self.modal_stack.push(form_id);
        self.apply_answer(form_id, &answer)?;
        let modal_result = |form: &Rc<RefCell<VclObject>>| match form.borrow().props.get("ModalResult") {
            Some(Value::Integer(value)) => *value,
            _ => 0,
        };
        // A form a scenario drives control by control is one window with
        // several answers (the harness's `repeat`): while the form stays
        // open and the next recorded answer names it again, that answer is
        // applied too.
        while modal_result(&form) == 0
            && self
                .answers
                .front()
                .is_some_and(|next| next.repeat && next.class == class_name && title.contains(next.title.as_str()))
        {
            let next = self.answers.pop_front().expect("checked above");
            self.apply_answer(form_id, &next)?;
        }
        let result = modal_result(&form);
        if self.modal_stack.last() == Some(&form_id) {
            self.modal_stack.pop();
        }
        if result != 0 {
            self.set_prop(&form, "Visible", Value::Bool(false));
            Ok(result)
        } else {
            self.pending = Some(ScriptInteraction {
                kind: "form".to_owned(),
                class: class_name,
                title,
                text: String::new(),
                buttons: Vec::new(),
                default: None,
            });
            Err(ValueError::external_class(
                INTERACTION_REQUIRED_CLASS,
                format!(
                    "the form is still open after its answer (no button with a modal result was clicked) and no further answer is available (script \"{}\")",
                    self.script
                ),
            ))
        }
    }

    /// One recorded answer applied to an open form: text into the first edit
    /// control, keys to the active control, then the button click (the order
    /// the oracle harness uses). An answer's `control` names the class of
    /// the child the text and the keys go to instead (the harness posts to
    /// that child: a form on its desktop has no focused control).
    fn apply_answer(&mut self, form_id: i64, answer: &ScriptAnswer) -> Result<(), ValueError> {
        let target = match &answer.control {
            Some(class) => {
                let Some(control) = self.first_child_of_class(form_id, class) else {
                    let form = self.object(form_id)?;
                    let title = self.caption_of(&form);
                    return Err(self.model_error(format!("the form \"{title}\" has no {class} control")));
                };
                Some(control)
            }
            None => None,
        };
        if let Some(text) = &answer.text
            && let Some(edit) = target.clone().or_else(|| self.first_edit_child(form_id))
        {
            self.set_prop(&edit, "Text", Value::Str(text.clone().into_bytes()));
        }
        for &key in &answer.keys {
            self.apply_key(form_id, key, target.clone())?;
        }
        if let Some(button) = &answer.button {
            self.click_button(form_id, button)?;
        }
        Ok(())
    }

    /// The first child that is an edit-shaped control (the oracle harness's
    /// `set_edit_text` targets the first edit control of the window).
    fn first_edit_child(&self, form_id: i64) -> Option<Rc<RefCell<VclObject>>> {
        let children = self.objects.get(&form_id)?.borrow().children.clone();
        children.into_iter().find_map(|child| {
            let object = self.objects.get(&child).cloned()?;
            let edit = self.is_a(&object, "TCustomEdit")
                || self.is_a(&object, "TLabeledEdit")
                || self.is_a(&object, "TMemo")
                || self.is_a(&object, "TCustomComboBox");
            if edit { Some(object) } else { None }
        })
    }

    /// One virtual key on a form's control: `target` when the answer named
    /// one, else the active control.
    fn apply_key(&mut self, form_id: i64, key: u16, target: Option<Rc<RefCell<VclObject>>>) -> Result<(), ValueError> {
        let active = match target {
            Some(control) => control,
            None => self
                .active_control(form_id)
                .ok_or_else(|| self.model_error("the form has no focused control".to_owned()))?,
        };
        match key {
            VK_DOWN | VK_UP => {
                if self.is_list(&active) {
                    // A list with nothing selected (-1, the control's start)
                    // selects its first entry on Down; the count bounds the
                    // cursor as the control does.
                    let step = if key == VK_DOWN { 1 } else { -1 };
                    let count = self.list_count(&active);
                    let index = (self.prop_i32(&active, "ItemIndex", -1) + step).max(0);
                    let index = if count > 0 { index.min(count - 1) } else { index };
                    self.set_prop(&active, "ItemIndex", Value::Integer(index));
                }
            }
            VK_SPACE => {
                if self.is_a(&active, "TCheckListBox") {
                    // `TCheckListBox.KeyPress` toggles the entry at
                    // `ItemIndex`; nothing happens while nothing is selected.
                    let index = self.prop_i32(&active, "ItemIndex", -1);
                    if index >= 0 {
                        let index = index as usize;
                        let mut borrowed = active.borrow_mut();
                        if borrowed.checks.len() <= index {
                            borrowed.checks.resize(index + 1, false);
                        }
                        borrowed.checks[index] = !borrowed.checks[index];
                    }
                } else if self.is_a(&active, "TCheckBox") {
                    let checked = self.prop_bool(&active, "Checked", false);
                    self.set_prop(&active, "Checked", Value::Bool(!checked));
                }
            }
            VK_MULTIPLY => {
                if self.is_a(&active, "TCheckListBox") {
                    let mut borrowed = active.borrow_mut();
                    for check in borrowed.checks.iter_mut() {
                        *check = !*check;
                    }
                }
            }
            VK_RETURN => {
                if let Some(button) = self.default_button(form_id) {
                    self.click_object(&button)?;
                }
            }
            VK_ESCAPE => {
                if let Some(button) = self.cancel_button(form_id) {
                    self.click_object(&button)?;
                } else {
                    let form = self.object(form_id)?;
                    self.set_prop(&form, "ModalResult", Value::Integer(2));
                }
            }
            _ => {}
        }
        Ok(())
    }

    /// Whether a control navigates with the arrow keys (a list box, a check
    /// list, a combo box).
    fn is_list(&self, object: &Rc<RefCell<VclObject>>) -> bool {
        self.is_a(object, "TCustomListBox")
            || self.is_a(object, "TCheckListBox")
            || self.is_a(object, "TCustomComboBox")
    }

    /// The entry count of a list control's `Items` (0 before any was added).
    fn list_count(&self, object: &Rc<RefCell<VclObject>>) -> i32 {
        let Some(items) = object.borrow().props.get("Items").cloned() else {
            return 0;
        };
        crate::script_strings::list_of(&items)
            .map(|list| crate::script_strings::borrow_list(&list).count() as i32)
            .unwrap_or(0)
    }

    fn button_children(&self, form_id: i64) -> Vec<Rc<RefCell<VclObject>>> {
        let children = match self.objects.get(&form_id) {
            Some(form) => form.borrow().children.clone(),
            None => return Vec::new(),
        };
        children
            .into_iter()
            .filter_map(|child| self.objects.get(&child).cloned())
            .filter(|object| self.is_a(object, "TButton") || self.is_a(object, "TBitBtn"))
            .collect()
    }

    /// `TButton.Click` semantics: a button whose `ModalResult` is set closes
    /// the modal form with it (a handler assigned to `OnClick` would run
    /// first upstream; the model stores it, see the module docs).
    fn click_object(&mut self, button: &Rc<RefCell<VclObject>>) -> Result<(), ValueError> {
        if !self.prop_bool(button, "Enabled", true) {
            return Ok(());
        }
        let modal = self.prop_i32(button, "ModalResult", 0);
        let id = button.borrow().id;
        if modal != 0
            && let Some(form) = self.owning_form(id)
        {
            let current = match form.borrow().props.get("ModalResult") {
                Some(Value::Integer(value)) => *value,
                _ => 0,
            };
            if current == 0 {
                self.set_prop(&form, "ModalResult", Value::Integer(modal));
            }
        }
        Ok(())
    }

    /// Clicks the child button whose caption matches (`&` ignored), the
    /// oracle harness's rule; a form without such a button fails like the
    /// harness's "the dialog has no <button> button".
    fn click_button(&mut self, form_id: i64, caption: &str) -> Result<(), ValueError> {
        let wanted = normalize_button(caption);
        let buttons = self.button_children(form_id);
        let found = buttons.into_iter().find(|button| {
            let label = self.caption_of(button);
            normalize_button(&label) == wanted
        });
        let Some(button) = found else {
            let form = self.object(form_id)?;
            let title = self.caption_of(&form);
            return Err(self.model_error(format!("the form \"{title}\" has no {caption} button")));
        };
        self.click_object(&button)
    }

    fn default_button(&self, form_id: i64) -> Option<Rc<RefCell<VclObject>>> {
        let buttons = self.button_children(form_id);
        buttons
            .iter()
            .find(|button| self.prop_bool(button, "Default", false))
            .cloned()
            .or_else(|| {
                buttons
                    .into_iter()
                    .find(|button| self.prop_i32(button, "ModalResult", 0) != 0)
            })
    }

    fn cancel_button(&self, form_id: i64) -> Option<Rc<RefCell<VclObject>>> {
        self.button_children(form_id)
            .into_iter()
            .find(|button| self.prop_i32(button, "ModalResult", 0) == 2)
    }

    /// `TOpenDialog`/`TSaveDialog.Execute`: the answer's `text` is the file
    /// name; cancel closes without picking.
    fn execute_dialog(&mut self, object: &Rc<RefCell<VclObject>>) -> Result<bool, ValueError> {
        let class_name = object.borrow().class_name.clone();
        let title = self
            .prop_str(object, "Title")
            .map(|bytes| String::from_utf8_lossy(&bytes).into_owned())
            .unwrap_or_else(|| {
                if class_name == "TSaveDialog" {
                    "Save As".to_owned()
                } else {
                    "Open".to_owned()
                }
            });
        let default = self
            .prop_str(object, "FileName")
            .map(|bytes| String::from_utf8_lossy(&bytes).into_owned())
            .unwrap_or_default();
        let accept = if class_name == "TSaveDialog" { "&Save" } else { "&Open" };
        let answer = self.ask(
            "#32770",
            &title,
            default,
            vec![accept.to_owned(), "Cancel".to_owned()],
            None,
            "file",
        )?;
        if input_cancelled(&answer) {
            return Ok(false);
        }
        let Some(file) = answer.text else {
            return Err(self.model_error(format!(
                "the recorded answer for the file dialog \"{title}\" carries no text (the file name)"
            )));
        };
        // The common dialog resolves a name typed without a drive or a root
        // against its folder (`InitialDir`), and `FileName` is the full
        // path.
        let relative = !(file.contains(':') || file.starts_with('\\') || file.starts_with('/'));
        let file = match self.prop_str(object, "InitialDir") {
            Some(dir) if relative && !dir.is_empty() => {
                // One separator between the folder and the name, however
                // many the folder ends with (the data path ends in two).
                let dir = String::from_utf8_lossy(&dir).into_owned();
                let mut path = dir.trim_end_matches(['\\', '/']).to_owned();
                path.push('\\');
                path.push_str(&file);
                path
            }
            _ => file,
        };
        self.set_prop(object, "FileName", Value::Str(file.into_bytes()));
        Ok(true)
    }

    //=== Member dispatch =====================================================

    fn dispatch_get(
        &mut self,
        object: &Rc<RefCell<VclObject>>,
        member: &str,
        value: &mut Value,
    ) -> Result<(), ValueError> {
        match member {
            "Show" => {
                self.set_prop(object, "Visible", Value::Bool(true));
                Ok(())
            }
            "ShowModal" => {
                let id = object.borrow().id;
                *value = Value::Integer(self.show_modal(id)?);
                Ok(())
            }
            "Close" => {
                let id = object.borrow().id;
                self.set_prop(object, "Visible", Value::Bool(false));
                let current = match object.borrow().props.get("ModalResult") {
                    Some(Value::Integer(modal)) => *modal,
                    _ => 0,
                };
                if current == 0 && self.modal_stack.contains(&id) {
                    self.set_prop(object, "ModalResult", Value::Integer(2));
                }
                Ok(())
            }
            "Hide" => {
                self.set_prop(object, "Visible", Value::Bool(false));
                Ok(())
            }
            "Free" | "Release" | "Destroy" => {
                let id = object.borrow().id;
                self.release(id);
                Ok(())
            }
            "SetFocus" | "FocusControl" => {
                let id = object.borrow().id;
                if let Some(form) = self.owning_form(id) {
                    let value = Value::Object(self.object(id)?);
                    self.set_prop(&form, "ActiveControl", value);
                }
                Ok(())
            }
            "Click" => self.click_object(object),
            "Execute" => {
                *value = Value::Bool(self.execute_dialog(object)?);
                Ok(())
            }
            "Clear" => {
                self.set_prop(object, "Text", Value::Str(Vec::new()));
                Ok(())
            }
            _ => {
                // The property read.
                if let Some(stored) = self.prop(object, member) {
                    *value = stored;
                    return Ok(());
                }
                if member == "Handle" {
                    *value = Value::Integer(object.borrow().id as i32);
                    return Ok(());
                }
                if member == "Visible" {
                    *value = Value::Bool(self.visible(object));
                    return Ok(());
                }
                if member == "ActiveControl" {
                    let id = object.borrow().id;
                    if let Some(active) = self.active_control(id) {
                        *value = Value::Object(active);
                    } else {
                        *value = Value::Null;
                    }
                    return Ok(());
                }
                if member == "Items" || member == "Lines" {
                    *value = self.items_of(object)?;
                    return Ok(());
                }
                if member == "Font" {
                    let id = object.borrow().id;
                    if let Some(font) = self.first_child_of_class(id, "TFont") {
                        *value = Value::Object(font);
                    } else {
                        *value = self.create_child(id, "TFont")?;
                    }
                    return Ok(());
                }
                if member == "Canvas" {
                    let id = object.borrow().id;
                    if let Some(canvas) = self.first_child_of_class(id, "TCanvas") {
                        *value = Value::Object(canvas);
                    } else {
                        *value = self.create_child(id, "TCanvas")?;
                    }
                    return Ok(());
                }
                if member == "EditLabel" {
                    let id = object.borrow().id;
                    if let Some(label) = self.first_child_of_class(id, "TBoundLabel") {
                        *value = Value::Object(label);
                    } else {
                        *value = self.create_child(id, "TBoundLabel")?;
                    }
                    return Ok(());
                }
                if member == "ItemIndex" && self.is_list(object) {
                    // A fresh list box or combo box selects nothing (-1); the
                    // model's lists start as the control does.
                    *value = Value::Integer(-1);
                    return Ok(());
                }
                if STRING_PROPS.contains(&member) {
                    *value = Value::Str(Vec::new());
                    return Ok(());
                }
                if INT_PROPS.contains(&member) {
                    *value = Value::Integer(0);
                    return Ok(());
                }
                if BOOL_PROPS.contains(&member) {
                    *value = Value::Bool(false);
                    return Ok(());
                }
                *value = Value::Empty;
                Ok(())
            }
        }
    }

    fn dispatch_set(&mut self, object: &Rc<RefCell<VclObject>>, member: &str, value: &Value) -> Result<(), ValueError> {
        match member {
            "Parent" => {
                let id = object.borrow().id;
                let new_parent = value.as_object().and_then(|parent| {
                    let borrowed = parent.borrow();
                    borrowed.as_any().downcast_ref::<VclObject>().map(|vcl| vcl.id)
                });
                if let Some(old) = self.parent_of(id)
                    && let Some(old_object) = self.objects.get(&old).cloned()
                {
                    old_object.borrow_mut().children.retain(|child| *child != id);
                }
                object.borrow_mut().parent = new_parent;
                if let Some(new_parent) = new_parent
                    && let Some(parent_object) = self.objects.get(&new_parent).cloned()
                {
                    parent_object.borrow_mut().children.push(id);
                }
                Ok(())
            }
            "ModalResult" => {
                self.set_prop(
                    object,
                    member,
                    Value::Integer(xedit_script::values::to_i32(value).unwrap_or(0)),
                );
                Ok(())
            }
            _ => {
                self.set_prop(object, member, value.clone());
                Ok(())
            }
        }
    }

    fn dispatch_indexed_get(
        &self,
        object: &Rc<RefCell<VclObject>>,
        member: &str,
        args: &Args,
        value: &mut Value,
    ) -> Result<(), ValueError> {
        let index = arg_i32(args, 0).unwrap_or(0).max(0) as usize;
        match member {
            "Checked" => {
                *value = Value::Bool(object.borrow().checked_at(index));
                Ok(())
            }
            "State" => {
                *value = Value::Integer(if object.borrow().checked_at(index) { 1 } else { 0 });
                Ok(())
            }
            _ => {
                *value = Value::Empty;
                Ok(())
            }
        }
    }

    fn dispatch_indexed_set(
        &mut self,
        object: &Rc<RefCell<VclObject>>,
        member: &str,
        args: &Args,
        value: &Value,
    ) -> Result<(), ValueError> {
        let index = arg_i32(args, 0).unwrap_or(0).max(0) as usize;
        match member {
            "Checked" | "State" => {
                let checked = match member {
                    "State" => match value {
                        Value::Integer(state) => *state == 1,
                        other => xedit_script::values::to_i32(other).unwrap_or(0) == 1,
                    },
                    _ => match value {
                        Value::Bool(checked) => *checked,
                        other => xedit_script::values::to_i32(other).unwrap_or(0) != 0,
                    },
                };
                let mut borrowed = object.borrow_mut();
                if borrowed.checks.len() <= index {
                    borrowed.checks.resize(index + 1, false);
                }
                borrowed.checks[index] = checked;
                Ok(())
            }
            _ => Ok(()),
        }
    }
}

/// Whether an answer closes a dialog without a value (Cancel or escape).
fn input_cancelled(answer: &ScriptAnswer) -> bool {
    answer
        .button
        .as_deref()
        .is_some_and(|button| normalize_button(button) == "cancel")
        || answer.keys.contains(&VK_ESCAPE)
}

/// `string(Args.Values[i])` of a value, for the `MessageBox` texts.
fn value_text(value: &Value) -> String {
    String::from_utf8_lossy(&xedit_script::values::to_str(value).unwrap_or_default()).into_owned()
}

/// A button label with `&` removed and lower-cased: how the oracle answer
/// matching compares buttons.
fn normalize_button(label: &str) -> String {
    label
        .chars()
        .filter(|c| *c != '&')
        .collect::<String>()
        .trim()
        .to_ascii_lowercase()
}

/// Property-name defaults: a read of an unset string-shaped property is the
/// empty string, of an integer-shaped one `0`, of a boolean one `false`
/// (upstream reads the control's own defaults; the model answers the typed
/// zero when a script never set it).
const STRING_PROPS: &[&str] = &[
    "Caption",
    "Text",
    "Hint",
    "Name",
    "Filter",
    "FileName",
    "DefaultExt",
    "InitialDir",
    "Title",
    "ShortHint",
    "LongHint",
    "HelpFile",
];
const INT_PROPS: &[&str] = &[
    "Left",
    "Top",
    "Width",
    "Height",
    "ClientWidth",
    "ClientHeight",
    "Tag",
    "TabOrder",
    "ItemIndex",
    "MaxLength",
    "DropDownCount",
    "ItemHeight",
    "Columns",
    "SelStart",
    "SelLength",
    "Color",
    "Position",
    "BorderStyle",
    "FormStyle",
    "WindowState",
    "Cursor",
    "ModalResult",
    "LabelPosition",
    "PixelsPerInch",
    "FilterIndex",
    "Ctl3D",
    "HelpContext",
];
const BOOL_PROPS: &[&str] = &[
    "Enabled",
    "Visible",
    "KeyPreview",
    "ShowHint",
    "Checked",
    "Default",
    "Sorted",
    "AllowGrayed",
    "AutoSize",
    "WordWrap",
    "ReadOnly",
    "Multiline",
    "Scaled",
    "DoubleBuffered",
    "ParentBackground",
    "Proportional",
    "Stretch",
    "Center",
    "Transparent",
    "TabStop",
    "Cancel",
    "HotTrack",
    "RightClickSelect",
    "RowSelect",
    "ShowLines",
    "ToolTips",
    "OwnerData",
    "SimplePanel",
    "AutoHotkeys",
    "Tracking",
    "Beveled",
    "Flat",
    "Down",
    "Expanded",
];
/// The published properties that hold an object or a set/enum the model
/// keeps as the assigned value, with no typed default (an unset read is
/// `Unassigned`, as a nil reference reads upstream).
const OTHER_PROPS: &[&str] = &[
    "Items",
    "Lines",
    "Font",
    "EditLabel",
    "Parent",
    "PopupMenu",
    "ActiveControl",
    "Align",
    "Alignment",
    "Anchors",
    "BevelOuter",
    "DragMode",
    "Duplicates",
    "LabelSpacing",
    "Options",
    "PageControl",
    "Picture",
    "PixelFormat",
    "PopupMode",
    "ScrollBars",
    "Style",
    "StyleElements",
    "ViewStyle",
    "SimpleText",
    "ShortCut",
    "Selected",
    "Increment",
    "MinSize",
    "Size",
    "TransparentColor",
    "Subject",
    "Columns",
    "Count",
    "ImageIndex",
    "Spacing",
    "Margin",
    "Layout",
    "Kind",
    "GroupIndex",
    "Min",
    "Max",
    "Frequency",
    "Step",
    "Smooth",
    "Value",
    "MaxValue",
    "MinValue",
    "Interval",
];

/// The canonical spelling of a published property the model answers through
/// the RTTI-shaped fallback, matched case-insensitively as Delphi's
/// `GetPropInfo` matches (`Wordwrap` in `NIF - Batch processing.pas` is
/// `WordWrap`); `None` for a name that is no published property.
fn published_name(identifier: &str) -> Option<&'static str> {
    [STRING_PROPS, INT_PROPS, BOOL_PROPS, OTHER_PROPS]
        .into_iter()
        .flatten()
        .copied()
        .find(|name| name.eq_ignore_ascii_case(identifier))
}

impl Vcl for VclModel {
    fn make_form(&mut self, _file_name: &str) -> Result<Value, ValueError> {
        Err(ValueError::external_class(
            VCL_ERROR_CLASS,
            format!(
                "JvInterpreterMakeForm is not available without a GUI (script \"{}\")",
                self.script
            ),
        ))
    }

    fn run_form(&mut self, _file_name: &str) -> Result<Value, ValueError> {
        Err(ValueError::external_class(
            VCL_ERROR_CLASS,
            format!(
                "JvInterpreterRunForm is not available without a GUI (script \"{}\")",
                self.script
            ),
        ))
    }

    fn run_form_modal(&mut self, _file_name: &str) -> Result<i64, ValueError> {
        Err(ValueError::external_class(
            VCL_ERROR_CLASS,
            format!(
                "JvInterpreterRunFormModal is not available without a GUI (script \"{}\")",
                self.script
            ),
        ))
    }

    fn run_unit(&mut self, _file_name: &str) -> Result<Value, ValueError> {
        Err(ValueError::external_class(
            VCL_ERROR_CLASS,
            format!(
                "JvInterpreterRunUnit is not available without a GUI (script \"{}\")",
                self.script
            ),
        ))
    }

    fn install_classes(&mut self, classes: HashMap<String, Rc<ClassDef>>) {
        self.classes = classes;
    }

    fn member(
        &mut self,
        class: &str,
        member: &str,
        kind: MemberKind,
        value: &mut Value,
        args: &mut Args,
    ) -> Result<(), ValueError> {
        // A constructor: the receiver is the class value (`TForm.Create`) or
        // nil; the owner, when the call names one, is in the arguments.
        let class_receiver = matches!(args.obj, None | Some(Value::Class(_)));
        if kind == MemberKind::Get && (member == "Create" || member == "CreateNew") && class_receiver {
            let owner = args.values.first().cloned();
            *value = self.create_object(class, owner.as_ref())?;
            return Ok(());
        }
        let object = self.receiver(args)?;
        match kind {
            MemberKind::Get => {
                if member == "CheckAll" {
                    // `CheckAll(AState, AAllowGrayed, ADoInit)` of the check
                    // list: every entry takes the state (`cbChecked` = 1);
                    // entries beyond the explicit vector read it back.
                    let state = arg_i32(args, 0).unwrap_or(0);
                    self.set_prop(&object, "__check_all", Value::Bool(state == 1));
                    object.borrow_mut().checks.clear();
                    return Ok(());
                }
                if member == "Add" {
                    // `TMenuItem.Add`/`Items.Add`: the argument is the child
                    // object, re-parented under the receiver.
                    if let Some(Value::Object(child)) = args.values.first() {
                        let child_id = child.borrow().as_any().downcast_ref::<VclObject>().map(|vcl| vcl.id);
                        if let Some(child_id) = child_id
                            && let Some(child_object) = self.objects.get(&child_id).cloned()
                        {
                            let parent_id = object.borrow().id;
                            if let Some(old) = self.parent_of(child_id)
                                && let Some(old_object) = self.objects.get(&old).cloned()
                            {
                                old_object.borrow_mut().children.retain(|entry| *entry != child_id);
                            }
                            child_object.borrow_mut().parent = Some(parent_id);
                            object.borrow_mut().children.push(child_id);
                        }
                    }
                    return Ok(());
                }
                self.dispatch_get(&object, member, value)
            }
            MemberKind::Set => self.dispatch_set(&object, member, value),
            MemberKind::IndexedGet => self.dispatch_indexed_get(&object, member, args, value),
            MemberKind::IndexedSet => self.dispatch_indexed_set(&object, member, args, value),
        }
    }

    fn function(&mut self, _unit: &str, function: &str, value: &mut Value, args: &mut Args) -> Result<(), ValueError> {
        match function {
            "MessageDlg" | "MessageDlgPos" | "MessageDlgPosHelp" => {
                *value = Value::Integer(self.message_dlg(args)?);
                Ok(())
            }
            "ShowMessage" | "ShowMessageFmt" | "ShowMessagePos" => self.show_message(args),
            "InputQuery" => {
                *value = Value::Bool(self.input_query(args)?);
                Ok(())
            }
            "InputBox" => {
                *value = Value::Str(self.input_box(args)?);
                Ok(())
            }
            "CreateMessageDialog" => {
                *value = self.create_message_dialog(args)?;
                Ok(())
            }
            "Screen" => {
                if let Some(id) = self.screen {
                    *value = Value::Object(self.object(id)?);
                } else {
                    let screen = self.create_object("TScreen", None)?;
                    self.screen = Self::id_of(&screen);
                    *value = screen;
                }
                Ok(())
            }
            "MessageBox" => {
                *value = Value::Integer(self.message_box(args)?);
                Ok(())
            }
            _ => Err(ValueError::external_class(
                VCL_ERROR_CLASS,
                format!("{function} is not available without a GUI (script \"{}\")", self.script),
            )),
        }
    }
}
