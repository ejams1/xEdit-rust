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
//!   unit in a fresh interpreter, exactly as `JvInterpreterFm.pas` does
//!   (issue #81): the unit's source is read from the file name (or its unit
//!   name), compiled by the fresh program the run installs
//!   ([`VclModel::set_fresh_program`]), and `MakeForm`/`RunForm`/
//!   `RunFormModal` read the unit's `.dfm` (the text form, [`super::dfm`])
//!   into a `TJvInterpreterForm`-shaped object. `RunFormModal` drives the
//!   same recorded-answer modal loop as a script-built form, and `RunUnit`
//!   runs the compiled unit's `main` routine. What is not modelled: the DFM's
//!   event properties are stored (not fired, as every event of the model),
//!   including `OnCreate`, which upstream calls at the end of `LoadForm`;
//!   and a binary (`TPF0`) DFM is refused with a message naming it.
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
use std::path::{Path, PathBuf};
use std::rc::Rc;

use xedit_script::Interpreter;
use xedit_script::adapter::Args;
use xedit_script::error::IE_DFM_NOT_FOUND;
use xedit_script::values::{ClassDef, ScriptObject, Value, ValueError};

use super::{MemberKind, VCL_ERROR_CLASS, Vcl};
use crate::script::{ScriptAnswer, ScriptInteraction};
use crate::script_rtl::classes::{
    CollectionState, ComponentState, collection_item_of, insert_component, remove_component,
};
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
    /// The `TComponent` side of the object: its `Name`, `Tag`, `Owner` and
    /// owned components, the state the `Classes` unit's `TComponent` members
    /// read and write (`FindComponent` finds a control by its name).
    pub component: Rc<RefCell<ComponentState>>,
    /// The `TCollection` side of a `TListColumns`: the `Add`, `Items` and
    /// `Count` members of its `TCollection` ancestor belong to the `Classes`
    /// unit, whose object model is this state (`collection_of` recognizes
    /// the object through it).
    pub collection: Option<Rc<RefCell<CollectionState>>>,
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
            component: Rc::new(RefCell::new(ComponentState::new())),
            collection: (class_name == "TListColumns").then(|| {
                Rc::new(RefCell::new(CollectionState {
                    item_class: None,
                    items: Vec::new(),
                    updating: 0,
                }))
            }),
        }
    }

    /// Wraps a new object into the shared cell its script values hold and
    /// points its component state back at it.
    fn into_cell(self) -> Rc<RefCell<VclObject>> {
        let cell = Rc::new(RefCell::new(self));
        let object: Rc<RefCell<dyn ScriptObject>> = cell.clone();
        cell.borrow().component.borrow_mut().object = Some(Rc::downgrade(&object));
        cell
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

/// The factory of the interpreter the form runner compiles a unit in
/// (upstream's `TJvInterpreterFm.Create(Application)`): a fresh program with
/// the run's registrations (upstream's own adapter is empty and every lookup
/// falls back to the global one, which carries the same surface) and the VCL
/// model its calls go through. It shares the answers queue of the calling
/// run, so a dialog of the compiled unit takes the next recorded answer.
pub type FreshProgram = Box<dyn FnMut(Rc<RefCell<VecDeque<ScriptAnswer>>>) -> (Interpreter, Rc<RefCell<VclModel>>)>;

/// The headless VCL object model (the [`Vcl`] seam of issue #26).
pub struct VclModel {
    /// The script being run, for the error texts.
    script: String,
    /// The registered classes ([`Vcl::install_classes`]).
    classes: HashMap<String, Rc<ClassDef>>,
    /// Every live object by id; the object value a script holds is the same
    /// cell.
    objects: HashMap<i64, Rc<RefCell<VclObject>>>,
    /// The recorded answers not consumed yet, in order. Shared with the
    /// fresh model a `JvInterpreterRunUnit` creates, as the GUI has one
    /// answer stream for every interpreter of the run.
    answers: Rc<RefCell<VecDeque<ScriptAnswer>>>,
    /// The dialog the run stopped on, when it did.
    pending: Option<ScriptInteraction>,
    next_id: i64,
    /// The `Screen` singleton of the `Forms` unit's `Screen` function.
    screen: Option<i64>,
    /// The forms currently shown modal, innermost last (a button's
    /// `ModalResult` closes the innermost).
    modal_stack: Vec<i64>,
    /// The main form (`frmMain` of the script host, issue #25), made on
    /// first use.
    main_form: Option<i64>,
    /// Builds the interpreter `JvInterpreterMakeForm`/`RunUnit` compile in;
    /// `None` in a caller that registered no host surface (the tests and the
    /// standalone registrations), whose calls then refuse.
    fresh_program: Option<FreshProgram>,
}

impl VclModel {
    pub fn new(script: impl Into<String>, answers: Vec<ScriptAnswer>) -> VclModel {
        VclModel::with_answers(script, Rc::new(RefCell::new(answers.into())))
    }

    /// A model over an answers queue shared with another model (the one a
    /// `JvInterpreterRunUnit` creates).
    pub fn with_answers(script: impl Into<String>, answers: Rc<RefCell<VecDeque<ScriptAnswer>>>) -> VclModel {
        VclModel {
            script: script.into(),
            classes: HashMap::new(),
            objects: HashMap::new(),
            answers,
            pending: None,
            next_id: 0,
            screen: None,
            modal_stack: Vec::new(),
            main_form: None,
            fresh_program: None,
        }
    }

    /// Installs the factory of the fresh interpreter (`JvInterpreterFm.pas`'s
    /// form runner). The run that registers the script host's surface places
    /// it; a model without one refuses the four functions.
    pub fn set_fresh_program(&mut self, factory: FreshProgram) {
        self.fresh_program = Some(factory);
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
        self.answers.borrow().front().cloned()
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
        // Upstream tries the RTTI property after the indexed readers fail with
        // the index arguments dropped (`Args.Count := 0`, `JvInterpreter.pas:4461`),
        // then reads the result's default indexed property: `Columns[0]` reads
        // the property `Columns` and indexes the `TListColumns` it returns.
        // A member read carrying call arguments is a call, not a property
        // read; `GetValueRTTI` itself gates only on the receiver
        // (`Args.ObjTyp` is its only gate) and the caller applies `[i]` to
        // the value it returns.
        if !args.values.is_empty() && !args.indexed {
            return Ok(false);
        }
        let Some(name) = published_name(identifier) else {
            return Ok(false);
        };
        // Events are `tkMethod` properties, which `GetValueRTTI` does not
        // read (its `case` has no arm for them).
        if is_event_name(name) {
            return Ok(false);
        }
        // A column of a list view's `Columns` is a `TCollectionItem` of the
        // classes unit whose real class (`TListColumn`) adds published
        // properties (`Caption`, `Width`) over it: they live on the item.
        if let Some(item) = args.obj.as_ref().and_then(collection_item_of) {
            *value = item
                .borrow()
                .props
                .get(name)
                .cloned()
                .unwrap_or_else(|| property_default(name));
            return Ok(true);
        }
        let Some(object) = self.model_receiver(args) else {
            return Ok(false);
        };
        if self.is_array_property(&object, name) {
            return Ok(false);
        }
        self.dispatch_get(&object, name, value)?;
        Ok(true)
    }

    /// `SetValueRTTI` (`JvInterpreter.pas:4586`, `:5156`): the write half of
    /// [`Self::rtti_get`]. The event (`tkMethod`) branch is the evaluator's
    /// own (`ScriptObject::set_event`), reached before this.
    pub fn rtti_set(&mut self, identifier: &str, value: &Value, args: &Args) -> Result<bool, ValueError> {
        let Some(name) = published_name(identifier) else {
            return Ok(false);
        };
        if is_event_name(name) {
            return Ok(false);
        }
        if let Some(item) = args.obj.as_ref().and_then(collection_item_of) {
            item.borrow_mut().props.insert(name.to_owned(), value.clone());
            return Ok(true);
        }
        let Some(object) = self.model_receiver(args) else {
            return Ok(false);
        };
        if self.is_array_property(&object, name) {
            return Ok(false);
        }
        self.dispatch_set(&object, name, value)?;
        Ok(true)
    }

    /// A name that is an array property of the receiver's class, which no
    /// RTTI lookup answers (`GetPropInfo` lists published properties only):
    /// a check list's `Checked[i]`/`State[i]`/`ItemEnabled[i]`/`Header[i]`
    /// are its indexed members, while `Checked` of a check box is published.
    /// Answering the bare name made `clb.Checked[i] := b` index the boolean
    /// (`Array type required`, found by issue #25).
    fn is_array_property(&self, object: &Rc<RefCell<VclObject>>, name: &str) -> bool {
        self.is_a(object, "TCheckListBox") && matches!(name, "Checked" | "State" | "ItemEnabled" | "Header")
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
        let object = VclObject::new(id, class, class_name).into_cell();
        let parent = owner.and_then(|value| value.as_object()).and_then(|owner| {
            let borrowed = owner.borrow();
            borrowed.as_any().downcast_ref::<VclObject>().map(|vcl| vcl.id)
        });
        object.borrow_mut().parent = parent;
        // `TComponent.Create(AOwner)`: the owner holds the new component.
        if let Some(owner_state) = owner.and_then(crate::script_rtl::classes::component_of) {
            let state = object.borrow().component.clone();
            insert_component(&owner_state, &state);
        }
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
        let object = VclObject::new(id, class, class_name).into_cell();
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
        let component = object.borrow().component.clone();
        remove_component(&component);
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

    /// The `Text` of an edit control as a string, what `SelectAll` measures
    /// and `CopyToClipboard` reads.
    fn edit_text(&self, object: &Rc<RefCell<VclObject>>) -> String {
        self.prop_str(object, "Text")
            .map(|text| String::from_utf8_lossy(&text).into_owned())
            .unwrap_or_default()
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
        self.child_of_class(id, name, 0)
    }

    /// The child of `id` at `index` among those whose class chain contains
    /// `name` (the answer's `control_index`).
    fn child_of_class(&self, id: i64, name: &str, index: usize) -> Option<Rc<RefCell<VclObject>>> {
        self.descendants(id)
            .into_iter()
            .filter(|object| self.is_a(object, name))
            .nth(index)
    }

    /// The descendants of `id` in the order the oracle harness sees a form's
    /// child windows (`EnumChildWindows`): depth first, and at each level the
    /// children in reverse creation order, because a newly created child
    /// sits above the older ones in the z-order.
    fn descendants(&self, id: i64) -> Vec<Rc<RefCell<VclObject>>> {
        let mut found = Vec::new();
        self.collect_descendants(id, &mut found);
        found
    }

    fn collect_descendants(&self, id: i64, found: &mut Vec<Rc<RefCell<VclObject>>>) {
        let Some(object) = self.objects.get(&id) else {
            return;
        };
        let children = object.borrow().children.clone();
        for child in children.into_iter().rev() {
            if let Some(child_object) = self.objects.get(&child).cloned() {
                found.push(child_object);
                self.collect_descendants(child, found);
            }
        }
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
        } else if self.is_a(object, "TListView") {
            // The list view is owner-drawn: `Items` is a `TListItems` whose
            // `Count` the script sets, not a string list.
            let id = object.borrow().id;
            self.create_child(id, "TListItems")?
        } else {
            let class = self.class_of("TStringList")?;
            let mut list = crate::script_strings::StringListObject::new(class);
            // The rich edit's `Lines` answers `''` for a line it does not
            // have; a `TStringList` raises `EStringListError`
            // (`NIF - Batch textures replacement.pas` reads one past).
            list.set_lenient_index(self.is_a(object, "TRichEdit"));
            Value::Object(Rc::new(RefCell::new(list)))
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

    //=== The host's forms (issue #25) ========================================

    /// An object of a form class the registration tables do not carry
    /// (`TfrmMain`, `TfrmFileSelect`): a class of its own below `TForm`, so
    /// `ClassName` names it and every `TForm` member answers.
    fn create_form_of_class(&mut self, unit: &str, class_name: &str) -> Result<Rc<RefCell<VclObject>>, ValueError> {
        let parent = self.class_of("TForm")?;
        let class = Rc::new(ClassDef {
            unit: unit.to_owned(),
            name: class_name.to_owned(),
            parent: Some(parent),
        });
        let id = self.next_id;
        self.next_id += 1;
        let object = VclObject::new(id, class, class_name).into_cell();
        self.objects.insert(id, object.clone());
        Ok(object)
    }

    /// A component of a form built from its `.dfm`: owned and parented by
    /// `owner`, named, with its design-time properties.
    fn dfm_component(
        &mut self,
        owner: &Rc<RefCell<VclObject>>,
        class_name: &str,
        name: &str,
        props: &[(&str, Value)],
    ) -> Result<Rc<RefCell<VclObject>>, ValueError> {
        let value = self.create_object(class_name, Some(&Value::Object(owner.clone())))?;
        let id = Self::id_of(&value).ok_or_else(|| self.model_error("the component was not made".to_owned()))?;
        let object = self.object(id)?;
        object.borrow().component.borrow_mut().name = name.to_owned();
        for (prop, value) in props {
            self.set_prop(&object, prop, value.clone());
        }
        Ok(object)
    }

    /// `O2V(frmMain)`: the main form, one object for the whole run.
    /// Headless it shows nothing; a script reads and sets its properties
    /// and owns its own forms by it (`TForm.Create(frmMain)`).
    fn main_form(&mut self) -> Result<Value, ValueError> {
        if let Some(id) = self.main_form
            && let Ok(form) = self.object(id)
        {
            return Ok(Value::Object(form));
        }
        let form = self.create_form_of_class("xeMainForm", "TfrmMain")?;
        form.borrow().component.borrow_mut().name = "frmMain".to_owned();
        self.set_prop(&form, "Visible", Value::Bool(true));
        self.main_form = Some(form.borrow().id);
        Ok(Value::Object(form))
    }

    /// `O2V(TfrmFileSelect.Create(nil))`: a new selection form each call,
    /// with the components of `xeFileSelectForm.dfm` and its `FormCreate`
    /// (`CheckListBox1.MultiSelect := True`). The script fills
    /// `CheckListBox1` (found with `FindComponent`) and shows it modal; the
    /// recorded answers check the entries (the keys go to the check list)
    /// and click `OK`, whose `ModalResult` is `mrOk`; return clicks `OK` and
    /// escape closes it with `mrCancel` (its `FormKeyDown`).
    fn file_select_form(&mut self) -> Result<Value, ValueError> {
        let form = self.create_form_of_class("xeFileSelectForm", "TfrmFileSelect")?;
        form.borrow().component.borrow_mut().name = "frmFileSelect".to_owned();
        let text = |value: &str| Value::Str(value.as_bytes().to_vec());
        self.set_prop(&form, "Caption", text("Master/Plugin Selection"));
        self.set_prop(&form, "ClientHeight", Value::Integer(615));
        self.set_prop(&form, "ClientWidth", Value::Integer(332));
        self.set_prop(&form, "KeyPreview", Value::Bool(true));
        // `poMainFormCenter`.
        self.set_prop(&form, "Position", Value::Integer(5));
        self.dfm_component(
            &form,
            "TLabel",
            "Label1",
            &[
                ("Left", Value::Integer(8)),
                ("Top", Value::Integer(11)),
                ("Width", Value::Integer(33)),
                ("Height", Value::Integer(13)),
                ("Caption", text("Search")),
            ],
        )?;
        let list = self.dfm_component(
            &form,
            "TCheckListBox",
            "CheckListBox1",
            &[
                ("Left", Value::Integer(3)),
                ("Top", Value::Integer(40)),
                ("Width", Value::Integer(326)),
                ("Height", Value::Integer(535)),
                ("ItemHeight", Value::Integer(13)),
                ("TabOrder", Value::Integer(0)),
                ("MultiSelect", Value::Bool(true)),
            ],
        )?;
        self.dfm_component(
            &form,
            "TEdit",
            "edSearch",
            &[
                ("Left", Value::Integer(47)),
                ("Top", Value::Integer(8)),
                ("Width", Value::Integer(282)),
                ("Height", Value::Integer(21)),
                ("TabOrder", Value::Integer(1)),
            ],
        )?;
        self.dfm_component(
            &form,
            "TCheckBox",
            "cbBackup",
            &[
                ("Left", Value::Integer(8)),
                ("Top", Value::Integer(582)),
                ("Width", Value::Integer(113)),
                ("Height", Value::Integer(17)),
                ("Caption", text("Backup plugins")),
                ("Checked", Value::Bool(true)),
                ("TabOrder", Value::Integer(2)),
                ("Visible", Value::Bool(false)),
            ],
        )?;
        self.dfm_component(
            &form,
            "TButton",
            "btnOK",
            &[
                ("Left", Value::Integer(249)),
                ("Top", Value::Integer(582)),
                ("Width", Value::Integer(75)),
                ("Height", Value::Integer(25)),
                ("Caption", text("OK")),
                ("ModalResult", Value::Integer(1)),
                ("TabOrder", Value::Integer(3)),
            ],
        )?;
        let menu = self.dfm_component(&form, "TPopupMenu", "PopupMenu1", &[])?;
        for (name, caption) in [
            ("SelectAll1", "Select All"),
            ("SelectNone1", "Select None"),
            ("InvertSelection1", "Invert Selection"),
        ] {
            let item = self.dfm_component(&form, "TMenuItem", name, &[("Caption", text(caption))])?;
            // The menu's items: the item is the menu's child in the tree
            // (`Items.Add`), and the form still owns it.
            let (item_id, menu_id) = (item.borrow().id, menu.borrow().id);
            if let Some(old) = self.parent_of(item_id)
                && let Some(old_object) = self.objects.get(&old).cloned()
            {
                old_object.borrow_mut().children.retain(|child| *child != item_id);
            }
            item.borrow_mut().parent = Some(menu_id);
            menu.borrow_mut().children.push(item_id);
        }
        self.set_prop(&list, "PopupMenu", Value::Object(menu.clone()));
        // The VCL focuses the first control in the tab order when the form
        // shows: the check list.
        self.set_prop(&form, "ActiveControl", Value::Object(list.clone()));
        Ok(Value::Object(form))
    }

    //=== The form runner (JvInterpreterFm.pas) ===============================

    /// A program for `MakeForm`/`RunUnit` to compile in, or the step-9
    /// refusal when the caller registered no factory.
    fn fresh_program(&mut self, function: &str) -> Result<(Interpreter, Rc<RefCell<VclModel>>), ValueError> {
        match self.fresh_program.as_mut() {
            Some(factory) => Ok(factory(self.answers.clone())),
            None => Err(ValueError::external_class(
                VCL_ERROR_CLASS,
                format!("{function} is not available without a GUI (script \"{}\")", self.script),
            )),
        }
    }

    /// `TJvInterpreterForm.CreateNew(Application)`: the form a unit's DFM is
    /// read into. Its runtime class -- and so its window class, which the
    /// recorded answers name -- is `TJvInterpreterForm`, not the DFM's root
    /// class (upstream reads the DFM into this instance).
    fn new_interpreter_form(&mut self) -> Result<Rc<RefCell<VclObject>>, ValueError> {
        self.create_form_of_class("JvInterpreterFm", "TJvInterpreterForm")
    }

    /// `TJvInterpreterFm.LoadForm`: the DFM's root properties and its
    /// components, owned by the form.
    fn load_dfm(&mut self, text: &[u8], form: &Rc<RefCell<VclObject>>) -> Result<(), ValueError> {
        let component = super::dfm::parse(text).map_err(|message| ValueError::external_class("Exception", message))?;
        self.apply_dfm_properties(form, &component.properties)?;
        for child in &component.children {
            self.add_dfm_component(form, child)?;
        }
        Ok(())
    }

    /// One nested DFM component: owned by the form (`TReader` reads every
    /// nested component with the root as its owner), parented to the
    /// component that holds it when it is a control, then its properties and
    /// children.
    fn add_dfm_component(
        &mut self,
        owner: &Rc<RefCell<VclObject>>,
        spec: &super::dfm::Component,
    ) -> Result<(), ValueError> {
        let value = self.create_object(&spec.class_name, None)?;
        let id = Self::id_of(&value).ok_or_else(|| self.model_error("the component was not made".to_owned()))?;
        let object = self.object(id)?;
        if !spec.name.is_empty() {
            object.borrow().component.borrow_mut().name = spec.name.clone();
        }
        {
            let owner_state = owner.borrow().component.clone();
            let state = object.borrow().component.clone();
            insert_component(&owner_state, &state);
        }
        if self.is_a(&object, "TControl") {
            self.dispatch_set(&object, "Parent", &Value::Object(owner.clone()))?;
        }
        self.apply_dfm_properties(&object, &spec.properties)?;
        for child in &spec.children {
            self.add_dfm_component(&object, child)?;
        }
        Ok(())
    }

    /// The design-time properties of one DFM component. A method value
    /// (`OnCreate = FormCreate`) goes to the object's event slot; a string
    /// list fills the control's `Items`/`Lines`; anything else is the
    /// model's ordinary property state.
    fn apply_dfm_properties(
        &mut self,
        object: &Rc<RefCell<VclObject>>,
        properties: &[(String, super::dfm::Value)],
    ) -> Result<(), ValueError> {
        for (name, value) in properties {
            match name.as_str() {
                "Name" => {
                    if let super::dfm::Value::Str(text) = value {
                        object.borrow().component.borrow_mut().name = String::from_utf8_lossy(text).into_owned();
                    }
                }
                "Tag" => {
                    if let super::dfm::Value::Integer(tag) = value {
                        object.borrow().component.borrow_mut().tag = *tag as i32;
                    }
                }
                "Items.Strings" | "Lines.Strings" => {
                    if let super::dfm::Value::List(lines) = value {
                        let items = self.items_of(object)?;
                        if let Some(list) = crate::script_strings::list_of(&items) {
                            for line in lines {
                                crate::script_strings::borrow_list_mut(&list).add_bytes(line.clone())?;
                            }
                        }
                    }
                }
                _ if is_event_name(name) => {
                    if let super::dfm::Value::Ident(handler) = value {
                        object.borrow_mut().events.insert(name.clone(), handler.clone());
                    }
                }
                _ => {
                    if let Some(value) = dfm_value(value) {
                        self.set_prop(object, name, value);
                    }
                }
            }
        }
        Ok(())
    }

    /// `TJvInterpreterFm.GetUnitSource`: the unit file the form runner
    /// compiles. The file name as given (with `.pas` when it has no
    /// extension) is read when it exists, else its base name is looked up in
    /// the file's folder and on the `PATH`; the same is then tried with the
    /// unit name (`ChangeFileExt(ExtractFileName(FileName), '')`).
    fn unit_file(file_name: &str) -> Option<PathBuf> {
        let with_extension = |name: &str| match Path::new(name).extension() {
            Some(_) => name.to_owned(),
            None => format!("{name}.pas"),
        };
        let directory = Path::new(file_name)
            .parent()
            .map(|parent| parent.to_string_lossy().into_owned())
            .unwrap_or_default();
        let first = with_extension(file_name);
        if Path::new(&first).is_file() {
            return Some(PathBuf::from(first));
        }
        if let Some(base) = Path::new(&first)
            .file_name()
            .map(|base| base.to_string_lossy().into_owned())
            && let Some(found) = find_in_path(&base, &directory)
        {
            return Some(found);
        }
        let unit = unit_name(file_name);
        let second = format!("{unit}.pas");
        if Path::new(&second).is_file() {
            return Some(PathBuf::from(second));
        }
        find_in_path(&second, &directory)
    }

    /// `CreateDfmStream`'s lookup:
    /// `FindInPath(ChangeFileExt(UnitName, '.dfm'), ExtractFilePath(FileName))`.
    fn dfm_file(file_name: &str) -> Option<PathBuf> {
        let directory = Path::new(file_name)
            .parent()
            .map(|parent| parent.to_string_lossy().into_owned())
            .unwrap_or_default();
        find_in_path(&format!("{}.dfm", unit_name(file_name)), &directory)
    }

    /// `JvInterpreterMakeForm`: compile the unit in a fresh program and read
    /// its DFM into a new interpreter form. The form is invisible until
    /// `Show`.
    fn interpreter_make_form(&mut self, file_name: &str) -> Result<Value, ValueError> {
        let unit = unit_name(file_name);
        let Some(path) = Self::unit_file(file_name) else {
            return Err(ValueError::interp_n(xedit_script::error::IE_UNIT_NOT_FOUND, unit));
        };
        let source = std::fs::read(&path)
            .map(|bytes| xedit_script::host::source_text(&bytes))
            .map_err(|error| ValueError::external_class("EFOpenError", format!("{}: {error}", path.display())))?;
        let (mut program, _model) = self.fresh_program("JvInterpreterMakeForm")?;
        // The fresh program has no host unit source hook: a `uses` of the
        // unit resolves to the empty `unit X; end.` its interface section
        // gets in `TJvInterpreterFm.GetUnitSource`.
        program.set_unit_source(|name| Some(xedit_script::host::host_unit_stub(name)));
        program.compile(&source).map_err(exec_error)?;
        // `Compile` comes first: a unit that does not compile never looks its
        // DFM up (`MakeForm` then `LoadForm` -> `CreateDfmStream`).
        let Some(dfm) = Self::dfm_file(file_name) else {
            return Err(ValueError::interp_n(IE_DFM_NOT_FOUND, unit));
        };
        let dfm_text = std::fs::read(&dfm)
            .map_err(|error| ValueError::external_class("EFOpenError", format!("{}: {error}", dfm.display())))?;
        let form = self.new_interpreter_form()?;
        self.load_dfm(&dfm_text, &form)?;
        self.set_prop(&form, "Visible", Value::Bool(false));
        Ok(Value::Object(form))
    }

    /// `JvInterpreterRunUnit`: compile the unit's source in a fresh program
    /// and run its `main` routine. The function's result is unassigned, as
    /// upstream's `RunUnit` never assigns one.
    fn interpreter_run_unit(&mut self, file_name: &str) -> Result<Value, ValueError> {
        let unit = unit_name(file_name);
        let Some(path) = Self::unit_file(file_name) else {
            return Err(ValueError::interp_n(xedit_script::error::IE_UNIT_NOT_FOUND, unit));
        };
        let source = std::fs::read(&path)
            .map(|bytes| xedit_script::host::source_text(&bytes))
            .map_err(|error| ValueError::external_class("EFOpenError", format!("{}: {error}", path.display())))?;
        let (mut program, model) = self.fresh_program("JvInterpreterRunUnit")?;
        program.set_unit_source(|name| Some(xedit_script::host::host_unit_stub(name)));
        let result = match program.compile(&source) {
            Ok(()) => program.run_main(),
            Err(error) => Err(error),
        };
        // A dialog of the unit carries its question to the calling run, so
        // `script.resume` can answer it and run the registry again.
        if let Some(pending) = model.borrow_mut().take_pending() {
            self.pending = Some(pending);
        }
        result.map_err(exec_error)?;
        Ok(Value::Empty)
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
        let next = self.answers.borrow().front().cloned();
        let Some(answer) = next else {
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
            self.answers.borrow_mut().pop_front();
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

    /// `SysUtils.ShowException(ExceptObject, ExceptAddr)` of a GUI program:
    /// `MessageBox(0, <ExceptionErrorMessage>, SExceptTitle, MB_OK or
    /// MB_ICONSTOP or MB_TASKMODAL)`, a box of the class `#32770` titled
    /// `Application Error` with `OK`; the call returns when it closes.
    fn show_exception(&mut self, args: &Args) -> Result<(), ValueError> {
        let (class, message) = match args.values.first().and_then(Value::as_object) {
            Some(object) => {
                let borrowed = object.borrow();
                let message = borrowed
                    .as_any()
                    .downcast_ref::<crate::script_rtl::sysutils::ExceptionObject>()
                    .map(|exception| exception.message.clone())
                    .unwrap_or_default();
                (borrowed.class().name.clone(), message)
            }
            None => (String::new(), String::new()),
        };
        let offered: Vec<&(&str, &str, i32)> = MSG_BUTTONS.iter().filter(|(name, _, _)| *name == "ok").collect();
        let buttons = vec!["OK".to_owned()];
        let answer = self.ask(
            "#32770",
            "Application Error",
            format!("Exception {class} in module  at 00000000.\r\n{message}."),
            buttons.clone(),
            buttons.first().cloned(),
            "message",
        )?;
        self.resolve_button(&answer, &offered, "Application Error")?;
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
        loop {
            if modal_result(&form) != 0 {
                break;
            }
            let next = self.answers.borrow().front().and_then(|next| {
                (next.repeat && next.class == class_name && title.contains(next.title.as_str())).then(|| next.clone())
            });
            let Some(next) = next else {
                break;
            };
            self.answers.borrow_mut().pop_front();
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
    /// the child the text and the keys go to instead, and its
    /// `control_index` the position among the children of that class (the
    /// harness posts to that child: a form on its desktop has no focused
    /// control).
    fn apply_answer(&mut self, form_id: i64, answer: &ScriptAnswer) -> Result<(), ValueError> {
        let target = match &answer.control {
            Some(class) => {
                let Some(control) = self.child_of_class(form_id, class, answer.control_index) else {
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

    /// The first edit-shaped control (the oracle harness's `set_edit_text`
    /// targets the first edit control of the window).
    fn first_edit_child(&self, form_id: i64) -> Option<Rc<RefCell<VclObject>>> {
        self.descendants(form_id).into_iter().find(|object| {
            self.is_a(object, "TCustomEdit")
                || self.is_a(object, "TLabeledEdit")
                || self.is_a(object, "TMemo")
                || self.is_a(object, "TCustomComboBox")
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
                    let index = (self.prop_i32(&active, "ItemIndex", self.default_item_index(&active)) + step).max(0);
                    let index = if count > 0 { index.min(count - 1) } else { index };
                    self.set_item_index(&active, index)?;
                }
            }
            VK_SPACE => {
                if self.is_a(&active, "TCheckListBox") {
                    // `TCheckListBox.KeyPress` toggles the entry at
                    // `ItemIndex`; nothing happens while nothing is selected.
                    // A multi-select list nobody moved has its caret on the
                    // first entry (parity #41: `Copy as override.pas` checks
                    // `<new file>` with one Space, as the release does).
                    let index = self.prop_i32(&active, "ItemIndex", self.default_item_index(&active));
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

    /// `ItemIndex := index` on a list control, with the `Text` follow of
    /// `TCustomComboBox.SetItemIndex`: the selected item's text, `''` for
    /// -1 (the script sets a combo's index and later reads `.Text`,
    /// `LODGen.pas`'s `cbWorld.Text`). A list box has no `Text` to follow.
    fn set_item_index(&mut self, object: &Rc<RefCell<VclObject>>, index: i32) -> Result<(), ValueError> {
        self.set_prop(object, "ItemIndex", Value::Integer(index));
        if !self.is_a(object, "TCustomComboBox") {
            return Ok(());
        }
        if index == -1 {
            self.set_prop(object, "Text", Value::Str(Vec::new()));
        } else if index >= 0 && index < self.list_count(object) {
            let items = self.items_of(object)?;
            let text = crate::script_strings::list_of(&items)
                .map(|list| crate::script_strings::borrow_list(&list).string_at(index as i64))
                .unwrap_or_default();
            self.set_prop(object, "Text", Value::Str(text.into_bytes()));
        }
        Ok(())
    }

    /// The `ItemIndex` of a list nobody moved: -1 (nothing selected), but
    /// the caret's 0 for a multi-select list, whose `ItemIndex` is the caret
    /// (`LB_GETCARETINDEX`): the first Down of an answer lands on the second
    /// entry there (`TfrmFileSelect`'s check list, `host-dialogs`).
    fn default_item_index(&self, object: &Rc<RefCell<VclObject>>) -> i32 {
        if self.prop_bool(object, "MultiSelect", false) {
            0
        } else {
            -1
        }
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
        self.descendants(form_id)
            .into_iter()
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
            // `TCustomEdit.SelectAll`: the whole text becomes the selection,
            // which `CopyToClipboard` then copies. The VCL's edit keeps the
            // selection as `SelStart`/`SelLength`; so does the model, so a
            // script that selects less copies less.
            "SelectAll" => {
                if self.is_a(object, "TCustomEdit") {
                    let length = self.edit_text(object).chars().count() as i32;
                    self.set_prop(object, "SelStart", Value::Integer(0));
                    self.set_prop(object, "SelLength", Value::Integer(length));
                }
                Ok(())
            }
            // `TCustomEdit.CopyToClipboard`: the selected text to the real
            // clipboard, as the VCL does (a script reads it back with
            // `Clipboard.AsText`, which the RTL's `SetClipboardText` shares).
            "CopyToClipboard" => {
                if self.is_a(object, "TCustomEdit") {
                    let text = self.edit_text(object);
                    let start = self.prop_i32(object, "SelStart", 0).max(0) as usize;
                    let length = self.prop_i32(object, "SelLength", 0).max(0) as usize;
                    let selected: String = if length == 0 {
                        String::new()
                    } else {
                        text.chars().skip(start).take(length).collect()
                    };
                    crate::script_misc::set_clipboard_text(&selected);
                }
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
                if member == "Parent" {
                    // A published `TControl.Parent`; scripts walk up from a
                    // control to the form that owns it (`TForm(cmb.Parent)`).
                    *value = match self.parent_of(object.borrow().id) {
                        Some(parent) => Value::Object(self.object(parent)?),
                        None => Value::Null,
                    };
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
                if member == "Items" && self.is_a(object, "TCustomListView") {
                    // A list view's `Items` is its `TListItems`, not a string
                    // list: `Items.Count := ...` is the owner-data count
                    // (`Create Shared Infos.pas`), and `Items[j]` a row.
                    let id = object.borrow().id;
                    if let Some(items) = self.first_child_of_class(id, "TListItems") {
                        *value = Value::Object(items);
                    } else {
                        *value = self.create_child(id, "TListItems")?;
                    }
                    return Ok(());
                }
                if member == "Columns" && self.is_a(object, "TCustomListView") {
                    // `Columns` is a published `TListColumns` property the
                    // tables do not register; the column objects live under
                    // the list view (`Columns.Add`, `Columns[0].Width`).
                    let id = object.borrow().id;
                    if let Some(columns) = self.first_child_of_class(id, "TListColumns") {
                        *value = Value::Object(columns);
                    } else {
                        *value = self.create_child(id, "TListColumns")?;
                    }
                    return Ok(());
                }
                if member == "Items" || member == "Lines" {
                    *value = self.items_of(object)?;
                    return Ok(());
                }
                if member == "Text"
                    && self.is_a(object, "TCustomComboBox")
                    && let Some(items) = self.prop(object, "Items")
                    && let Some(list) = crate::script_strings::list_of(&items)
                {
                    // A `csDropDownList` combo's text is the selected item's
                    // (`TComboBox.GetText`): the batch NIF scripts read it
                    // back (`StrToInt(cmbDigits.Text)`).
                    let index = self.prop_i32(object, "ItemIndex", self.default_item_index(object));
                    let list = crate::script_strings::borrow_list(&list);
                    if index >= 0 && (index as usize) < list.count() {
                        *value = Value::Str(list.string_at(i64::from(index)).into_bytes());
                        return Ok(());
                    }
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
                    *value = Value::Integer(self.default_item_index(object));
                    return Ok(());
                }
                if member == "Options" {
                    // A set the script extends (`dlg.Options := dlg.Options +
                    // [ofOverwritePrompt]`): a file dialog starts with
                    // `[ofHideReadOnly, ofEnableSizing]` (bits 2 and 19 of
                    // `TOpenOption`), other controls with the empty set.
                    let default = if self.is_a(object, "TOpenDialog") {
                        (1 << 2) | (1 << 19)
                    } else {
                        0
                    };
                    *value = Value::Set(default);
                    return Ok(());
                }
                if member == "Anchors" {
                    // `[akLeft, akTop]`.
                    *value = Value::Set(0b11);
                    return Ok(());
                }
                *value = property_default(member);
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
            "ItemIndex" => self.set_item_index(object, xedit_script::values::to_i32(value).unwrap_or(-1)),
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
        if member == "Items" && self.is_a(object, "TListColumns") {
            // `Columns[i]`: the column at that position of the collection.
            let children = object.borrow().children.clone();
            if let Some(child) = children.get(index)
                && let Some(child_object) = self.objects.get(child).cloned()
            {
                *value = Value::Object(child_object);
            }
            return Ok(());
        }
        match member {
            "Checked" => {
                *value = Value::Bool(object.borrow().checked_at(index));
                Ok(())
            }
            "State" => {
                *value = Value::Integer(if object.borrow().checked_at(index) { 1 } else { 0 });
                Ok(())
            }
            // `Columns[0]`/`Items[0]`: the default indexed property's value
            // of the collection, the child at the index.
            "Items" if self.is_a(object, "TListColumns") => {
                *value = self.child_at(object, index);
                Ok(())
            }
            "Item" if self.is_a(object, "TListItems") => {
                *value = self.child_at(object, index);
                Ok(())
            }
            _ => {
                *value = Value::Empty;
                Ok(())
            }
        }
    }

    /// The `index`th child object of `object` as a value (`Null` past the
    /// end).
    fn child_at(&self, object: &Rc<RefCell<VclObject>>, index: usize) -> Value {
        object
            .borrow()
            .children
            .get(index)
            .and_then(|id| self.objects.get(id))
            .map(|child| Value::Object(child.clone()))
            .unwrap_or(Value::Null)
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

/// The value an unset published property reads: the empty string, `0` or
/// `false` by its shape, `Unassigned` for the object- and set-shaped ones.
fn property_default(name: &str) -> Value {
    if STRING_PROPS.contains(&name) {
        Value::Str(Vec::new())
    } else if INT_PROPS.contains(&name) {
        Value::Integer(0)
    } else if BOOL_PROPS.contains(&name) {
        Value::Bool(false)
    } else {
        Value::Empty
    }
}

/// `ChangeFileExt(ExtractFileName(FileName), '')`: the unit name a form
/// runner reports and looks its `.dfm` up by.
fn unit_name(file_name: &str) -> String {
    Path::new(file_name)
        .file_stem()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_default()
}

/// `JvJCLUtils.FindInPath`: the file in `directory` first, then in every
/// `PATH` entry. Upstream does not check the current directory here (the
/// callers' `FileExists` does that first).
fn find_in_path(name: &str, directory: &str) -> Option<PathBuf> {
    if !directory.is_empty() {
        let candidate = Path::new(directory).join(name);
        if candidate.is_file() {
            return Some(candidate);
        }
    }
    let path = std::env::var_os("PATH")?;
    std::env::split_paths(&path)
        .map(|entry| entry.join(name))
        .find(|candidate| candidate.is_file())
}

/// One DFM property value as the model's value; `None` for the kinds the
/// model keeps no state for (a collection, a binary blob).
fn dfm_value(value: &super::dfm::Value) -> Option<Value> {
    Some(match value {
        super::dfm::Value::Str(text) => Value::Str(text.clone()),
        super::dfm::Value::Integer(integer) => match i32::try_from(*integer) {
            Ok(integer) => Value::Integer(integer),
            Err(_) => Value::Int64(*integer),
        },
        super::dfm::Value::Float(float) => Value::Double(*float),
        super::dfm::Value::Ident(ident) if ident.eq_ignore_ascii_case("nil") => Value::Null,
        super::dfm::Value::Ident(ident) if ident.eq_ignore_ascii_case("true") => Value::Bool(true),
        super::dfm::Value::Ident(ident) if ident.eq_ignore_ascii_case("false") => Value::Bool(false),
        super::dfm::Value::Ident(ident) => Value::Str(ident.clone().into_bytes()),
        super::dfm::Value::Set(members) => Value::Str(members.join(",").into_bytes()),
        super::dfm::Value::List(lines) => Value::Str(lines.concat()),
        super::dfm::Value::Collection(_) | super::dfm::Value::Binary(_) => return None,
    })
}

/// An error of the fresh program a form runner compiles in, as the calling
/// script sees it: the oracle's `EJvInterpreterError` (or the exception the
/// unit raised) with its message, which already carries the unit and line
/// of a compile failure.
fn exec_error(error: xedit_script::ExecError) -> ValueError {
    // An interpreter error keeps its class; another exception of the fresh
    // run (`EConvertError`, ...) has a dynamic class name the `ValueError`
    // shape cannot carry, so it reports as `Exception` with its message.
    let class = if error.class_name() == "EJvInterpreterError" {
        "EJvInterpreterError"
    } else {
        "Exception"
    };
    ValueError::external_class(class, error.message)
}

impl Vcl for VclModel {
    fn make_form(&mut self, file_name: &str) -> Result<Value, ValueError> {
        self.interpreter_make_form(file_name)
    }

    fn run_form(&mut self, file_name: &str) -> Result<Value, ValueError> {
        let value = self.interpreter_make_form(file_name)?;
        // `Result := MakeForm(FileName); Result.Show;`
        if let Some(id) = Self::id_of(&value) {
            let form = self.object(id)?;
            self.set_prop(&form, "Visible", Value::Bool(true));
        }
        Ok(value)
    }

    fn run_form_modal(&mut self, file_name: &str) -> Result<i64, ValueError> {
        // `with MakeForm(FileName) do try Result := ShowModal; finally Free; end;`
        let value = self.interpreter_make_form(file_name)?;
        let id = Self::id_of(&value).ok_or_else(|| self.model_error("the form was not made".to_owned()))?;
        let result = self.show_modal(id);
        self.release(id);
        result.map(i64::from)
    }

    fn run_unit(&mut self, file_name: &str) -> Result<Value, ValueError> {
        self.interpreter_run_unit(file_name)
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
                    // `TListColumns.Add`: a new column, returned so a chained
                    // `Columns.Add.Caption := ...` writes the new object.
                    if self.is_a(&object, "TListColumns") {
                        let parent_id = object.borrow().id;
                        *value = self.create_child(parent_id, "TListColumn")?;
                        return Ok(());
                    }
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
            "ShowException" => self.show_exception(args),
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
            "frmMain" => {
                *value = self.main_form()?;
                Ok(())
            }
            "frmFileSelect" => {
                *value = self.file_select_form()?;
                Ok(())
            }
            _ => Err(ValueError::external_class(
                VCL_ERROR_CLASS,
                format!("{function} is not available without a GUI (script \"{}\")", self.script),
            )),
        }
    }
}
