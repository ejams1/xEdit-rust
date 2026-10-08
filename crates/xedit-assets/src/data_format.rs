// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

// Ported from xEdit: Core/wbDataFormat.pas

//! The data format framework: definitions (`TdfDef` and its subclasses)
//! and the element tree that a definition reads binary data into, edits
//! and writes back (`TdfElement` and its subclasses). NIF files, materials
//! and the small LOD and FUZ formats are built on it.
//!
//! Upstream elements are objects with parent pointers; here they live in
//! an arena (`Tree`) and are named by `El`. Every upstream method of
//! `TdfElement` is a method of `Tree` that takes the element. Virtual
//! methods dispatch on the element's `Class`, which is the upstream class
//! of the object. Exceptions are `DfError`s.

use std::sync::atomic::{AtomicUsize, Ordering};

use xedit_core::delphi::{
    HALF_MAX_VALUE, HALF_MIN_VALUE, HALF_NAN, HALF_POS_INF, MAX_SINGLE, float_to_half, float_to_str_f_fixed,
    half_to_float, same_value, str_to_float,
};

use xedit_io::encoding::Encoding;

use crate::json::Json;
use crate::variant::{Variant, str_to_int, str_to_int64};

/// An upstream exception.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("{0}")]
pub struct DfError(pub String);

impl DfError {
    pub fn new(message: impl Into<String>) -> Self {
        DfError(message.into())
    }
}

pub type R<T> = Result<T, DfError>;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum DataType {
    None,
    Struct,
    Array,
    Union,
    Merge,
    Bytes,
    Chars,
    U8,
    S8,
    U16,
    S16,
    U32,
    S32,
    U64,
    S64,
    Float16,
    Float32,
}

impl DataType {
    /// `DefSizes`.
    pub fn size(self) -> i32 {
        match self {
            DataType::U8 | DataType::S8 => 1,
            DataType::U16 | DataType::S16 | DataType::Float16 => 2,
            DataType::U32 | DataType::S32 | DataType::Float32 => 4,
            DataType::U64 | DataType::S64 => 8,
            _ => 0,
        }
    }
}

/// `sFlagsDelimiter`.
pub const FLAGS_DELIMITER: &str = "|";
/// `sFloatByte128`.
pub const FLOAT_BYTE_128: &str = "0.501961";

/// `dfFloatDecimalDigits`: the decimals of a float's edit value.
pub static FLOAT_DECIMAL_DIGITS: AtomicUsize = AtomicUsize::new(6);

/// The bits of `SingleNaN` (`0.0/0.0`): the default quiet NaN.
const SINGLE_NAN_BITS: u32 = 0xFFC0_0000;

/// `IsNegZeroString`: `-` followed by zeroes and points only.
fn is_neg_zero_string(value: &str) -> bool {
    value.len() >= 2 && value.starts_with('-') && value[1..].bytes().all(|byte| byte == b'.' || byte == b'0')
}

/// `dfFloatToStr`: fixed notation with `dfFloatDecimalDigits` decimals; a
/// value that is negative zero as a single prints with its sign.
pub fn df_float_to_str(value: f64) -> String {
    let digits = FLOAT_DECIMAL_DIGITS.load(Ordering::Relaxed);
    if (value as f32).to_bits() == 0x8000_0000 {
        format!("-{}", float_to_str_f_fixed(0.0, digits))
    } else {
        float_to_str_f_fixed(value, digits)
    }
}

fn not_a_float(value: &str) -> DfError {
    DfError::new(format!("'{value}' is not a valid floating point value"))
}

/// `dfStrToFloat`.
pub fn df_str_to_float(value: &str) -> R<f64> {
    Ok(if value.eq_ignore_ascii_case("NaN") {
        f64::from(f32::from_bits(SINGLE_NAN_BITS))
    } else if value.eq_ignore_ascii_case("Inf") {
        f64::INFINITY
    } else if value.eq_ignore_ascii_case("Max") {
        MAX_SINGLE
    } else if value.eq_ignore_ascii_case("Min") {
        -MAX_SINGLE
    } else if is_neg_zero_string(value) {
        -0.0
    } else if value.is_empty() {
        0.0
    } else {
        str_to_float(value).ok_or_else(|| not_a_float(value))?
    })
}

/// `dfStrToHalfFloat`.
fn df_str_to_half_float(value: &str) -> R<f64> {
    Ok(if value.eq_ignore_ascii_case("NaN") {
        f64::from(half_to_float(HALF_NAN))
    } else if value.eq_ignore_ascii_case("Inf") {
        f64::from(half_to_float(HALF_POS_INF))
    } else if value.eq_ignore_ascii_case("Max") {
        f64::from(half_to_float(HALF_MAX_VALUE))
    } else if value.eq_ignore_ascii_case("Min") {
        f64::from(half_to_float(HALF_MIN_VALUE))
    } else if is_neg_zero_string(value) {
        -0.0
    } else if value.is_empty() {
        0.0
    } else {
        str_to_float(value).ok_or_else(|| not_a_float(value))?
    })
}

/// `dfCalcHash`: Delphi's `HashName` of the name as an ANSI string, a
/// case-insensitive hash: each character in lower case is added with an
/// exclusive or and the sum rotated left by five bits. Upstream finds
/// elements by name by comparing these hashes only. (The form was found
/// from the oracle's `dfCalcHash`, `cargo xtask parity nif --text`.)
pub fn df_calc_hash(name: &str) -> u32 {
    let hash = |bytes: &[u8]| {
        bytes.iter().fold(0u32, |result, byte| {
            (result ^ u32::from(byte.to_ascii_lowercase())).rotate_left(5)
        })
    };
    if name.is_ascii() {
        hash(name.as_bytes())
    } else {
        hash(&Encoding::Mbcs(ANSI_CODE_PAGE).get_bytes(name))
    }
}

/// The ANSI code page of the strings that upstream converts with
/// `AnsiString(s)`: the system's (`CP_ACP`).
pub const ANSI_CODE_PAGE: u32 = 0;

/// `IntToStr` of an `Integer` that the Pascal code shifts: `1 shl n` with
/// a 32 bit `1`, whose shift count wraps at 32 and whose bit 31 is the sign.
pub fn shl32(count: i64) -> i64 {
    i64::from(1i32.wrapping_shl(count as u32))
}

pub type OnCreate = fn(&mut Tree, El) -> R<()>;
pub type OnDestroy = fn(&mut Tree, El) -> R<()>;
pub type OnEnabled = fn(&mut Tree, El) -> R<bool>;
pub type OnDecide = fn(&mut Tree, El) -> R<i32>;
/// The data start of upstream is only ever tested for `nil`: `has_data`.
pub type OnAfterLoad = fn(&mut Tree, El, bool, i32) -> R<()>;
pub type OnBeforeSave = fn(&mut Tree, El) -> R<()>;
pub type OnValue = fn(&mut Tree, El, &mut Variant) -> R<()>;
pub type OnText = fn(&mut Tree, El, &mut String) -> R<()>;
pub type OnCount = fn(&mut Tree, El, &mut i32) -> R<()>;
pub type OnLinksTo = fn(&mut Tree, El) -> R<Option<El>>;

/// One entry of an events array (`[DF_OnGetEnabled, @Proc, ...]`).
#[derive(Clone, Copy)]
pub enum Event {
    Create(OnCreate),
    Destroy(OnDestroy),
    GetEnabled(OnEnabled),
    Decide(OnDecide),
    AfterLoad(OnAfterLoad),
    BeforeSave(OnBeforeSave),
    GetValue(OnValue),
    SetValue(OnValue),
    GetText(OnText),
    SetText(OnText),
    GetCount(OnCount),
    SetCount(OnCount),
    LinksTo(OnLinksTo),
}

#[derive(Clone, Copy, Default)]
pub struct Events {
    pub on_create: Option<OnCreate>,
    pub on_destroy: Option<OnDestroy>,
    pub on_get_enabled: Option<OnEnabled>,
    pub on_decide: Option<OnDecide>,
    pub on_after_load: Option<OnAfterLoad>,
    pub on_before_save: Option<OnBeforeSave>,
    pub on_get_value: Option<OnValue>,
    pub on_set_value: Option<OnValue>,
    pub on_get_text: Option<OnText>,
    pub on_set_text: Option<OnText>,
    pub on_get_count: Option<OnCount>,
    pub on_set_count: Option<OnCount>,
    pub on_links_to: Option<OnLinksTo>,
}

/// The subclass of `TdfDef`, with the fields only it has.
#[derive(Clone)]
pub enum DefKind {
    /// `TdfDef` itself.
    Plain,
    Struct,
    /// `TwbNifBlockDef`.
    NifBlock,
    Array {
        counter: String,
    },
    Union,
    ValueUnion,
    Merge {
        /// Offset and size of each value.
        offsets: Vec<(i32, i32)>,
        delimiter: String,
    },
    Integer,
    MappedInteger {
        map: Vec<(i64, String)>,
    },
    Flags {
        map: Vec<(i64, String)>,
    },
    Enum {
        map: Vec<(i64, String)>,
    },
    Float,
    Bytes,
    Chars {
        terminator: u8,
        terminated: bool,
    },
    /// `TwbNiRefDef`.
    NiRef {
        template: String,
        ptr: bool,
    },
}

/// `TdfDef`.
#[derive(Clone)]
pub struct Def {
    pub name: String,
    pub name_hash: u32,
    pub data_type: DataType,
    pub defs: Vec<Def>,
    pub default_value: String,
    pub size: i32,
    pub events: Events,
    pub kind: DefKind,
}

impl Def {
    /// `TdfDef.Create` of the class `kind`.
    pub fn new(kind: DefKind, name: &str, data_type: DataType, defs: Vec<Def>) -> Def {
        let mut def = Def {
            name: name.to_owned(),
            name_hash: df_calc_hash(name),
            data_type,
            defs,
            default_value: String::new(),
            size: 0,
            events: Events::default(),
            kind,
        };
        if let DefKind::Merge { .. } = def.kind {
            def.kind = DefKind::Merge {
                offsets: Vec::new(),
                delimiter: " ".to_owned(),
            };
            def.update_offsets();
        }
        def
    }

    /// `AssignEvents`.
    pub fn assign_events(&mut self, events: &[Event]) {
        for event in events {
            let ev = &mut self.events;
            match *event {
                Event::Create(f) => ev.on_create = Some(f),
                Event::Destroy(f) => ev.on_destroy = Some(f),
                Event::GetEnabled(f) => ev.on_get_enabled = Some(f),
                Event::Decide(f) => ev.on_decide = Some(f),
                Event::AfterLoad(f) => ev.on_after_load = Some(f),
                Event::BeforeSave(f) => ev.on_before_save = Some(f),
                Event::GetValue(f) => ev.on_get_value = Some(f),
                Event::SetValue(f) => ev.on_set_value = Some(f),
                Event::GetText(f) => ev.on_get_text = Some(f),
                Event::SetText(f) => ev.on_set_text = Some(f),
                Event::GetCount(f) => ev.on_get_count = Some(f),
                Event::SetCount(f) => ev.on_set_count = Some(f),
                Event::LinksTo(f) => ev.on_links_to = Some(f),
            }
        }
    }

    /// Whether the definition is a `TdfValueDef`.
    pub fn is_value_def(&self) -> bool {
        !matches!(
            self.kind,
            DefKind::Plain | DefKind::Struct | DefKind::NifBlock | DefKind::Array { .. }
        )
    }

    /// `DefaultDataSize`.
    pub fn default_data_size(&self) -> i32 {
        match &self.kind {
            DefKind::Bytes | DefKind::Chars { .. } if self.size > 0 => self.size,
            DefKind::Merge { offsets, .. } => offsets.iter().map(|&(_, size)| size).sum(),
            _ => {
                if self.size >= 0 {
                    self.data_type.size()
                } else {
                    -self.size
                }
            }
        }
    }

    /// `TdfMergeDef.UpdateOffsets`.
    fn update_offsets(&mut self) {
        let mut offset = 0;
        let mut table = Vec::with_capacity(self.defs.len());
        for def in &self.defs {
            let size = def.default_data_size();
            debug_assert!(size != 0, "merge member without size");
            table.push((offset, size));
            offset += size;
        }
        if let DefKind::Merge { offsets, .. } = &mut self.kind {
            *offsets = table;
        }
    }

    /// `AddDef`.
    pub fn add_def(&mut self, def: Def) {
        self.defs.push(def);
        self.update_offsets_if_merge();
    }

    fn update_offsets_if_merge(&mut self) {
        if matches!(self.kind, DefKind::Merge { .. }) {
            self.update_offsets();
        }
    }

    /// `InsertDefsFrom`: copies of the members of `def` inserted at `index`.
    pub fn insert_defs_from(&mut self, def: &Def, index: usize) -> R<()> {
        if def.defs.is_empty() {
            return Ok(());
        }
        let index = if self.defs.is_empty() {
            0
        } else if index >= self.defs.len() {
            return Err(DfError::new("Invalid index to insert defs"));
        } else {
            index
        };
        let tail = self.defs.split_off(index);
        self.defs.extend(def.defs.iter().cloned());
        self.defs.extend(tail);
        Ok(())
    }

    /// The value map of a mapped integer, flags or enumeration.
    pub fn values_map(&self) -> &[(i64, String)] {
        match &self.kind {
            DefKind::MappedInteger { map } | DefKind::Flags { map } | DefKind::Enum { map } => map,
            _ => &[],
        }
    }

    /// `IndexOfKey`.
    pub fn index_of_key(&self, key: i64) -> Option<usize> {
        self.values_map().iter().position(|(k, _)| *k == key)
    }

    /// `IndexOfValue`: case-insensitive.
    pub fn index_of_value(&self, value: &str) -> Option<usize> {
        self.values_map().iter().position(|(_, v)| same_text(v, value))
    }

    /// `AssignValuesMap`; for flags the bits without a name become `Bit N`.
    pub fn assign_values_map(&mut self, values: &[(i64, &str)]) {
        let mut map: Vec<(i64, String)> = values.iter().map(|&(key, value)| (key, value.to_owned())).collect();
        if let DefKind::Flags { .. } = self.kind {
            for bit in 0..i64::from(self.default_data_size() * 8) {
                if !map.iter().any(|(key, _)| *key == bit) {
                    map.push((bit, format!("Bit {bit}")));
                }
            }
        }
        match &mut self.kind {
            DefKind::MappedInteger { map: target } | DefKind::Flags { map: target } | DefKind::Enum { map: target } => {
                *target = map;
            }
            _ => {}
        }
    }

    pub fn counter(&self) -> &str {
        match &self.kind {
            DefKind::Array { counter } => counter,
            _ => "",
        }
    }

    pub fn delimiter(&self) -> &str {
        match &self.kind {
            DefKind::Merge { delimiter, .. } => delimiter,
            _ => "",
        }
    }

    pub fn set_delimiter(&mut self, value: &str) {
        if let DefKind::Merge { delimiter, .. } = &mut self.kind {
            *delimiter = value.to_owned();
        }
    }

    fn merge_offset(&self, index: usize) -> (usize, usize) {
        match &self.kind {
            DefKind::Merge { offsets, .. } => {
                let (offset, size) = offsets[index];
                (offset as usize, size as usize)
            }
            _ => (0, 0),
        }
    }

    pub fn set_default_value(mut self, value: &str) -> Def {
        self.default_value = value.to_owned();
        self
    }

    pub fn set_on_create(mut self, f: OnCreate) -> Def {
        self.events.on_create = Some(f);
        self
    }

    pub fn set_on_destroy(mut self, f: OnDestroy) -> Def {
        self.events.on_destroy = Some(f);
        self
    }

    pub fn set_on_enabled(mut self, f: OnEnabled) -> Def {
        self.events.on_get_enabled = Some(f);
        self
    }

    pub fn set_on_decide(mut self, f: OnDecide) -> Def {
        self.events.on_decide = Some(f);
        self
    }

    pub fn set_on_after_load(mut self, f: OnAfterLoad) -> Def {
        self.events.on_after_load = Some(f);
        self
    }

    pub fn set_on_before_save(mut self, f: OnBeforeSave) -> Def {
        self.events.on_before_save = Some(f);
        self
    }

    pub fn set_on_get_value(mut self, f: OnValue) -> Def {
        self.events.on_get_value = Some(f);
        self
    }

    pub fn set_on_set_value(mut self, f: OnValue) -> Def {
        self.events.on_set_value = Some(f);
        self
    }

    pub fn set_on_get_text(mut self, f: OnText) -> Def {
        self.events.on_get_text = Some(f);
        self
    }

    pub fn set_on_set_text(mut self, f: OnText) -> Def {
        self.events.on_set_text = Some(f);
        self
    }

    pub fn set_on_get_count(mut self, f: OnCount) -> Def {
        self.events.on_get_count = Some(f);
        self
    }

    pub fn set_on_set_count(mut self, f: OnCount) -> Def {
        self.events.on_set_count = Some(f);
        self
    }

    pub fn set_on_links_to(mut self, f: OnLinksTo) -> Def {
        self.events.on_links_to = Some(f);
        self
    }

    /// `TwbNiRefDef.Template`.
    pub fn template(&self) -> &str {
        match &self.kind {
            DefKind::NiRef { template, .. } => template,
            _ => "",
        }
    }

    /// `TwbNiRefDef.Ptr`.
    pub fn ptr(&self) -> bool {
        matches!(self.kind, DefKind::NiRef { ptr: true, .. })
    }

    pub fn set_ptr(&mut self, value: bool) {
        if let DefKind::NiRef { ptr, .. } = &mut self.kind {
            *ptr = value;
        }
    }

    pub fn set_template(&mut self, value: &str) {
        if let DefKind::NiRef { template, .. } = &mut self.kind {
            *template = value.to_owned();
        }
    }

    fn chars(&self) -> (u8, bool) {
        match self.kind {
            DefKind::Chars { terminator, terminated } => (terminator, terminated),
            _ => (0, false),
        }
    }
}

/// An element that upstream uses without checking it for `nil`.
pub fn req(el: Option<El>) -> R<El> {
    el.ok_or_else(|| DfError::new("Access violation: the element does not exist"))
}

/// `SameText` for the ASCII names of the definitions.
pub fn same_text(a: &str, b: &str) -> bool {
    a.eq_ignore_ascii_case(b)
}

/// An element of a `Tree`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct El(pub u32);

/// The upstream class of an element object.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Class {
    /// `TdfElement`.
    Element,
    /// A `TdfValue` subclass.
    Value(ValueClass),
    /// A `TdfStruct` subclass.
    Struct(StructClass),
    /// A `TdfArray` subclass.
    Array,
    Union,
    /// `TwbNifFile`, a `TdfContainer`.
    NifFile,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ValueClass {
    /// `TdfValue`, `TdfInteger`, `TdfEnum`, `TdfFloat`, `TwbNiRef`.
    Plain,
    Merge,
    Flags,
    Bytes,
    Chars,
    ValueUnion,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StructClass {
    Plain,
    NifBlock,
    /// `TwbBGSMFile`.
    Bgsm,
    /// `TwbBGEMFile`.
    Bgem,
    /// `TwbFUZFile`.
    Fuz,
}

impl Class {
    pub fn is_container(self) -> bool {
        matches!(self, Class::Struct(_) | Class::Array | Class::Union | Class::NifFile)
    }

    pub fn is_value(self) -> bool {
        matches!(self, Class::Value(_))
    }

    pub fn is_struct(self) -> bool {
        matches!(self, Class::Struct(_))
    }

    pub fn is_nif_block(self) -> bool {
        matches!(self, Class::Struct(StructClass::NifBlock))
    }
}

const DS_INITIALIZED: u8 = 1;
const DS_ENABLED: u8 = 2;
const DS_UPDATING: u8 = 4;
const DS_DECIDING: u8 = 8;
const DS_DESTROYING: u8 = 16;
const DS_FREE: u8 = 128;

/// The references and indexed strings of a NIF block (`FRefs`, `FStrings`).
#[derive(Debug, Default, Clone)]
pub struct BlockLists {
    pub refs: Vec<El>,
    pub strings: Vec<El>,
}

struct Node {
    def: &'static Def,
    parent: Option<El>,
    state: u8,
    class: Class,
    user_data: i32,
    /// The data of a value.
    bytes: Vec<u8>,
    /// The elements of a container.
    items: Vec<El>,
    block: Option<Box<BlockLists>>,
}

/// The elements of one document: a NIF file, a material or another file.
pub struct Tree {
    nodes: Vec<Node>,
    free: Vec<u32>,
    root: Option<El>,
    /// The fields of a `TwbNifFile` root.
    pub nif: crate::data_format_nif::NifState,
}

impl Default for Tree {
    fn default() -> Self {
        Self::new()
    }
}

/// The class of the element a definition creates (`CreateElement`).
fn class_of(def: &Def) -> Class {
    match def.kind {
        DefKind::Plain => Class::Element,
        DefKind::Struct => Class::Struct(StructClass::Plain),
        DefKind::NifBlock => Class::Struct(StructClass::NifBlock),
        DefKind::Array { .. } => Class::Array,
        DefKind::Union => Class::Union,
        DefKind::ValueUnion => Class::Value(ValueClass::ValueUnion),
        DefKind::Merge { .. } => Class::Value(ValueClass::Merge),
        DefKind::Integer | DefKind::MappedInteger { .. } | DefKind::Enum { .. } | DefKind::NiRef { .. } => {
            Class::Value(ValueClass::Plain)
        }
        DefKind::Float => Class::Value(ValueClass::Plain),
        DefKind::Flags { .. } => Class::Value(ValueClass::Flags),
        DefKind::Bytes => Class::Value(ValueClass::Bytes),
        DefKind::Chars { .. } => Class::Value(ValueClass::Chars),
    }
}

fn split_last(path: &str) -> Option<(&str, &str)> {
    path.rfind('\\').map(|index| (&path[..index], &path[index + 1..]))
}

/// `(Length(aName) > 2) and (aName[1] = '[') and (aName[Length(aName)] = ']')`.
fn bracket_index(name: &str) -> Option<&str> {
    (name.len() > 2 && name.starts_with('[') && name.ends_with(']')).then(|| &name[1..name.len() - 1])
}

/// `StrToIntDef`.
fn str_to_int_def(text: &str, default: i32) -> i32 {
    str_to_int(text).unwrap_or(default)
}

/// `SplitString`: splits at every character of `delimiters`; an empty
/// text gives no parts.
pub fn split_string(text: &str, delimiters: &str) -> Vec<String> {
    if text.is_empty() {
        return Vec::new();
    }
    text.split(|ch: char| delimiters.contains(ch))
        .map(str::to_owned)
        .collect()
}

fn read_int(bytes: &[u8], size: usize) -> u64 {
    let mut buffer = [0u8; 8];
    let take = size.min(bytes.len());
    buffer[..take].copy_from_slice(&bytes[..take]);
    u64::from_le_bytes(buffer)
}

fn invalid_integer(value: &str) -> DfError {
    DfError::new(format!("'{value}' is not a valid integer value"))
}

impl Tree {
    pub fn new() -> Tree {
        Tree {
            nodes: Vec::new(),
            free: Vec::new(),
            root: None,
            nif: Default::default(),
        }
    }

    /// The root element.
    pub fn root_el(&self) -> El {
        self.root.expect("tree without root")
    }

    /// Creates the root element: `Create(aDef, nil)` of `class`.
    pub fn create_root(&mut self, def: &'static Def, class: Class) -> R<El> {
        let el = self.alloc(def, None, class);
        self.root = Some(el);
        if let Some(on_create) = def.events.on_create {
            on_create(self, el)?;
        }
        Ok(el)
    }

    fn alloc(&mut self, def: &'static Def, parent: Option<El>, class: Class) -> El {
        let node = Node {
            def,
            parent,
            state: DS_ENABLED,
            class,
            user_data: 0,
            bytes: Vec::new(),
            items: Vec::new(),
            block: class.is_nif_block().then(Box::default),
        };
        match self.free.pop() {
            Some(index) => {
                self.nodes[index as usize] = node;
                El(index)
            }
            None => {
                self.nodes.push(node);
                El((self.nodes.len() - 1) as u32)
            }
        }
    }

    fn n(&self, el: El) -> &Node {
        &self.nodes[el.0 as usize]
    }

    fn nm(&mut self, el: El) -> &mut Node {
        &mut self.nodes[el.0 as usize]
    }

    /// `TdfDef.CreateElement`: a new element of `def` under `parent`. The
    /// element is not put into the parent's list.
    pub fn create_element(&mut self, def: &'static Def, parent: Option<El>) -> R<El> {
        let el = self.alloc(def, parent, class_of(def));
        if let Some(on_create) = def.events.on_create {
            on_create(self, el)?;
        }
        Ok(el)
    }

    /// `Free`: runs the destructor chain and releases the element and its
    /// elements.
    pub fn free_element(&mut self, el: El) -> R<()> {
        if self.n(el).state & DS_FREE != 0 {
            return Ok(());
        }
        self.nm(el).state |= DS_DESTROYING;
        if let Some(on_destroy) = self.n(el).def.events.on_destroy {
            on_destroy(self, el)?;
        }
        if self.n(el).class.is_container() {
            self.clear(el)?;
        }
        let node = self.nm(el);
        node.state = DS_FREE;
        node.bytes = Vec::new();
        node.items = Vec::new();
        node.block = None;
        node.parent = None;
        self.free.push(el.0);
        Ok(())
    }

    /// `TdfContainer.Clear`.
    pub fn clear(&mut self, el: El) -> R<()> {
        let items = std::mem::take(&mut self.nm(el).items);
        for item in items {
            self.free_element(item)?;
        }
        Ok(())
    }

    /// The class of the element.
    pub fn class(&self, el: El) -> Class {
        self.n(el).class
    }

    /// `FDef`: the definition the element was created from.
    pub fn raw_def(&self, el: El) -> &'static Def {
        self.n(el).def
    }

    /// `Def`: for a union the definition of its active member.
    pub fn def(&mut self, el: El) -> R<&'static Def> {
        let node = self.n(el);
        match node.class {
            Class::Union | Class::Value(ValueClass::ValueUnion) => {
                let def = node.def;
                let index = self.active_index(el)?;
                def.defs
                    .get(index)
                    .ok_or_else(|| DfError::new(format!("Union without member {index}")))
            }
            _ => Ok(node.def),
        }
    }

    pub fn parent(&self, el: El) -> Option<El> {
        self.n(el).parent
    }

    pub fn set_parent(&mut self, el: El, parent: Option<El>) {
        self.nm(el).parent = parent;
    }

    pub fn user_data(&self, el: El) -> i32 {
        self.n(el).user_data
    }

    pub fn set_user_data(&mut self, el: El, value: i32) {
        self.nm(el).user_data = value;
    }

    pub fn block_lists(&self, el: El) -> Option<&BlockLists> {
        self.n(el).block.as_deref()
    }

    pub fn block_lists_mut(&mut self, el: El) -> Option<&mut BlockLists> {
        self.nm(el).block.as_deref_mut()
    }

    pub fn is_destroying(&self, el: El) -> bool {
        self.n(el).state & DS_DESTROYING != 0
    }

    /// The elements of a container as stored (`FElements`), whatever its
    /// `Count` reports.
    pub fn raw_items(&self, el: El) -> &[El] {
        &self.n(el).items
    }

    /// The bytes of a value (`FDataStart` to `FDataEnd`).
    pub fn value_bytes(&self, el: El) -> &[u8] {
        &self.n(el).bytes
    }

    /// `AllocateValue`: resizes the value's data. Upstream reallocates and
    /// keeps the bytes that fit; new bytes are zero here.
    pub fn allocate_value(&mut self, el: El, length: usize) {
        self.nm(el).bytes.resize(length, 0);
    }

    // ---- TdfElement ----

    /// `DoException`.
    pub fn exception(&mut self, el: El, message: &str) -> DfError {
        let path = self.path(el).unwrap_or_default();
        DfError::new(format!("Error in \"{path}\": {message}"))
    }

    /// `ValidateData`.
    fn validate_data(&mut self, el: El, data: Option<&[u8]>, size: i32) -> R<()> {
        if let Some(data) = data
            && size as i64 > data.len() as i64
        {
            let message = format!("Unexpected end of stream: need {size} bytes, available {}", data.len());
            return Err(self.exception(el, &message));
        }
        Ok(())
    }

    pub fn initialized(&self, el: El) -> bool {
        self.n(el).state & DS_INITIALIZED != 0
    }

    /// `Updating`. A merge never reports updating.
    pub fn updating(&self, el: El) -> bool {
        match self.n(el).class {
            Class::Value(ValueClass::Merge) => false,
            _ => self.n(el).state & DS_UPDATING != 0,
        }
    }

    pub fn begin_update(&mut self, el: El) {
        self.nm(el).state |= DS_UPDATING;
    }

    pub fn end_update(&mut self, el: El) {
        self.nm(el).state &= !DS_UPDATING;
    }

    /// `Enabled`: the definition's own `OnGetEnabled`, not a union member's.
    pub fn enabled(&mut self, el: El) -> R<bool> {
        match self.n(el).def.events.on_get_enabled {
            Some(on_enabled) => on_enabled(self, el),
            None => Ok(self.n(el).state & DS_ENABLED != 0),
        }
    }

    /// `SetEnabled`. UPSTREAM-QUIRK: disables the element whatever the
    /// value.
    pub fn set_enabled(&mut self, el: El, _enabled: bool) {
        self.nm(el).state &= !DS_ENABLED;
    }

    /// `ActiveIndex` of a union.
    pub fn active_index(&mut self, el: El) -> R<usize> {
        let node = self.n(el);
        let def = node.def;
        match def.events.on_decide {
            Some(on_decide) if node.state & DS_DECIDING == 0 => {
                self.nm(el).state |= DS_DECIDING;
                let result = on_decide(self, el);
                self.nm(el).state &= !DS_DECIDING;
                let result = result?;
                if result < 0 || result as usize >= def.defs.len() {
                    Ok(0)
                } else {
                    Ok(result as usize)
                }
            }
            _ => Ok(0),
        }
    }

    /// `DataType`.
    pub fn data_type(&mut self, el: El) -> R<DataType> {
        Ok(self.def(el)?.data_type)
    }

    fn parent_is_array(&mut self, el: El) -> R<bool> {
        match self.parent(el) {
            Some(parent) => Ok(self.data_type(parent)? == DataType::Array),
            None => Ok(false),
        }
    }

    /// `Name`.
    pub fn name(&mut self, el: El) -> R<String> {
        if self.class(el).is_nif_block() {
            let block_type = self.raw_def(el).name.clone();
            if block_type != "NiHeader" && block_type != "NiFooter" {
                return Ok(format!("{} {block_type}", self.index(el)?));
            }
            return Ok(block_type);
        }
        let mut name = self.def(el)?.name.clone();
        if self.parent_is_array(el)? {
            let parent = self.parent(el).unwrap_or(el);
            name = format!("{name} #{}", self.index_of(parent, el));
        }
        Ok(name)
    }

    /// `NameHash`.
    pub fn name_hash(&mut self, el: El) -> R<u32> {
        if self.parent_is_array(el)? {
            let name = self.name(el)?;
            Ok(df_calc_hash(&name))
        } else {
            Ok(self.def(el)?.name_hash)
        }
    }

    /// `Path`: the names from below the root down to the element.
    pub fn path(&mut self, el: El) -> R<String> {
        let mut result = self.name(el)?;
        let mut element = self.parent(el);
        while let Some(current) = element {
            let Some(parent) = self.parent(current) else { break };
            result = format!("{}\\{result}", self.name(current)?);
            element = Some(parent);
        }
        Ok(result)
    }

    /// `Root`.
    pub fn root(&self, el: El) -> El {
        let mut result = el;
        while let Some(parent) = self.parent(result) {
            result = parent;
        }
        result
    }

    /// `Index`.
    pub fn index(&mut self, el: El) -> R<i32> {
        let mut result = match self.parent(el) {
            Some(parent) => self.index_of(parent, el),
            None => -1,
        };
        if self.class(el).is_nif_block() && self.raw_def(el).name != "NiHeader" {
            result -= 1;
        }
        Ok(result)
    }

    /// `IndexOf`.
    pub fn index_of(&self, el: El, element: El) -> i32 {
        if self.class(el).is_container() {
            self.n(el)
                .items
                .iter()
                .position(|&item| item == element)
                .map_or(-1, |index| index as i32)
        } else {
            -1
        }
    }

    /// `Count`.
    pub fn count(&self, el: El) -> i32 {
        match self.class(el) {
            Class::Union => 0,
            class if class.is_container() => self.n(el).items.len() as i32,
            _ => 0,
        }
    }

    /// `Count := NewCount`.
    pub fn set_count(&mut self, el: El, new_count: i32) -> R<()> {
        match self.class(el) {
            Class::Struct(_) => Err(self.exception(el, "Can not change the size of structure")),
            Class::Union => Ok(()),
            Class::Array => {
                if self.raw_def(el).size > 0 {
                    return Ok(());
                }
                self.container_set_count(el, new_count)
            }
            Class::NifFile => self.container_set_count(el, new_count),
            _ => Ok(()),
        }
    }

    /// `TdfContainer.SetCount`.
    fn container_set_count(&mut self, el: El, new_count: i32) -> R<()> {
        self.begin_update(el);
        let result = (|| {
            let new_count = new_count.max(0) as usize;
            let count = self.n(el).items.len();
            if count > new_count {
                let removed: Vec<El> = self.nm(el).items.drain(new_count..).collect();
                for item in removed.into_iter().rev() {
                    self.free_element(item)?;
                }
            }
            for _ in count..new_count {
                self.add(el)?;
            }
            Ok(())
        })();
        self.end_update(el);
        result
    }

    /// `Items[Index]`.
    pub fn item(&mut self, el: El, index: i32) -> R<El> {
        if !self.class(el).is_container() {
            return Err(self.exception(el, &format!("List index ({index}) is out of bounds")));
        }
        match usize::try_from(index)
            .ok()
            .and_then(|index| self.n(el).items.get(index))
        {
            Some(&item) => Ok(item),
            None => Err(self.exception(el, &format!("List index ({index}) is out of bounds"))),
        }
    }

    /// `Put`: appends when `index` is past the end, else replaces.
    pub fn put(&mut self, el: El, index: usize, element: El) {
        if !self.class(el).is_container() {
            return;
        }
        let items = &mut self.nm(el).items;
        if index >= items.len() {
            items.push(element);
        } else {
            items[index] = element;
        }
    }

    /// `ElementByName`.
    pub fn element_by_name(&mut self, el: El, name: &str, enabled_only: bool) -> R<Option<El>> {
        let result = if name == ".." {
            self.parent(el)
        } else if name == "." {
            Some(self.root(el))
        } else if let Some(index) = bracket_index(name) {
            let index = str_to_int_def(index, -1);
            if index >= 0 && index < self.count(el) {
                Some(self.item(el, index)?)
            } else {
                None
            }
        } else {
            None
        };
        if result.is_some() || name.is_empty() || !self.class(el).is_container() {
            return Ok(result);
        }
        let hash = df_calc_hash(name);
        let count = self.count(el) as usize;
        for index in 0..count {
            let Some(&element) = self.n(el).items.get(index) else {
                break;
            };
            if self.initialized(element)
                && self.name_hash(element)? == hash
                && (!enabled_only || self.enabled(element)?)
            {
                return Ok(Some(element));
            }
        }
        Ok(None)
    }

    /// `ElementByPath`.
    pub fn element_by_path(&mut self, el: El, path: &str, enabled_only: bool) -> R<Option<El>> {
        let mut result = match path.find('\\') {
            Some(index) => match self.element_by_name(el, &path[..index], enabled_only)? {
                Some(element) => self.element_by_path(element, &path[index + 1..], enabled_only)?,
                None => None,
            },
            None => self.element_by_name(el, path, enabled_only)?,
        };
        // Try to traverse into the linked element.
        if result.is_none()
            && self.def(el)?.events.on_links_to.is_some()
            && let Some(element) = self.links_to(el)?
        {
            result = self.element_by_path(element, path, enabled_only)?;
        }
        Ok(result)
    }

    /// `Elements[aPath]`: `ElementByPath` of enabled elements.
    pub fn elements(&mut self, el: El, path: &str) -> R<Option<El>> {
        self.element_by_path(el, path, true)
    }

    /// `LinksTo`.
    pub fn links_to(&mut self, el: El) -> R<Option<El>> {
        match self.def(el)?.events.on_links_to {
            Some(links_to) => links_to(self, el),
            None => Ok(None),
        }
    }

    // ---- values ----

    /// `NativeValue`.
    pub fn native_value(&mut self, el: El) -> R<Variant> {
        match self.class(el) {
            Class::Value(_) => {
                let def = self.def(el)?;
                let len = self.n(el).bytes.len();
                let mut value = Variant::Empty;
                self.def_get_native(def, el, 0, len, &mut value)?;
                Ok(value)
            }
            Class::Union => {
                let index = self.active_index(el)?;
                let item = self.item(el, index as i32)?;
                self.native_value(item)
            }
            _ => Ok(Variant::Empty),
        }
    }

    /// `NativeValue := aValue`.
    pub fn set_native_value(&mut self, el: El, value: Variant) -> R<()> {
        match self.class(el) {
            Class::Value(_) => {
                let def = self.def(el)?;
                let len = self.n(el).bytes.len();
                let mut value = value;
                self.def_set_native(def, el, 0, len, &mut value)
            }
            Class::Union => {
                let index = self.active_index(el)?;
                let item = self.item(el, index as i32)?;
                self.set_native_value(item, value)
            }
            _ => Ok(()),
        }
    }

    /// `EditValue`.
    pub fn edit_value(&mut self, el: El) -> R<String> {
        match self.class(el) {
            Class::Value(_) => {
                let def = self.def(el)?;
                let len = self.n(el).bytes.len();
                let mut value = String::new();
                self.def_get_edit(def, el, 0, len, &mut value)?;
                Ok(value)
            }
            Class::Union => {
                let index = self.active_index(el)?;
                let item = self.item(el, index as i32)?;
                self.edit_value(item)
            }
            _ => Ok(String::new()),
        }
    }

    /// `EditValue := aValue`.
    pub fn set_edit_value(&mut self, el: El, value: &str) -> R<()> {
        match self.class(el) {
            Class::Value(_) => {
                let def = self.def(el)?;
                let len = self.n(el).bytes.len();
                let mut value = value.to_owned();
                self.def_set_edit(def, el, 0, len, &mut value)
            }
            Class::Union => {
                let index = self.active_index(el)?;
                let item = self.item(el, index as i32)?;
                self.set_edit_value(item, value)
            }
            _ => Ok(()),
        }
    }

    /// The flag, merge member or union redirection of the `*Values[aPath]`
    /// accessors, if the element class has one for `path`.
    fn virtual_target(&mut self, el: El, path: &str) -> R<VirtualTarget> {
        match self.class(el) {
            Class::Union => {
                if self.n(el).items.is_empty() {
                    return Ok(VirtualTarget::None);
                }
                let index = self.active_index(el)?;
                let item = self.item(el, index as i32)?;
                Ok(VirtualTarget::Redirect(item))
            }
            Class::Value(ValueClass::Flags) => {
                if path.contains('\\') {
                    return Ok(VirtualTarget::None);
                }
                let def = self.def(el)?;
                Ok(match def.index_of_value(path) {
                    Some(index) => VirtualTarget::Flag(def.values_map()[index].0),
                    None => VirtualTarget::FlagPath,
                })
            }
            Class::Value(ValueClass::Merge) => {
                if path.contains('\\') {
                    return Ok(VirtualTarget::None);
                }
                let def = self.raw_def(el);
                if let Some(index) = bracket_index(path) {
                    let index = str_to_int_def(index, 0);
                    return Ok(VirtualTarget::MergeIndex(index.max(0) as usize));
                }
                Ok(VirtualTarget::MergeName(
                    def.defs.iter().position(|member| same_text(&member.name, path)),
                ))
            }
            _ => Ok(VirtualTarget::None),
        }
    }

    /// `NativeValues[aPath]`.
    pub fn native_values(&mut self, el: El, path: &str) -> R<Variant> {
        match self.virtual_target(el, path)? {
            VirtualTarget::Redirect(item) => return self.native_values(item, path),
            VirtualTarget::Flag(key) => {
                let value = self.native_value(el)?.to_i64()?;
                return Ok(Variant::Bool(value & shl32(key) != 0));
            }
            VirtualTarget::MergeIndex(index) => return self.merge_get_native(el, index),
            VirtualTarget::MergeName(Some(index)) => return self.merge_get_native(el, index),
            VirtualTarget::MergeName(None) => return Ok(Variant::Empty),
            VirtualTarget::None | VirtualTarget::FlagPath => {}
        }
        let (element, last) = match split_last(path) {
            Some((prefix, last)) => (self.element_by_path(el, prefix, true)?, Some(last)),
            None => (self.element_by_path(el, path, true)?, None),
        };
        match (element, last) {
            (Some(element), None) => self.native_value(element),
            (Some(element), Some(last)) => self.native_values(element, last),
            (None, _) => Ok(Variant::Empty),
        }
    }

    /// `NativeValues[aPath] := aValue`.
    pub fn set_native_values(&mut self, el: El, path: &str, value: Variant) -> R<()> {
        match self.virtual_target(el, path)? {
            VirtualTarget::Redirect(item) => return self.set_native_values(item, path, value),
            VirtualTarget::Flag(key) => {
                let current = self.native_value(el)?.to_i64()?;
                let new = if value.to_bool()? {
                    current | shl32(key)
                } else {
                    current & !shl32(key)
                };
                return self.set_native_value(el, Variant::Int(new));
            }
            VirtualTarget::MergeIndex(index) => return self.merge_set_native(el, index, value),
            VirtualTarget::MergeName(Some(index)) => return self.merge_set_native(el, index, value),
            VirtualTarget::MergeName(None) => return Ok(()),
            VirtualTarget::None | VirtualTarget::FlagPath => {}
        }
        let (element, last) = match split_last(path) {
            Some((prefix, last)) => (self.element_by_path(el, prefix, true)?, Some(last)),
            None => (self.element_by_path(el, path, true)?, None),
        };
        match (element, last) {
            (Some(element), None) => self.set_native_value(element, value),
            (Some(element), Some(last)) => self.set_native_values(element, last, value),
            (None, _) => Ok(()),
        }
    }

    /// `EditValues[aPath]`.
    pub fn edit_values(&mut self, el: El, path: &str) -> R<String> {
        match self.virtual_target(el, path)? {
            VirtualTarget::Redirect(item) => return self.edit_values(item, path),
            VirtualTarget::Flag(_) | VirtualTarget::FlagPath => {
                let set = self.native_values(el, path)?.to_bool()?;
                return Ok(if set { "1" } else { "0" }.to_owned());
            }
            VirtualTarget::MergeIndex(index) => return self.merge_get_edit(el, index),
            VirtualTarget::MergeName(found) => {
                let def = self.raw_def(el);
                let index = found.or_else(|| (0..def.defs.len()).find(|index| path == index.to_string()));
                return match index {
                    Some(index) => self.merge_get_edit(el, index),
                    None => Ok(String::new()),
                };
            }
            VirtualTarget::None => {}
        }
        let (element, last) = match split_last(path) {
            Some((prefix, last)) => (self.element_by_path(el, prefix, true)?, Some(last)),
            None => (self.element_by_path(el, path, true)?, None),
        };
        match (element, last) {
            (Some(element), None) => self.edit_value(element),
            (Some(element), Some(last)) => self.edit_values(element, last),
            (None, _) => Ok(String::new()),
        }
    }

    /// `EditValues[aPath] := aValue`.
    pub fn set_edit_values(&mut self, el: El, path: &str, value: &str) -> R<()> {
        match self.virtual_target(el, path)? {
            VirtualTarget::Redirect(item) => return self.set_edit_values(item, path, value),
            VirtualTarget::Flag(_) | VirtualTarget::FlagPath => {
                return self.set_native_values(el, path, Variant::Bool(!(value == "0" || value.is_empty())));
            }
            VirtualTarget::MergeIndex(index) => return self.merge_set_edit(el, index, value),
            VirtualTarget::MergeName(found) => {
                let def = self.raw_def(el);
                let index = found.or_else(|| (0..def.defs.len()).find(|index| path == index.to_string()));
                return match index {
                    Some(index) => self.merge_set_edit(el, index, value),
                    None => Ok(()),
                };
            }
            VirtualTarget::None => {}
        }
        let (element, last) = match split_last(path) {
            Some((prefix, last)) => (self.element_by_path(el, prefix, true)?, Some(last)),
            None => (self.element_by_path(el, path, true)?, None),
        };
        match (element, last) {
            (Some(element), None) => self.set_edit_value(element, value),
            (Some(element), Some(last)) => self.set_edit_values(element, last, value),
            (None, _) => Ok(()),
        }
    }

    fn merge_member(&self, el: El, index: usize) -> R<(&'static Def, usize, usize)> {
        let def = self.raw_def(el);
        let member = def
            .defs
            .get(index)
            .ok_or_else(|| DfError::new(format!("List index ({index}) is out of bounds")))?;
        let (offset, size) = def.merge_offset(index);
        Ok((member, offset, offset + size))
    }

    fn merge_get_native(&mut self, el: El, index: usize) -> R<Variant> {
        let (member, start, end) = self.merge_member(el, index)?;
        let mut value = Variant::Empty;
        self.def_get_native(member, el, start, end, &mut value)?;
        Ok(value)
    }

    fn merge_set_native(&mut self, el: El, index: usize, value: Variant) -> R<()> {
        let (member, start, end) = self.merge_member(el, index)?;
        let mut value = value;
        self.def_set_native(member, el, start, end, &mut value)
    }

    fn merge_get_edit(&mut self, el: El, index: usize) -> R<String> {
        let (member, start, end) = self.merge_member(el, index)?;
        let mut value = String::new();
        self.def_get_edit(member, el, start, end, &mut value)?;
        Ok(value)
    }

    fn merge_set_edit(&mut self, el: El, index: usize, value: &str) -> R<()> {
        let (member, start, end) = self.merge_member(el, index)?;
        let mut value = value.to_owned();
        self.def_set_edit(member, el, start, end, &mut value)
    }

    // ---- definition value accessors (TdfDef.GetElement*) ----

    fn def_bytes(&self, el: El, start: usize, end: usize) -> &[u8] {
        let bytes = &self.n(el).bytes;
        let end = end.min(bytes.len());
        &bytes[start.min(end)..end]
    }

    fn write_bytes(&mut self, el: El, start: usize, data: &[u8]) {
        let bytes = &mut self.nm(el).bytes;
        if bytes.len() < start + data.len() {
            bytes.resize(start + data.len(), 0);
        }
        bytes[start..start + data.len()].copy_from_slice(data);
    }

    fn event_get_value(&mut self, def: &Def, el: El, value: &mut Variant) -> R<()> {
        if let Some(on_get_value) = def.events.on_get_value
            && !self.updating(el)
        {
            self.begin_update(el);
            let result = on_get_value(self, el, value);
            self.end_update(el);
            result?;
        }
        Ok(())
    }

    fn event_set_value(&mut self, def: &Def, el: El, value: &mut Variant) -> R<()> {
        if let Some(on_set_value) = def.events.on_set_value
            && !self.updating(el)
        {
            self.begin_update(el);
            let result = on_set_value(self, el, value);
            self.end_update(el);
            result?;
        }
        Ok(())
    }

    fn event_get_text(&mut self, def: &Def, el: El, value: &mut String) -> R<()> {
        if let Some(on_get_text) = def.events.on_get_text
            && !self.updating(el)
        {
            self.begin_update(el);
            let result = on_get_text(self, el, value);
            self.end_update(el);
            result?;
        }
        Ok(())
    }

    fn event_set_text(&mut self, def: &Def, el: El, value: &mut String) -> R<()> {
        if let Some(on_set_text) = def.events.on_set_text
            && !self.updating(el)
        {
            self.begin_update(el);
            let result = on_set_text(self, el, value);
            self.end_update(el);
            result?;
        }
        Ok(())
    }

    /// `GetElementNativeValue` of `def` on the bytes `start..end` of `el`.
    pub fn def_get_native(
        &mut self,
        def: &'static Def,
        el: El,
        start: usize,
        end: usize,
        value: &mut Variant,
    ) -> R<()> {
        match &def.kind {
            DefKind::Integer
            | DefKind::MappedInteger { .. }
            | DefKind::Flags { .. }
            | DefKind::Enum { .. }
            | DefKind::NiRef { .. } => {
                let bytes = self.def_bytes(el, start, end);
                let raw = read_int(bytes, def.data_type.size() as usize);
                match def.data_type {
                    DataType::U8 => *value = Variant::Int(raw as u8 as i64),
                    DataType::S8 => *value = Variant::Int(raw as u8 as i8 as i64),
                    DataType::U16 => *value = Variant::Int(raw as u16 as i64),
                    DataType::S16 => *value = Variant::Int(raw as u16 as i16 as i64),
                    DataType::U32 => *value = Variant::Int(raw as u32 as i64),
                    DataType::S32 => *value = Variant::Int(raw as u32 as i32 as i64),
                    DataType::U64 => *value = Variant::UInt(raw),
                    DataType::S64 => *value = Variant::Int(raw as i64),
                    _ => {}
                }
                self.event_get_value(def, el, value)
            }
            DefKind::Float => {
                let bytes = self.def_bytes(el, start, end);
                match def.data_type {
                    DataType::Float16 => {
                        let bits = read_int(bytes, 2) as u16;
                        *value = Variant::Float(f64::from(half_to_float(bits)));
                    }
                    DataType::Float32 => {
                        let bits = read_int(bytes, 4) as u32;
                        *value = Variant::Float(match bits {
                            0x7F7F_FFFF => MAX_SINGLE,
                            0xFF7F_FFFF => -MAX_SINGLE,
                            0x8000_0000 => -0.0,
                            _ => f64::from(f32::from_bits(bits)),
                        });
                    }
                    _ => {}
                }
                self.event_get_value(def, el, value)
            }
            DefKind::Bytes => {
                let prefix = if def.size < 0 { (-def.size) as usize } else { 0 };
                let bytes = self.def_bytes(el, start, end);
                *value = Variant::Bytes(bytes.get(prefix..).unwrap_or_default().to_vec());
                self.event_get_value(def, el, value)
            }
            DefKind::Chars { .. } => {
                let data = self.def_bytes(el, start, end).to_vec();
                let mut text = Vec::new();
                self.read_chars(el, def, Some(&data), &mut text)?;
                *value = Variant::Str(String::from_utf8_lossy(&text).into_owned());
                self.event_get_value(def, el, value)
            }
            DefKind::Merge { .. } => {
                *value = Variant::Bytes(self.def_bytes(el, start, end).to_vec());
                Ok(())
            }
            _ => self.event_get_value(def, el, value),
        }
    }

    /// `SetElementNativeValue` of `def` on the bytes `start..end` of `el`.
    pub fn def_set_native(
        &mut self,
        def: &'static Def,
        el: El,
        start: usize,
        _end: usize,
        value: &mut Variant,
    ) -> R<()> {
        match &def.kind {
            DefKind::Integer
            | DefKind::MappedInteger { .. }
            | DefKind::Flags { .. }
            | DefKind::Enum { .. }
            | DefKind::NiRef { .. } => {
                self.event_set_value(def, el, value)?;
                let size = def.data_type.size() as usize;
                if size == 0 {
                    return Ok(());
                }
                let raw = match def.data_type {
                    DataType::U64 => value.to_u64()?,
                    _ => value.to_i64()? as u64,
                };
                self.write_bytes(el, start, &raw.to_le_bytes()[..size]);
                Ok(())
            }
            DefKind::Float => {
                self.event_set_value(def, el, value)?;
                let number = value.to_f64()?;
                let single = number as f32;
                match def.data_type {
                    DataType::Float16 => {
                        let bits = float_to_half(number);
                        self.write_bytes(el, start, &bits.to_le_bytes());
                    }
                    DataType::Float32 => {
                        let bits: u32 = if value.is_empty() || number.is_nan() {
                            SINGLE_NAN_BITS
                        } else if number.is_infinite() || same_value(number, MAX_SINGLE) || number > MAX_SINGLE {
                            0x7F7F_FFFF
                        } else if same_value(number, -MAX_SINGLE) || number < -MAX_SINGLE {
                            0xFF7F_FFFF
                        } else if single.to_bits() == 0x8000_0000 {
                            0x8000_0000
                        } else {
                            single.to_bits()
                        };
                        self.write_bytes(el, start, &bits.to_le_bytes());
                    }
                    _ => {}
                }
                Ok(())
            }
            DefKind::Bytes => {
                self.event_set_value(def, el, value)?;
                let prefix: u32 = if def.size < 0 { (-def.size) as u32 } else { 0 };
                let data = value.to_bytes()?;
                let value_bytes = data.len() as u32;
                let mut bytes = if def.size > 0 { def.size as u32 } else { value_bytes };
                if prefix > 0 && bytes >= 1u32.wrapping_shl(prefix * 8 - 1) {
                    bytes = 1u32.wrapping_shl(prefix * 8 - 1) - 1;
                }
                self.allocate_value(el, (bytes + prefix) as usize);
                match prefix {
                    1 => self.write_bytes(el, 0, &[bytes as u8]),
                    2 => self.write_bytes(el, 0, &(bytes as u16).to_le_bytes()),
                    4 => self.write_bytes(el, 0, &bytes.to_le_bytes()),
                    _ => {}
                }
                let take = bytes.min(value_bytes) as usize;
                self.write_bytes(el, prefix as usize, &data[..take]);
                Ok(())
            }
            DefKind::Chars { .. } => {
                self.event_set_value(def, el, value)?;
                let text = value.to_str()?;
                self.write_chars(el, def, text.as_bytes());
                Ok(())
            }
            DefKind::Merge { .. } => {
                let mut bytes = value.to_bytes()?;
                bytes.resize(def.default_data_size() as usize, 0);
                self.write_bytes(el, start, &bytes);
                Ok(())
            }
            _ => self.event_set_value(def, el, value),
        }
    }

    /// `GetElementEditValue` of `def` on the bytes `start..end` of `el`.
    pub fn def_get_edit(&mut self, def: &'static Def, el: El, start: usize, end: usize, value: &mut String) -> R<()> {
        match &def.kind {
            DefKind::Integer
            | DefKind::MappedInteger { .. }
            | DefKind::Flags { .. }
            | DefKind::Enum { .. }
            | DefKind::NiRef { .. } => {
                let mut native = Variant::Empty;
                self.def_get_native(def, el, start, end, &mut native)?;
                *value = match def.data_type {
                    DataType::U64 | DataType::None => native.to_u64()?.to_string(),
                    _ => native.to_i64()?.to_string(),
                };
                self.event_get_text(def, el, value)?;
                match &def.kind {
                    DefKind::Enum { map } => {
                        let key = str_to_int64(value).ok_or_else(|| invalid_integer(value))?;
                        if let Some((_, name)) = map.iter().find(|(k, _)| *k == key) {
                            *value = name.clone();
                        }
                    }
                    DefKind::Flags { map } => {
                        let key = str_to_int64(value).ok_or_else(|| invalid_integer(value))?;
                        let mut text = String::new();
                        for bit in 0..i64::from(def.default_data_size() * 8) {
                            if key & shl32(bit) == 0 {
                                continue;
                            }
                            if let Some((_, name)) = map.iter().find(|(k, _)| *k == bit) {
                                if !text.is_empty() {
                                    text.push_str(" | ");
                                }
                                text.push_str(name);
                            }
                        }
                        *value = text;
                    }
                    _ => {}
                }
                Ok(())
            }
            DefKind::Float => {
                let mut native = Variant::Empty;
                self.def_get_native(def, el, start, end, &mut native)?;
                let number = native.to_f64()?;
                *value = match def.data_type {
                    DataType::Float16 => df_float_to_str(number),
                    DataType::Float32 => {
                        if number.is_nan() {
                            "NaN".to_owned()
                        } else if number.is_infinite() {
                            "Inf".to_owned()
                        } else if same_value(number, MAX_SINGLE) || number > MAX_SINGLE {
                            "Max".to_owned()
                        } else if same_value(number, -MAX_SINGLE) || number < -MAX_SINGLE {
                            "Min".to_owned()
                        } else {
                            df_float_to_str(number)
                        }
                    }
                    _ => String::new(),
                };
                self.event_get_text(def, el, value)
            }
            DefKind::Bytes => {
                let mut native = Variant::Empty;
                self.def_get_native(def, el, start, end, &mut native)?;
                let bytes = native.to_bytes()?;
                *value = bytes
                    .iter()
                    .map(|byte| format!("{byte:02X}"))
                    .collect::<Vec<_>>()
                    .join(" ");
                self.event_get_text(def, el, value)
            }
            DefKind::Chars { .. } => {
                let mut native = Variant::Empty;
                self.def_get_native(def, el, start, end, &mut native)?;
                *value = native.to_str()?;
                self.event_get_text(def, el, value)
            }
            DefKind::Merge { .. } => {
                let delimiter = def.delimiter().to_owned();
                let mut text = String::new();
                for (index, member) in def.defs.iter().enumerate() {
                    let (offset, size) = def.merge_offset(index);
                    let mut part = String::new();
                    self.def_get_edit(member, el, start + offset, start + offset + size, &mut part)?;
                    if index > 0 {
                        text.push_str(&delimiter);
                    }
                    text.push_str(&part);
                }
                *value = text;
                self.event_get_text(def, el, value)
            }
            _ => self.event_get_text(def, el, value),
        }
    }

    /// `SetElementEditValue` of `def` on the bytes `start..end` of `el`.
    pub fn def_set_edit(&mut self, def: &'static Def, el: El, start: usize, end: usize, value: &mut String) -> R<()> {
        match &def.kind {
            DefKind::Enum { map } => {
                let text = match map.iter().find(|(_, name)| same_text(name, value.trim_matches(' '))) {
                    Some((key, _)) => key.to_string(),
                    None => value.clone(),
                };
                let mut text = text;
                self.integer_set_edit(def, el, start, end, &mut text)
            }
            DefKind::Flags { map } => {
                let mut key: i64 = 0;
                let parts = split_string(value, FLAGS_DELIMITER);
                let mut last = String::new();
                for part in &parts {
                    if let Some((bit, _)) = map.iter().find(|(_, name)| same_text(name, part.trim_matches(' '))) {
                        key |= shl32(*bit);
                    }
                    last = part.clone();
                }
                // UPSTREAM-QUIRK: when no name matches, the text set is the
                // last part of the split, which makes a number work.
                let mut text = if key != 0 || value.is_empty() {
                    key.to_string()
                } else {
                    last
                };
                self.integer_set_edit(def, el, start, end, &mut text)
            }
            DefKind::Integer | DefKind::MappedInteger { .. } | DefKind::NiRef { .. } => {
                self.integer_set_edit(def, el, start, end, value)
            }
            DefKind::Float => {
                self.event_set_text(def, el, value)?;
                let number = match def.data_type {
                    DataType::Float16 => df_str_to_half_float(value)?,
                    DataType::Float32 => df_str_to_float(value)?,
                    _ => 0.0,
                };
                let mut native = Variant::Float(number);
                self.def_set_native(def, el, start, end, &mut native)
            }
            DefKind::Bytes => {
                self.event_set_text(def, el, value)?;
                let chars: Vec<char> = value.chars().collect();
                let mut bytes = Vec::new();
                let mut index = 0;
                while index < chars.len() {
                    match chars[index] {
                        ' ' | ',' | ';' => index += 1,
                        ch if ch.is_ascii_hexdigit() => {
                            if index + 1 == chars.len() {
                                return Err(
                                    self.exception(el, "Unexpected end of value. Single digit in hexadecimal pair")
                                );
                            }
                            let next = chars[index + 1];
                            if next.is_ascii_hexdigit() {
                                let pair: String = [ch, next].iter().collect();
                                bytes.push(u8::from_str_radix(&pair, 16).unwrap_or(0));
                                index += 2;
                            } else {
                                let message = format!("\"{next}\" at position {} is not a valid character", index + 2);
                                return Err(self.exception(el, &message));
                            }
                        }
                        ch => {
                            let message = format!("\"{ch}\" at position {} is not a valid character", index + 1);
                            return Err(self.exception(el, &message));
                        }
                    }
                }
                let mut native = Variant::Bytes(bytes);
                self.def_set_native(def, el, start, end, &mut native)
            }
            DefKind::Chars { .. } => {
                self.event_set_text(def, el, value)?;
                let mut native = Variant::Str(value.clone());
                self.def_set_native(def, el, start, end, &mut native)
            }
            DefKind::Merge { .. } => {
                self.event_set_text(def, el, value)?;
                let delimiter = def.delimiter().to_owned();
                for (index, part) in split_string(value, &delimiter).into_iter().enumerate() {
                    if index >= def.defs.len() {
                        break;
                    }
                    let (offset, size) = def.merge_offset(index);
                    let mut part = part;
                    self.def_set_edit(&def.defs[index], el, start + offset, start + offset + size, &mut part)?;
                }
                Ok(())
            }
            _ => self.event_set_text(def, el, value),
        }
    }

    /// `TdfIntegerDef.SetElementEditValue`.
    fn integer_set_edit(&mut self, def: &'static Def, el: El, start: usize, end: usize, value: &mut String) -> R<()> {
        self.event_set_text(def, el, value)?;
        let mut native = match def.data_type {
            DataType::U8 | DataType::S8 | DataType::U16 | DataType::S16 | DataType::S32 => {
                if value.is_empty() {
                    Variant::Int(0)
                } else {
                    Variant::Int(i64::from(str_to_int(value).ok_or_else(|| invalid_integer(value))?))
                }
            }
            DataType::U32 | DataType::U64 | DataType::S64 | DataType::None => {
                if value.is_empty() {
                    Variant::Int(0)
                } else {
                    Variant::Int(str_to_int64(value).ok_or_else(|| invalid_integer(value))?)
                }
            }
            _ => return Ok(()),
        };
        self.def_set_native(def, el, start, end, &mut native)
    }

    // ---- chars ----

    /// `TdfChars.ReadChars`: the characters of the data (without the
    /// terminator) and the bytes they take.
    fn read_chars(&mut self, el: El, def: &Def, data: Option<&[u8]>, out: &mut Vec<u8>) -> R<i32> {
        let size = def.size;
        let (terminator, terminated) = def.chars();
        out.clear();
        if size > 0 {
            self.validate_data(el, data, size)?;
            let data = data.unwrap_or_default();
            let take = if terminated { size - 1 } else { size } as usize;
            out.extend_from_slice(&data[..take.min(data.len())]);
            if out.len() < take {
                out.resize(take, 0);
            }
            Ok(size)
        } else if size == 0 {
            let data = data.unwrap_or_default();
            if !terminated {
                out.extend_from_slice(data);
                Ok(data.len() as i32)
            } else {
                self.validate_data(el, Some(data), 1)?;
                match data.iter().position(|&byte| byte == terminator) {
                    Some(position) => {
                        out.extend_from_slice(&data[..position]);
                        Ok(position as i32 + 1)
                    }
                    None => Err(self.exception(el, "Terminator character not found for terminated string")),
                }
            }
        } else {
            let prefix = -size;
            self.validate_data(el, data, prefix)?;
            let data = data.unwrap_or_default();
            let mut string_size = read_int(data, prefix as usize) as u32 as i64;
            if prefix == 4 {
                string_size = i64::from(string_size as u32 as i32);
            }
            let result = prefix as i64 + string_size;
            if result > i32::MAX as i64 {
                return Err(self.exception(
                    el,
                    &format!(
                        "Unexpected end of stream: need {result} bytes, available {}",
                        data.len()
                    ),
                ));
            }
            self.validate_data(el, Some(data), result as i32)?;
            if string_size > 0 && terminated && data.get((result - 1) as usize) == Some(&terminator) {
                string_size -= 1;
            }
            if string_size < 0 {
                return Err(self.exception(el, "Prefixed string size is negative"));
            }
            let begin = prefix as usize;
            out.extend_from_slice(&data[begin..begin + string_size as usize]);
            Ok(result as i32)
        }
    }

    /// `TdfChars.WriteChars`.
    fn write_chars(&mut self, el: El, def: &Def, text: &[u8]) {
        let size = def.size;
        let (terminator, terminated) = def.chars();
        if size > 0 {
            let size = size as usize;
            let mut bytes = vec![terminator; size];
            // UPSTREAM-QUIRK: upstream copies the whole string even when it
            // is longer than the field, past the end of the value.
            let take = text.len().min(size);
            bytes[..take].copy_from_slice(&text[..take]);
            if terminated {
                bytes[size - 1] = terminator;
            }
            self.nm(el).bytes = bytes;
        } else if size == 0 {
            let mut bytes = text.to_vec();
            if terminated {
                bytes.push(terminator);
            }
            self.nm(el).bytes = bytes;
        } else {
            let prefix = (-size) as u32;
            let mut string_size = text.len() as u32;
            if terminated {
                string_size += 1;
            }
            let cap = 1u32.wrapping_shl(prefix * 8 - 1);
            if string_size >= cap {
                string_size = cap - 1;
            }
            let mut bytes = vec![0u8; (prefix + string_size) as usize];
            bytes[..prefix as usize].copy_from_slice(&string_size.to_le_bytes()[..prefix as usize]);
            let copy = if terminated { string_size - 1 } else { string_size } as usize;
            let copy = copy.min(text.len());
            bytes[prefix as usize..prefix as usize + copy].copy_from_slice(&text[..copy]);
            if terminated {
                let last = bytes.len() - 1;
                bytes[last] = terminator;
            }
            self.nm(el).bytes = bytes;
        }
    }

    // ---- serialization ----

    /// `UnSerialize`: reads the element from `data` (`None` for `nil`: the
    /// defaults) and returns the bytes it took.
    pub fn unserialize(&mut self, el: El, data: Option<&[u8]>, data_size: i32) -> R<i32> {
        match self.class(el) {
            Class::Element => self.element_unserialize(el, data.is_some(), data_size).map(|()| 0),
            Class::Value(ValueClass::Bytes) => self.bytes_unserialize(el, data, data_size),
            Class::Value(ValueClass::Chars) => self.chars_unserialize(el, data, data_size),
            Class::Value(ValueClass::Merge) => {
                let result = self.value_unserialize(el, data, data_size)?;
                let def = self.raw_def(el);
                if def.default_value.is_empty() && data.is_none() {
                    for (index, member) in def.defs.iter().enumerate() {
                        if !member.default_value.is_empty() {
                            let (offset, size) = def.merge_offset(index);
                            let mut value = member.default_value.clone();
                            self.def_set_edit(member, el, offset, offset + size, &mut value)?;
                        }
                    }
                }
                Ok(result)
            }
            Class::Value(_) => self.value_unserialize(el, data, data_size),
            Class::Struct(StructClass::Bgsm) | Class::Struct(StructClass::Bgem) => {
                crate::data_format_material::material_unserialize(self, el, data, data_size)
            }
            Class::Struct(StructClass::Fuz) => crate::data_format_misc::fuz_unserialize(self, el, data, data_size),
            Class::Struct(_) => self.struct_unserialize(el, data, data_size),
            Class::Array => self.array_unserialize(el, data, data_size),
            Class::Union => self.union_unserialize(el, data, data_size),
            Class::NifFile => crate::data_format_nif::nif_unserialize(self, el, data, data_size),
        }
    }

    /// `TdfElement.UnSerialize`: marks the element initialized and calls
    /// `OnAfterLoad`.
    pub(crate) fn element_unserialize(&mut self, el: El, has_data: bool, data_size: i32) -> R<()> {
        self.nm(el).state |= DS_INITIALIZED;
        if let Some(on_after_load) = self.raw_def(el).events.on_after_load {
            on_after_load(self, el, has_data, data_size)?;
        }
        Ok(())
    }

    /// `TdfValue.UnSerialize`.
    fn value_unserialize(&mut self, el: El, data: Option<&[u8]>, data_size: i32) -> R<i32> {
        // A disabled element does not advance and takes its default value.
        let pdata = if self.enabled(el)? { data } else { None };
        let mut result = if data_size != 0 {
            data_size
        } else {
            self.def(el)?.default_data_size()
        };
        if result != 0 {
            self.validate_data(el, pdata, result)?;
            let size = result.max(0) as usize;
            match pdata {
                Some(pdata) => {
                    let take = size.min(pdata.len());
                    let mut bytes = pdata[..take].to_vec();
                    bytes.resize(size, 0);
                    self.nm(el).bytes = bytes;
                }
                None => self.nm(el).bytes = vec![0; size],
            }
        } else {
            self.nm(el).bytes = Vec::new();
        }
        self.element_unserialize(el, data.is_some(), data_size)?;
        if pdata.is_none() {
            result = 0;
            let def = self.def(el)?;
            if !def.default_value.is_empty() {
                let value = def.default_value.clone();
                self.set_edit_value(el, &value)?;
            }
        }
        Ok(result)
    }

    /// `TdfBytes.UnSerialize`.
    fn bytes_unserialize(&mut self, el: El, data: Option<&[u8]>, data_size: i32) -> R<i32> {
        let def = self.raw_def(el);
        let mut result: i32 = 0;
        let size = def.size;
        if size < 0 {
            let prefix = -size;
            if let Some(data) = data
                && self.enabled(el)?
            {
                self.validate_data(el, Some(data), prefix)?;
                result = read_int(data, prefix as usize) as u32 as i32;
            }
            result += prefix;
        } else if size == 0 {
            if self.enabled(el)? {
                match def.events.on_get_count {
                    Some(on_get_count) if !self.updating(el) => on_get_count(self, el, &mut result)?,
                    // UPSTREAM-QUIRK: with no data upstream takes the
                    // distance from nil to the data end.
                    _ => result = data.map_or(0, |data| data.len() as i32),
                }
            }
        } else {
            result = size;
        }
        let _ = data_size;
        self.value_unserialize(el, data, result)
    }

    /// `TdfChars.UnSerialize`.
    fn chars_unserialize(&mut self, el: El, data: Option<&[u8]>, data_size: i32) -> R<i32> {
        match data {
            None => {
                let result = self.value_unserialize(el, None, data_size)?;
                let def = self.raw_def(el);
                let (_, terminated) = def.chars();
                if terminated && def.default_value.is_empty() {
                    self.write_chars(el, def, b"");
                }
                Ok(result)
            }
            Some(data) => {
                if self.enabled(el)? {
                    let def = self.raw_def(el);
                    let mut text = Vec::new();
                    let size = self.read_chars(el, def, Some(data), &mut text)?;
                    self.value_unserialize(el, Some(data), size)
                } else {
                    // UPSTREAM-QUIRK: a disabled string with data is left
                    // uninitialized.
                    Ok(0)
                }
            }
        }
    }

    /// `TdfStruct.UnSerialize`.
    pub fn struct_unserialize(&mut self, el: El, data: Option<&[u8]>, _data_size: i32) -> R<i32> {
        let mut pdata = if self.enabled(el)? { data } else { None };
        self.clear(el)?;
        let def = self.raw_def(el);
        self.nm(el).items.reserve(def.defs.len());
        let mut result: i32 = 0;
        for member in &def.defs {
            let element = self.create_element(member, Some(el))?;
            self.put(el, usize::MAX, element);
            let bytes = self.unserialize(element, pdata, 0)?;
            result += bytes;
            if let Some(current) = pdata {
                pdata = Some(current.get(bytes.max(0) as usize..).unwrap_or_default());
            }
        }
        self.element_unserialize(el, data.is_some(), result)?;
        Ok(result)
    }

    /// `TdfArray.UnSerialize`.
    fn array_unserialize(&mut self, el: El, data: Option<&[u8]>, _data_size: i32) -> R<i32> {
        let enabled = self.enabled(el)?;
        let mut pdata = if enabled { data } else { None };
        self.clear(el)?;
        let mut result: i32 = 0;
        let def = self.raw_def(el);
        let mut size = def.size;
        let auto_expanding = size == 0 && def.counter().is_empty() && def.events.on_get_count.is_none();
        if size < 0 {
            let prefix = -size;
            let stored = match pdata {
                None => 0,
                Some(current) => {
                    // Upstream validates against the data start.
                    self.validate_data(el, data, prefix)?;
                    match prefix {
                        1 => read_int(current, 1) as i32,
                        2 => read_int(current, 2) as i32,
                        4 => read_int(current, 4) as u32 as i32,
                        _ => 0,
                    }
                }
            };
            // With no data the prefix is not counted either.
            if let Some(current) = pdata {
                pdata = Some(current.get(prefix as usize..).unwrap_or_default());
                result = prefix;
            }
            size = stored;
        } else if size == 0 {
            size = self.get_external_count(el)?;
        }
        if !enabled && def.size <= 0 {
            size = 0;
        }
        if size > 0 {
            self.nm(el).items.reserve(size as usize);
        }
        let member = &def.defs[0];
        let mut index = 0;
        while index < size || (auto_expanding && pdata.is_some_and(|current| !current.is_empty())) {
            let element = self.create_element(member, Some(el))?;
            self.put(el, usize::MAX, element);
            let bytes = self.unserialize(element, pdata, 0)?;
            result += bytes;
            if let Some(current) = pdata {
                pdata = Some(current.get(bytes.max(0) as usize..).unwrap_or_default());
            }
            index += 1;
            if auto_expanding && bytes == 0 && index >= size {
                // A member that takes no bytes would never end the array.
                break;
            }
        }
        self.element_unserialize(el, data.is_some(), result)?;
        Ok(result)
    }

    /// `TdfUnion.UnSerialize`.
    fn union_unserialize(&mut self, el: El, data: Option<&[u8]>, _data_size: i32) -> R<i32> {
        let pdata = if self.enabled(el)? { data } else { None };
        let index = self.active_index(el)?;
        self.clear(el)?;
        let def = self.raw_def(el);
        let parent = self.parent(el);
        let mut result = 0;
        for (i, member) in def.defs.iter().enumerate() {
            // The members are created with the union's parent.
            let element = self.create_element(member, parent)?;
            self.put(el, usize::MAX, element);
            if i == index {
                result = self.unserialize(element, pdata, 0)?;
            } else {
                self.set_to_default(element)?;
            }
        }
        self.element_unserialize(el, data.is_some(), result)?;
        Ok(result)
    }

    /// `TdfArray.GetExternalCount`.
    pub fn get_external_count(&mut self, el: El) -> R<i32> {
        let def = self.raw_def(el);
        let counter = def.counter();
        if !counter.is_empty() {
            let element = match self.parent(el) {
                Some(parent) => self.element_by_path(parent, counter, false)?,
                None => None,
            };
            let Some(element) = element else {
                return Err(self.exception(el, &format!("Counter element \"{counter}\" not found for array")));
            };
            if self.initialized(element) {
                self.native_value(element)?.to_i32()
            } else {
                Ok(0)
            }
        } else if let Some(on_get_count) = def.events.on_get_count {
            let mut result = 0;
            on_get_count(self, el, &mut result)?;
            Ok(result)
        } else {
            Ok(0)
        }
    }

    /// `Serialize`: appends the element's bytes to `out` and returns their
    /// number.
    pub fn serialize(&mut self, el: El, out: &mut Vec<u8>) -> R<i32> {
        match self.class(el) {
            Class::Element => Ok(0),
            Class::Value(_) => {
                if !self.enabled(el)? {
                    return Ok(0);
                }
                if !self.initialized(el) {
                    self.set_to_default(el)?;
                }
                let bytes = &self.n(el).bytes;
                out.extend_from_slice(bytes);
                Ok(bytes.len() as i32)
            }
            Class::Array => {
                if !self.enabled(el)? {
                    return Ok(0);
                }
                if !self.initialized(el) {
                    self.set_to_default(el)?;
                }
                let size = self.raw_def(el).size;
                let mut result = 0;
                if size < 0 {
                    let count = self.n(el).items.len() as u32;
                    out.extend_from_slice(&count.to_le_bytes()[..(-size) as usize]);
                    result = -size;
                }
                Ok(result + self.container_serialize(el, out)?)
            }
            Class::Union => {
                if !self.enabled(el)? {
                    return Ok(0);
                }
                let index = self.active_index(el)?;
                let item = self.item(el, index as i32)?;
                self.serialize(item, out)
            }
            Class::NifFile => crate::data_format_nif::nif_serialize(self, el, out),
            Class::Struct(_) => self.container_serialize(el, out),
        }
    }

    /// `TdfContainer.Serialize`.
    fn container_serialize(&mut self, el: El, out: &mut Vec<u8>) -> R<i32> {
        if !self.enabled(el)? {
            return Ok(0);
        }
        let mut result = 0;
        let mut index = 0;
        while let Some(&item) = self.n(el).items.get(index) {
            result += self.serialize(item, out)?;
            index += 1;
        }
        Ok(result)
    }

    /// `DataSize`: the bytes the element serializes to. Runs `OnBeforeSave`
    /// and brings counted arrays to their counters, as upstream does before
    /// saving.
    pub fn data_size(&mut self, el: El) -> R<i32> {
        match self.class(el) {
            Class::Element => {
                self.before_save(el)?;
                Ok(0)
            }
            Class::Value(_) => {
                if !self.enabled(el)? {
                    return Ok(0);
                }
                if !self.initialized(el) {
                    self.set_to_default(el)?;
                }
                self.before_save(el)?;
                Ok(self.n(el).bytes.len() as i32)
            }
            Class::Union => {
                if !self.enabled(el)? {
                    return Ok(0);
                }
                let index = self.active_index(el)?;
                let item = self.item(el, index as i32)?;
                self.data_size(item)
            }
            Class::Array => {
                if !self.enabled(el)? {
                    return Ok(0);
                }
                let def = self.raw_def(el);
                if !def.counter().is_empty() || def.events.on_get_count.is_some() {
                    let count = self.get_external_count(el)?;
                    if self.count(el) != count {
                        self.set_count(el, count)?;
                    }
                }
                let mut result = self.container_data_size(el)?;
                if def.size < 0 {
                    result += -def.size;
                }
                Ok(result)
            }
            Class::NifFile => crate::data_format_nif::nif_data_size(self, el),
            Class::Struct(_) => self.container_data_size(el),
        }
    }

    fn before_save(&mut self, el: El) -> R<()> {
        if let Some(on_before_save) = self.raw_def(el).events.on_before_save {
            on_before_save(self, el)?;
        }
        Ok(())
    }

    /// `TdfContainer.DataSize`.
    fn container_data_size(&mut self, el: El) -> R<i32> {
        if !self.enabled(el)? {
            return Ok(0);
        }
        self.before_save(el)?;
        let mut result = 0;
        let mut index = 0;
        while let Some(&item) = self.n(el).items.get(index) {
            result += self.data_size(item)?;
            index += 1;
        }
        Ok(result)
    }

    /// `SetToDefault`.
    pub fn set_to_default(&mut self, el: El) -> R<()> {
        self.unserialize(el, None, 0).map(|_| ())
    }

    /// `LoadFromData`.
    pub fn load_from_data(&mut self, el: El, data: &[u8]) -> R<()> {
        self.unserialize(el, Some(data), 0).map(|_| ())
    }

    /// `SaveToData`: the buffer is sized by `DataSize` first, then filled.
    pub fn save_to_data(&mut self, el: El) -> R<Vec<u8>> {
        let size = self.data_size(el)?.max(0) as usize;
        let mut out = Vec::with_capacity(size);
        self.serialize(el, &mut out)?;
        // UPSTREAM-QUIRK: upstream writes into a buffer of `DataSize` bytes;
        // a shorter serialization leaves zeroes, a longer one runs past it.
        out.resize(size, 0);
        Ok(out)
    }

    // ---- structure changes ----

    /// `Add`.
    pub fn add(&mut self, el: El) -> R<El> {
        match self.class(el) {
            Class::Array => {
                let def = self.raw_def(el);
                if def.size > 0 {
                    return Err(self.exception(el, "Can not change size of a fixed array"));
                }
                let element = self.create_element(&def.defs[0], Some(el))?;
                self.set_to_default(element)?;
                self.put(el, usize::MAX, element);
                Ok(element)
            }
            _ => Err(self.exception(el, "Can not add to this element")),
        }
    }

    /// `Delete(Index)`.
    pub fn delete(&mut self, el: El, index: i32) -> R<()> {
        match self.class(el) {
            Class::Struct(_) => Err(self.exception(el, "Can not change the size of structure")),
            Class::Array => {
                if self.raw_def(el).size > 0 {
                    return Err(self.exception(el, "Can not change size of a fixed array"));
                }
                self.container_delete(el, index)
            }
            Class::NifFile => crate::data_format_nif::nif_delete(self, el, index),
            Class::Union => self.container_delete(el, index),
            _ => Err(self.exception(el, "Can not delete from this element")),
        }
    }

    /// `TdfContainer.Delete`.
    pub fn container_delete(&mut self, el: El, index: i32) -> R<()> {
        let count = self.n(el).items.len() as i32;
        if index < 0 || index >= count {
            return Err(self.exception(el, &format!("List index ({index}) is out of bounds")));
        }
        let item = self.n(el).items[index as usize];
        self.free_element(item)?;
        self.nm(el).items.remove(index as usize);
        Ok(())
    }

    /// `Remove`.
    pub fn remove(&mut self, el: El) -> R<()> {
        if let Some(parent) = self.parent(el)
            && self.data_type(parent)? == DataType::Array
        {
            let index = self.index_of(parent, el);
            return self.delete(parent, index);
        }
        Err(self.exception(el, "Can not remove this element"))
    }

    /// `Move(CurIndex, NewIndex)`.
    pub fn move_item(&mut self, el: El, cur_index: i32, new_index: i32) -> R<()> {
        match self.class(el) {
            Class::Struct(_) => Err(self.exception(el, "Can not reorder a structure")),
            Class::NifFile => crate::data_format_nif::nif_move(self, el, cur_index, new_index),
            Class::Array | Class::Union => self.container_move(el, cur_index, new_index),
            _ => Err(self.exception(el, "Can not reorder in this element")),
        }
    }

    /// `TdfContainer.Move`.
    pub fn container_move(&mut self, el: El, cur_index: i32, new_index: i32) -> R<()> {
        let count = self.n(el).items.len() as i32;
        if cur_index < 0 || cur_index >= count {
            return Err(self.exception(el, &format!("List index ({cur_index}) is out of bounds")));
        }
        if new_index < 0 || new_index >= count {
            return Err(self.exception(el, &format!("List index ({new_index}) is out of bounds")));
        }
        let item = self.nm(el).items.remove(cur_index as usize);
        self.nm(el).items.insert(new_index as usize, item);
        Ok(())
    }

    /// `Sort`: the quick sort of upstream, which reports whether any
    /// element moved.
    pub fn sort(&mut self, el: El, compare: &mut dyn FnMut(&mut Tree, El, El) -> i32) -> R<bool> {
        if !self.class(el).is_container() {
            return Err(self.exception(el, "Can not sort in this element"));
        }
        let mut items = std::mem::take(&mut self.nm(el).items);
        let result = if items.len() > 1 {
            let last = items.len() as i32 - 1;
            quick_sort(self, &mut items, 0, last, compare)
        } else {
            false
        };
        self.nm(el).items = items;
        Ok(result)
    }

    /// `Remap`: the element at `i` moves to `map[i]`.
    pub fn remap(&mut self, el: El, map: &[u32]) -> R<()> {
        if !self.class(el).is_container() {
            return Err(self.exception(el, "Can not remap in this element"));
        }
        let items = &self.n(el).items;
        if items.is_empty() {
            return Ok(());
        }
        let mut remapped = items.clone();
        for (index, &target) in map.iter().enumerate().take(items.len()) {
            remapped[target as usize] = items[index];
        }
        self.nm(el).items = remapped;
        Ok(())
    }

    // ---- assign ----

    /// `Assign` from an element of the same tree.
    pub fn assign(&mut self, el: El, source: Option<El>) -> R<()> {
        self.assign_impl(el, None, source)
    }

    /// `Assign` from an element of another tree.
    pub fn assign_from(&mut self, el: El, other: &mut Tree, source: Option<El>) -> R<()> {
        self.assign_impl(el, Some(other), source)
    }

    fn assign_impl(&mut self, el: El, mut other: Option<&mut Tree>, source: Option<El>) -> R<()> {
        fn src<'a>(this: &'a mut Tree, other: &'a mut Option<&mut Tree>) -> &'a mut Tree {
            match other {
                Some(tree) => tree,
                None => this,
            }
        }
        let Some(source) = source else { return Ok(()) };
        let class = self.class(el);
        if matches!(class, Class::Element | Class::NifFile) {
            return Ok(());
        }
        if !self.enabled(el)? {
            return Ok(());
        }
        let source_class = src(self, &mut other).class(source);
        match class {
            Class::Value(_) => {
                if !(source_class.is_value() || source_class == Class::Union) {
                    return Ok(());
                }
                let same_shape = source_class.is_value() && {
                    let own_type = self.data_type(el)?;
                    let source_type = src(self, &mut other).data_type(source)?;
                    own_type == source_type && {
                        let own_size = self.data_size(el)?;
                        let source_size = src(self, &mut other).data_size(source)?;
                        own_size == source_size
                    }
                };
                if same_shape {
                    let bytes = src(self, &mut other).save_to_data(source)?;
                    self.load_from_data(el, &bytes)
                } else {
                    let value = src(self, &mut other).edit_value(source)?;
                    self.set_edit_value(el, &value)
                }
            }
            Class::Union => {
                if !(source_class.is_value() || source_class == Class::Union) {
                    return Ok(());
                }
                let value = src(self, &mut other).edit_value(source)?;
                self.set_edit_value(el, &value)
            }
            Class::Struct(_) => {
                if !source_class.is_struct() {
                    return Ok(());
                }
                let items = self.n(el).items.clone();
                for item in items {
                    let name = self.name(item)?;
                    let found = src(self, &mut other).element_by_name(source, &name, true)?;
                    match other.as_deref_mut() {
                        Some(tree) => self.assign_impl(item, Some(tree), found)?,
                        None => self.assign_impl(item, None, found)?,
                    }
                }
                Ok(())
            }
            Class::Array => {
                if source_class != Class::Array {
                    return Ok(());
                }
                let count = src(self, &mut other).count(source);
                self.set_count(el, count)?;
                let items = self.n(el).items.clone();
                for (index, item) in items.into_iter().enumerate() {
                    let found = src(self, &mut other).item(source, index as i32)?;
                    match other.as_deref_mut() {
                        Some(tree) => self.assign_impl(item, Some(tree), Some(found))?,
                        None => self.assign_impl(item, None, Some(found))?,
                    }
                }
                Ok(())
            }
            Class::Element | Class::NifFile => Ok(()),
        }
    }

    // ---- JSON and text ----

    /// `SerializeToJSON` into `json`, an object or an array.
    pub fn serialize_to_json(&mut self, el: El, json: &mut Json) -> R<()> {
        match self.class(el) {
            Class::Value(_) | Class::Union => {
                if self.enabled(el)? {
                    let value = Json::Str(self.edit_value(el)?);
                    if json.is_object() {
                        let name = self.name(el)?;
                        json.set(&name, value);
                    } else {
                        json.push(value);
                    }
                }
                Ok(())
            }
            Class::Struct(_) => {
                if !self.enabled(el)? {
                    return Ok(());
                }
                let mut object = Json::object();
                let items = self.n(el).items.clone();
                for item in items {
                    self.serialize_to_json(item, &mut object)?;
                }
                if json.is_object() {
                    let name = self.name(el)?;
                    json.set(&name, object);
                } else {
                    json.push(object);
                }
                Ok(())
            }
            Class::Array => {
                if !self.enabled(el)? {
                    return Ok(());
                }
                let mut array = Json::array();
                let items = self.n(el).items.clone();
                for item in items {
                    self.serialize_to_json(item, &mut array)?;
                }
                if json.is_object() {
                    let name = self.name(el)?;
                    json.set(&name, array);
                } else {
                    json.push(array);
                }
                Ok(())
            }
            Class::NifFile => {
                let items = self.n(el).items.clone();
                for item in items {
                    self.serialize_to_json(item, json)?;
                }
                Ok(())
            }
            Class::Element => Ok(()),
        }
    }

    /// `UnSerializeFromJSON` from `json`, an object or an array.
    pub fn unserialize_from_json(&mut self, el: El, json: &Json) -> R<()> {
        match self.class(el) {
            Class::Value(_) | Class::Union => {
                if !self.enabled(el)? {
                    return Ok(());
                }
                let value = if json.is_object() {
                    let name = self.name(el)?;
                    json.get(&name).map(Json::as_str).unwrap_or_default()
                } else {
                    let index = self.index(el)?;
                    json.index(index.max(0) as usize).map(Json::as_str).unwrap_or_default()
                };
                self.set_edit_value(el, &value)?;
                self.element_unserialize(el, true, 0)
            }
            Class::Struct(_) => {
                if !self.enabled(el)? {
                    return Ok(());
                }
                let object = if json.is_object() {
                    let name = self.name(el)?;
                    json.get(&name).cloned().unwrap_or_else(Json::object)
                } else {
                    let index = self.index(el)?;
                    json.index(index.max(0) as usize).cloned().unwrap_or_else(Json::object)
                };
                let items = self.n(el).items.clone();
                for item in items {
                    self.unserialize_from_json(item, &object)?;
                }
                self.element_unserialize(el, true, 0)
            }
            Class::Array => {
                if !self.enabled(el)? {
                    return Ok(());
                }
                let array = if json.is_object() {
                    let name = self.name(el)?;
                    json.get(&name).cloned().unwrap_or_else(Json::array)
                } else {
                    let index = self.index(el)?;
                    json.index(index.max(0) as usize).cloned().unwrap_or_else(Json::array)
                };
                let fixed = self.raw_def(el).size > 0;
                if !fixed {
                    self.set_count(el, 0)?;
                }
                for index in 0..array.len() {
                    if fixed {
                        let item = self.item(el, index as i32)?;
                        self.unserialize_from_json(item, &array)?;
                    } else {
                        let item = self.add(el)?;
                        self.unserialize_from_json(item, &array)?;
                    }
                }
                self.element_unserialize(el, true, 0)
            }
            Class::NifFile => crate::data_format_nif::nif_unserialize_from_json(self, el, json),
            Class::Element => self.element_unserialize(el, true, 0),
        }
    }

    /// `ToJSON`.
    pub fn to_json(&mut self, el: El, compact: bool) -> R<String> {
        let mut json = Json::object();
        self.serialize_to_json(el, &mut json)?;
        Ok(json.to_text(compact))
    }

    /// `FromJSON`.
    pub fn from_json(&mut self, el: El, text: &str) -> R<()> {
        let json = Json::parse(text)?;
        self.set_to_default(el)?;
        self.unserialize_from_json(el, &json)
    }

    /// `ToText`: the name and value of every enabled element, one per line,
    /// indented with tabs.
    pub fn to_text(&mut self, el: El, indent: usize) -> R<String> {
        let mut out = String::new();
        self.write_text(el, indent, &mut out)?;
        Ok(out)
    }

    fn write_text(&mut self, el: El, indent: usize, out: &mut String) -> R<()> {
        if !self.enabled(el)? {
            return Ok(());
        }
        for _ in 0..indent {
            out.push('\t');
        }
        out.push_str(&self.name(el)?);
        if self.def(el)?.is_value_def() {
            out.push_str(": ");
            out.push_str(&self.edit_value(el)?);
        }
        out.push_str("\r\n");
        for index in 0..self.count(el) {
            let item = self.item(el, index)?;
            self.write_text(item, indent + 1, out)?;
        }
        Ok(())
    }
}

enum VirtualTarget {
    None,
    Redirect(El),
    Flag(i64),
    /// A path of a flags element without a backslash that names no flag:
    /// upstream still reads and writes its edit value as a boolean.
    FlagPath,
    MergeIndex(usize),
    MergeName(Option<usize>),
}

/// The quick sort of `TdfContainer.Sort`.
fn quick_sort(
    tree: &mut Tree,
    list: &mut [El],
    mut l: i32,
    mut r: i32,
    compare: &mut dyn FnMut(&mut Tree, El, El) -> i32,
) -> bool {
    let mut result = false;
    if l < r {
        loop {
            if r - l == 1 {
                if compare(tree, list[l as usize], list[r as usize]) > 0 {
                    list.swap(l as usize, r as usize);
                    result = true;
                }
                break;
            }
            let mut i = l;
            let mut j = r;
            let p = list[((l + r) >> 1) as usize];
            loop {
                while compare(tree, list[i as usize], p) < 0 {
                    i += 1;
                }
                while compare(tree, list[j as usize], p) > 0 {
                    j -= 1;
                }
                if i <= j {
                    if i != j {
                        list.swap(i as usize, j as usize);
                        result = true;
                    }
                    i += 1;
                    j -= 1;
                }
                if i > j {
                    break;
                }
            }
            if j - l > r - i {
                if i < r {
                    result = quick_sort(tree, list, i, r, compare) || result;
                }
                r = j;
            } else {
                if l < j {
                    result = quick_sort(tree, list, l, j, compare) || result;
                }
                l = i;
            }
            if l >= r {
                break;
            }
        }
    }
    result
}

// ---- definition functions ----

/// `dfStruct`.
pub fn df_struct(name: &str, defs: Vec<Def>, events: &[Event]) -> Def {
    let mut def = Def::new(DefKind::Struct, name, DataType::Struct, defs);
    def.assign_events(events);
    def
}

/// `dfArray`.
pub fn df_array(name: &str, member: Def, size: i32, counter: &str, events: &[Event]) -> Def {
    debug_assert!(size >= 0 || size == -1 || size == -2 || size == -4);
    let mut def = Def::new(
        DefKind::Array {
            counter: counter.to_owned(),
        },
        name,
        DataType::Array,
        vec![member],
    );
    def.size = size;
    def.assign_events(events);
    def
}

/// `dfUnion`.
pub fn df_union(decider: Option<OnDecide>, defs: Vec<Def>, events: &[Event]) -> Def {
    debug_assert!(defs.len() > 1);
    let mut def = Def::new(DefKind::Union, "", DataType::Union, defs);
    def.events.on_decide = decider;
    def.assign_events(events);
    def
}

/// `dfValueUnion`.
pub fn df_value_union(data_type: DataType, decider: Option<OnDecide>, defs: Vec<Def>, events: &[Event]) -> Def {
    debug_assert!(defs.len() > 1);
    let mut def = Def::new(DefKind::ValueUnion, "", data_type, defs);
    def.events.on_decide = decider;
    def.assign_events(events);
    def
}

/// `dfMerge`.
pub fn df_merge(name: &str, defs: Vec<Def>, default_value: &str, events: &[Event]) -> Def {
    let mut def = Def::new(
        DefKind::Merge {
            offsets: Vec::new(),
            delimiter: " ".to_owned(),
        },
        name,
        DataType::Merge,
        defs,
    );
    def.default_value = default_value.to_owned();
    def.assign_events(events);
    def
}

/// `dfInteger`.
pub fn df_integer(name: &str, data_type: DataType, default_value: &str, events: &[Event]) -> Def {
    let mut def = Def::new(DefKind::Integer, name, data_type, Vec::new());
    def.default_value = default_value.to_owned();
    def.assign_events(events);
    def
}

fn get_text_hex_int(tree: &mut Tree, el: El, text: &mut String) -> R<()> {
    let value = str_to_int64(text).ok_or_else(|| invalid_integer(text))?;
    let digits = (tree.data_size(el)? * 2).max(0) as usize;
    *text = int_to_hex(value, digits);
    Ok(())
}

fn set_text_hex_int(_tree: &mut Tree, _el: El, text: &mut String) -> R<()> {
    match str_to_int64(&format!("${text}")) {
        Some(value) => {
            *text = value.to_string();
            Ok(())
        }
        None => Err(DfError::new("Invalid hex value")),
    }
}

/// `IntToHex` of an `Int64`: upper case, at least `digits` digits.
pub fn int_to_hex(value: i64, digits: usize) -> String {
    format!("{:0digits$X}", value as u64)
}

/// `dfHexInteger`.
pub fn df_hex_integer(name: &str, data_type: DataType) -> Def {
    df_integer(
        name,
        data_type,
        "",
        &[Event::GetText(get_text_hex_int), Event::SetText(set_text_hex_int)],
    )
}

/// `dfFlags`.
pub fn df_flags(name: &str, data_type: DataType, flags: &[(i64, &str)], default_value: &str, events: &[Event]) -> Def {
    let mut def = Def::new(DefKind::Flags { map: Vec::new() }, name, data_type, Vec::new());
    def.default_value = default_value.to_owned();
    def.assign_events(events);
    def.assign_values_map(flags);
    def
}

/// `dfEnum`.
pub fn df_enum(name: &str, data_type: DataType, values: &[(i64, &str)], default_value: &str, events: &[Event]) -> Def {
    let mut def = Def::new(DefKind::Enum { map: Vec::new() }, name, data_type, Vec::new());
    def.default_value = default_value.to_owned();
    def.assign_events(events);
    def.assign_values_map(values);
    def
}

/// `dfBool`.
pub fn df_bool(name: &str, data_type: DataType, default_value: &str, events: &[Event]) -> Def {
    df_enum(name, data_type, &[(0, "no"), (1, "yes")], default_value, events)
}

/// `dfFloat`.
pub fn df_float(name: &str, data_type: DataType, default_value: &str, events: &[Event]) -> Def {
    debug_assert!(matches!(data_type, DataType::Float16 | DataType::Float32));
    let mut def = Def::new(DefKind::Float, name, data_type, Vec::new());
    def.default_value = default_value.to_owned();
    def.assign_events(events);
    def
}

/// `dfBytes`.
pub fn df_bytes(name: &str, size: i32, events: &[Event]) -> Def {
    let mut def = Def::new(DefKind::Bytes, name, DataType::Bytes, Vec::new());
    def.size = size;
    def.assign_events(events);
    def
}

/// `dfChars`.
pub fn df_chars(name: &str, size: i32, default_value: &str, terminator: u8, terminated: bool, events: &[Event]) -> Def {
    let mut def = Def::new(
        DefKind::Chars { terminator, terminated },
        name,
        DataType::Chars,
        Vec::new(),
    );
    def.size = size;
    def.default_value = default_value.to_owned();
    def.assign_events(events);
    def
}

#[cfg(test)]
mod tests {
    use super::*;

    fn leak(def: Def) -> &'static Def {
        Box::leak(Box::new(def))
    }

    fn load(def: &'static Def, data: &[u8]) -> (Tree, El) {
        let mut tree = Tree::new();
        let root = tree.create_root(def, class_of(def)).unwrap();
        tree.load_from_data(root, data).unwrap();
        (tree, root)
    }

    #[test]
    fn struct_round_trip() {
        let def = leak(df_struct(
            "S",
            vec![
                df_integer("Count", DataType::U16, "", &[]),
                df_array("Values", df_float("Value", DataType::Float32, "", &[]), 0, "Count", &[]),
                df_chars("Name", -4, "", 0, false, &[]),
            ],
            &[],
        ));
        let mut data = vec![2, 0];
        data.extend_from_slice(&1.5f32.to_le_bytes());
        data.extend_from_slice(&(-2.25f32).to_le_bytes());
        data.extend_from_slice(&3u32.to_le_bytes());
        data.extend_from_slice(b"abc");
        let (mut tree, root) = load(def, &data);
        assert_eq!(tree.edit_values(root, "Values\\[1]").unwrap(), "-2.250000");
        assert_eq!(tree.edit_values(root, "Name").unwrap(), "abc");
        assert_eq!(tree.save_to_data(root).unwrap(), data);
        let text = tree.to_text(root, 0).unwrap();
        assert!(text.contains("\tValues\r\n\t\tValue #0: 1.500000\r\n"), "{text}");
    }

    #[test]
    fn counted_array_follows_counter_on_save() {
        let def = leak(df_struct(
            "S",
            vec![
                df_integer("Count", DataType::U8, "", &[]),
                df_array("Values", df_integer("Value", DataType::U8, "", &[]), 0, "Count", &[]),
            ],
            &[],
        ));
        let (mut tree, root) = load(def, &[3, 1, 2, 3]);
        tree.set_native_values(root, "Count", Variant::Int(1)).unwrap();
        assert_eq!(tree.save_to_data(root).unwrap(), vec![1, 1]);
    }

    #[test]
    fn flags_and_enums() {
        let def = leak(df_struct(
            "S",
            vec![
                df_flags("Flags", DataType::U8, &[(0, "A"), (2, "C")], "", &[]),
                df_enum("Kind", DataType::U8, &[(1, "One")], "", &[]),
            ],
            &[],
        ));
        let (mut tree, root) = load(def, &[0b0000_0111, 1]);
        assert_eq!(tree.edit_values(root, "Flags").unwrap(), "A | Bit 1 | C");
        assert_eq!(tree.edit_values(root, "Flags\\C").unwrap(), "1");
        assert_eq!(tree.edit_values(root, "Kind").unwrap(), "One");
        tree.set_edit_values(root, "Flags", "C").unwrap();
        tree.set_edit_values(root, "Kind", "7").unwrap();
        assert_eq!(tree.save_to_data(root).unwrap(), vec![4, 7]);
    }

    #[test]
    fn merge_values() {
        let def = leak(df_struct(
            "S",
            vec![df_merge(
                "V",
                vec![
                    df_float("X", DataType::Float32, "", &[]),
                    df_float("Y", DataType::Float32, "", &[]),
                ],
                "",
                &[],
            )],
            &[],
        ));
        let mut data = Vec::new();
        data.extend_from_slice(&1.0f32.to_le_bytes());
        data.extend_from_slice(&2.0f32.to_le_bytes());
        let (mut tree, root) = load(def, &data);
        assert_eq!(tree.edit_values(root, "V").unwrap(), "1.000000 2.000000");
        assert_eq!(tree.native_values(root, "V\\Y").unwrap(), Variant::Float(2.0));
        tree.set_edit_values(root, "V", "3 4").unwrap();
        assert_eq!(tree.edit_values(root, "V\\[1]").unwrap(), "4.000000");
    }

    #[test]
    fn hash_is_case_insensitive() {
        assert_eq!(df_calc_hash("Num Blocks"), df_calc_hash("num blocks"));
        assert_ne!(df_calc_hash("Num Blocks"), df_calc_hash("NumBlocks"));
        // Values of the oracle's dfCalcHash.
        assert_eq!(df_calc_hash("A"), 0xC20);
        assert_eq!(df_calc_hash("@"), 0x800);
        assert_eq!(df_calc_hash(".."), 0xBDC0);
    }

    fn version_is_one(tree: &mut Tree, el: El) -> R<i32> {
        Ok(i32::from(tree.native_values(el, "..\\Version")? == 1))
    }

    #[test]
    fn union_member_follows_decider() {
        let def = leak(df_struct(
            "S",
            vec![
                df_integer("Version", DataType::U8, "", &[]),
                df_union(
                    Some(version_is_one),
                    vec![
                        df_integer("Value", DataType::U16, "", &[]),
                        df_float("Value", DataType::Float32, "", &[]),
                    ],
                    &[],
                ),
            ],
            &[],
        ));
        let mut data = vec![1];
        data.extend_from_slice(&2.5f32.to_le_bytes());
        let (mut tree, root) = load(def, &data);
        assert_eq!(tree.edit_values(root, "Value").unwrap(), "2.500000");
        assert_eq!(tree.save_to_data(root).unwrap(), data);
        let (mut tree, root) = load(def, &[0, 7, 0]);
        assert_eq!(tree.edit_values(root, "Value").unwrap(), "7");
        // The union has no elements of its own, and its name is its member's.
        let union = tree.item(root, 1).unwrap();
        assert_eq!(tree.count(union), 0);
        assert_eq!(tree.name(union).unwrap(), "Value");
    }

    #[test]
    fn strings_of_every_size() {
        let def = leak(df_struct(
            "S",
            vec![
                df_chars("Fixed", 4, "", 0, true, &[]),
                df_chars("Line", 0, "", 0x0A, true, &[]),
                df_chars("Prefixed", -1, "", 0, true, &[]),
                df_chars("Rest", 0, "", 0, false, &[]),
            ],
            &[],
        ));
        let data = b"ab\0\0line\n\x03xy\0tail".to_vec();
        let (mut tree, root) = load(def, &data);
        assert_eq!(tree.edit_values(root, "Fixed").unwrap(), "ab\0");
        assert_eq!(tree.edit_values(root, "Line").unwrap(), "line");
        assert_eq!(tree.edit_values(root, "Prefixed").unwrap(), "xy");
        assert_eq!(tree.edit_values(root, "Rest").unwrap(), "tail");
        tree.set_edit_values(root, "Prefixed", "abc").unwrap();
        assert_eq!(tree.save_to_data(root).unwrap(), b"ab\0\0line\n\x04abc\0tail".to_vec());
        let mut tree = Tree::new();
        let root = tree.create_root(def, class_of(def)).unwrap();
        let error = tree.unserialize(root, Some(b"ab\0\0no terminator"), 0).unwrap_err();
        assert_eq!(
            error.0,
            "Error in \"Line\": Terminator character not found for terminated string"
        );
    }

    #[test]
    fn flags_take_a_number_as_text() {
        let def = leak(df_struct(
            "S",
            vec![df_flags("Flags", DataType::U16, &[(0, "A")], "", &[])],
            &[],
        ));
        let (mut tree, root) = load(def, &[0, 0]);
        // UPSTREAM-QUIRK: a text without a flag name is set as a number.
        tree.set_edit_values(root, "Flags", "5").unwrap();
        assert_eq!(tree.save_to_data(root).unwrap(), vec![5, 0]);
        assert_eq!(tree.edit_values(root, "Flags").unwrap(), "A | Bit 2");
    }

    #[test]
    fn json_round_trip() {
        let def = leak(df_struct(
            "S",
            vec![
                df_integer("Count", DataType::U8, "", &[]),
                df_array("Values", df_integer("Value", DataType::S16, "", &[]), 0, "Count", &[]),
            ],
            &[],
        ));
        let (mut tree, root) = load(def, &[2, 1, 0, 0xFF, 0xFF]);
        let json = tree.to_json(root, false).unwrap();
        assert_eq!(
            json,
            "{\n\t\"S\": {\n\t\t\"Count\": \"2\",\n\t\t\"Values\": [\n\t\t\t\"1\",\n\t\t\t\"-1\"\n\t\t]\n\t}\n}\n"
        );
        let mut other = Tree::new();
        let other_root = other.create_root(def, class_of(def)).unwrap();
        other.from_json(other_root, &json).unwrap();
        assert_eq!(other.save_to_data(other_root).unwrap(), vec![2, 1, 0, 0xFF, 0xFF]);
    }
}
