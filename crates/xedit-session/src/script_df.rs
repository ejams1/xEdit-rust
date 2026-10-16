// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

// Ported from xEdit: xEdit/JvI/xejviScriptAdapterDF.pas (the whole unit: the
// `wbDataFormat` script surface -- TdfDef, TdfElement with its container and
// value subclasses, TwbNiRef, TwbNifBlock, TwbNifFile and the BGSM, BGEM,
// LOD, FUZ and DDS file classes) over the phase 5 data format port
// (`xedit-assets`), with the resource callback of Core/wbDataFormat.pas
// (`dfResourceGetDataCallback`) bound to the container handler
// (`xedit_core::container_handler::open_resource_data`).

//! The data format (`DF`) script adapter: the `wbDataFormat` unit a script's
//! `TwbNifFile`/`TdfElement`/`TwbNifBlock` names belong to, registered on the
//! run's interpreter by [`register`].
//!
//! A DF object is a wrapper over a document -- a [`DfDoc`] holding the phase
//! 5 [`Tree`] -- plus the element index inside it, so every object a script
//! holds from one `Create` points into the same tree and sees the others'
//! edits. A `TdfDef` object is the static definition itself ([`Def`]); the
//! script class of an object is derived from the port's element class and
//! definition kind, so the method dispatch of the registration lists sees the
//! upstream hierarchy (`TdfValue` is a `TdfElement`, `TwbNifBlock` a
//! `TdfStruct`, and so on).
//!
//! Every registration of `xejviScriptAdapterDF.pas` is bound here except
//! `TdfDef.Create`, whose registration is commented out upstream (line 767;
//! the ledger marks it `excluded`). Three entries reach outside the tree:
//! `LoadFromFile` reads a loose file, `LoadFromResource` reads through the
//! container handler (what the GUI sets `dfResourceGetDataCallback` to) with
//! upstream's `Resource not found` messages, and `SaveToFile`/
//! `SaveToJSONFile` write the serialized data.
//!
//! What differs from the Delphi model, all of it invisible to the corpus
//! scripts (their uses are the `DF` calls of `Assets manager.pas`,
//! `Assets browser.pas`, `FUZer.pas`, the four `NIF -` scripts,
//! `LOD Statistics.pas` and `DDS - Find textures without mipmaps.pas`):
//!
//! * `O2V(nil)` -- an element getter with nothing to return, such as
//!   `LinksTo` of an element that links to nothing -- is `Value::Empty`
//!   (`Unassigned`) here; upstream carries a `varObject` with a nil pointer.
//!   `Assigned` sees both as not assigned.
//! * `Free` of the root object marks the document released and every later
//!   call on its elements fails with a clear message; upstream destroys the
//!   Delphi tree and a later use is a use-after-free. `Free` of a non-root
//!   element does nothing (upstream frees the subtree and leaves a dangling
//!   pointer in its parent).
//! * The `TList`/`TStrings` a `GetAssetsList` or a `...ByType` call fills are
//!   the JVCL `Classes` unit's (phase 6 step 8): the append goes through
//!   `ScriptLists` of the run state, and until that unit registers its `Add`
//!   the call fails with a message naming it. A `...ByType` call whose list
//!   argument is given fills the list and yields no result.
//! * A raised error uses the interpreter's external path, so the run log
//!   prints `[EJvInterpreterError]` where the GUI prints the raised class
//!   (`[Exception]`); the message text is the port's, which phase 5 matched
//!   to `TdfElement.DoException`'s.
//! * A `LoadFromFile`/`SaveToFile` that fails with an OSError carries the
//!   crate's `Cannot open file "<path>". <os error>` text where the GUI's
//!   `CreateFile`/`TFileStream` route raises `RaiseLastOSError` (the bare
//!   `SysErrorMessage` text).

use std::any::Any;
use std::cell::RefCell;
use std::rc::Rc;

use xedit_assets::asset::{AssetFile, AssetKind};
use xedit_assets::data_format::{
    Class, DataType, Def, DefKind, DfError, El, StructClass, Tree, ValueClass, df_calc_hash, df_float_to_str,
    df_str_to_float,
};
use xedit_assets::data_format_material::MaterialFile;
use xedit_assets::data_format_misc::{MiscFile, create_misc_file};
use xedit_assets::data_format_nif::{
    self as nif, NifVersion, block_add_child, block_add_extra_data, block_add_property, block_child_by_type,
    block_children_by_type, block_extra_data_by_name, block_extra_data_by_type, block_extra_datas_by_type,
    block_is_ni_object, block_properties_by_type, block_property_by_name, block_property_by_type, block_refs,
    block_strings, block_update_bounds, block_update_normals, block_update_tangents, convert_block, get_assets_list,
    set_nif_version,
};
use xedit_assets::variant::Variant;
use xedit_core::container_handler;
use xedit_script::adapter::{Adapter, AdapterGetFn, AdapterSetFn, Args, PR_ARGS_NO_CHECK};
use xedit_script::error::{IE_NOT_ENOUGH_PARAMS, IE_TOO_MANY_PARAMS};
use xedit_script::values::{
    ClassDef, ParamType, ScriptObject, Value, ValueError, VarType, VariantArray, to_bool, to_f64, to_i32, to_str,
};

use crate::script::ScriptRunState;

/// `sDFUnitName`: the unit every registration of the adapter belongs to
/// (`'wbDataFormat'`; the `dt` prefix of the data type constants conflicts
/// with the plugin data types, so they are registered as `dft*`).
const UNIT: &str = "wbDataFormat";

/// The message of a call on an element of a document a script freed
/// (`Args.Obj.Free`). Upstream is a use-after-free there.
const FREED: &str = "The file has been freed";

/// One data format document: the element tree every script object of the
/// document points into.
pub struct DfDoc {
    pub tree: Tree,
    /// The root element (`Tree::root_el`).
    pub root: El,
    /// The kind the document was created as: the script class of the root
    /// (`TwbNifFile`, `TwbDDSFile`...). `Tree` alone cannot tell a DDS root
    /// from a LOD settings root, both plain structs.
    pub kind: Option<AssetKind>,
    /// `Free` of the root object.
    pub released: bool,
}

impl DfDoc {
    /// A new document of `kind`, as `AssetFile::new` builds it.
    pub fn create(kind: AssetKind) -> Result<DfDoc, DfError> {
        let file = AssetFile::new(kind)?;
        Ok(DfDoc {
            tree: file.tree,
            root: file.root,
            kind: Some(kind),
            released: false,
        })
    }
}

/// A `TdfElement` (or an instance of one of its subclasses) a script holds.
pub struct DfElement {
    doc: Rc<RefCell<DfDoc>>,
    el: El,
    class: Rc<ClassDef>,
}

impl DfElement {
    /// The element a script value carries, when it is one.
    fn of(value: &Value) -> Option<DfElement> {
        let Value::Object(object) = value else {
            return None;
        };
        let borrowed = object.borrow();
        let element = borrowed.as_any().downcast_ref::<DfElement>()?;
        Some(DfElement {
            doc: element.doc.clone(),
            el: element.el,
            class: element.class.clone(),
        })
    }
}

impl ScriptObject for DfElement {
    fn class(&self) -> Rc<ClassDef> {
        self.class.clone()
    }

    fn as_any(&self) -> &dyn Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }

    /// `Args.Obj.Free` (`JvInterpreter.pas:3976`): upstream destroys the
    /// Delphi object. The port marks the document released when the receiver
    /// is its root (`TwbNifFile.Free`, the only form the corpus uses) and
    /// leaves a non-root element alone.
    fn free(&mut self) {
        let mut doc = self.doc.borrow_mut();
        if self.el == doc.root {
            doc.released = true;
        }
    }
}

/// A `TdfDef` a script holds (the `Def` property of an element).
pub struct DfDefObject {
    def: &'static Def,
    class: Rc<ClassDef>,
}

impl ScriptObject for DfDefObject {
    fn class(&self) -> Rc<ClassDef> {
        self.class.clone()
    }

    fn as_any(&self) -> &dyn Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }
}

/// The classes `AddClass` registers, in the `TdfDef` and `TdfElement`
/// hierarchies of `wbDataFormat.pas` (41 of them; Delphi reads the parents
/// from RTTI, this port is told them).
struct DfClasses {
    // TdfDef
    tdf_def: Rc<ClassDef>,
    /// Registered as the parent of `TdfUnionDef` and the rest; no
    /// definition is exactly this class, so nothing reads it back.
    #[allow(dead_code)]
    tdf_value_def: Rc<ClassDef>,
    tdf_struct_def: Rc<ClassDef>,
    tdf_array_def: Rc<ClassDef>,
    tdf_union_def: Rc<ClassDef>,
    tdf_value_union_def: Rc<ClassDef>,
    tdf_merge_def: Rc<ClassDef>,
    tdf_integer_def: Rc<ClassDef>,
    tdf_mapped_integer_def: Rc<ClassDef>,
    tdf_flags_def: Rc<ClassDef>,
    tdf_enum_def: Rc<ClassDef>,
    tdf_float_def: Rc<ClassDef>,
    tdf_bytes_def: Rc<ClassDef>,
    tdf_chars_def: Rc<ClassDef>,
    twb_ni_ref_def: Rc<ClassDef>,
    twb_nif_block_def: Rc<ClassDef>,
    // TdfElement
    tdf_element: Rc<ClassDef>,
    tdf_value: Rc<ClassDef>,
    /// The parent of `TdfStruct`, `TdfArray` and `TdfUnion` and of
    /// `TwbNifFile`; nothing is exactly this class.
    #[allow(dead_code)]
    tdf_container: Rc<ClassDef>,
    tdf_struct: Rc<ClassDef>,
    tdf_array: Rc<ClassDef>,
    tdf_union: Rc<ClassDef>,
    tdf_value_union: Rc<ClassDef>,
    tdf_merge: Rc<ClassDef>,
    tdf_integer: Rc<ClassDef>,
    tdf_flags: Rc<ClassDef>,
    tdf_enum: Rc<ClassDef>,
    tdf_float: Rc<ClassDef>,
    tdf_bytes: Rc<ClassDef>,
    tdf_chars: Rc<ClassDef>,
    twb_ni_ref: Rc<ClassDef>,
    twb_nif_block: Rc<ClassDef>,
    twb_nif_file: Rc<ClassDef>,
    twb_bgsm_file: Rc<ClassDef>,
    twb_bgem_file: Rc<ClassDef>,
    twb_lod_tree_lst_file: Rc<ClassDef>,
    twb_lod_tree_btt_file: Rc<ClassDef>,
    twb_lod_settings_fo3_file: Rc<ClassDef>,
    twb_lod_settings_tes5_file: Rc<ClassDef>,
    twb_fuz_file: Rc<ClassDef>,
    twb_dds_file: Rc<ClassDef>,
}

/// The document a registration body works on: the shared handle (for
/// wrapping elements around the call), the borrowed tree it may change, and
/// the receiver element.
struct Doc<'a> {
    rc: &'a Rc<RefCell<DfDoc>>,
    file: &'a mut DfDoc,
    el: El,
}

impl Doc<'_> {
    /// The script value of an element of this document.
    fn element(&self, ctx: &Ctx, el: El) -> Value {
        ctx.element_value(self.rc, self.file, el)
    }

    /// `O2V` of an optional element slot: the element, or `Unassigned`.
    fn optional_element(&self, ctx: &Ctx, item: Option<El>) -> Value {
        match item {
            Some(item) => self.element(ctx, item),
            None => Value::Empty,
        }
    }
}

/// The classes and the run state a registration closure needs.
struct Ctx {
    classes: Rc<DfClasses>,
    state: Rc<RefCell<ScriptRunState>>,
}

impl Ctx {
    fn clone(&self) -> Ctx {
        Ctx {
            classes: self.classes.clone(),
            state: self.state.clone(),
        }
    }

    /// The script value of an element, with the document already borrowed.
    fn element_value(&self, rc: &Rc<RefCell<DfDoc>>, doc: &DfDoc, el: El) -> Value {
        Value::Object(Rc::new(RefCell::new(DfElement {
            doc: rc.clone(),
            el,
            class: class_of_element(&self.classes, doc, el),
        })))
    }

    /// The script value of a `TdfDef`.
    fn def_value(&self, def: &'static Def) -> Value {
        Value::Object(Rc::new(RefCell::new(DfDefObject {
            def,
            class: class_of_def(&self.classes, def),
        })))
    }

    /// A new, empty document of the kind.
    fn new_doc(&self, kind: AssetKind) -> Result<Rc<RefCell<DfDoc>>, ValueError> {
        Ok(Rc::new(RefCell::new(DfDoc::create(kind).map_err(df_error)?)))
    }
}

/// The script class of an element: the port's element class, the definition
/// kind of a value, or the document's kind for the root.
fn class_of_element(classes: &DfClasses, doc: &DfDoc, el: El) -> Rc<ClassDef> {
    if el == doc.root {
        match doc.kind {
            Some(AssetKind::Nif) => return classes.twb_nif_file.clone(),
            Some(AssetKind::Bgsm) => return classes.twb_bgsm_file.clone(),
            Some(AssetKind::Bgem) => return classes.twb_bgem_file.clone(),
            Some(AssetKind::LodSettingsTes5) => return classes.twb_lod_settings_tes5_file.clone(),
            Some(AssetKind::LodSettingsFo3) => return classes.twb_lod_settings_fo3_file.clone(),
            Some(AssetKind::LodTreeLst) => return classes.twb_lod_tree_lst_file.clone(),
            Some(AssetKind::LodTreeBtt) => return classes.twb_lod_tree_btt_file.clone(),
            Some(AssetKind::Fuz) => return classes.twb_fuz_file.clone(),
            Some(AssetKind::Dds) => return classes.twb_dds_file.clone(),
            None => {}
        }
    }
    match doc.tree.class(el) {
        Class::Element => classes.tdf_element.clone(),
        Class::Value(class) => class_of_value(classes, doc.tree.raw_def(el), class),
        Class::Struct(class) => match class {
            StructClass::Plain => classes.tdf_struct.clone(),
            StructClass::NifBlock => classes.twb_nif_block.clone(),
            StructClass::Bgsm => classes.twb_bgsm_file.clone(),
            StructClass::Bgem => classes.twb_bgem_file.clone(),
            StructClass::Fuz => classes.twb_fuz_file.clone(),
        },
        Class::Array => classes.tdf_array.clone(),
        Class::Union => classes.tdf_union.clone(),
        Class::NifFile => classes.twb_nif_file.clone(),
    }
}

/// The class of a value element: the definition kind tells a `TwbNiRef`, a
/// `TdfInteger`, a `TdfEnum` and a `TdfFloat` apart (the port keeps them one
/// `Class::Value`).
fn class_of_value(classes: &DfClasses, def: &Def, class: ValueClass) -> Rc<ClassDef> {
    match class {
        ValueClass::Merge => classes.tdf_merge.clone(),
        ValueClass::Flags => classes.tdf_flags.clone(),
        ValueClass::Bytes => classes.tdf_bytes.clone(),
        ValueClass::Chars => classes.tdf_chars.clone(),
        ValueClass::ValueUnion => classes.tdf_value_union.clone(),
        ValueClass::Plain => match def.kind {
            DefKind::NiRef { .. } => classes.twb_ni_ref.clone(),
            DefKind::Integer | DefKind::MappedInteger { .. } => classes.tdf_integer.clone(),
            DefKind::Enum { .. } => classes.tdf_enum.clone(),
            DefKind::Float => classes.tdf_float.clone(),
            _ => classes.tdf_value.clone(),
        },
    }
}

/// The script class of a definition (`TdfDef` and its subclasses).
fn class_of_def(classes: &DfClasses, def: &Def) -> Rc<ClassDef> {
    match def.kind {
        DefKind::Plain => classes.tdf_def.clone(),
        DefKind::Struct => classes.tdf_struct_def.clone(),
        DefKind::NifBlock => classes.twb_nif_block_def.clone(),
        DefKind::Array { .. } => classes.tdf_array_def.clone(),
        DefKind::Union => classes.tdf_union_def.clone(),
        DefKind::ValueUnion => classes.tdf_value_union_def.clone(),
        DefKind::Merge { .. } => classes.tdf_merge_def.clone(),
        DefKind::Integer => classes.tdf_integer_def.clone(),
        DefKind::MappedInteger { .. } => classes.tdf_mapped_integer_def.clone(),
        DefKind::Flags { .. } => classes.tdf_flags_def.clone(),
        DefKind::Enum { .. } => classes.tdf_enum_def.clone(),
        DefKind::Float => classes.tdf_float_def.clone(),
        DefKind::Bytes => classes.tdf_bytes_def.clone(),
        DefKind::Chars { .. } => classes.tdf_chars_def.clone(),
        DefKind::NiRef { .. } => classes.twb_ni_ref_def.clone(),
    }
}

//=== the argument and result conversions =====================================

/// A failed data format operation as the interpreter's external error (the
/// raised `Exception` of `TdfElement.DoException` and its kin).
fn df_error(error: DfError) -> ValueError {
    ValueError::External(error.0)
}

/// `dfStrToFloat`'s conversion failures are the RTL's `EConvertError` (it
/// calls `StrToFloat`), so the abort line names that class as the oracle's
/// does (parity #39).
fn df_convert_error(error: DfError) -> ValueError {
    ValueError::external_class("EConvertError", error.0)
}

/// The receiver of a method call, as a DF element.
fn receiver(args: &Args) -> Result<DfElement, ValueError> {
    args.obj
        .as_ref()
        .and_then(DfElement::of)
        .ok_or_else(|| ValueError::External("Not a data format element".to_owned()))
}

/// A `const ...: string` parameter (`VarToStr` of the argument).
fn text_arg(value: &Value) -> Result<String, ValueError> {
    Ok(String::from_utf8_lossy(&to_str(value)?).into_owned())
}

/// An `Index: Integer` parameter.
fn int_arg(value: &Value) -> Result<i32, ValueError> {
    to_i32(value)
}

/// A `Boolean` parameter with its Pascal default.
fn bool_arg(value: Option<&Value>, default: bool) -> Result<bool, ValueError> {
    match value {
        Some(value) => to_bool(value),
        None => Ok(default),
    }
}

/// A script value as a data format `Variant` (`NativeValue` setters).
fn variant_of(value: &Value) -> Result<Variant, ValueError> {
    Ok(match value {
        Value::Empty | Value::Null => Variant::Empty,
        Value::Smallint(v) => Variant::Int(i64::from(*v)),
        Value::Integer(v) => Variant::Int(i64::from(*v)),
        Value::Byte(v) => Variant::Int(i64::from(*v)),
        Value::Int64(v) => Variant::Int(*v),
        Value::Double(v) | Value::Date(v) => Variant::Float(*v),
        Value::Str(bytes) => Variant::Str(String::from_utf8_lossy(bytes).into_owned()),
        Value::Bool(v) => Variant::Bool(*v),
        Value::Set(v) => Variant::Int(i64::from(*v)),
        _ => return Err(ValueError::Variant),
    })
}

/// A data format `Variant` as a script value (`NativeValue` getters).
fn variant_value(variant: &Variant) -> Value {
    match variant {
        Variant::Empty => Value::Empty,
        Variant::Int(v) => match i32::try_from(*v) {
            Ok(v) => Value::Integer(v),
            Err(_) => Value::Int64(*v),
        },
        Variant::UInt(v) => Value::Int64(*v as i64),
        Variant::Float(v) => Value::Double(*v),
        Variant::Bool(v) => Value::Bool(*v),
        Variant::Str(text) => Value::Str(text.clone().into_bytes()),
        Variant::Bytes(bytes) => Value::VariantArray(Rc::new(RefCell::new(VariantArray::new(
            bytes.iter().map(|byte| Value::Byte(*byte)).collect(),
        )))),
    }
}

/// A `TBytes` argument (`LoadFromData`): an array of bytes, or the raw bytes
/// of a string.
fn bytes_arg(value: &Value) -> Result<Vec<u8>, ValueError> {
    match value {
        Value::Str(bytes) => Ok(bytes.clone()),
        Value::VariantArray(array) => array.borrow().values.iter().map(byte_of).collect(),
        _ => Err(ValueError::Variant),
    }
}

fn byte_of(value: &Value) -> Result<u8, ValueError> {
    match value {
        Value::Integer(v) => Ok(*v as u8),
        Value::Smallint(v) => Ok(*v as u8),
        Value::Byte(v) => Ok(*v),
        Value::Int64(v) => Ok(*v as u8),
        _ => Err(ValueError::Variant),
    }
}

/// A `TStringDynArray` result (the `GetAssetsList` with no list argument).
fn string_array(values: Vec<String>) -> Value {
    Value::VariantArray(Rc::new(RefCell::new(VariantArray::new(
        values.into_iter().map(|text| Value::Str(text.into_bytes())).collect(),
    ))))
}

/// Appends one element to the `TList` a `...ByType` call was given
/// (`lst.Add(b)`), through the run state's `ScriptLists` seam (the JVCL
/// `Classes` unit, phase 6 step 8).
fn append_to_list(ctx: &Ctx, list: &Value, item: Value) -> Result<(), ValueError> {
    let add = ctx.state.borrow().lists.list_add.clone();
    match add {
        Some(add) => add(list, item),
        None => Err(ValueError::External(
            "A script list is not available: the Classes unit (TList) is not registered yet".to_owned(),
        )),
    }
}

/// `TStrings(V2O(list)).Add(text)` (`GetAssetsList`), through the same seam
/// as [`append_to_list`].
fn append_to_strings(ctx: &Ctx, list: &Value, text: &str) -> Result<(), ValueError> {
    let add = ctx.state.borrow().lists.strings_add.clone();
    match add {
        Some(add) => add(list, text),
        None => Err(ValueError::External(
            "A script list is not available: the Classes unit (TStrings) is not registered yet".to_owned(),
        )),
    }
}

//=== registration ============================================================

/// Registers the `xejviScriptAdapterDF.pas` surface: the 41 classes, the 25
/// constants, the three `df` helpers and every method of `TdfDef`,
/// `TdfElement`, `TwbNiRef`, `TwbNifBlock`, `TwbNifFile` and the eight file
/// classes. `TdfDef.Create` stays out: its registration is commented out
/// upstream.
pub fn register(adapter: &mut Adapter, state: &Rc<RefCell<ScriptRunState>>) {
    let classes = Rc::new(define_classes(adapter));
    let ctx = Ctx {
        classes,
        state: state.clone(),
    };
    register_consts(adapter);
    register_functions(adapter);
    register_tdf_def(adapter, &ctx);
    register_tdf_element(adapter, &ctx);
    register_nif(adapter, &ctx);
    register_others(adapter, &ctx);
}

/// The `AddClass` calls, in the order of the unit.
fn define_classes(adapter: &mut Adapter) -> DfClasses {
    let class = |adapter: &mut Adapter, name: &str, parent: Option<Rc<ClassDef>>| adapter.add_class(UNIT, name, parent);
    // { TdfDef }
    let tdf_def = class(adapter, "TdfDef", None);
    let tdf_value_def = class(adapter, "TdfValueDef", Some(tdf_def.clone()));
    let tdf_struct_def = class(adapter, "TdfStructDef", Some(tdf_def.clone()));
    let tdf_array_def = class(adapter, "TdfArrayDef", Some(tdf_def.clone()));
    let tdf_union_def = class(adapter, "TdfUnionDef", Some(tdf_value_def.clone()));
    let tdf_value_union_def = class(adapter, "TdfValueUnionDef", Some(tdf_value_def.clone()));
    let tdf_merge_def = class(adapter, "TdfMergeDef", Some(tdf_value_def.clone()));
    let tdf_integer_def = class(adapter, "TdfIntegerDef", Some(tdf_value_def.clone()));
    let tdf_mapped_integer_def = class(adapter, "TdfMappedIntegerDef", Some(tdf_integer_def.clone()));
    let tdf_flags_def = class(adapter, "TdfFlagsDef", Some(tdf_mapped_integer_def.clone()));
    let tdf_enum_def = class(adapter, "TdfEnumDef", Some(tdf_mapped_integer_def.clone()));
    let tdf_float_def = class(adapter, "TdfFloatDef", Some(tdf_value_def.clone()));
    let tdf_bytes_def = class(adapter, "TdfBytesDef", Some(tdf_value_def.clone()));
    let tdf_chars_def = class(adapter, "TdfCharsDef", Some(tdf_value_def.clone()));
    let twb_ni_ref_def = class(adapter, "TwbNiRefDef", Some(tdf_integer_def.clone()));
    let twb_nif_block_def = class(adapter, "TwbNifBlockDef", Some(tdf_struct_def.clone()));
    // { TdfElement }
    let tdf_element = class(adapter, "TdfElement", None);
    let tdf_value = class(adapter, "TdfValue", Some(tdf_element.clone()));
    let tdf_container = class(adapter, "TdfContainer", Some(tdf_element.clone()));
    let tdf_struct = class(adapter, "TdfStruct", Some(tdf_container.clone()));
    let tdf_array = class(adapter, "TdfArray", Some(tdf_container.clone()));
    let tdf_union = class(adapter, "TdfUnion", Some(tdf_container.clone()));
    let tdf_value_union = class(adapter, "TdfValueUnion", Some(tdf_value.clone()));
    let tdf_merge = class(adapter, "TdfMerge", Some(tdf_value.clone()));
    let tdf_integer = class(adapter, "TdfInteger", Some(tdf_value.clone()));
    let tdf_flags = class(adapter, "TdfFlags", Some(tdf_integer.clone()));
    let tdf_enum = class(adapter, "TdfEnum", Some(tdf_integer.clone()));
    let tdf_float = class(adapter, "TdfFloat", Some(tdf_value.clone()));
    let tdf_bytes = class(adapter, "TdfBytes", Some(tdf_value.clone()));
    let tdf_chars = class(adapter, "TdfChars", Some(tdf_value.clone()));
    let twb_ni_ref = class(adapter, "TwbNiRef", Some(tdf_integer.clone()));
    let twb_nif_block = class(adapter, "TwbNifBlock", Some(tdf_struct.clone()));
    let twb_nif_file = class(adapter, "TwbNifFile", Some(tdf_container.clone()));
    // { Others }
    let twb_bgsm_file = class(adapter, "TwbBGSMFile", Some(tdf_struct.clone()));
    let twb_bgem_file = class(adapter, "TwbBGEMFile", Some(tdf_struct.clone()));
    let twb_lod_tree_lst_file = class(adapter, "TwbLODTreeLSTFile", Some(tdf_array.clone()));
    let twb_lod_tree_btt_file = class(adapter, "TwbLODTreeBTTFile", Some(tdf_array.clone()));
    let twb_lod_settings_fo3_file = class(adapter, "TwbLODSettingsFO3File", Some(tdf_struct.clone()));
    let twb_lod_settings_tes5_file = class(adapter, "TwbLODSettingsTES5File", Some(tdf_struct.clone()));
    let twb_fuz_file = class(adapter, "TwbFUZFile", Some(tdf_struct.clone()));
    let twb_dds_file = class(adapter, "TwbDDSFile", Some(tdf_struct.clone()));
    DfClasses {
        tdf_def,
        tdf_value_def,
        tdf_struct_def,
        tdf_array_def,
        tdf_union_def,
        tdf_value_union_def,
        tdf_merge_def,
        tdf_integer_def,
        tdf_mapped_integer_def,
        tdf_flags_def,
        tdf_enum_def,
        tdf_float_def,
        tdf_bytes_def,
        tdf_chars_def,
        twb_ni_ref_def,
        twb_nif_block_def,
        tdf_element,
        tdf_value,
        tdf_container,
        tdf_struct,
        tdf_array,
        tdf_union,
        tdf_value_union,
        tdf_merge,
        tdf_integer,
        tdf_flags,
        tdf_enum,
        tdf_float,
        tdf_bytes,
        tdf_chars,
        twb_ni_ref,
        twb_nif_block,
        twb_nif_file,
        twb_bgsm_file,
        twb_bgem_file,
        twb_lod_tree_lst_file,
        twb_lod_tree_btt_file,
        twb_lod_settings_fo3_file,
        twb_lod_settings_tes5_file,
        twb_fuz_file,
        twb_dds_file,
    }
}

/// `AddGet` with the `varEmpty` parameter types every method of the unit
/// registers.
fn get(adapter: &mut Adapter, class: &Rc<ClassDef>, identifier: &str, param_count: i32, func: AdapterGetFn) {
    adapter.add_get(
        class.clone(),
        identifier,
        func,
        param_count,
        param_types(param_count, VarType::Empty),
        VarType::Empty,
    );
}

/// `AddSet`.
fn set(adapter: &mut Adapter, class: &Rc<ClassDef>, identifier: &str, param_count: i32, func: AdapterSetFn) {
    adapter.add_set(
        class.clone(),
        identifier,
        func,
        param_count,
        param_types(param_count, VarType::Empty),
    );
}

/// `AddIGet`.
fn iget(adapter: &mut Adapter, class: &Rc<ClassDef>, identifier: &str, param_count: i32, func: AdapterGetFn) {
    adapter.add_iget(
        class.clone(),
        identifier,
        func,
        param_count,
        param_types(param_count, VarType::Empty),
        VarType::Empty,
    );
}

/// `AddISet` (the index parameter of the unit's is registered `varNull`).
fn iset(adapter: &mut Adapter, class: &Rc<ClassDef>, identifier: &str, param_count: i32, func: AdapterSetFn) {
    adapter.add_iset(
        class.clone(),
        identifier,
        func,
        param_count,
        param_types(param_count, VarType::Null),
    );
}

/// The parameter types a registration passes: `prArgsNoCheck` (`-1`) carries
/// the one `varEmpty` upstream writes (`[varEmpty, varEmpty]` for the
/// two-argument ones, which `CheckArgs` never reads).
fn param_types(param_count: i32, typ: VarType) -> Vec<ParamType> {
    let count = if param_count == PR_ARGS_NO_CHECK {
        1
    } else {
        param_count.max(0) as usize
    };
    vec![ParamType::value(typ); count]
}

/// Runs `body` with the receiver's document, refusing one a script freed.
fn run_with_doc<T>(element: &DfElement, body: impl FnOnce(Doc<'_>) -> Result<T, ValueError>) -> Result<T, ValueError> {
    let mut borrowed = element.doc.borrow_mut();
    if borrowed.released {
        return Err(ValueError::External(FREED.to_owned()));
    }
    body(Doc {
        rc: &element.doc,
        file: &mut borrowed,
        el: element.el,
    })
}

/// `AddGet` of a `TdfElement` reader without arguments.
fn element_get<F>(adapter: &mut Adapter, class: &Rc<ClassDef>, identifier: &str, param_count: i32, ctx: &Ctx, body: F)
where
    F: Fn(&Ctx, Doc<'_>) -> Result<Value, ValueError> + 'static,
{
    let ctx = ctx.clone();
    get(
        adapter,
        class,
        identifier,
        param_count,
        Rc::new(move |value, args| {
            let element = receiver(args)?;
            *value = run_with_doc(&element, |doc| body(&ctx, doc))?;
            Ok(())
        }),
    );
}

/// `AddGet` of a `TdfElement` reader with its arguments (the `case Args.Count`
/// methods).
fn element_get_args<F>(
    adapter: &mut Adapter,
    class: &Rc<ClassDef>,
    identifier: &str,
    param_count: i32,
    ctx: &Ctx,
    body: F,
) where
    F: Fn(&Ctx, Doc<'_>, &Args) -> Result<Value, ValueError> + 'static,
{
    let ctx = ctx.clone();
    get(
        adapter,
        class,
        identifier,
        param_count,
        Rc::new(move |value, args| {
            let element = receiver(args)?;
            *value = run_with_doc(&element, |doc| body(&ctx, doc, args))?;
            Ok(())
        }),
    );
}

/// `AddIGet` of a `TdfElement` reader with its index arguments.
fn element_iget_args<F>(
    adapter: &mut Adapter,
    class: &Rc<ClassDef>,
    identifier: &str,
    param_count: i32,
    ctx: &Ctx,
    body: F,
) where
    F: Fn(&Ctx, Doc<'_>, &Args) -> Result<Value, ValueError> + 'static,
{
    let ctx = ctx.clone();
    iget(
        adapter,
        class,
        identifier,
        param_count,
        Rc::new(move |value, args| {
            let element = receiver(args)?;
            *value = run_with_doc(&element, |doc| body(&ctx, doc, args))?;
            Ok(())
        }),
    );
}

/// `AddSet` of a `TdfElement` writer without arguments.
fn element_set<F>(adapter: &mut Adapter, class: &Rc<ClassDef>, identifier: &str, param_count: i32, body: F)
where
    F: Fn(Doc<'_>, &Value) -> Result<(), ValueError> + 'static,
{
    set(
        adapter,
        class,
        identifier,
        param_count,
        Rc::new(move |value, args| {
            let element = receiver(args)?;
            run_with_doc(&element, |doc| body(doc, value)).map(|_| ())
        }),
    );
}

/// `AddISet` of an indexed `TdfElement` writer (the path or index in `Args`).
fn element_set_args<F>(adapter: &mut Adapter, class: &Rc<ClassDef>, identifier: &str, param_count: i32, body: F)
where
    F: Fn(Doc<'_>, &Args, &Value) -> Result<(), ValueError> + 'static,
{
    iset(
        adapter,
        class,
        identifier,
        param_count,
        Rc::new(move |value, args| {
            let element = receiver(args)?;
            run_with_doc(&element, |doc| body(doc, args, value)).map(|_| ())
        }),
    );
}

/// The `dft*`, `nfo*` and `nf*` constants.
fn register_consts(adapter: &mut Adapter) {
    // 'dt' prefix conflicts with xEdit's data types for plugins, use 'dft'
    // instead.
    const DATA_TYPES: &[(&str, DataType)] = &[
        ("dftNone", DataType::None),
        ("dftStruct", DataType::Struct),
        ("dftArray", DataType::Array),
        ("dftUnion", DataType::Union),
        ("dftMerge", DataType::Merge),
        ("dftBytes", DataType::Bytes),
        ("dftChars", DataType::Chars),
        ("dftU8", DataType::U8),
        ("dftS8", DataType::S8),
        ("dftU16", DataType::U16),
        ("dftS16", DataType::S16),
        ("dftU32", DataType::U32),
        ("dftS32", DataType::S32),
        ("dftU64", DataType::U64),
        ("dftS64", DataType::S64),
        ("dftFloat16", DataType::Float16),
        ("dftFloat32", DataType::Float32),
    ];
    for (name, value) in DATA_TYPES {
        adapter.add_const(UNIT, name, Value::Integer(*value as i32));
    }
    // { TwbNifOptions: nfoCollapseLinkArrays, nfoRemoveUnusedStrings }
    adapter.add_const(UNIT, "nfoCollapseLinkArrays", Value::Integer(0));
    adapter.add_const(UNIT, "nfoRemoveUnusedStrings", Value::Integer(1));
    // { TwbNifVersion }
    const NIF_VERSIONS: &[(&str, NifVersion)] = &[
        ("nfTES3", NifVersion::Tes3),
        ("nfTES4", NifVersion::Tes4),
        ("nfFO3", NifVersion::Fo3),
        ("nfTES5", NifVersion::Tes5),
        ("nfSSE", NifVersion::Sse),
        ("nfFO4", NifVersion::Fo4),
    ];
    for (name, value) in NIF_VERSIONS {
        adapter.add_const(UNIT, name, Value::Integer(*value as i32));
    }
}

/// The three `df` helper functions.
fn register_functions(adapter: &mut Adapter) {
    adapter.add_function(
        UNIT,
        "dfFloatToStr",
        Rc::new(|value, args| {
            let number = to_f64(&args.values[0])?;
            *value = Value::Str(df_float_to_str(number).into_bytes());
            Ok(())
        }),
        1,
        param_types(1, VarType::Empty),
        VarType::Empty,
    );
    adapter.add_function(
        UNIT,
        "dfStrToFloat",
        Rc::new(|value, args| {
            let text = text_arg(&args.values[0])?;
            *value = Value::Double(df_str_to_float(&text).map_err(df_convert_error)?);
            Ok(())
        }),
        1,
        param_types(1, VarType::Empty),
        VarType::Empty,
    );
    adapter.add_function(
        UNIT,
        "dfCalcHash",
        Rc::new(|value, args| {
            let text = text_arg(&args.values[0])?;
            // `Value := dfCalcHash(...)` is a `Cardinal` (varLongWord); the
            // port keeps it in an `Int64` so the value and its text stay the
            // unsigned 32 bit number for hashes above `High(Integer)`
            // (parity #39; the `VarType` is 20 rather than 19, owed with
            // #28's value model).
            *value = Value::Int64(i64::from(df_calc_hash(&text)));
            Ok(())
        }),
        1,
        param_types(1, VarType::Empty),
        VarType::Empty,
    );
}

/// `{ TdfDef }`: the four property readers. `Create` is commented out
/// upstream (line 767) and stays unregistered.
fn register_tdf_def(adapter: &mut Adapter, ctx: &Ctx) {
    let class = ctx.classes.tdf_def.clone();
    for (identifier, what) in [
        ("Name", TdfDefValue::Name),
        ("DefaultDataSize", TdfDefValue::DefaultDataSize),
        // UPSTREAM-QUIRK: `TdfDef_DefaultValue` reads `DefaultDataSize` (the
        // copy-paste error of the line above); the port keeps it wrong the
        // same way.
        ("DefaultValue", TdfDefValue::DefaultDataSize),
        ("Size", TdfDefValue::Size),
    ] {
        get(adapter, &class, identifier, 0, {
            Rc::new(move |value, args| {
                let Some(Value::Object(object)) = args.obj.as_ref() else {
                    return Err(ValueError::External("Not a data format definition".to_owned()));
                };
                let borrowed = object.borrow();
                let Some(def) = borrowed.as_any().downcast_ref::<DfDefObject>().map(|object| object.def) else {
                    return Err(ValueError::External("Not a data format definition".to_owned()));
                };
                *value = match what {
                    TdfDefValue::Name => Value::Str(def.name.clone().into_bytes()),
                    TdfDefValue::DefaultDataSize => Value::Integer(def.default_data_size()),
                    TdfDefValue::Size => Value::Integer(def.size),
                };
                Ok(())
            })
        });
    }
}

/// What a `TdfDef` reader answers.
#[derive(Clone, Copy)]
enum TdfDefValue {
    Name,
    DefaultDataSize,
    Size,
}

/// `{ TdfElement }`: the property readers and writers, the indexed ones, and
/// the methods every element has.
fn register_tdf_element(adapter: &mut Adapter, ctx: &Ctx) {
    let class = ctx.classes.tdf_element.clone();

    // ---- simple readers ----
    // `TdfElement.GetDataType`/`GetDef` read `FDef`, the element's own
    // definition -- for a union the union's, not the active member's, which
    // is what the tree's `def()` resolves (`TdfElement_DataType`,
    // `TdfElement_Def`).
    element_get(adapter, &class, "DataType", 0, ctx, |_ctx, doc| {
        Ok(Value::Integer(doc.file.tree.raw_def(doc.el).data_type as i32))
    });
    element_get(adapter, &class, "Def", 0, ctx, |ctx, doc| {
        Ok(ctx.def_value(doc.file.tree.raw_def(doc.el)))
    });
    element_get(adapter, &class, "Name", 0, ctx, |_ctx, doc| {
        Ok(Value::Str(doc.file.tree.name(doc.el).map_err(df_error)?.into_bytes()))
    });
    element_get(adapter, &class, "Path", 0, ctx, |_ctx, doc| {
        Ok(Value::Str(doc.file.tree.path(doc.el).map_err(df_error)?.into_bytes()))
    });
    element_get(adapter, &class, "Root", 0, ctx, |ctx, doc| {
        let root = doc.file.tree.root(doc.el);
        Ok(doc.element(ctx, root))
    });
    element_get(adapter, &class, "Parent", 0, ctx, |ctx, doc| {
        let parent = doc.file.tree.parent(doc.el);
        Ok(doc.optional_element(ctx, parent))
    });
    element_get(adapter, &class, "DataSize", 0, ctx, |_ctx, doc| {
        Ok(Value::Integer(doc.file.tree.data_size(doc.el).map_err(df_error)?))
    });
    element_get(adapter, &class, "Index", 0, ctx, |_ctx, doc| {
        Ok(Value::Integer(doc.file.tree.index(doc.el).map_err(df_error)?))
    });
    element_get(adapter, &class, "Count", 0, ctx, |_ctx, doc| {
        Ok(Value::Integer(doc.file.tree.count(doc.el)))
    });
    element_get(adapter, &class, "Enabled", 0, ctx, |_ctx, doc| {
        Ok(Value::Bool(doc.file.tree.enabled(doc.el).map_err(df_error)?))
    });
    element_get(adapter, &class, "LinksTo", 0, ctx, |ctx, doc| {
        let target = doc.file.tree.links_to(doc.el).map_err(df_error)?;
        Ok(doc.optional_element(ctx, target))
    });
    element_get(adapter, &class, "NativeValue", 0, ctx, |_ctx, doc| {
        Ok(variant_value(&doc.file.tree.native_value(doc.el).map_err(df_error)?))
    });
    element_get(adapter, &class, "EditValue", 0, ctx, |_ctx, doc| {
        Ok(Value::Str(
            doc.file.tree.edit_value(doc.el).map_err(df_error)?.into_bytes(),
        ))
    });

    element_get_args(adapter, &class, "IndexOf", 1, ctx, |_ctx, doc, args| {
        // `IndexOf` compares the element objects; one of another document is
        // -1, which the port has to check because its `El` numbers would
        // collide.
        let other = DfElement::of(&args.values[0]);
        Ok(Value::Integer(match other {
            Some(other) if Rc::ptr_eq(&other.doc, doc.rc) => doc.file.tree.index_of(doc.el, other.el),
            _ => -1,
        }))
    });

    // ---- simple writers ----
    element_set(adapter, &class, "Parent", 0, |doc, value| {
        let parent = DfElement::of(value);
        let parent = match parent {
            Some(parent) if Rc::ptr_eq(&parent.doc, doc.rc) => Some(parent.el),
            _ => None,
        };
        doc.file.tree.set_parent(doc.el, parent);
        Ok(())
    });
    element_set(adapter, &class, "Enabled", 0, |doc, value| {
        doc.file.tree.set_enabled(doc.el, to_bool(value)?);
        Ok(())
    });
    element_set(adapter, &class, "Count", 0, |doc, value| {
        doc.file.tree.set_count(doc.el, to_i32(value)?).map_err(df_error)
    });
    element_set(adapter, &class, "NativeValue", 0, |doc, value| {
        doc.file
            .tree
            .set_native_value(doc.el, variant_of(value)?)
            .map_err(df_error)
    });
    element_set(adapter, &class, "EditValue", 0, |doc, value| {
        doc.file
            .tree
            .set_edit_value(doc.el, &text_arg(value)?)
            .map_err(df_error)
    });

    // ---- indexed property readers ----
    indexed_get(adapter, &class, "Items", IndexedKind::Item, ctx);
    for (identifier, kind) in [
        ("Elements", IndexedKind::Element),
        ("NativeValues", IndexedKind::Native),
        ("EditValues", IndexedKind::Edit),
    ] {
        indexed_get(adapter, &class, identifier, kind, ctx);
    }
    // `AddIDGet(TdfElement, TdfElement_Read_Items, ...)`: `el[i]` is
    // `Items[i]`.
    {
        let ctx = ctx.clone();
        let func: AdapterGetFn = Rc::new(move |value, args| {
            let element = receiver(args)?;
            let index = int_arg(&args.values[0])?;
            *value = run_with_doc(&element, |doc| {
                let item = doc.file.tree.item(doc.el, index).ok();
                Ok(doc.optional_element(&ctx, item))
            })?;
            Ok(())
        });
        adapter.add_id_get(class.clone(), func, 1, param_types(1, VarType::Empty), VarType::Empty);
    }

    // ---- indexed property writers ----
    element_set_args(adapter, &class, "NativeValues", 1, |doc, args, value| {
        doc.file
            .tree
            .set_native_values(doc.el, &text_arg(&args.values[0])?, variant_of(value)?)
            .map_err(df_error)
    });
    element_set_args(adapter, &class, "EditValues", 1, |doc, args, value| {
        doc.file
            .tree
            .set_edit_values(doc.el, &text_arg(&args.values[0])?, &text_arg(value)?)
            .map_err(df_error)
    });

    // ---- methods ----
    element_get(adapter, &class, "BeginUpdate", 0, ctx, |_ctx, doc| {
        doc.file.tree.begin_update(doc.el);
        Ok(Value::Empty)
    });
    element_get(adapter, &class, "EndUpdate", 0, ctx, |_ctx, doc| {
        doc.file.tree.end_update(doc.el);
        Ok(Value::Empty)
    });
    element_get(adapter, &class, "Add", 0, ctx, |ctx, doc| {
        let item = doc.file.tree.add(doc.el).map_err(df_error)?;
        Ok(doc.element(ctx, item))
    });
    element_get(adapter, &class, "Remove", 0, ctx, |_ctx, doc| {
        doc.file.tree.remove(doc.el).map_err(df_error)?;
        Ok(Value::Empty)
    });
    element_get_args(adapter, &class, "Delete", 1, ctx, |_ctx, doc, args| {
        doc.file
            .tree
            .delete(doc.el, int_arg(&args.values[0])?)
            .map_err(df_error)?;
        Ok(Value::Empty)
    });
    element_get_args(adapter, &class, "Move", 2, ctx, |_ctx, doc, args| {
        let current = int_arg(&args.values[0])?;
        let new_index = int_arg(&args.values[1])?;
        doc.file.tree.move_item(doc.el, current, new_index).map_err(df_error)?;
        Ok(Value::Empty)
    });
    element_get_args(adapter, &class, "ElementByName", 2, ctx, |ctx, doc, args| {
        let name = text_arg(&args.values[0])?;
        let enabled_only = to_bool(&args.values[1])?;
        let item = doc
            .file
            .tree
            .element_by_name(doc.el, &name, enabled_only)
            .map_err(df_error)?;
        Ok(doc.optional_element(ctx, item))
    });
    element_get_args(adapter, &class, "ElementByPath", 2, ctx, |ctx, doc, args| {
        let path = text_arg(&args.values[0])?;
        let enabled_only = to_bool(&args.values[1])?;
        let item = doc
            .file
            .tree
            .element_by_path(doc.el, &path, enabled_only)
            .map_err(df_error)?;
        Ok(doc.optional_element(ctx, item))
    });
    element_get_args(adapter, &class, "LoadFromData", 1, ctx, |_ctx, doc, args| {
        let data = bytes_arg(&args.values[0])?;
        doc.file.tree.load_from_data(doc.el, &data).map_err(df_error)?;
        Ok(Value::Empty)
    });
    element_get_args(adapter, &class, "LoadFromFile", 1, ctx, |_ctx, doc, args| {
        let path = text_arg(&args.values[0])?;
        let data = read_file(&path)?;
        doc.file.tree.load_from_data(doc.el, &data).map_err(df_error)?;
        Ok(Value::Empty)
    });
    element_get_args(adapter, &class, "LoadFromJSONFile", 1, ctx, |_ctx, doc, args| {
        let path = text_arg(&args.values[0])?;
        let bytes = read_file(&path)?;
        // `TStringList.LoadFromFile`: a byte order mark, else ANSI.
        let text = xedit_io::encoding::string_list_text(&bytes);
        doc.file.tree.from_json(doc.el, &text).map_err(df_error)?;
        Ok(Value::Empty)
    });
    element_get_args(
        adapter,
        &class,
        "LoadFromResource",
        PR_ARGS_NO_CHECK,
        ctx,
        |_ctx, doc, args| {
            let (container, file_name) = match args.values.len() {
                0 => return Err(ValueError::interp(IE_NOT_ENOUGH_PARAMS)),
                1 => (String::new(), text_arg(&args.values[0])?),
                2 => (text_arg(&args.values[0])?, text_arg(&args.values[1])?),
                _ => return Err(ValueError::interp(IE_TOO_MANY_PARAMS)),
            };
            let data = container_handler::open_resource_data(&container, &file_name);
            if data.is_empty() {
                // The `Resource not found` texts of the two overloads.
                let message = if container.is_empty() {
                    format!("Resource not found (File \"{file_name}\")")
                } else {
                    format!("Resource not found (File \"{file_name}\" in container \"{container}\")")
                };
                return Err(ValueError::External(message));
            }
            doc.file.tree.load_from_data(doc.el, &data).map_err(df_error)?;
            Ok(Value::Empty)
        },
    );
    element_get_args(adapter, &class, "SaveToFile", 1, ctx, |_ctx, doc, args| {
        let path = text_arg(&args.values[0])?;
        let data = doc.file.tree.save_to_data(doc.el).map_err(df_error)?;
        write_file(&path, &data)?;
        Ok(Value::Empty)
    });
    element_get_args(adapter, &class, "SaveToJSONFile", 2, ctx, |_ctx, doc, args| {
        let path = text_arg(&args.values[0])?;
        let compact = to_bool(&args.values[1])?;
        let text = doc.file.tree.to_json(doc.el, compact).map_err(df_error)?;
        write_file(&path, text.as_bytes())?;
        Ok(Value::Empty)
    });
    element_get(adapter, &class, "SetToDefault", 0, ctx, |_ctx, doc| {
        doc.file.tree.set_to_default(doc.el).map_err(df_error)?;
        Ok(Value::Empty)
    });
    element_get_args(adapter, &class, "Assign", 1, ctx, |_ctx, doc, args| {
        let source = DfElement::of(&args.values[0]);
        let source_el = source.as_ref().map(|source| source.el);
        match source.as_ref().filter(|source| !Rc::ptr_eq(&source.doc, doc.rc)) {
            // `Assign` from another document copies through
            // `Tree::assign_from`.
            Some(other) => {
                let other_doc = other.doc.clone();
                let mut other_doc = other_doc.borrow_mut();
                if other_doc.released {
                    return Err(ValueError::External(FREED.to_owned()));
                }
                doc.file
                    .tree
                    .assign_from(doc.el, &mut other_doc.tree, source_el)
                    .map_err(df_error)?;
            }
            None => doc.file.tree.assign(doc.el, source_el).map_err(df_error)?,
        }
        Ok(Value::Empty)
    });
    element_get_args(adapter, &class, "FromJSON", 1, ctx, |_ctx, doc, args| {
        let text = text_arg(&args.values[0])?;
        doc.file.tree.from_json(doc.el, &text).map_err(df_error)?;
        Ok(Value::Empty)
    });
    element_get_args(adapter, &class, "ToJSON", 1, ctx, |_ctx, doc, args| {
        let compact = to_bool(&args.values[0])?;
        Ok(Value::Str(
            doc.file.tree.to_json(doc.el, compact).map_err(df_error)?.into_bytes(),
        ))
    });
    element_get(adapter, &class, "ToText", 0, ctx, |_ctx, doc| {
        Ok(Value::Str(
            doc.file.tree.to_text(doc.el, 0).map_err(df_error)?.into_bytes(),
        ))
    });
}

/// What an indexed reader of `TdfElement` reads.
#[derive(Clone, Copy)]
enum IndexedKind {
    /// `Items[index]`: an element of the container.
    Item,
    /// `Elements[path]`: the element the path names.
    Element,
    /// `NativeValues[path]`.
    Native,
    /// `EditValues[path]`.
    Edit,
}

/// `AddIGet` of an indexed `TdfElement` reader (`Items` is the plain `item`
/// of the tree, the others take a path).
fn indexed_get(adapter: &mut Adapter, class: &Rc<ClassDef>, identifier: &str, kind: IndexedKind, ctx: &Ctx) {
    let ctx = ctx.clone();
    iget(
        adapter,
        class,
        identifier,
        1,
        Rc::new(move |value, args| {
            let element = receiver(args)?;
            *value = run_with_doc(&element, |doc| match kind {
                IndexedKind::Item => {
                    let index = int_arg(&args.values[0])?;
                    let item = doc.file.tree.item(doc.el, index).ok();
                    Ok(doc.optional_element(&ctx, item))
                }
                IndexedKind::Element => {
                    let path = text_arg(&args.values[0])?;
                    let item = doc.file.tree.elements(doc.el, &path).map_err(df_error)?;
                    Ok(doc.optional_element(&ctx, item))
                }
                IndexedKind::Native => {
                    let path = text_arg(&args.values[0])?;
                    Ok(variant_value(
                        &doc.file.tree.native_values(doc.el, &path).map_err(df_error)?,
                    ))
                }
                IndexedKind::Edit => {
                    let path = text_arg(&args.values[0])?;
                    Ok(Value::Str(
                        doc.file.tree.edit_values(doc.el, &path).map_err(df_error)?.into_bytes(),
                    ))
                }
            })?;
            Ok(())
        }),
    );
}

/// `{ NIF }`: `TwbNiRef`, `TwbNifBlock` and `TwbNifFile` (the
/// `TwbNifOptions`/`TwbNifVersion` constants are in [`register_consts`]).
fn register_nif(adapter: &mut Adapter, ctx: &Ctx) {
    // ---- TwbNiRef ----
    let class = ctx.classes.twb_ni_ref.clone();
    element_get(adapter, &class, "Template", 0, ctx, |_ctx, doc| {
        Ok(Value::Str(doc.file.tree.raw_def(doc.el).template().as_bytes().to_vec()))
    });
    element_get(adapter, &class, "Ptr", 0, ctx, |_ctx, doc| {
        Ok(Value::Bool(doc.file.tree.raw_def(doc.el).ptr()))
    });

    // ---- TwbNifBlock ----
    let class = ctx.classes.twb_nif_block.clone();
    element_get(adapter, &class, "BlockType", 0, ctx, |_ctx, doc| {
        Ok(Value::Str(nif::block_type(&doc.file.tree, doc.el).as_bytes().to_vec()))
    });
    element_get(adapter, &class, "NifFile", 0, ctx, |ctx, doc| {
        let root = doc.file.root;
        Ok(doc.element(ctx, root))
    });
    element_get(adapter, &class, "RefsCount", 0, ctx, |_ctx, doc| {
        Ok(Value::Integer(block_refs(&doc.file.tree, doc.el).len() as i32))
    });
    element_iget_args(adapter, &class, "Refs", 1, ctx, |ctx, doc, args| {
        let index = int_arg(&args.values[0])?;
        let item = block_refs(&doc.file.tree, doc.el).get(index.max(0) as usize).copied();
        Ok(doc.optional_element(ctx, item))
    });
    element_get(adapter, &class, "StringsCount", 0, ctx, |_ctx, doc| {
        Ok(Value::Integer(block_strings(&doc.file.tree, doc.el).len() as i32))
    });
    element_iget_args(adapter, &class, "Strings", 1, ctx, |ctx, doc, args| {
        let index = int_arg(&args.values[0])?;
        let item = block_strings(&doc.file.tree, doc.el)
            .get(index.max(0) as usize)
            .copied();
        Ok(doc.optional_element(ctx, item))
    });
    for (identifier, update) in [
        ("UpdateNormals", block_update_normals as BlockUpdate),
        ("UpdateTangents", block_update_tangents as BlockUpdate),
    ] {
        element_get_args(
            adapter,
            &class,
            identifier,
            PR_ARGS_NO_CHECK,
            ctx,
            move |_ctx, doc, args| {
                // The `aAddIfMissing: Boolean = True` default.
                let add = bool_arg(args.values.first(), true)?;
                Ok(Value::Bool(update(&mut doc.file.tree, doc.el, add).map_err(df_error)?))
            },
        );
    }
    element_get_args(
        adapter,
        &class,
        "IsNiObject",
        PR_ARGS_NO_CHECK,
        ctx,
        |_ctx, doc, args| {
            let (block_type, inherited) = match args.values.len() {
                0 => return Err(ValueError::interp(IE_NOT_ENOUGH_PARAMS)),
                // The `aInherited: Boolean = True` default.
                1 => (text_arg(&args.values[0])?, true),
                2 => (text_arg(&args.values[0])?, to_bool(&args.values[1])?),
                _ => return Err(ValueError::interp(IE_TOO_MANY_PARAMS)),
            };
            Ok(Value::Bool(block_is_ni_object(
                &doc.file.tree,
                doc.el,
                &block_type,
                inherited,
            )))
        },
    );
    for (identifier, add) in [
        ("AddChild", block_add_child as BlockAdd),
        ("AddExtraData", block_add_extra_data as BlockAdd),
        ("AddProperty", block_add_property as BlockAdd),
    ] {
        element_get_args(adapter, &class, identifier, 1, ctx, move |ctx, doc, args| {
            let block_type = text_arg(&args.values[0])?;
            let item = add(&mut doc.file.tree, doc.el, &block_type).map_err(df_error)?;
            Ok(doc.element(ctx, item))
        });
    }
    for (identifier, find) in [
        ("PropertyByName", block_property_by_name as BlockFind),
        ("ExtraDataByName", block_extra_data_by_name as BlockFind),
    ] {
        element_get_args(adapter, &class, identifier, 1, ctx, move |ctx, doc, args| {
            let name = text_arg(&args.values[0])?;
            let item = find(&mut doc.file.tree, doc.el, &name).map_err(df_error)?;
            Ok(doc.optional_element(ctx, item))
        });
    }
    for (identifier, by_type) in [
        ("ChildrenByType", block_children_by_type as BlockByType),
        ("PropertiesByType", block_properties_by_type as BlockByType),
        ("ExtraDatasByType", block_extra_datas_by_type as BlockByType),
    ] {
        element_get_args(
            adapter,
            &class,
            identifier,
            PR_ARGS_NO_CHECK,
            ctx,
            move |ctx, doc, args| {
                let (block_type, inherited, list) = match args.values.len() {
                    0 | 1 => return Err(ValueError::interp(IE_NOT_ENOUGH_PARAMS)),
                    // The `aInherited: Boolean = False` default.
                    2 => (text_arg(&args.values[0])?, false, args.values[1].clone()),
                    3 => (
                        text_arg(&args.values[0])?,
                        to_bool(&args.values[1])?,
                        args.values[2].clone(),
                    ),
                    _ => return Err(ValueError::interp(IE_TOO_MANY_PARAMS)),
                };
                let items = by_type(&mut doc.file.tree, doc.el, &block_type, inherited).map_err(df_error)?;
                for item in items {
                    let value = doc.element(ctx, item);
                    append_to_list(ctx, &list, value)?;
                }
                Ok(Value::Empty)
            },
        );
    }
    for (identifier, by_one) in [
        ("ChildByType", block_child_by_type as BlockByOne),
        ("PropertyByType", block_property_by_type as BlockByOne),
        ("ExtraDataByType", block_extra_data_by_type as BlockByOne),
    ] {
        element_get_args(
            adapter,
            &class,
            identifier,
            PR_ARGS_NO_CHECK,
            ctx,
            move |ctx, doc, args| {
                let (block_type, inherited) = match args.values.len() {
                    0 => return Err(ValueError::interp(IE_NOT_ENOUGH_PARAMS)),
                    1 => (text_arg(&args.values[0])?, false),
                    2 => (text_arg(&args.values[0])?, to_bool(&args.values[1])?),
                    _ => return Err(ValueError::interp(IE_TOO_MANY_PARAMS)),
                };
                let item = by_one(&mut doc.file.tree, doc.el, &block_type, inherited).map_err(df_error)?;
                Ok(doc.optional_element(ctx, item))
            },
        );
    }
    element_get(adapter, &class, "UpdateBounds", 0, ctx, |_ctx, doc| {
        Ok(Value::Bool(
            block_update_bounds(&mut doc.file.tree, doc.el).map_err(df_error)?,
        ))
    });
    element_get_args(
        adapter,
        &class,
        "GetAssetsList",
        PR_ARGS_NO_CHECK,
        ctx,
        |_ctx, _doc, args| {
            // `{ Deprecated } Result := nil`: the 0-argument form is
            // unassigned and the list form adds nothing.
            match args.values.len() {
                0 => Ok(Value::Empty),
                1 => Ok(Value::Empty),
                _ => Err(ValueError::interp(IE_TOO_MANY_PARAMS)),
            }
        },
    );

    // ---- TwbNifFile ----
    let class = ctx.classes.twb_nif_file.clone();
    get(adapter, &class, "Create", 0, {
        let ctx = ctx.clone();
        Rc::new(move |value, _args| {
            let doc = ctx.new_doc(AssetKind::Nif)?;
            *value = {
                let borrowed = doc.borrow();
                ctx.element_value(&doc, &borrowed, borrowed.root)
            };
            Ok(())
        })
    });
    element_get(adapter, &class, "NifVersion", 0, ctx, |_ctx, doc| {
        Ok(Value::Integer(doc.file.tree.nif.nif_version as i32))
    });
    element_set(adapter, &class, "NifVersion", 0, |doc, value| {
        set_nif_version(&mut doc.file.tree, nif_version_of(to_i32(value)?)).map_err(df_error)
    });
    element_get(adapter, &class, "InternalUpdates", 0, ctx, |_ctx, doc| {
        Ok(Value::Bool(doc.file.tree.nif.internal_updates))
    });
    element_set(adapter, &class, "InternalUpdates", 0, |doc, value| {
        doc.file.tree.nif.internal_updates = to_bool(value)?;
        Ok(())
    });
    // `Options: TwbNifOptions` is a set; `S2V(Byte(...))` of the two members.
    element_get(adapter, &class, "Options", 0, ctx, |_ctx, doc| {
        let options = &doc.file.tree.nif.options;
        Ok(Value::Integer(
            i32::from(options.collapse_link_arrays) | (i32::from(options.remove_unused_strings) << 1),
        ))
    });
    element_set(adapter, &class, "Options", 0, |doc, value| {
        let options = to_i32(value)?;
        doc.file.tree.nif.options.collapse_link_arrays = options & 1 != 0;
        doc.file.tree.nif.options.remove_unused_strings = options & 2 != 0;
        Ok(())
    });
    element_get(adapter, &class, "Header", 0, ctx, |ctx, doc| {
        let header = nif::header(&mut doc.file.tree).map_err(df_error)?;
        Ok(doc.element(ctx, header))
    });
    element_get(adapter, &class, "Footer", 0, ctx, |ctx, doc| {
        let footer = nif::footer(&mut doc.file.tree).map_err(df_error)?;
        Ok(doc.element(ctx, footer))
    });
    element_get(adapter, &class, "BlocksCount", 0, ctx, |_ctx, doc| {
        Ok(Value::Integer(nif::blocks_count(&mut doc.file.tree).map_err(df_error)?))
    });
    element_iget_args(adapter, &class, "Blocks", 1, ctx, |ctx, doc, args| {
        let index = int_arg(&args.values[0])?;
        let item = nif::block(&mut doc.file.tree, index).ok();
        Ok(doc.optional_element(ctx, item))
    });
    element_get_args(adapter, &class, "AddBlock", 1, ctx, |ctx, doc, args| {
        let block_type = text_arg(&args.values[0])?;
        let item = nif::add_block(&mut doc.file.tree, &block_type).map_err(df_error)?;
        Ok(doc.element(ctx, item))
    });
    element_get_args(adapter, &class, "InsertBlock", 2, ctx, |ctx, doc, args| {
        let index = int_arg(&args.values[0])?;
        let block_type = text_arg(&args.values[1])?;
        let item = nif::insert_block(&mut doc.file.tree, index, &block_type).map_err(df_error)?;
        Ok(doc.element(ctx, item))
    });
    element_get_args(adapter, &class, "ConvertBlock", 2, ctx, |_ctx, doc, args| {
        let index = int_arg(&args.values[0])?;
        let block_type = text_arg(&args.values[1])?;
        convert_block(&mut doc.file.tree, index, &block_type).map_err(df_error)?;
        Ok(Value::Empty)
    });
    element_get_args(adapter, &class, "CopyBlock", 1, ctx, |ctx, doc, args| {
        let index = int_arg(&args.values[0])?;
        let item = nif::copy_block(&mut doc.file.tree, index).map_err(df_error)?;
        Ok(doc.element(ctx, item))
    });
    element_get_args(
        adapter,
        &class,
        "BlockByType",
        PR_ARGS_NO_CHECK,
        ctx,
        |ctx, doc, args| {
            let (block_type, inherited) = by_one_args(&args.values)?;
            let item = nif::block_by_type(&mut doc.file.tree, &block_type, inherited).map_err(df_error)?;
            Ok(doc.optional_element(ctx, item))
        },
    );
    element_get_args(
        adapter,
        &class,
        "BlocksByType",
        PR_ARGS_NO_CHECK,
        ctx,
        |ctx, doc, args| {
            let (block_type, inherited, list) = by_type_args(&args.values)?;
            let items = nif::blocks_by_type(&mut doc.file.tree, &block_type, inherited).map_err(df_error)?;
            for item in items {
                let value = doc.element(ctx, item);
                append_to_list(ctx, &list, value)?;
            }
            Ok(Value::Empty)
        },
    );
    element_get_args(adapter, &class, "BlockByName", 1, ctx, |ctx, doc, args| {
        let name = text_arg(&args.values[0])?;
        let item = nif::block_by_name(&mut doc.file.tree, &name, "").map_err(df_error)?;
        Ok(doc.optional_element(ctx, item))
    });
    for (identifier, spell) in [
        (
            "SpellTriangulate",
            xedit_assets::data_format_nif::spell_triangulate as Spell,
        ),
        (
            "SpellFaceNormals",
            xedit_assets::data_format_nif::spell_face_normals as Spell,
        ),
        (
            "SpellUpdateTangents",
            xedit_assets::data_format_nif::spell_update_tangents as Spell,
        ),
        (
            "SpellAddUpdateTangents",
            xedit_assets::data_format_nif::spell_add_update_tangents as Spell,
        ),
    ] {
        element_get(adapter, &class, identifier, 0, ctx, move |_ctx, doc| {
            Ok(Value::Bool(spell(&mut doc.file.tree).map_err(df_error)?))
        });
    }
    element_get_args(
        adapter,
        &class,
        "GetAssetsList",
        PR_ARGS_NO_CHECK,
        ctx,
        |ctx, doc, args| {
            let names = get_assets_list(&mut doc.file.tree).map_err(df_error)?;
            match args.values.len() {
                0 => Ok(string_array(names)),
                1 => {
                    let list = args.values[0].clone();
                    for name in names {
                        append_to_strings(ctx, &list, &name)?;
                    }
                    Ok(Value::Empty)
                }
                _ => Err(ValueError::interp(IE_TOO_MANY_PARAMS)),
            }
        },
    );
}

/// The `...ByType` functions that fill a list.
type BlockByType = fn(&mut Tree, El, &str, bool) -> Result<Vec<El>, DfError>;
/// The `...ByType` functions that answer one element.
type BlockByOne = fn(&mut Tree, El, &str, bool) -> Result<Option<El>, DfError>;
/// `AddChild`/`AddExtraData`/`AddProperty`.
type BlockAdd = fn(&mut Tree, El, &str) -> Result<El, DfError>;
/// `PropertyByName`/`ExtraDataByName`.
type BlockFind = fn(&mut Tree, El, &str) -> Result<Option<El>, DfError>;
/// `UpdateNormals`/`UpdateTangents`.
type BlockUpdate = fn(&mut Tree, El, bool) -> Result<bool, DfError>;
/// The `Spell*` functions.
type Spell = fn(&mut Tree) -> Result<bool, DfError>;

/// `aBlockType[, aInherited]`, the default `False`.
fn by_one_args(values: &[Value]) -> Result<(String, bool), ValueError> {
    match values.len() {
        0 => Err(ValueError::interp(IE_NOT_ENOUGH_PARAMS)),
        1 => Ok((text_arg(&values[0])?, false)),
        2 => Ok((text_arg(&values[0])?, to_bool(&values[1])?)),
        _ => Err(ValueError::interp(IE_TOO_MANY_PARAMS)),
    }
}

/// `aBlockType, aInherited, aList` of the `...ByType` methods that fill a
/// list (the list is what the caller adds to).
fn by_type_args(values: &[Value]) -> Result<(String, bool, Value), ValueError> {
    match values.len() {
        0 | 1 => Err(ValueError::interp(IE_NOT_ENOUGH_PARAMS)),
        2 => Ok((text_arg(&values[0])?, false, values[1].clone())),
        3 => Ok((text_arg(&values[0])?, to_bool(&values[1])?, values[2].clone())),
        _ => Err(ValueError::interp(IE_TOO_MANY_PARAMS)),
    }
}

/// `TwbNifVersion(Byte(...))`: the ordinal as the enum; anything above the
/// last member is `nfUnknown`.
fn nif_version_of(value: i32) -> NifVersion {
    match value {
        1 => NifVersion::Tes3,
        2 => NifVersion::Tes4,
        3 => NifVersion::Fo3,
        4 => NifVersion::Tes5,
        5 => NifVersion::Sse,
        6 => NifVersion::Fo4,
        _ => NifVersion::Unknown,
    }
}

/// `{ Others }`: the eight file classes and their `Create` methods.
fn register_others(adapter: &mut Adapter, ctx: &Ctx) {
    let material = |adapter: &mut Adapter, ctx: &Ctx, class: &Rc<ClassDef>, kind: AssetKind| {
        get(adapter, class, "Create", 0, {
            let ctx = ctx.clone();
            Rc::new(move |value, _args| {
                let file = match kind {
                    AssetKind::Bgsm => MaterialFile::new_bgsm().map_err(df_error)?,
                    _ => MaterialFile::new_bgem().map_err(df_error)?,
                };
                let doc = Rc::new(RefCell::new(DfDoc {
                    tree: file.tree,
                    root: file.root,
                    kind: Some(kind),
                    released: false,
                }));
                *value = {
                    let borrowed = doc.borrow();
                    ctx.element_value(&doc, &borrowed, borrowed.root)
                };
                Ok(())
            })
        });
    };
    material(adapter, ctx, &ctx.classes.twb_bgsm_file, AssetKind::Bgsm);
    material(adapter, ctx, &ctx.classes.twb_bgem_file, AssetKind::Bgem);
    for (class, misc) in [
        (&ctx.classes.twb_lod_tree_lst_file, MiscFile::LodTreeLst),
        (&ctx.classes.twb_lod_tree_btt_file, MiscFile::LodTreeBtt),
        (&ctx.classes.twb_lod_settings_fo3_file, MiscFile::LodSettingsFo3),
        (&ctx.classes.twb_lod_settings_tes5_file, MiscFile::LodSettingsTes5),
        (&ctx.classes.twb_fuz_file, MiscFile::Fuz),
        (&ctx.classes.twb_dds_file, MiscFile::Dds),
    ] {
        let kind = asset_kind_of(misc);
        get(adapter, class, "Create", 0, {
            let ctx = ctx.clone();
            Rc::new(move |value, _args| {
                let (tree, root) = create_misc_file(misc).map_err(df_error)?;
                let doc = Rc::new(RefCell::new(DfDoc {
                    tree,
                    root,
                    kind: Some(kind),
                    released: false,
                }));
                *value = {
                    let borrowed = doc.borrow();
                    ctx.element_value(&doc, &borrowed, borrowed.root)
                };
                Ok(())
            })
        });
    }
}

/// The [`AssetKind`] of a [`MiscFile`].
fn asset_kind_of(kind: MiscFile) -> AssetKind {
    match kind {
        MiscFile::LodSettingsTes5 => AssetKind::LodSettingsTes5,
        MiscFile::LodSettingsFo3 => AssetKind::LodSettingsFo3,
        MiscFile::LodTreeLst => AssetKind::LodTreeLst,
        MiscFile::LodTreeBtt => AssetKind::LodTreeBtt,
        MiscFile::Fuz => AssetKind::Fuz,
        MiscFile::Dds => AssetKind::Dds,
    }
}

/// `LoadFromFile`'s read (the OSError of the GUI's `CreateFile` route with
/// the crate's `Cannot open file` convention).
fn read_file(path: &str) -> Result<Vec<u8>, ValueError> {
    std::fs::read(path).map_err(|error| ValueError::External(format!("Cannot open file \"{path}\". {error}")))
}

/// A write of `SaveToFile`/`SaveToJSONFile`.
fn write_file(path: &str, data: &[u8]) -> Result<(), ValueError> {
    std::fs::write(path, data).map_err(|error| ValueError::External(format!("Cannot open file \"{path}\". {error}")))
}

#[cfg(test)]
mod tests {
    use super::*;
    use xedit_script::Interpreter;

    /// A run's adapter with the DF registrations, and the state.
    fn adapter() -> (Interpreter, Rc<RefCell<ScriptRunState>>) {
        let mut interpreter = Interpreter::new();
        let element_class = interpreter.adapter.add_class("", "IwbElement", None);
        let state = Rc::new(RefCell::new(ScriptRunState::new(Vec::new(), element_class)));
        register(&mut interpreter.adapter, &state);
        (interpreter, state)
    }

    /// One dispatch: the identifier, the receiver and the arguments.
    fn call(
        interpreter: &mut Interpreter,
        identifier: &str,
        obj: Option<Value>,
        values: Vec<Value>,
        indexed: bool,
    ) -> Result<Value, ValueError> {
        let mut args = Args::new();
        args.obj = obj;
        args.values = values;
        args.indexed = indexed;
        let mut value = Value::Empty;
        if interpreter
            .adapter
            .get_value(identifier.as_bytes(), &mut value, &mut args)?
        {
            Ok(value)
        } else {
            panic!("{identifier} is not registered")
        }
    }

    /// One set dispatch.
    fn set_call(interpreter: &mut Interpreter, identifier: &str, obj: Value, assigned: Value, index: Vec<Value>) {
        let mut args = Args::new();
        args.obj = Some(obj);
        args.values = index;
        args.indexed = !args.values.is_empty();
        let handled = interpreter
            .adapter
            .set_value(identifier.as_bytes(), &assigned, &mut args)
            .unwrap_or_else(|error| panic!("{identifier}: {error:?}"));
        assert!(handled, "{identifier} is not registered");
    }

    fn class_name(value: &Value) -> String {
        value.as_object().expect("an object").borrow().class().name.clone()
    }

    fn int_of(value: &Value) -> i32 {
        xedit_script::values::to_i32(value).unwrap()
    }

    fn text_of(value: &Value) -> String {
        String::from_utf8_lossy(&xedit_script::values::to_str(value).unwrap()).into_owned()
    }

    /// The `TwbNifFile.Create` of a fresh adapter.
    fn create_nif(interpreter: &mut Interpreter) -> Value {
        let class = interpreter.adapter.find_class(b"TwbNifFile").expect("the class");
        call(interpreter, "Create", Some(Value::Class(class)), Vec::new(), false).expect("Create")
    }

    #[test]
    fn the_constants_match_the_enums() {
        let (mut interpreter, _state) = adapter();
        let mut constant = |identifier: &str| {
            let mut args = Args::new();
            let mut value = Value::Empty;
            assert!(
                interpreter
                    .adapter
                    .get_value(identifier.as_bytes(), &mut value, &mut args)
                    .unwrap(),
                "{identifier}"
            );
            int_of(&value)
        };
        assert_eq!(constant("dftNone"), 0);
        assert_eq!(constant("dftStruct"), 1);
        assert_eq!(constant("dftFloat32"), 16);
        assert_eq!(constant("nfoRemoveUnusedStrings"), 1);
        assert_eq!(constant("nfTES3"), 1);
        assert_eq!(constant("nfFO4"), 6);
    }

    #[test]
    fn the_helper_functions_answer_as_upstream() {
        let (mut interpreter, _state) = adapter();
        let mut function = |identifier: &str, args: Vec<Value>| {
            let mut call_args = Args::new();
            call_args.values = args;
            let mut value = Value::Empty;
            assert!(
                interpreter
                    .adapter
                    .get_value(identifier.as_bytes(), &mut value, &mut call_args)
                    .unwrap(),
                "{identifier}"
            );
            value
        };
        let text = text_of(&function("dfFloatToStr", vec![Value::Double(1.5)]));
        assert!(text.starts_with("1.5"), "{text}");
        assert_eq!(
            xedit_script::values::to_f64(&function("dfStrToFloat", vec![Value::Str(b"2.5".to_vec())])).unwrap(),
            2.5
        );
        // `HashName` of an ASCII name: the hash phase 5 checked against the
        // oracle for 1596 definition names. The value is the full unsigned
        // 32 bit hash (`varLongWord` upstream, kept in an `Int64` here).
        assert_eq!(
            xedit_script::values::to_i64(&function("dfCalcHash", vec![Value::Str(b"Name".to_vec())])).unwrap(),
            i64::from(df_calc_hash("Name"))
        );
    }

    #[test]
    fn a_nif_is_built_read_and_written() {
        let (mut interpreter, _state) = adapter();
        let nif = create_nif(&mut interpreter);
        assert_eq!(class_name(&nif), "TwbNifFile");

        call(&mut interpreter, "SetToDefault", Some(nif.clone()), Vec::new(), false).expect("SetToDefault");
        set_call(
            &mut interpreter,
            "NifVersion",
            nif.clone(),
            Value::Integer(5),
            Vec::new(),
        );
        assert_eq!(
            int_of(&call(&mut interpreter, "NifVersion", Some(nif.clone()), Vec::new(), false).unwrap()),
            5
        );

        let node = call(
            &mut interpreter,
            "AddBlock",
            Some(nif.clone()),
            vec![Value::Str(b"NiNode".to_vec())],
            false,
        )
        .expect("AddBlock");
        assert_eq!(class_name(&node), "TwbNifBlock");
        assert_eq!(
            text_of(&call(&mut interpreter, "BlockType", Some(node.clone()), Vec::new(), false).unwrap()),
            "NiNode"
        );

        let child = call(
            &mut interpreter,
            "AddChild",
            Some(node.clone()),
            vec![Value::Str(b"NiNode".to_vec())],
            false,
        )
        .expect("AddChild");
        assert_eq!(
            int_of(&call(&mut interpreter, "BlocksCount", Some(nif.clone()), Vec::new(), false).unwrap()),
            2
        );
        let child_index = int_of(&call(&mut interpreter, "Index", Some(child.clone()), Vec::new(), false).unwrap());
        let refs_count = int_of(&call(&mut interpreter, "RefsCount", Some(node.clone()), Vec::new(), false).unwrap());
        assert!(refs_count > 0, "the block has references");

        // One of the block's references is the child: `LinksTo` of each
        // resolves to the block it points to.
        let mut linked = false;
        for index in 0..refs_count {
            let reference = call(
                &mut interpreter,
                "Refs",
                Some(node.clone()),
                vec![Value::Integer(index)],
                true,
            )
            .expect("Refs");
            assert_eq!(class_name(&reference), "TwbNiRef");
            let links_to = call(&mut interpreter, "LinksTo", Some(reference), Vec::new(), false).unwrap();
            if links_to.is_empty() {
                continue;
            }
            assert_eq!(class_name(&links_to), "TwbNifBlock");
            assert_eq!(
                int_of(&call(&mut interpreter, "Index", Some(links_to), Vec::new(), false).unwrap()),
                child_index
            );
            linked = true;
        }
        assert!(linked, "a reference links to the child");

        // The values of an element: the indexed readers and writers.
        set_call(
            &mut interpreter,
            "EditValues",
            child.clone(),
            Value::Str(b"TestNode".to_vec()),
            vec![Value::Str(b"Name".to_vec())],
        );
        let name = call(
            &mut interpreter,
            "EditValues",
            Some(child.clone()),
            vec![Value::Str(b"Name".to_vec())],
            true,
        )
        .unwrap();
        assert_eq!(text_of(&name), "TestNode");
        // `Items[0]` and `el[i]`: the default indexed property.
        let first = call(
            &mut interpreter,
            "Items",
            Some(child.clone()),
            vec![Value::Integer(0)],
            true,
        )
        .unwrap();
        assert_eq!(
            text_of(&call(&mut interpreter, "Name", Some(first.clone()), Vec::new(), false).unwrap()),
            "Name"
        );
        let mut args = Args::new();
        args.values = vec![Value::Integer(0)];
        args.indexed = true;
        let mut via_default = Value::Empty;
        assert!(
            interpreter
                .adapter
                .get_element(&child, &mut via_default, &mut args)
                .unwrap()
        );
        assert_eq!(
            text_of(&call(&mut interpreter, "EditValue", Some(via_default), Vec::new(), false).unwrap()),
            "TestNode"
        );

        // `Def` of the block is a definition object with its own readers.
        let def = call(&mut interpreter, "Def", Some(node.clone()), Vec::new(), false).unwrap();
        assert_eq!(
            text_of(&call(&mut interpreter, "Name", Some(def.clone()), Vec::new(), false).unwrap()),
            "NiNode"
        );
        assert_eq!(class_name(&def), "TwbNifBlockDef");
        let data_type = call(&mut interpreter, "DataType", Some(node.clone()), Vec::new(), false).unwrap();
        assert_eq!(int_of(&data_type), DataType::Struct as i32);
    }

    #[test]
    fn the_element_tree_of_a_nif_is_walkable() {
        let (mut interpreter, _state) = adapter();
        let nif = create_nif(&mut interpreter);
        call(&mut interpreter, "SetToDefault", Some(nif.clone()), Vec::new(), false).unwrap();
        set_call(
            &mut interpreter,
            "NifVersion",
            nif.clone(),
            Value::Integer(5),
            Vec::new(),
        );
        call(
            &mut interpreter,
            "AddBlock",
            Some(nif.clone()),
            vec![Value::Str(b"NiNode".to_vec())],
            false,
        )
        .unwrap();

        let header = call(&mut interpreter, "Header", Some(nif.clone()), Vec::new(), false).unwrap();
        assert_eq!(class_name(&header), "TwbNifBlock");
        let block = call(
            &mut interpreter,
            "Blocks",
            Some(nif.clone()),
            vec![Value::Integer(0)],
            true,
        )
        .unwrap();
        assert_eq!(
            int_of(&call(&mut interpreter, "Index", Some(block.clone()), Vec::new(), false).unwrap()),
            0
        );

        // The document is a tree: header, block, footer, and the block's
        // elements below it. A block's name has no ` #n` suffix: its parent
        // is the file root, a structure (the ` #n` suffix is an array
        // member's, `TdfElement.GetName`).
        let name = call(&mut interpreter, "Name", Some(block.clone()), Vec::new(), false).unwrap();
        assert_eq!(text_of(&name), "0 NiNode");
        let path = call(&mut interpreter, "Path", Some(block.clone()), Vec::new(), false).unwrap();
        assert_eq!(text_of(&path), "0 NiNode");

        // A freed document refuses the calls on its elements.
        call(&mut interpreter, "Free", Some(nif.clone()), Vec::new(), false).unwrap();
        let error = call(&mut interpreter, "BlocksCount", Some(nif.clone()), Vec::new(), false).unwrap_err();
        assert_eq!(error, ValueError::External(FREED.to_owned()));
    }

    #[test]
    fn the_by_type_methods_fill_a_script_list() {
        let (mut interpreter, state) = adapter();
        // The `Classes` unit's `TList.Add`, stood in for by a sink.
        let added = Rc::new(RefCell::new(Vec::new()));
        let sink = added.clone();
        state.borrow_mut().lists.list_add = Some(Rc::new(move |_list, item| {
            sink.borrow_mut().push(item);
            Ok(())
        }));
        let nif = create_nif(&mut interpreter);
        call(&mut interpreter, "SetToDefault", Some(nif.clone()), Vec::new(), false).unwrap();
        set_call(
            &mut interpreter,
            "NifVersion",
            nif.clone(),
            Value::Integer(5),
            Vec::new(),
        );
        let node = call(
            &mut interpreter,
            "AddBlock",
            Some(nif.clone()),
            vec![Value::Str(b"NiNode".to_vec())],
            false,
        )
        .unwrap();
        call(
            &mut interpreter,
            "AddChild",
            Some(node.clone()),
            vec![Value::Str(b"NiNode".to_vec())],
            false,
        )
        .unwrap();
        call(
            &mut interpreter,
            "AddChild",
            Some(node.clone()),
            vec![Value::Str(b"NiNode".to_vec())],
            false,
        )
        .unwrap();

        // `ChildrenByType('NiNode', list)`: the inherited default is False,
        // and both children are NiNodes.
        let list = Value::Empty; // the seam does not look at it
        call(
            &mut interpreter,
            "ChildrenByType",
            Some(node.clone()),
            vec![Value::Str(b"NiNode".to_vec()), list.clone()],
            false,
        )
        .expect("ChildrenByType");
        assert_eq!(added.borrow().len(), 2);
        assert_eq!(class_name(&added.borrow()[0]), "TwbNifBlock");

        // The count cases of the adapter.
        let error = call(
            &mut interpreter,
            "ChildrenByType",
            Some(node.clone()),
            vec![Value::Str(b"NiNode".to_vec())],
            false,
        )
        .unwrap_err();
        assert_eq!(error, ValueError::interp(IE_NOT_ENOUGH_PARAMS));

        // Without the seam the call names the unit that would provide it.
        state.borrow_mut().lists.list_add = None;
        added.borrow_mut().clear();
        let error = call(
            &mut interpreter,
            "ChildrenByType",
            Some(node.clone()),
            vec![Value::Str(b"NiNode".to_vec()), Value::Empty],
            false,
        )
        .unwrap_err();
        match error {
            ValueError::External(message) => assert!(message.contains("Classes unit"), "{message}"),
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn the_file_classes_answer_their_empty_trees() {
        let (mut interpreter, _state) = adapter();
        for (class_text, probe, expected) in [
            ("TwbBGSMFile", "Textures", ""),
            ("TwbBGEMFile", "Textures", ""),
            ("TwbFUZFile", "LIP Size", "0"),
            ("TwbDDSFile", "HEADER\\dwMipMapCount", "0"),
            ("TwbLODSettingsTES5File", "Version", ""),
            ("TwbLODSettingsFO3File", "Version", ""),
        ] {
            let class = interpreter.adapter.find_class(class_text.as_bytes()).expect("class");
            let file = call(&mut interpreter, "Create", Some(Value::Class(class)), Vec::new(), false).unwrap();
            assert_eq!(class_name(&file), class_text);
            call(&mut interpreter, "SetToDefault", Some(file.clone()), Vec::new(), false).unwrap();
            let value = call(
                &mut interpreter,
                "EditValues",
                Some(file.clone()),
                vec![Value::Str(probe.as_bytes().to_vec())],
                true,
            )
            .unwrap();
            assert_eq!(text_of(&value), expected, "{class_text}.{probe}");
        }
        // The LOD tree files are arrays.
        for class_text in ["TwbLODTreeLSTFile", "TwbLODTreeBTTFile"] {
            let class = interpreter.adapter.find_class(class_text.as_bytes()).expect("class");
            let file = call(&mut interpreter, "Create", Some(Value::Class(class)), Vec::new(), false).unwrap();
            call(&mut interpreter, "SetToDefault", Some(file.clone()), Vec::new(), false).unwrap();
            assert_eq!(
                int_of(&call(&mut interpreter, "Count", Some(file.clone()), Vec::new(), false).unwrap()),
                0
            );
        }
    }
}
