// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

// Ported from xEdit: External/jvcl/jvcl/run/JvInterpreter_Classes.pas (the
// `Classes` unit: TList, TPersistent, TCollectionItem, TCollection, TStrings,
// TStringList, the TStream family, TComponent, the eighteen constants and
// the two event-type registrations)

//! `JvInterpreter_Classes.pas`: the `Classes` unit of the script dialect.
//!
//! The list and string-list objects keep *values*, where upstream stores
//! raw pointers (`V2P` of whatever a script passes): a `TList` item is the
//! value of the `Add` call and `Items[i]` hands it back unchanged, and a
//! `TStrings` line carries the `AddObject` object as a value. Each object
//! holds its mutable state behind a shared `Rc<RefCell<...>>`, the pattern
//! the data format adapter uses, so an adapter closure reaches it with a
//! downcast of the shared borrow and never through a pointer cast.
//!
//! What the port answers differently:
//!
//! * `TList.Sort`, `TStrings.StringsAdapter` (write) and the `TStream`
//!   component-streaming methods (`ReadComponent`, `WriteComponent`, the
//!   `*Res` and `WriteDescendent*` forms) raise upstream's own
//!   `NotImplemented` text, as they do upstream.
//! * `TComponent`'s `ComponentState`/`ComponentStyle` raise
//!   `NotImplemented('TComponent.ComponentState')` -- upstream's own body,
//!   copy-paste text and all.
//! * A `TPersistent.Assign` on a class without an override raises the
//!   RTL's `EConvertError` text (`Cannot assign a X to a Y`); `TStrings`
//!   overrides it, as upstream.
//! * `TCollection.Add` builds a `TCollectionItem` of the collection's item
//!   class when it is a registered one (the VCL item classes are step 9's);
//!   upstream instantiates the script's class through
//!   `TCollectionItemClass`.
//! * `TStrings.LoadFromFile` reads the bytes as they are, strips a UTF-8
//!   BOM and decodes a UTF-16 BOM (the port's strings are bytes); a file in
//!   another code page is not re-encoded.
//! * `TStringList.Sort` and the sorted `Add`/`Find`/`IndexOf` compare with
//!   the locale case-insensitive collation (`AnsiCompareText`), which is
//!   `CaseSensitive = False`, the RTL default.

use std::cell::RefCell;
use std::rc::Rc;

use xedit_script::adapter::{Adapter, Args};
use xedit_script::lists::list_value;
use xedit_script::strings::{Duplicates, StringList, string_list_value};
use xedit_script::values::{ClassDef, ScriptObject, Value, ValueError, to_bool, to_i32, to_str};

use crate::script::ScriptRunState;

use super::{
    BYREF, EMPTY, NULL, arg, arg_i32, arg_str, arg_text, constant, default_get, default_set, index_get, index_set,
    method, setter,
};

/// `cClasses = 'Classes'` (`JvInterpreter_Classes.pas:1164`).
const UNIT: &str = "Classes";

/// `SReadError`.
const S_READ_ERROR: &str = "Stream read error";
/// `SWriteError`.
const S_WRITE_ERROR: &str = "Stream write error";

/// `dupIgnore`/`dupAccept`/`dupError`.
const DUP_IGNORE: i32 = 0;
const DUP_ACCEPT: i32 = 1;
const DUP_ERROR: i32 = 2;

/// `fmOpen*` modes.
const FM_OPEN_READ: i32 = 0x0000;
const FM_OPEN_WRITE: i32 = 0x0001;
const FM_OPEN_READWRITE: i32 = 0x0002;

//=== The shared list objects =================================================

/// The object of a `TList` value: step 5's `ScriptList`, which the host
/// functions of `xejviScriptAdapterResource.pas` fill through the same
/// [`List`] trait.
pub fn list_object(value: &Value) -> Option<Rc<RefCell<dyn ScriptObject>>> {
    let object = value.as_object()?.clone();
    object.borrow().list()?;
    Some(object)
}

/// The object of a `TStrings`/`TStringList` value: step 5's
/// `ScriptStringList`, which is also the value `TwbFastStringList.Create`
/// builds (the resource adapter's subclass of `TStringList`).
pub fn strings_object(value: &Value) -> Option<Rc<RefCell<dyn ScriptObject>>> {
    let object = value.as_object()?.clone();
    object.borrow().string_list()?;
    Some(object)
}

//=== TStream =================================================================

/// The backing store of a `TStream`.
pub enum StreamKind {
    File(std::fs::File),
    Memory(Vec<u8>),
    Text(Vec<u8>),
}

pub struct StreamState {
    pub kind: StreamKind,
    pub pos: u64,
}

pub struct StreamObject {
    class: Rc<ClassDef>,
    state: Rc<RefCell<StreamState>>,
}

impl ScriptObject for StreamObject {
    fn class(&self) -> Rc<ClassDef> {
        self.class.clone()
    }
    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
    fn as_any_mut(&mut self) -> &mut dyn std::any::Any {
        self
    }
}

pub fn stream_of(value: &Value) -> Option<Rc<RefCell<StreamState>>> {
    let object = value.as_object()?;
    let borrowed = object.borrow();
    borrowed
        .as_any()
        .downcast_ref::<StreamObject>()
        .map(|stream| stream.state.clone())
}

//=== TComponent, TCollection =================================================

pub struct ComponentState {
    pub name: String,
    pub tag: i32,
    pub owner: Option<Rc<RefCell<ComponentState>>>,
    pub components: Vec<Rc<RefCell<ComponentState>>>,
    pub component_index: i32,
    pub design_info: i32,
}

pub struct ComponentObject {
    class: Rc<ClassDef>,
    state: Rc<RefCell<ComponentState>>,
}

impl ScriptObject for ComponentObject {
    fn class(&self) -> Rc<ClassDef> {
        self.class.clone()
    }
    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
    fn as_any_mut(&mut self) -> &mut dyn std::any::Any {
        self
    }
}

/// The state of a `TComponent` value.
pub fn component_of(value: &Value) -> Option<Rc<RefCell<ComponentState>>> {
    let object = value.as_object()?;
    let borrowed = object.borrow();
    borrowed
        .as_any()
        .downcast_ref::<ComponentObject>()
        .map(|component| component.state.clone())
}

pub struct CollectionState {
    pub item_class: Option<Value>,
    pub items: Vec<Rc<RefCell<CollectionItemState>>>,
    pub updating: i32,
}

pub struct CollectionItemState {
    pub collection: Option<Value>,
    pub id: i32,
    pub index: i32,
    pub display_name: String,
}

pub struct CollectionObject {
    class: Rc<ClassDef>,
    state: Rc<RefCell<CollectionState>>,
}

impl ScriptObject for CollectionObject {
    fn class(&self) -> Rc<ClassDef> {
        self.class.clone()
    }
    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
    fn as_any_mut(&mut self) -> &mut dyn std::any::Any {
        self
    }
}

pub struct CollectionItemObject {
    class: Rc<ClassDef>,
    state: Rc<RefCell<CollectionItemState>>,
}

impl ScriptObject for CollectionItemObject {
    fn class(&self) -> Rc<ClassDef> {
        self.class.clone()
    }
    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
    fn as_any_mut(&mut self) -> &mut dyn std::any::Any {
        self
    }
}

pub fn collection_of(value: &Value) -> Option<Rc<RefCell<CollectionState>>> {
    let object = value.as_object()?;
    let borrowed = object.borrow();
    borrowed
        .as_any()
        .downcast_ref::<CollectionObject>()
        .map(|collection| collection.state.clone())
}

pub fn collection_item_of(value: &Value) -> Option<Rc<RefCell<CollectionItemState>>> {
    let object = value.as_object()?;
    let borrowed = object.borrow();
    borrowed
        .as_any()
        .downcast_ref::<CollectionItemObject>()
        .map(|item| item.state.clone())
}

//=== Registration ============================================================

/// The classes the unit registers.
struct Classes {
    t_persistent: Rc<ClassDef>,
    t_collection_item: Rc<ClassDef>,
    t_collection: Rc<ClassDef>,
    t_strings: Rc<ClassDef>,
    t_string_list: Rc<ClassDef>,
    t_stream: Rc<ClassDef>,
    t_file_stream: Rc<ClassDef>,
    t_memory_stream: Rc<ClassDef>,
    t_string_stream: Rc<ClassDef>,
    t_component: Rc<ClassDef>,
}

/// Registers the unit.
pub fn register(adapter: &mut Adapter, state: &Rc<RefCell<ScriptRunState>>) {
    // `AddHandler(cClasses, 'TNotifyEvent', TJvInterpreterClassesEvent, ...)`
    // and `THelpEvent`: the handler *class* both registrations name.
    adapter.add_handler(UNIT, "TNotifyEvent", "TJvInterpreterClassesEvent");
    adapter.add_handler(UNIT, "THelpEvent", "TJvInterpreterClassesEvent");

    // `TAlignment`, `TShiftState`.
    constant(adapter, UNIT, "taLeftJustify", 0);
    constant(adapter, UNIT, "taRightJustify", 1);
    constant(adapter, UNIT, "taCenter", 2);
    for (index, name) in [
        "ssShift", "ssAlt", "ssCtrl", "ssLeft", "ssRight", "ssMiddle", "ssDouble",
    ]
    .iter()
    .enumerate()
    {
        constant(adapter, UNIT, name, index as i32);
    }

    // The classes of the unit. A class another step already registered stays
    // that step's (the misc family of step 7 registers `TStrings` and
    // `TStringList` under these names, and a second class of one name would
    // only ever be reached through the first), so the lookup comes first and
    // only a missing class is added. `TList`'s parent is the `TObject` the
    // `System` unit of this step registered.
    let class_of = |adapter: &mut Adapter, name: &str, parent: Option<Rc<ClassDef>>| {
        adapter
            .find_class(name.as_bytes())
            .unwrap_or_else(|| adapter.add_class(UNIT, name, parent))
    };
    let t_object = adapter.find_class(b"TObject");
    let t_list = class_of(adapter, "TList", t_object.clone());
    let t_persistent = class_of(adapter, "TPersistent", t_object);
    let t_collection_item = class_of(adapter, "TCollectionItem", Some(t_persistent.clone()));
    let t_collection = class_of(adapter, "TCollection", Some(t_persistent.clone()));
    let t_strings = class_of(adapter, "TStrings", Some(t_persistent.clone()));
    let t_string_list = class_of(adapter, "TStringList", Some(t_strings.clone()));
    let t_stream = class_of(adapter, "TStream", None);
    let t_file_stream = class_of(adapter, "TFileStream", Some(t_stream.clone()));
    let t_memory_stream = class_of(adapter, "TMemoryStream", Some(t_stream.clone()));
    let t_string_stream = class_of(adapter, "TStringStream", Some(t_stream.clone()));
    let t_component = class_of(adapter, "TComponent", Some(t_persistent.clone()));

    let classes = Classes {
        t_persistent,
        t_collection_item,
        t_collection,
        t_strings,
        t_string_list,
        t_stream,
        t_file_stream,
        t_memory_stream,
        t_string_stream,
        t_component,
    };

    register_list(adapter, &t_list);
    register_persistent(adapter, &classes);
    register_collection(adapter, &classes);
    register_strings(adapter, &classes);
    register_stream(adapter, &classes);
    register_component(adapter, &classes);

    // The string-list constants.
    constant(adapter, UNIT, "dupIgnore", DUP_IGNORE);
    constant(adapter, UNIT, "dupAccept", DUP_ACCEPT);
    constant(adapter, UNIT, "dupError", DUP_ERROR);
    // `TComponentState`, `TComponentStyle`.
    for (index, name) in [
        "csLoading",
        "csReading",
        "csWriting",
        "csDestroying",
        "csDesigning",
        "csAncestor",
        "csUpdating",
        "csFixups",
    ]
    .iter()
    .enumerate()
    {
        constant(adapter, UNIT, name, index as i32);
    }
    constant(adapter, UNIT, "csInheritable", 0);
    constant(adapter, UNIT, "csCheckPropAvail", 1);

    // The `ScriptLists` seam of the run state: what `GetAssetsList` and the
    // `...ByType` calls of the data format adapter append through. The
    // append goes through the same `TList`/`TStrings` objects a script
    // holds, so the seam and `Add` can never disagree.
    {
        let mut state = state.borrow_mut();
        state.lists.list_add = Some(Rc::new(|list, item| {
            let object = list_object(list).ok_or(ValueError::Variant)?;
            let mut borrowed = object.borrow_mut();
            borrowed.list_mut().ok_or(ValueError::Variant)?.add(item);
            Ok(())
        }));
        state.lists.strings_add = Some(Rc::new(|list, text| {
            let object = strings_object(list).ok_or(ValueError::Variant)?;
            let mut borrowed = object.borrow_mut();
            borrowed
                .string_list_mut()
                .ok_or(ValueError::Variant)?
                .add(text.as_bytes().to_vec());
            Ok(())
        }));
    }
}

/// Whether a member of `class` is already registered by another step of the
/// phase: the adapter's method scans take the first matching entry, so the
/// port registers each member once -- the unit that comes first owns it
/// (the misc family of step 7 registers the `TStrings` members it needs on
/// the same class, and `xedit_script::strings`/`lists` is the one object
/// model both sides use, so the behaviour is the same either way).
fn has_member(adapter: &Adapter, kind: &str, class: &Rc<ClassDef>, identifier: Option<&str>) -> bool {
    adapter.registrations().iter().any(|entry| {
        entry.kind == kind
            && entry.class.as_deref() == Some(class.name.as_str())
            && match identifier {
                Some(identifier) => entry.identifier.as_deref() == Some(identifier),
                None => true,
            }
    })
}

/// [`method`](super::method), unless another step registered the member.
fn method_new(
    adapter: &mut Adapter,
    class: &Rc<ClassDef>,
    identifier: &str,
    params: &[super::ParamType],
    func: impl Fn(&mut Value, &mut Args) -> Result<(), ValueError> + 'static,
) {
    if !has_member(adapter, "get", class, Some(identifier)) {
        method(adapter, class, identifier, params, func);
    }
}

/// [`setter`](super::setter), unless another step registered the member.
fn setter_new(
    adapter: &mut Adapter,
    class: &Rc<ClassDef>,
    identifier: &str,
    params: &[super::ParamType],
    func: impl Fn(&Value, &mut Args) -> Result<(), ValueError> + 'static,
) {
    if !has_member(adapter, "set", class, Some(identifier)) {
        setter(adapter, class, identifier, params, func);
    }
}

/// [`index_get`](super::index_get), unless another step registered the member.
fn index_get_new(
    adapter: &mut Adapter,
    class: &Rc<ClassDef>,
    identifier: &str,
    params: &[super::ParamType],
    func: impl Fn(&mut Value, &mut Args) -> Result<(), ValueError> + 'static,
) {
    if !has_member(adapter, "iget", class, Some(identifier)) {
        index_get(adapter, class, identifier, params, func);
    }
}

/// [`index_set`](super::index_set), unless another step registered the member.
fn index_set_new(
    adapter: &mut Adapter,
    class: &Rc<ClassDef>,
    identifier: &str,
    params: &[super::ParamType],
    func: impl Fn(&Value, &mut Args) -> Result<(), ValueError> + 'static,
) {
    if !has_member(adapter, "iset", class, Some(identifier)) {
        index_set(adapter, class, identifier, params, func);
    }
}

/// [`default_get`](super::default_get), unless another step registered one on
/// the class.
fn default_get_new(
    adapter: &mut Adapter,
    class: &Rc<ClassDef>,
    params: &[super::ParamType],
    func: impl Fn(&mut Value, &mut Args) -> Result<(), ValueError> + 'static,
) {
    if !has_member(adapter, "idget", class, None) {
        default_get(adapter, class, params, func);
    }
}

/// [`default_set`](super::default_set), unless another step registered one on
/// the class.
fn default_set_new(
    adapter: &mut Adapter,
    class: &Rc<ClassDef>,
    params: &[super::ParamType],
    func: impl Fn(&Value, &mut Args) -> Result<(), ValueError> + 'static,
) {
    if !has_member(adapter, "idset", class, None) {
        default_set(adapter, class, params, func);
    }
}

/// The receiver of a method call.
fn receiver(args: &Args) -> Result<Value, ValueError> {
    args.obj.clone().ok_or(ValueError::Variant)
}

fn list_arg(args: &Args) -> Result<Rc<RefCell<dyn ScriptObject>>, ValueError> {
    list_object(&receiver(args)?).ok_or(ValueError::Variant)
}

fn strings_arg(args: &Args) -> Result<Rc<RefCell<dyn ScriptObject>>, ValueError> {
    strings_object(&receiver(args)?).ok_or(ValueError::Variant)
}

fn stream_arg(args: &Args) -> Result<Rc<RefCell<StreamState>>, ValueError> {
    stream_of(&receiver(args)?).ok_or(ValueError::Variant)
}

fn component_arg(args: &Args) -> Result<Rc<RefCell<ComponentState>>, ValueError> {
    component_of(&receiver(args)?).ok_or(ValueError::Variant)
}

fn collection_arg(args: &Args) -> Result<Rc<RefCell<CollectionState>>, ValueError> {
    collection_of(&receiver(args)?).ok_or(ValueError::Variant)
}

fn collection_item_arg(args: &Args) -> Result<Rc<RefCell<CollectionItemState>>, ValueError> {
    collection_item_of(&receiver(args)?).ok_or(ValueError::Variant)
}

//=== TList ===================================================================

fn register_list(adapter: &mut Adapter, t_list: &Rc<ClassDef>) {
    {
        let class = t_list.clone();
        method_new(adapter, t_list, "Create", &[], move |value, _args| {
            // The one `TList` object the port ships (`xedit_script::lists`),
            // shared with the host functions that fill a list.
            let _ = &class;
            *value = list_value(class.clone());
            Ok(())
        });
    }
    method_new(adapter, t_list, "Add", &[EMPTY], |value, args| {
        let object = list_arg(args)?;
        let item = arg(args, 0).clone();
        let mut borrowed = object.borrow_mut();
        let list = borrowed.list_mut().ok_or(ValueError::Variant)?;
        *value = Value::Integer(list.add(item));
        Ok(())
    });
    method_new(adapter, t_list, "Clear", &[], |_value, args| {
        let object = list_arg(args)?;
        let mut borrowed = object.borrow_mut();
        borrowed.list_mut().ok_or(ValueError::Variant)?.clear();
        Ok(())
    });
    method_new(adapter, t_list, "Delete", &[EMPTY], |_value, args| {
        let object = list_arg(args)?;
        let index = arg_i32(args, 0)?;
        let mut borrowed = object.borrow_mut();
        borrowed.list_mut().ok_or(ValueError::Variant)?.delete_at(index);
        Ok(())
    });
    method_new(adapter, t_list, "Exchange", &[EMPTY, EMPTY], |_value, args| {
        let object = list_arg(args)?;
        let first = arg_i32(args, 0)?;
        let second = arg_i32(args, 1)?;
        let mut borrowed = object.borrow_mut();
        borrowed.list_mut().ok_or(ValueError::Variant)?.exchange(first, second);
        Ok(())
    });
    method_new(adapter, t_list, "Expand", &[], |value, args| {
        // `TList.Expand` returns the list itself.
        *value = receiver(args)?;
        Ok(())
    });
    method_new(adapter, t_list, "First", &[], |value, args| {
        let object = list_arg(args)?;
        let borrowed = object.borrow();
        let list = borrowed.list().ok_or(ValueError::Variant)?;
        *value = list.items().first().cloned().unwrap_or(Value::Pointer(0));
        Ok(())
    });
    method_new(adapter, t_list, "IndexOf", &[EMPTY], |value, args| {
        let object = list_arg(args)?;
        let item = arg(args, 0).clone();
        let borrowed = object.borrow();
        let list = borrowed.list().ok_or(ValueError::Variant)?;
        let index = list
            .items()
            .iter()
            .position(|entry| *entry == item)
            .map(|at| at as i32)
            .unwrap_or(-1);
        *value = Value::Integer(index);
        Ok(())
    });
    method_new(adapter, t_list, "Insert", &[EMPTY, EMPTY], |_value, args| {
        let object = list_arg(args)?;
        let index = arg_i32(args, 0)?;
        let item = arg(args, 1).clone();
        let mut borrowed = object.borrow_mut();
        borrowed.list_mut().ok_or(ValueError::Variant)?.insert(index, item);
        Ok(())
    });
    method_new(adapter, t_list, "Last", &[], |value, args| {
        let object = list_arg(args)?;
        let borrowed = object.borrow();
        let list = borrowed.list().ok_or(ValueError::Variant)?;
        *value = list.items().last().cloned().unwrap_or(Value::Pointer(0));
        Ok(())
    });
    method_new(adapter, t_list, "Move", &[EMPTY, EMPTY], |_value, args| {
        let object = list_arg(args)?;
        let from = arg_i32(args, 0)?;
        let to = arg_i32(args, 1)?;
        let mut borrowed = object.borrow_mut();
        borrowed.list_mut().ok_or(ValueError::Variant)?.move_item(from, to);
        Ok(())
    });
    method_new(adapter, t_list, "Remove", &[EMPTY], |value, args| {
        let object = list_arg(args)?;
        let item = arg(args, 0).clone();
        let mut borrowed = object.borrow_mut();
        *value = Value::Integer(borrowed.list_mut().ok_or(ValueError::Variant)?.remove(&item));
        Ok(())
    });
    method_new(adapter, t_list, "Pack", &[], |_value, args| {
        let object = list_arg(args)?;
        let mut borrowed = object.borrow_mut();
        borrowed.list_mut().ok_or(ValueError::Variant)?.pack();
        Ok(())
    });
    method_new(adapter, t_list, "Sort", &[EMPTY], |_value, _args| {
        // Upstream's own body raises `NotImplemented('TList.Sort')`.
        Err(ValueError::not_implemented("TList.Sort"))
    });
    method_new(adapter, t_list, "Capacity", &[], |value, args| {
        let object = list_arg(args)?;
        let borrowed = object.borrow();
        *value = Value::Integer(borrowed.list().ok_or(ValueError::Variant)?.capacity() as i32);
        Ok(())
    });
    setter_new(adapter, t_list, "Capacity", &[EMPTY], |value, args| {
        let capacity = to_i32(value)?.max(0) as usize;
        let object = list_arg(args)?;
        let mut borrowed = object.borrow_mut();
        borrowed.list_mut().ok_or(ValueError::Variant)?.reserve(capacity);
        Ok(())
    });
    method_new(adapter, t_list, "Count", &[], |value, args| {
        let object = list_arg(args)?;
        let borrowed = object.borrow();
        *value = Value::Integer(borrowed.list().ok_or(ValueError::Variant)?.count());
        Ok(())
    });
    setter_new(adapter, t_list, "Count", &[EMPTY], |value, args| {
        let count = to_i32(value)?.max(0) as usize;
        let object = list_arg(args)?;
        let mut borrowed = object.borrow_mut();
        borrowed.list_mut().ok_or(ValueError::Variant)?.resize(count);
        Ok(())
    });
    let read_item = |value: &mut Value, args: &mut Args| {
        let object = list_arg(args)?;
        let index = arg_i32(args, 0)?;
        let borrowed = object.borrow();
        let list = borrowed.list().ok_or(ValueError::Variant)?;
        *value = list.get(index).cloned().unwrap_or(Value::Pointer(0));
        Ok(())
    };
    let write_item = |value: &Value, args: &mut Args| {
        let object = list_arg(args)?;
        let index = arg_i32(args, 0)?;
        let mut borrowed = object.borrow_mut();
        borrowed
            .list_mut()
            .ok_or(ValueError::Variant)?
            .set(index, value.clone());
        Ok(())
    };
    index_get_new(adapter, t_list, "Items", &[EMPTY], read_item);
    index_set_new(adapter, t_list, "Items", &[NULL], write_item);
    default_get_new(adapter, t_list, &[EMPTY], read_item);
    default_set_new(adapter, t_list, &[NULL], write_item);
    method_new(adapter, t_list, "List", &[], |value, _args| {
        // `TList.List` is the internal `PPointerList`; the port has no raw
        // memory to hand out.
        *value = Value::Pointer(0);
        Ok(())
    });
}

//=== TPersistent =============================================================

fn register_persistent(adapter: &mut Adapter, classes: &Classes) {
    method(adapter, &classes.t_persistent, "Assign", &[EMPTY], |_value, args| {
        // The RTL's base `TPersistent.Assign` raises `EConvertError`
        // (`Cannot assign a %s to a %s`) unless the class overrides it.
        let source = arg(args, 0)
            .as_object()
            .map(|object| object.borrow().class().name.clone())
            .unwrap_or_default();
        let target = receiver(args)?
            .as_object()
            .map(|object| object.borrow().class().name.clone())
            .unwrap_or_default();
        Err(ValueError::External(format!("Cannot assign a {source} to a {target}")))
    });
    method(adapter, &classes.t_persistent, "GetNamePath", &[], |value, _args| {
        *value = Value::Str(Vec::new());
        Ok(())
    });
}

//=== TCollection =============================================================

fn register_collection(adapter: &mut Adapter, classes: &Classes) {
    {
        let class = classes.t_collection_item.clone();
        method(
            adapter,
            &classes.t_collection_item,
            "Create",
            &[EMPTY],
            move |value, args| {
                let collection = if arg(args, 0).is_nil() {
                    None
                } else {
                    Some(arg(args, 0).clone())
                };
                *value = Value::Object(Rc::new(RefCell::new(CollectionItemObject {
                    class: class.clone(),
                    state: Rc::new(RefCell::new(CollectionItemState {
                        collection,
                        id: 0,
                        index: 0,
                        display_name: String::new(),
                    })),
                })));
                Ok(())
            },
        );
    }
    method(adapter, &classes.t_collection_item, "Collection", &[], |value, args| {
        *value = collection_item_arg(args)?
            .borrow()
            .collection
            .clone()
            .unwrap_or(Value::Null);
        Ok(())
    });
    setter(
        adapter,
        &classes.t_collection_item,
        "Collection",
        &[EMPTY],
        |value, args| {
            collection_item_arg(args)?.borrow_mut().collection = Some(value.clone());
            Ok(())
        },
    );
    method(adapter, &classes.t_collection_item, "ID", &[], |value, args| {
        *value = Value::Integer(collection_item_arg(args)?.borrow().id);
        Ok(())
    });
    method(adapter, &classes.t_collection_item, "Index", &[], |value, args| {
        *value = Value::Integer(collection_item_arg(args)?.borrow().index);
        Ok(())
    });
    setter(adapter, &classes.t_collection_item, "Index", &[EMPTY], |value, args| {
        collection_item_arg(args)?.borrow_mut().index = to_i32(value)?;
        Ok(())
    });
    method(
        adapter,
        &classes.t_collection_item,
        "DisplayName",
        &[],
        |value, args| {
            *value = Value::Str(collection_item_arg(args)?.borrow().display_name.clone().into_bytes());
            Ok(())
        },
    );
    setter(
        adapter,
        &classes.t_collection_item,
        "DisplayName",
        &[EMPTY],
        |value, args| {
            collection_item_arg(args)?.borrow_mut().display_name =
                String::from_utf8_lossy(&to_str(value)?).into_owned();
            Ok(())
        },
    );

    {
        let class = classes.t_collection.clone();
        method(
            adapter,
            &classes.t_collection,
            "Create",
            &[EMPTY],
            move |value, args| {
                let item_class = if arg(args, 0).is_nil() {
                    None
                } else {
                    Some(arg(args, 0).clone())
                };
                *value = Value::Object(Rc::new(RefCell::new(CollectionObject {
                    class: class.clone(),
                    state: Rc::new(RefCell::new(CollectionState {
                        item_class,
                        items: Vec::new(),
                        updating: 0,
                    })),
                })));
                Ok(())
            },
        );
    }
    {
        let item_class = classes.t_collection_item.clone();
        method(adapter, &classes.t_collection, "Add", &[], move |value, args| {
            let collection = collection_arg(args)?;
            let mut collection = collection.borrow_mut();
            let index = collection.items.len();
            let item = Value::Object(Rc::new(RefCell::new(CollectionItemObject {
                class: item_class.clone(),
                state: Rc::new(RefCell::new(CollectionItemState {
                    collection: Some(receiver(args)?),
                    id: index as i32,
                    index: index as i32,
                    display_name: String::new(),
                })),
            })));
            collection.items.push(collection_item_of(&item).expect("just built"));
            drop(collection);
            *value = item;
            Ok(())
        });
    }
    method(adapter, &classes.t_collection, "Assign", &[EMPTY], |_value, _args| {
        Err(ValueError::not_implemented("TCollection.Assign"))
    });
    method(adapter, &classes.t_collection, "BeginUpdate", &[], |_value, args| {
        collection_arg(args)?.borrow_mut().updating += 1;
        Ok(())
    });
    method(adapter, &classes.t_collection, "Clear", &[], |_value, args| {
        collection_arg(args)?.borrow_mut().items.clear();
        Ok(())
    });
    method(adapter, &classes.t_collection, "EndUpdate", &[], |_value, args| {
        let collection = collection_arg(args)?;
        collection.borrow_mut().updating = (collection.borrow().updating - 1).max(0);
        Ok(())
    });
    {
        let item_class = classes.t_collection_item.clone();
        method(
            adapter,
            &classes.t_collection,
            "FindItemID",
            &[EMPTY],
            move |value, args| {
                let collection = collection_arg(args)?;
                let id = arg_i32(args, 0)?;
                let found = collection
                    .borrow()
                    .items
                    .iter()
                    .find(|item| item.borrow().id == id)
                    .map(|item| {
                        Value::Object(Rc::new(RefCell::new(CollectionItemObject {
                            class: item_class.clone(),
                            state: item.clone(),
                        })))
                    });
                *value = found.unwrap_or(Value::Null);
                Ok(())
            },
        );
    }
    method(adapter, &classes.t_collection, "Count", &[], |value, args| {
        *value = Value::Integer(collection_arg(args)?.borrow().items.len() as i32);
        Ok(())
    });
    method(adapter, &classes.t_collection, "ItemClass", &[], |value, args| {
        *value = collection_arg(args)?.borrow().item_class.clone().unwrap_or(Value::Null);
        Ok(())
    });
    {
        let item_class = classes.t_collection_item.clone();
        let read = move |value: &mut Value, args: &mut Args| {
            let collection = collection_arg(args)?;
            let index = arg_i32(args, 0)?.max(0) as usize;
            let item = collection.borrow().items.get(index).cloned();
            *value = match item {
                Some(state) => Value::Object(Rc::new(RefCell::new(CollectionItemObject {
                    class: item_class.clone(),
                    state,
                }))),
                None => Value::Null,
            };
            Ok(())
        };
        index_get(adapter, &classes.t_collection, "Items", &[EMPTY], read.clone());
        default_get(adapter, &classes.t_collection, &[EMPTY], read);
    }
    {
        let write = |value: &Value, args: &mut Args| {
            let collection = collection_arg(args)?;
            let index = arg_i32(args, 0)?;
            if index >= 0 {
                // Replacing an item needs the new value to be a collection
                // item; a mis-typed write is the variant error upstream's
                // hard cast (`as TCollectionItem`) faults on.
                let item = collection_item_of(value).ok_or(ValueError::Variant)?;
                let mut collection = collection.borrow_mut();
                let index = index as usize;
                if index < collection.items.len() {
                    collection.items[index] = item;
                }
            }
            Ok(())
        };
        index_set(adapter, &classes.t_collection, "Items", &[NULL], write);
        default_set(adapter, &classes.t_collection, &[NULL], write);
    }
}

//=== TStrings / TStringList ==================================================

/// The bytes of a text split as `TStrings.SetTextStr` splits it: at CR, LF
/// or CRLF, with no empty line after a final break.
fn split_lines(text: &[u8]) -> Vec<Vec<u8>> {
    let mut lines = Vec::new();
    let mut start = 0usize;
    let mut i = 0usize;
    while i < text.len() {
        match text[i] {
            b'\r' => {
                lines.push(text[start..i].to_vec());
                i += if text.get(i + 1) == Some(&b'\n') { 2 } else { 1 };
                start = i;
            }
            b'\n' => {
                lines.push(text[start..i].to_vec());
                i += 1;
                start = i;
            }
            _ => i += 1,
        }
    }
    if start < text.len() {
        lines.push(text[start..].to_vec());
    }
    lines
}

/// The `Text` getter: each line followed by CRLF.
fn join_text(lines: &[Vec<u8>]) -> Vec<u8> {
    let mut text = Vec::new();
    for line in lines {
        text.extend_from_slice(line);
        text.extend_from_slice(b"\r\n");
    }
    text
}

/// `TStringList.Find`-shaped binary search with the list's own
/// `CompareStrings` ordering: `Ok(index)` when found, `Err(insertion point)`
/// otherwise. The `Classes` unit's `Find` needs the insertion point, which
/// the shared trait's `index_of` does not give.
fn find_sorted(list: &dyn StringList, text: &[u8]) -> Result<usize, usize> {
    list.lines().binary_search_by(|line| list.compare_strings(line, text))
}

/// The name part of a `Name=Value` line.
fn name_of(line: &[u8]) -> Vec<u8> {
    match line.iter().position(|byte| *byte == b'=') {
        Some(at) => line[..at].to_vec(),
        None => line.to_vec(),
    }
}

/// The two parts of a `Name=Value` line.
fn split_name_value(line: &[u8]) -> Option<(Vec<u8>, Vec<u8>)> {
    let at = line.iter().position(|byte| *byte == b'=')?;
    Some((line[..at].to_vec(), line[at + 1..].to_vec()))
}

/// The bytes of a text file: a UTF-8 BOM is stripped, a UTF-16 BOM decoded
/// to UTF-8 (the port's strings are bytes).
fn decode_text_file(bytes: Vec<u8>) -> Vec<u8> {
    if bytes.starts_with(&[0xEF, 0xBB, 0xBF]) {
        return bytes[3..].to_vec();
    }
    if bytes.starts_with(&[0xFF, 0xFE]) || bytes.starts_with(&[0xFE, 0xFF]) {
        let little_endian = bytes[0] == 0xFF;
        let mut units = Vec::new();
        let mut i = 2usize;
        while i + 1 < bytes.len() {
            let unit = if little_endian {
                u16::from_le_bytes([bytes[i], bytes[i + 1]])
            } else {
                u16::from_be_bytes([bytes[i], bytes[i + 1]])
            };
            units.push(unit);
            i += 2;
        }
        return String::from_utf16_lossy(&units).into_bytes();
    }
    bytes
}

fn register_strings(adapter: &mut Adapter, classes: &Classes) {
    {
        let class = classes.t_string_list.clone();
        method_new(adapter, &classes.t_string_list, "Create", &[], move |value, _args| {
            // The one `TStringList` object the port ships
            // (`xedit_script::strings`), shared with the host functions that
            // fill or read a `TStrings` and with `TwbFastStringList` (the
            // resource adapter's subclass).
            *value = string_list_value(class.clone());
            Ok(())
        });
    }

    // The `TStrings` members.
    let add = |value: &mut Value, args: &mut Args| {
        let object = strings_arg(args)?;
        let text = arg_str(args, 0);
        let mut borrowed = object.borrow_mut();
        let list = borrowed.string_list_mut().ok_or(ValueError::Variant)?;
        *value = Value::Integer(list.add(text));
        Ok(())
    };
    method_new(adapter, &classes.t_strings, "Add", &[EMPTY], add);
    method_new(adapter, &classes.t_string_list, "Add", &[EMPTY], add);
    method(
        adapter,
        &classes.t_strings,
        "AddObject",
        &[EMPTY, EMPTY],
        |value, args| {
            let item = arg(args, 1).clone();
            let object = strings_arg(args)?;
            let text = arg_str(args, 0);
            let mut borrowed = object.borrow_mut();
            let list = borrowed.string_list_mut().ok_or(ValueError::Variant)?;
            *value = Value::Integer(list.add_object(text, item));
            Ok(())
        },
    );
    method_new(adapter, &classes.t_strings, "Append", &[EMPTY], |_value, args| {
        let object = strings_arg(args)?;
        let text = arg_str(args, 0);
        let mut borrowed = object.borrow_mut();
        borrowed.string_list_mut().ok_or(ValueError::Variant)?.add(text);
        Ok(())
    });
    method_new(adapter, &classes.t_strings, "AddStrings", &[EMPTY], |_value, args| {
        let source = strings_object(arg(args, 0)).ok_or(ValueError::Variant)?;
        let lines = lines_of(&source)?;
        let object = strings_arg(args)?;
        let mut borrowed = object.borrow_mut();
        let list = borrowed.string_list_mut().ok_or(ValueError::Variant)?;
        for (line, item) in lines {
            list.add_object(line, item);
        }
        Ok(())
    });
    method_new(adapter, &classes.t_strings, "Assign", &[EMPTY], |_value, args| {
        let source = strings_object(arg(args, 0)).ok_or(ValueError::Variant)?;
        let target = strings_arg(args)?;
        if Rc::ptr_eq(&source, &target) {
            return Ok(());
        }
        let lines = lines_of(&source)?;
        let mut borrowed = target.borrow_mut();
        let list = borrowed.string_list_mut().ok_or(ValueError::Variant)?;
        list.clear();
        for (line, item) in lines {
            list.add_object(line, item);
        }
        Ok(())
    });
    method_new(adapter, &classes.t_strings, "BeginUpdate", &[], |_value, _args| Ok(()));
    let clear = |_value: &mut Value, args: &mut Args| {
        let object = strings_arg(args)?;
        let mut borrowed = object.borrow_mut();
        borrowed.string_list_mut().ok_or(ValueError::Variant)?.clear();
        Ok(())
    };
    method_new(adapter, &classes.t_strings, "Clear", &[], clear);
    method_new(adapter, &classes.t_string_list, "Clear", &[], clear);
    let delete = |_value: &mut Value, args: &mut Args| {
        let object = strings_arg(args)?;
        let index = arg_i32(args, 0)?;
        let mut borrowed = object.borrow_mut();
        borrowed.string_list_mut().ok_or(ValueError::Variant)?.delete(index);
        Ok(())
    };
    method_new(adapter, &classes.t_strings, "Delete", &[EMPTY], delete);
    method_new(adapter, &classes.t_string_list, "Delete", &[EMPTY], delete);
    method_new(adapter, &classes.t_strings, "EndUpdate", &[], |_value, _args| Ok(()));
    method_new(adapter, &classes.t_strings, "Equals", &[EMPTY], |value, args| {
        let other = strings_object(arg(args, 0)).ok_or(ValueError::Variant)?;
        let object = strings_arg(args)?;
        let equal = {
            let a = object.borrow();
            let b = other.borrow();
            let a = a.string_list().ok_or(ValueError::Variant)?;
            let b = b.string_list().ok_or(ValueError::Variant)?;
            a.lines() == b.lines()
        };
        *value = Value::Bool(equal);
        Ok(())
    });
    let exchange = |_value: &mut Value, args: &mut Args| {
        let object = strings_arg(args)?;
        let first = arg_i32(args, 0)?;
        let second = arg_i32(args, 1)?;
        let mut borrowed = object.borrow_mut();
        borrowed
            .string_list_mut()
            .ok_or(ValueError::Variant)?
            .exchange(first, second);
        Ok(())
    };
    method_new(adapter, &classes.t_strings, "Exchange", &[EMPTY, EMPTY], exchange);
    method_new(adapter, &classes.t_string_list, "Exchange", &[EMPTY, EMPTY], exchange);
    let index_of = |value: &mut Value, args: &mut Args| {
        let object = strings_arg(args)?;
        let text = arg_str(args, 0);
        let borrowed = object.borrow();
        let list = borrowed.string_list().ok_or(ValueError::Variant)?;
        *value = Value::Integer(list.index_of(&text));
        Ok(())
    };
    method_new(adapter, &classes.t_strings, "IndexOf", &[EMPTY], index_of);
    method_new(adapter, &classes.t_string_list, "IndexOf", &[EMPTY], index_of);
    method_new(adapter, &classes.t_strings, "IndexOfName", &[EMPTY], |value, args| {
        let name = arg_str(args, 0);
        let object = strings_arg(args)?;
        let borrowed = object.borrow();
        let list = borrowed.string_list().ok_or(ValueError::Variant)?;
        let index = list
            .lines()
            .iter()
            .position(|line| name_of(line).eq_ignore_ascii_case(&name))
            .map(|at| at as i32)
            .unwrap_or(-1);
        *value = Value::Integer(index);
        Ok(())
    });
    method_new(adapter, &classes.t_strings, "IndexOfObject", &[EMPTY], |value, args| {
        let item = arg(args, 0).clone();
        let object = strings_arg(args)?;
        let borrowed = object.borrow();
        let list = borrowed.string_list().ok_or(ValueError::Variant)?;
        let index = list
            .objects()
            .iter()
            .position(|entry| entry == &item)
            .map(|at| at as i32)
            .unwrap_or(-1);
        *value = Value::Integer(index);
        Ok(())
    });
    let insert = |_value: &mut Value, args: &mut Args| {
        let object = strings_arg(args)?;
        let index = arg_i32(args, 0)?;
        let text = arg_str(args, 1);
        let mut borrowed = object.borrow_mut();
        borrowed
            .string_list_mut()
            .ok_or(ValueError::Variant)?
            .insert(index, text, Value::Empty);
        Ok(())
    };
    method_new(adapter, &classes.t_strings, "Insert", &[EMPTY, EMPTY], insert);
    method_new(adapter, &classes.t_string_list, "Insert", &[EMPTY, EMPTY], insert);
    method(
        adapter,
        &classes.t_strings,
        "InsertObject",
        &[EMPTY, EMPTY, EMPTY],
        |_value, args| {
            let index = arg_i32(args, 0)?;
            let text = arg_str(args, 1);
            let item = arg(args, 2).clone();
            let object = strings_arg(args)?;
            let mut borrowed = object.borrow_mut();
            borrowed
                .string_list_mut()
                .ok_or(ValueError::Variant)?
                .insert(index, text, item);
            Ok(())
        },
    );
    method_new(adapter, &classes.t_strings, "LoadFromFile", &[EMPTY], |_value, args| {
        let path = arg_text(args, 0)?;
        let bytes = std::fs::read(&path).map_err(file_error(&path))?;
        let text = decode_text_file(bytes);
        let object = strings_arg(args)?;
        let mut borrowed = object.borrow_mut();
        let list = borrowed.string_list_mut().ok_or(ValueError::Variant)?;
        list.clear();
        for line in split_lines(&text) {
            list.add_object(line, Value::Empty);
        }
        Ok(())
    });
    method(
        adapter,
        &classes.t_strings,
        "LoadFromStream",
        &[EMPTY],
        |_value, args| {
            let stream = stream_of(arg(args, 0)).ok_or(ValueError::Variant)?;
            let bytes = stream_read_to_end(&stream)?;
            let text = decode_text_file(bytes);
            let object = strings_arg(args)?;
            let mut borrowed = object.borrow_mut();
            let list = borrowed.string_list_mut().ok_or(ValueError::Variant)?;
            list.clear();
            for line in split_lines(&text) {
                list.add_object(line, Value::Empty);
            }
            Ok(())
        },
    );
    method_new(adapter, &classes.t_strings, "Move", &[EMPTY, EMPTY], |_value, args| {
        let object = strings_arg(args)?;
        let from = arg_i32(args, 0)?;
        let to = arg_i32(args, 1)?;
        let mut borrowed = object.borrow_mut();
        borrowed
            .string_list_mut()
            .ok_or(ValueError::Variant)?
            .move_line(from, to);
        Ok(())
    });
    method_new(adapter, &classes.t_strings, "SaveToFile", &[EMPTY], |_value, args| {
        let path = arg_text(args, 0)?;
        let object = strings_arg(args)?;
        let bytes = {
            let borrowed = object.borrow();
            let list = borrowed.string_list().ok_or(ValueError::Variant)?;
            join_text(list.lines())
        };
        std::fs::write(&path, bytes).map_err(file_error(&path))?;
        Ok(())
    });
    method_new(adapter, &classes.t_strings, "SaveToStream", &[EMPTY], |_value, args| {
        let stream = stream_of(arg(args, 0)).ok_or(ValueError::Variant)?;
        let object = strings_arg(args)?;
        let bytes = {
            let borrowed = object.borrow();
            let list = borrowed.string_list().ok_or(ValueError::Variant)?;
            join_text(list.lines())
        };
        stream_write_at_end(&stream, &bytes)
    });
    method_new(adapter, &classes.t_strings, "Capacity", &[], |value, args| {
        let object = strings_arg(args)?;
        let borrowed = object.borrow();
        *value = Value::Integer(borrowed.string_list().ok_or(ValueError::Variant)?.capacity() as i32);
        Ok(())
    });
    setter_new(adapter, &classes.t_strings, "Capacity", &[EMPTY], |value, args| {
        let capacity = to_i32(value)?.max(0) as usize;
        let object = strings_arg(args)?;
        let mut borrowed = object.borrow_mut();
        borrowed.string_list_mut().ok_or(ValueError::Variant)?.reserve(capacity);
        Ok(())
    });
    method_new(adapter, &classes.t_strings, "CommaText", &[], |value, args| {
        let object = strings_arg(args)?;
        let borrowed = object.borrow();
        let list = borrowed.string_list().ok_or(ValueError::Variant)?;
        let lines: Vec<String> = list
            .lines()
            .iter()
            .map(|line| String::from_utf8_lossy(line).into_owned())
            .collect();
        *value = Value::Str(xedit_assets::sniff::processor::comma_text_of(&lines).into_bytes());
        Ok(())
    });
    setter_new(adapter, &classes.t_strings, "CommaText", &[EMPTY], |value, args| {
        let text = String::from_utf8_lossy(&to_str(value)?).into_owned();
        let fields = xedit_assets::sniff::processor::comma_text(&text);
        let object = strings_arg(args)?;
        let mut borrowed = object.borrow_mut();
        let list = borrowed.string_list_mut().ok_or(ValueError::Variant)?;
        list.clear();
        for field in fields {
            list.add_object(field.into_bytes(), Value::Empty);
        }
        Ok(())
    });
    method_new(adapter, &classes.t_strings, "Count", &[], |value, args| {
        let object = strings_arg(args)?;
        let borrowed = object.borrow();
        let list = borrowed.string_list().ok_or(ValueError::Variant)?;
        *value = Value::Integer(list.lines().len() as i32);
        Ok(())
    });
    index_get_new(adapter, &classes.t_strings, "Names", &[EMPTY], |value, args| {
        // `TStrings.Names[Index: Integer]`: the name part of the line at
        // that index, `EStringListError` (`Name not found`) when the line
        // holds no `Name=Value` pair (`GetName`, `Classes.pas`).
        let index = arg_i32(args, 0)?;
        let object = strings_arg(args)?;
        let borrowed = object.borrow();
        let list = borrowed.string_list().ok_or(ValueError::Variant)?;
        let line = usize::try_from(index).ok().and_then(|index| list.lines().get(index));
        match line.and_then(|line| split_name_value(line)) {
            Some((left, _)) => {
                *value = Value::Str(left);
                Ok(())
            }
            None => Err(ValueError::External("Name not found".to_owned())),
        }
    });
    index_get_new(adapter, &classes.t_strings, "Values", &[EMPTY], |value, args| {
        let name = arg_str(args, 0);
        let object = strings_arg(args)?;
        let borrowed = object.borrow();
        let list = borrowed.string_list().ok_or(ValueError::Variant)?;
        for line in list.lines() {
            if let Some((left, right)) = split_name_value(line)
                && left.eq_ignore_ascii_case(&name)
            {
                *value = Value::Str(right);
                return Ok(());
            }
        }
        *value = Value::Str(Vec::new());
        Ok(())
    });
    index_set_new(adapter, &classes.t_strings, "Values", &[NULL], |value, args| {
        let name = arg_str(args, 0);
        let text = to_str(value)?;
        let mut line = name.clone();
        line.push(b'=');
        line.extend_from_slice(&text);
        let object = strings_arg(args)?;
        let mut borrowed = object.borrow_mut();
        let list = borrowed.string_list_mut().ok_or(ValueError::Variant)?;
        let existing = (0..list.lines().len()).find(|index| {
            split_name_value(&list.lines()[*index])
                .map(|(left, _)| left.eq_ignore_ascii_case(&name))
                .unwrap_or(false)
        });
        match existing {
            Some(index) => list.set_line(index as i32, line),
            None => {
                list.add(line);
            }
        }
        Ok(())
    });
    index_get_new(adapter, &classes.t_strings, "Objects", &[EMPTY], |value, args| {
        let index = arg_i32(args, 0)?;
        let object = strings_arg(args)?;
        let borrowed = object.borrow();
        let list = borrowed.string_list().ok_or(ValueError::Variant)?;
        *value = usize::try_from(index)
            .ok()
            .and_then(|index| list.objects().get(index))
            .filter(|item| !item.is_empty())
            .cloned()
            .unwrap_or(Value::Null);
        Ok(())
    });
    index_set_new(adapter, &classes.t_strings, "Objects", &[NULL], |value, args| {
        let index = arg_i32(args, 0)?;
        let object = strings_arg(args)?;
        let mut borrowed = object.borrow_mut();
        borrowed
            .string_list_mut()
            .ok_or(ValueError::Variant)?
            .set_object(index, value.clone());
        Ok(())
    });
    let read_string_at = |value: &mut Value, args: &mut Args| {
        let index = arg_i32(args, 0)?;
        let object = strings_arg(args)?;
        let borrowed = object.borrow();
        let list = borrowed.string_list().ok_or(ValueError::Variant)?;
        *value = Value::Str(
            usize::try_from(index)
                .ok()
                .and_then(|index| list.lines().get(index))
                .cloned()
                .unwrap_or_default(),
        );
        Ok(())
    };
    let write_string_at = |value: &Value, args: &mut Args| {
        let index = arg_i32(args, 0)?;
        let text = to_str(value)?;
        let object = strings_arg(args)?;
        let mut borrowed = object.borrow_mut();
        borrowed
            .string_list_mut()
            .ok_or(ValueError::Variant)?
            .set_line(index, text);
        Ok(())
    };
    index_get_new(adapter, &classes.t_strings, "Strings", &[EMPTY], read_string_at);
    index_set_new(adapter, &classes.t_strings, "Strings", &[NULL], write_string_at);
    default_get_new(adapter, &classes.t_strings, &[EMPTY], read_string_at);
    default_set_new(adapter, &classes.t_strings, &[NULL], write_string_at);
    method_new(adapter, &classes.t_strings, "Text", &[], |value, args| {
        let object = strings_arg(args)?;
        let borrowed = object.borrow();
        let list = borrowed.string_list().ok_or(ValueError::Variant)?;
        *value = Value::Str(join_text(list.lines()));
        Ok(())
    });
    setter_new(adapter, &classes.t_strings, "Text", &[EMPTY], |value, args| {
        let text = to_str(value)?;
        let object = strings_arg(args)?;
        let mut borrowed = object.borrow_mut();
        let list = borrowed.string_list_mut().ok_or(ValueError::Variant)?;
        list.clear();
        for line in split_lines(&text) {
            list.add_object(line, Value::Empty);
        }
        Ok(())
    });
    method_new(adapter, &classes.t_strings, "StringsAdapter", &[], |value, _args| {
        // `TStrings.StringsAdapter` is an interface bound to a control; the
        // port's strings are not, so it is nil.
        *value = Value::Null;
        Ok(())
    });
    setter(
        adapter,
        &classes.t_strings,
        "StringsAdapter",
        &[EMPTY],
        |_value, _args| Err(ValueError::not_implemented("TStrings.StringsAdapter")),
    );

    // `TStringList`'s own members.
    method(
        adapter,
        &classes.t_string_list,
        "Find",
        &[EMPTY, BYREF],
        |value, args| {
            let text = arg_str(args, 0);
            let object = strings_arg(args)?;
            let borrowed = object.borrow();
            let list = borrowed.string_list().ok_or(ValueError::Variant)?;
            match find_sorted(list, &text) {
                Ok(index) => {
                    args.values[1] = Value::Integer(index as i32);
                    *value = Value::Bool(true);
                }
                Err(index) => {
                    args.values[1] = Value::Integer(index as i32);
                    *value = Value::Bool(false);
                }
            }
            Ok(())
        },
    );
    method_new(adapter, &classes.t_string_list, "Sort", &[], |_value, args| {
        let object = strings_arg(args)?;
        let mut borrowed = object.borrow_mut();
        borrowed.string_list_mut().ok_or(ValueError::Variant)?.sort();
        Ok(())
    });
    method_new(adapter, &classes.t_string_list, "Duplicates", &[], |value, args| {
        let object = strings_arg(args)?;
        let borrowed = object.borrow();
        *value = Value::Integer(borrowed.string_list().ok_or(ValueError::Variant)?.duplicates().as_int());
        Ok(())
    });
    setter(
        adapter,
        &classes.t_string_list,
        "Duplicates",
        &[EMPTY],
        |value, args| {
            let duplicates = Duplicates::from_int(i64::from(to_i32(value)?));
            let object = strings_arg(args)?;
            let mut borrowed = object.borrow_mut();
            borrowed
                .string_list_mut()
                .ok_or(ValueError::Variant)?
                .set_duplicates(duplicates);
            Ok(())
        },
    );
    method_new(adapter, &classes.t_string_list, "Sorted", &[], |value, args| {
        let object = strings_arg(args)?;
        let borrowed = object.borrow();
        *value = Value::Bool(borrowed.string_list().ok_or(ValueError::Variant)?.sorted());
        Ok(())
    });
    setter_new(adapter, &classes.t_string_list, "Sorted", &[EMPTY], |value, args| {
        let sorted = to_bool(value)?;
        let object = strings_arg(args)?;
        let mut borrowed = object.borrow_mut();
        borrowed
            .string_list_mut()
            .ok_or(ValueError::Variant)?
            .set_sorted(sorted);
        Ok(())
    });
}

/// The lines and their objects of a `TStrings` value, for `Assign` and
/// `AddStrings`.
fn lines_of(object: &Rc<RefCell<dyn ScriptObject>>) -> Result<Vec<(Vec<u8>, Value)>, ValueError> {
    let borrowed = object.borrow();
    let list = borrowed.string_list().ok_or(ValueError::Variant)?;
    Ok(list
        .lines()
        .iter()
        .cloned()
        .zip((0..list.lines().len()).map(|index| list.objects().get(index).cloned().unwrap_or(Value::Empty)))
        .collect())
}

/// The `Cannot open file "<path>". <os error>` text of a failed file call.
fn file_error(path: &str) -> impl Fn(std::io::Error) -> ValueError + '_ {
    move |error| {
        ValueError::External(format!(
            "Cannot open file \"{path}\". {}",
            crate::script_strings::os_error_text(&error)
        ))
    }
}

//=== TStream =================================================================

/// The rest of a stream's bytes (its position advances to its end).
fn stream_read_to_end(stream: &Rc<RefCell<StreamState>>) -> Result<Vec<u8>, ValueError> {
    let mut stream = stream.borrow_mut();
    let mut buffer = Vec::new();
    let pos = stream.pos;
    match &mut stream.kind {
        StreamKind::File(file) => {
            use std::io::{Read, Seek, SeekFrom};
            file.seek(SeekFrom::Start(pos)).map_err(|_| ValueError::Variant)?;
            file.read_to_end(&mut buffer).map_err(|_| ValueError::Variant)?;
        }
        StreamKind::Memory(memory) | StreamKind::Text(memory) => {
            let start = (pos as usize).min(memory.len());
            buffer.extend_from_slice(&memory[start..]);
        }
    }
    stream.pos += buffer.len() as u64;
    Ok(buffer)
}

/// Writes at the stream's end (the `SaveToStream` shape).
fn stream_write_at_end(stream: &Rc<RefCell<StreamState>>, bytes: &[u8]) -> Result<(), ValueError> {
    let mut stream = stream.borrow_mut();
    match &mut stream.kind {
        StreamKind::File(file) => {
            use std::io::{Seek, SeekFrom, Write};
            let end = file.seek(SeekFrom::End(0)).map_err(|_| ValueError::Variant)?;
            file.write_all(bytes).map_err(|_| ValueError::Variant)?;
            stream.pos = end + bytes.len() as u64;
        }
        StreamKind::Memory(memory) | StreamKind::Text(memory) => {
            memory.extend_from_slice(bytes);
            stream.pos = memory.len() as u64;
        }
    }
    Ok(())
}

/// `TStream.Read`/`ReadBuffer`: up to `count` bytes from the position.
fn stream_read(stream: &Rc<RefCell<StreamState>>, count: usize) -> Result<Vec<u8>, ValueError> {
    let mut stream = stream.borrow_mut();
    let pos = stream.pos;
    let mut buffer = vec![0u8; count];
    let read = match &mut stream.kind {
        StreamKind::File(file) => {
            use std::io::{Read, Seek, SeekFrom};
            file.seek(SeekFrom::Start(pos)).map_err(|_| ValueError::Variant)?;
            file.read(&mut buffer).map_err(|_| ValueError::Variant)?
        }
        StreamKind::Memory(memory) | StreamKind::Text(memory) => {
            let start = (pos as usize).min(memory.len());
            let available = &memory[start..];
            let read = available.len().min(count);
            buffer[..read].copy_from_slice(&available[..read]);
            read
        }
    };
    buffer.truncate(read);
    stream.pos += read as u64;
    Ok(buffer)
}

/// `TStream.Write`/`WriteBuffer`: `bytes` at the position.
fn stream_write(stream: &Rc<RefCell<StreamState>>, bytes: &[u8]) -> Result<usize, ValueError> {
    let mut stream = stream.borrow_mut();
    let pos = stream.pos as usize;
    match &mut stream.kind {
        StreamKind::File(file) => {
            use std::io::{Seek, SeekFrom, Write};
            file.seek(SeekFrom::Start(pos as u64))
                .map_err(|_| ValueError::Variant)?;
            let written = file.write(bytes).map_err(|_| ValueError::Variant)?;
            stream.pos += written as u64;
            Ok(written)
        }
        StreamKind::Memory(memory) | StreamKind::Text(memory) => {
            if pos > memory.len() {
                memory.resize(pos, 0);
            }
            let end = pos + bytes.len();
            if end > memory.len() {
                memory.resize(end, 0);
            }
            memory[pos..end].copy_from_slice(bytes);
            stream.pos = end as u64;
            Ok(bytes.len())
        }
    }
}

/// The size of a stream.
fn stream_size(stream: &Rc<RefCell<StreamState>>) -> Result<u64, ValueError> {
    let stream = stream.borrow();
    match &stream.kind {
        StreamKind::File(file) => file
            .metadata()
            .map(|metadata| metadata.len())
            .map_err(|_| ValueError::Variant),
        StreamKind::Memory(memory) | StreamKind::Text(memory) => Ok(memory.len() as u64),
    }
}

/// `TStream.Size` write.
fn stream_set_size(stream: &Rc<RefCell<StreamState>>, size: u64) -> Result<(), ValueError> {
    let mut stream = stream.borrow_mut();
    match &mut stream.kind {
        StreamKind::File(file) => file.set_len(size).map_err(|_| ValueError::Variant),
        StreamKind::Memory(memory) | StreamKind::Text(memory) => {
            memory.resize(size as usize, 0);
            Ok(())
        }
    }
}

/// `TStream.Seek` with the `TSeekOrigin` value.
fn stream_seek(stream: &Rc<RefCell<StreamState>>, offset: i64, origin: i32) -> Result<i64, ValueError> {
    let mut stream = stream.borrow_mut();
    let current = stream.pos as i64;
    let size = match &stream.kind {
        StreamKind::File(file) => file.metadata().map(|metadata| metadata.len() as i64).unwrap_or(0),
        StreamKind::Memory(memory) | StreamKind::Text(memory) => memory.len() as i64,
    };
    let position = match origin {
        1 => current + offset,
        2 => size + offset,
        _ => offset,
    };
    if position < 0 {
        return Err(ValueError::External("Invalid seek offset".to_owned()));
    }
    stream.pos = position as u64;
    Ok(position)
}

fn register_stream(adapter: &mut Adapter, classes: &Classes) {
    // The base `TStream` members.
    method(adapter, &classes.t_stream, "Read", &[BYREF, EMPTY], |value, args| {
        let stream = stream_arg(args)?;
        let count = arg_i32(args, 1)?.max(0) as usize;
        let bytes = stream_read(&stream, count)?;
        let read = bytes.len() as i32;
        args.values[0] = Value::Str(bytes);
        *value = Value::Integer(read);
        Ok(())
    });
    method(adapter, &classes.t_stream, "Write", &[EMPTY, EMPTY], |value, args| {
        let stream = stream_arg(args)?;
        let buffer = payload(arg(args, 0));
        let count = arg_i32(args, 1)?.max(0) as usize;
        let data = &buffer[..count.min(buffer.len())];
        *value = Value::Integer(stream_write(&stream, data)? as i32);
        Ok(())
    });
    method(adapter, &classes.t_stream, "Seek", &[EMPTY, EMPTY], |value, args| {
        let stream = stream_arg(args)?;
        let offset = xedit_script::values::to_i64(arg(args, 0))?;
        let origin = arg_i32(args, 1)?;
        *value = Value::Integer(stream_seek(&stream, offset, origin)? as i32);
        Ok(())
    });
    method(
        adapter,
        &classes.t_stream,
        "ReadBuffer",
        &[BYREF, EMPTY],
        |_value, args| {
            let stream = stream_arg(args)?;
            let count = arg_i32(args, 1)?.max(0) as usize;
            let bytes = stream_read(&stream, count)?;
            if bytes.len() < count {
                return Err(ValueError::External(S_READ_ERROR.to_owned()));
            }
            args.values[0] = Value::Str(bytes);
            Ok(())
        },
    );
    method(
        adapter,
        &classes.t_stream,
        "WriteBuffer",
        &[EMPTY, EMPTY],
        |_value, args| {
            let stream = stream_arg(args)?;
            let buffer = payload(arg(args, 0));
            let count = arg_i32(args, 1)?.max(0) as usize;
            let data = &buffer[..count.min(buffer.len())];
            if stream_write(&stream, data)? != data.len() {
                return Err(ValueError::External(S_WRITE_ERROR.to_owned()));
            }
            Ok(())
        },
    );
    method(
        adapter,
        &classes.t_stream,
        "CopyFrom",
        &[EMPTY, EMPTY],
        |value, args| {
            let target = stream_arg(args)?;
            let source = stream_of(arg(args, 0)).ok_or(ValueError::Variant)?;
            let count = xedit_script::values::to_i64(arg(args, 1))?;
            // `Count = 0` copies from the source's position to its end.
            let bytes = if count <= 0 {
                stream_read_to_end(&source)?
            } else {
                stream_read(&source, count as usize)?
            };
            *value = Value::Integer(stream_write(&target, &bytes)? as i32);
            Ok(())
        },
    );
    // The component-streaming members need the VCL's component reader; each
    // registration keeps its upstream parameter count.
    for (name, params) in [
        ("ReadComponent", 1usize),
        ("ReadComponentRes", 1),
        ("WriteComponent", 1),
        ("WriteComponentRes", 2),
        ("WriteDescendent", 2),
        ("WriteDescendentRes", 3),
    ] {
        let _ = params;
        let params = vec![EMPTY; params];
        method(adapter, &classes.t_stream, name, &params, |_value, _args| {
            Err(ValueError::not_implemented("TStream.ReadComponent"))
        });
    }
    method(adapter, &classes.t_stream, "ReadResHeader", &[], |_value, _args| {
        Err(ValueError::not_implemented("TStream.ReadResHeader"))
    });
    method(adapter, &classes.t_stream, "Position", &[], |value, args| {
        let stream = stream_arg(args)?;
        let position = stream.borrow().pos;
        *value = Value::Integer(position as i32);
        Ok(())
    });
    setter(adapter, &classes.t_stream, "Position", &[EMPTY], |value, args| {
        let stream = stream_arg(args)?;
        let position = to_i32(value)?.max(0) as u64;
        stream.borrow_mut().pos = position;
        Ok(())
    });
    method(adapter, &classes.t_stream, "Size", &[], |value, args| {
        let stream = stream_arg(args)?;
        *value = Value::Integer(stream_size(&stream)? as i32);
        Ok(())
    });
    setter(adapter, &classes.t_stream, "Size", &[EMPTY], |value, args| {
        let stream = stream_arg(args)?;
        let size = to_i32(value)?.max(0) as u64;
        stream_set_size(&stream, size)
    });

    // `TFileStream.Create(FileName, Mode)`.
    {
        let class = classes.t_file_stream.clone();
        method(
            adapter,
            &classes.t_file_stream,
            "Create",
            &[EMPTY, EMPTY],
            move |value, args| {
                let path = arg_text(args, 0)?;
                let mode = arg_i32(args, 1)?;
                let access = mode & 0x0F;
                let read = access == FM_OPEN_READ || access == FM_OPEN_READWRITE;
                let write = access == FM_OPEN_WRITE || access == FM_OPEN_READWRITE;
                let mut options = std::fs::OpenOptions::new();
                options.read(read).write(write);
                #[cfg(windows)]
                {
                    use std::os::windows::fs::OpenOptionsExt;
                    let share = match mode & 0xF0 {
                        0x10 => 0,
                        0x20 => 0x1,
                        0x30 => 0x2,
                        0x40 => 0x7,
                        _ => 0,
                    };
                    options.share_mode(share);
                }
                #[cfg(not(windows))]
                let _ = &mode;
                let file = options.open(&path).map_err(|error| {
                    ValueError::External(format!(
                        "Cannot open file \"{path}\". {}",
                        crate::script_strings::os_error_text(&error)
                    ))
                })?;
                *value = Value::Object(Rc::new(RefCell::new(StreamObject {
                    class: class.clone(),
                    state: Rc::new(RefCell::new(StreamState {
                        kind: StreamKind::File(file),
                        pos: 0,
                    })),
                })));
                Ok(())
            },
        );
    }

    // `TMemoryStream`.
    {
        let class = classes.t_memory_stream.clone();
        method(adapter, &classes.t_memory_stream, "Create", &[], move |value, _args| {
            *value = Value::Object(Rc::new(RefCell::new(StreamObject {
                class: class.clone(),
                state: Rc::new(RefCell::new(StreamState {
                    kind: StreamKind::Memory(Vec::new()),
                    pos: 0,
                })),
            })));
            Ok(())
        });
    }
    method(
        adapter,
        &classes.t_memory_stream,
        "LoadFromFile",
        &[EMPTY],
        |_value, args| {
            let stream = stream_arg(args)?;
            let path = arg_text(args, 0)?;
            let bytes = std::fs::read(&path).map_err(|error| {
                ValueError::External(format!(
                    "Cannot open file \"{path}\". {}",
                    crate::script_strings::os_error_text(&error)
                ))
            })?;
            let mut stream = stream.borrow_mut();
            stream.kind = StreamKind::Memory(bytes);
            stream.pos = 0;
            Ok(())
        },
    );
    method(
        adapter,
        &classes.t_memory_stream,
        "LoadFromStream",
        &[EMPTY],
        |_value, args| {
            let stream = stream_arg(args)?;
            let source = stream_of(arg(args, 0)).ok_or(ValueError::Variant)?;
            let bytes = stream_read_to_end(&source)?;
            let mut stream = stream.borrow_mut();
            stream.kind = StreamKind::Memory(bytes);
            stream.pos = 0;
            Ok(())
        },
    );
    method(
        adapter,
        &classes.t_memory_stream,
        "SaveToFile",
        &[EMPTY],
        |_value, args| {
            let stream = stream_arg(args)?;
            let path = arg_text(args, 0)?;
            let bytes = match &stream.borrow().kind {
                StreamKind::Memory(memory) | StreamKind::Text(memory) => memory.clone(),
                StreamKind::File(_) => Vec::new(),
            };
            std::fs::write(&path, bytes).map_err(|error| {
                ValueError::External(format!(
                    "Cannot open file \"{path}\". {}",
                    crate::script_strings::os_error_text(&error)
                ))
            })?;
            Ok(())
        },
    );
    method(
        adapter,
        &classes.t_memory_stream,
        "SaveToStream",
        &[EMPTY],
        |_value, args| {
            let stream = stream_arg(args)?;
            let target = stream_of(arg(args, 0)).ok_or(ValueError::Variant)?;
            let bytes = match &stream.borrow().kind {
                StreamKind::Memory(memory) | StreamKind::Text(memory) => memory.clone(),
                StreamKind::File(_) => Vec::new(),
            };
            stream_write_at_end(&target, &bytes)
        },
    );

    // `TStringStream`.
    {
        let class = classes.t_string_stream.clone();
        method(
            adapter,
            &classes.t_string_stream,
            "Create",
            &[EMPTY],
            move |value, args| {
                let text = arg_text(args, 0)?;
                *value = Value::Object(Rc::new(RefCell::new(StreamObject {
                    class: class.clone(),
                    state: Rc::new(RefCell::new(StreamState {
                        kind: StreamKind::Text(text.into_bytes()),
                        pos: 0,
                    })),
                })));
                Ok(())
            },
        );
    }
    method(
        adapter,
        &classes.t_string_stream,
        "Read",
        &[BYREF, EMPTY],
        |value, args| {
            let stream = stream_arg(args)?;
            let count = arg_i32(args, 1)?.max(0) as usize;
            let bytes = stream_read(&stream, count)?;
            let read = bytes.len() as i32;
            args.values[0] = Value::Str(bytes);
            *value = Value::Integer(read);
            Ok(())
        },
    );
    method(
        adapter,
        &classes.t_string_stream,
        "ReadString",
        &[EMPTY],
        |value, args| {
            let stream = stream_arg(args)?;
            let count = arg_i32(args, 0)?.max(0) as usize;
            let bytes = stream_read(&stream, count)?;
            *value = Value::Str(bytes);
            Ok(())
        },
    );
    method(
        adapter,
        &classes.t_string_stream,
        "Seek",
        &[EMPTY, EMPTY],
        |value, args| {
            let stream = stream_arg(args)?;
            let offset = xedit_script::values::to_i64(arg(args, 0))?;
            let origin = arg_i32(args, 1)?;
            *value = Value::Integer(stream_seek(&stream, offset, origin)? as i32);
            Ok(())
        },
    );
    method(
        adapter,
        &classes.t_string_stream,
        "Write",
        &[EMPTY, EMPTY],
        |value, args| {
            let stream = stream_arg(args)?;
            let buffer = payload(arg(args, 0));
            let count = arg_i32(args, 1)?.max(0) as usize;
            let data = &buffer[..count.min(buffer.len())];
            *value = Value::Integer(stream_write(&stream, data)? as i32);
            Ok(())
        },
    );
    method(
        adapter,
        &classes.t_string_stream,
        "WriteString",
        &[EMPTY],
        |_value, args| {
            let stream = stream_arg(args)?;
            let text = arg_str(args, 0);
            stream_write(&stream, &text)?;
            Ok(())
        },
    );
    method(adapter, &classes.t_string_stream, "DataString", &[], |value, args| {
        let stream = stream_arg(args)?;
        let bytes = match &stream.borrow().kind {
            StreamKind::Text(text) => text.clone(),
            StreamKind::Memory(memory) => memory.clone(),
            StreamKind::File(_) => Vec::new(),
        };
        *value = Value::Str(bytes);
        Ok(())
    });
}

/// The bytes of a buffer argument.
fn payload(value: &Value) -> Vec<u8> {
    match value {
        Value::Str(bytes) => bytes.clone(),
        other => xedit_script::values::to_str(other).unwrap_or_default(),
    }
}

//=== TComponent ==============================================================

fn register_component(adapter: &mut Adapter, classes: &Classes) {
    {
        let class = classes.t_component.clone();
        method(adapter, &classes.t_component, "Create", &[EMPTY], move |value, args| {
            let owner = component_of(arg(args, 0));
            let state = Rc::new(RefCell::new(ComponentState {
                name: String::new(),
                tag: 0,
                owner: owner.clone(),
                components: Vec::new(),
                component_index: -1,
                design_info: 0,
            }));
            if let Some(owner) = owner {
                let mut owner = owner.borrow_mut();
                let index = owner.components.len() as i32;
                owner.components.push(state.clone());
                drop(owner);
                state.borrow_mut().component_index = index;
            }
            *value = Value::Object(Rc::new(RefCell::new(ComponentObject {
                class: class.clone(),
                state,
            })));
            Ok(())
        });
    }
    method(
        adapter,
        &classes.t_component,
        "DestroyComponents",
        &[],
        |_value, args| {
            let component = component_arg(args)?;
            let children: Vec<Rc<RefCell<ComponentState>>> = std::mem::take(&mut component.borrow_mut().components);
            for child in children {
                child.borrow_mut().owner = None;
            }
            Ok(())
        },
    );
    method(adapter, &classes.t_component, "Destroying", &[], |_value, _args| Ok(()));
    {
        let class = classes.t_component.clone();
        method(
            adapter,
            &classes.t_component,
            "FindComponent",
            &[EMPTY],
            move |value, args| {
                let component = component_arg(args)?;
                let name = arg_text(args, 0)?;
                let found = component
                    .borrow()
                    .components
                    .iter()
                    .find(|child| child.borrow().name.eq_ignore_ascii_case(&name))
                    .cloned();
                *value = match found {
                    Some(state) => Value::Object(Rc::new(RefCell::new(ComponentObject {
                        class: class.clone(),
                        state,
                    }))),
                    None => Value::Null,
                };
                Ok(())
            },
        );
    }
    method(
        adapter,
        &classes.t_component,
        "FreeNotification",
        &[EMPTY],
        |_value, _args| Ok(()),
    );
    method(adapter, &classes.t_component, "FreeOnRelease", &[], |_value, _args| {
        Ok(())
    });
    method(
        adapter,
        &classes.t_component,
        "GetParentComponent",
        &[],
        |value, _args| {
            *value = Value::Null;
            Ok(())
        },
    );
    method(adapter, &classes.t_component, "HasParent", &[], |value, _args| {
        *value = Value::Bool(false);
        Ok(())
    });
    method(
        adapter,
        &classes.t_component,
        "InsertComponent",
        &[EMPTY],
        |_value, args| {
            let component = component_arg(args)?;
            let child = component_of(arg(args, 0)).ok_or(ValueError::Variant)?;
            {
                let mut child_state = child.borrow_mut();
                child_state.owner = Some(component.clone());
                child_state.component_index = component.borrow().components.len() as i32;
            }
            component.borrow_mut().components.push(child);
            Ok(())
        },
    );
    method(
        adapter,
        &classes.t_component,
        "RemoveComponent",
        &[EMPTY],
        |_value, args| {
            let component = component_arg(args)?;
            let child = component_of(arg(args, 0)).ok_or(ValueError::Variant)?;
            component
                .borrow_mut()
                .components
                .retain(|entry| !Rc::ptr_eq(entry, &child));
            child.borrow_mut().owner = None;
            Ok(())
        },
    );
    method(
        adapter,
        &classes.t_component,
        "SafeCallException",
        &[EMPTY, EMPTY],
        |value, _args| {
            *value = Value::Integer(0);
            Ok(())
        },
    );
    method(adapter, &classes.t_component, "ComObject", &[], |value, _args| {
        // `TComponent.ComObject` is a COM interface; the port has none.
        *value = Value::Null;
        Ok(())
    });
    {
        let class = classes.t_component.clone();
        index_get(
            adapter,
            &classes.t_component,
            "Components",
            &[EMPTY],
            move |value, args| {
                let component = component_arg(args)?;
                let index = arg_i32(args, 0)?.max(0) as usize;
                let child = component.borrow().components.get(index).cloned();
                *value = match child {
                    Some(state) => Value::Object(Rc::new(RefCell::new(ComponentObject {
                        class: class.clone(),
                        state,
                    }))),
                    None => Value::Null,
                };
                Ok(())
            },
        );
    }
    method(adapter, &classes.t_component, "ComponentCount", &[], |value, args| {
        *value = Value::Integer(component_arg(args)?.borrow().components.len() as i32);
        Ok(())
    });
    method(adapter, &classes.t_component, "ComponentIndex", &[], |value, args| {
        *value = Value::Integer(component_arg(args)?.borrow().component_index);
        Ok(())
    });
    setter(
        adapter,
        &classes.t_component,
        "ComponentIndex",
        &[EMPTY],
        |value, args| {
            component_arg(args)?.borrow_mut().component_index = to_i32(value)?;
            Ok(())
        },
    );
    for name in ["ComponentState", "ComponentStyle"] {
        method(adapter, &classes.t_component, name, &[], |_value, _args| {
            // UPSTREAM-QUIRK: both bodies raise with the text
            // `TComponent.ComponentState` (`JvInterpreter_Classes.pas:1070`).
            Err(ValueError::not_implemented("TComponent.ComponentState"))
        });
    }
    method(adapter, &classes.t_component, "DesignInfo", &[], |value, args| {
        *value = Value::Integer(component_arg(args)?.borrow().design_info);
        Ok(())
    });
    setter(adapter, &classes.t_component, "DesignInfo", &[EMPTY], |value, args| {
        component_arg(args)?.borrow_mut().design_info = to_i32(value)?;
        Ok(())
    });
    {
        let class = classes.t_component.clone();
        method(adapter, &classes.t_component, "Owner", &[], move |value, args| {
            let owner = component_arg(args)?.borrow().owner.clone();
            *value = match owner {
                Some(state) => Value::Object(Rc::new(RefCell::new(ComponentObject {
                    class: class.clone(),
                    state,
                }))),
                None => Value::Null,
            };
            Ok(())
        });
    }
    method(adapter, &classes.t_component, "VCLComObject", &[], |value, _args| {
        *value = Value::Pointer(0);
        Ok(())
    });
    setter(
        adapter,
        &classes.t_component,
        "VCLComObject",
        &[EMPTY],
        |_value, _args| Ok(()),
    );
    method(adapter, &classes.t_component, "Name", &[], |value, args| {
        *value = Value::Str(component_arg(args)?.borrow().name.clone().into_bytes());
        Ok(())
    });
    setter(adapter, &classes.t_component, "Name", &[EMPTY], |value, args| {
        let name = String::from_utf8_lossy(&to_str(value)?).into_owned();
        component_arg(args)?.borrow_mut().name = name;
        Ok(())
    });
    method(adapter, &classes.t_component, "Tag", &[], |value, args| {
        *value = Value::Integer(component_arg(args)?.borrow().tag);
        Ok(())
    });
    setter(adapter, &classes.t_component, "Tag", &[EMPTY], |value, args| {
        let tag = to_i32(value)?;
        component_arg(args)?.borrow_mut().tag = tag;
        Ok(())
    });
}
