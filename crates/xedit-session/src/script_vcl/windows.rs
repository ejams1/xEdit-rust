// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

// Ported from xEdit: External/jvcl/jvcl/run/JvInterpreter_Windows.pas

//! The `Windows` interpreter unit: the `TPoint` and `TRect` value types with
//! their constructor functions and the two `TRect` field accessors, the
//! `Windows` external unit name, and `MessageBox` -- the one `AddExtFun` of
//! the ten units, which shows a modal window and therefore goes through the
//! [`Vcl`] seam like a form. The 137 `MB_*`/`VK_*` constants are in
//! [`super::tables`].

use std::cell::RefCell;
use std::rc::Rc;

use xedit_script::adapter::{Adapter, AdapterGetFn, AdapterSetFn, Args};
use xedit_script::values::{ParamType, RecordDef, RecordField, Value, ValueError, VarType, to_i64};

use super::{Vcl, helpers};

/// The registered record types, so the two accessors can read and write the
/// fields by name.
struct Rects {
    point: Rc<RecordDef>,
    rect: Rc<RecordDef>,
}

/// Registers what `JvInterpreter_Windows.pas` registers: the records and
/// their methods, the external unit, the three constructor functions and
/// `MessageBox`.
pub(crate) fn register(adapter: &mut Adapter, vcl: &Rc<RefCell<dyn Vcl>>) {
    let rects = Rc::new(Rects {
        point: adapter.add_rec("Windows", "TPoint", 8, vec![field("X", 0), field("Y", 4)]),
        rect: adapter.add_rec(
            "Windows",
            "TRect",
            16,
            vec![
                field("Left", 0),
                field("Top", 4),
                field("Right", 8),
                field("Bottom", 12),
            ],
        ),
    });

    adapter.add_ext_unit("Windows");

    {
        let point = rects.point.clone();
        adapter.add_function(
            "Windows",
            "Point",
            Rc::new(move |value: &mut Value, args: &mut Args| {
                let x = to_i64(&args.values[0])? as i32;
                let y = to_i64(&args.values[1])? as i32;
                *value = point_value(&point, x, y);
                Ok(())
            }),
            2,
            vec![ParamType::value(VarType::Integer), ParamType::value(VarType::Integer)],
            VarType::Record,
        );
    }
    {
        let rect = rects.rect.clone();
        adapter.add_function(
            "Windows",
            "Rect",
            Rc::new(move |value: &mut Value, args: &mut Args| {
                let left = to_i64(&args.values[0])? as i32;
                let top = to_i64(&args.values[1])? as i32;
                let right = to_i64(&args.values[2])? as i32;
                let bottom = to_i64(&args.values[3])? as i32;
                *value = rect_value(&rect, left, top, right, bottom);
                Ok(())
            }),
            4,
            vec![ParamType::value(VarType::Integer); 4],
            VarType::Record,
        );
    }
    {
        let rect = rects.rect.clone();
        adapter.add_function(
            "Windows",
            "Bounds",
            Rc::new(move |value: &mut Value, args: &mut Args| {
                let left = to_i64(&args.values[0])? as i32;
                let top = to_i64(&args.values[1])? as i32;
                let width = to_i64(&args.values[2])? as i32;
                let height = to_i64(&args.values[3])? as i32;
                *value = rect_value(&rect, left, top, left + width, top + height);
                Ok(())
            }),
            4,
            vec![ParamType::value(VarType::Integer); 4],
            VarType::Record,
        );
    }

    // `TopLeft` and `BottomRight` are fields of the record as much as the four
    // integers are; their handlers build and read `TPoint`s.
    {
        let point = rects.point.clone();
        adapter.add_rec_get(
            rects.rect.clone(),
            "TopLeft",
            Rc::new(move |value: &mut Value, args: &mut Args| {
                let (left, top, _, _) = rect_fields(&args.obj)?;
                *value = point_value(&point, left, top);
                Ok(())
            }) as AdapterGetFn,
            0,
            vec![ParamType::value(VarType::Empty)],
            VarType::Record,
        );
    }
    {
        adapter.add_rec_set(
            rects.rect.clone(),
            "TopLeft",
            Rc::new(move |value: &Value, args: &mut Args| {
                let (x, y) = point_fields(value)?;
                write_rect(&args.obj, Some((x, y)), None)
            }) as AdapterSetFn,
            0,
            vec![ParamType::value(VarType::Empty)],
        );
    }
    {
        let point = rects.point.clone();
        adapter.add_rec_get(
            rects.rect.clone(),
            "BottomRight",
            Rc::new(move |value: &mut Value, args: &mut Args| {
                let (_, _, right, bottom) = rect_fields(&args.obj)?;
                *value = point_value(&point, right, bottom);
                Ok(())
            }) as AdapterGetFn,
            0,
            vec![ParamType::value(VarType::Empty)],
            VarType::Record,
        );
    }
    {
        adapter.add_rec_set(
            rects.rect.clone(),
            "BottomRight",
            Rc::new(move |value: &Value, args: &mut Args| {
                let (x, y) = point_fields(value)?;
                write_rect(&args.obj, None, Some((x, y)))
            }) as AdapterSetFn,
            0,
            vec![ParamType::value(VarType::Empty)],
        );
    }

    // `AddExtFun(cWindows, 'MessageBox', 0, user32, 'MessageBoxA', -1, 4, ...)`:
    // a modal window, answered by the seam (the GUI phase calls the real
    // `MessageBoxA`; headless it refuses with the class's error).
    adapter.add_ext_fun(
        "Windows",
        "MessageBox",
        Value::Str(b"user32".to_vec()),
        "MessageBoxA",
        -1,
        4,
        vec![
            ParamType::value(VarType::Integer),
            ParamType::value(VarType::Str),
            ParamType::value(VarType::Str),
            ParamType::value(VarType::Integer),
        ],
        VarType::Integer,
    );
    helpers::register_external(adapter, vcl.clone());
}

fn field(name: &str, offset: i64) -> RecordField {
    RecordField {
        name: name.to_owned(),
        offset,
        typ: VarType::Integer,
        data_type: None,
    }
}

/// A `TPoint` value (`Point2Var`).
fn point_value(def: &Rc<RecordDef>, x: i32, y: i32) -> Value {
    let mut record = xedit_script::values::RecordInstance::new(def.clone());
    record.values = vec![Value::Integer(x), Value::Integer(y)];
    Value::Record(Rc::new(RefCell::new(record)))
}

/// A `TRect` value (`Rect2Var`).
fn rect_value(def: &Rc<RecordDef>, left: i32, top: i32, right: i32, bottom: i32) -> Value {
    let mut record = xedit_script::values::RecordInstance::new(def.clone());
    record.values = vec![
        Value::Integer(left),
        Value::Integer(top),
        Value::Integer(right),
        Value::Integer(bottom),
    ];
    Value::Record(Rc::new(RefCell::new(record)))
}

/// The four integers of the record receiver (`TRect(P2R(Args.Obj)^)`).
///
/// Upstream's `V2R` reads the variant's raw record pointer whatever its type,
/// so a receiver that is not a `TRect` is garbage there; the port reports the
/// interpreter's type mismatch instead.
fn rect_fields(obj: &Option<Value>) -> Result<(i32, i32, i32, i32), ValueError> {
    let Some(Value::Record(record)) = obj else {
        return Err(ValueError::Variant);
    };
    let record = record.borrow();
    let field = |index: usize| -> Result<i32, ValueError> {
        match record.values.get(index) {
            Some(Value::Integer(value)) => Ok(*value),
            _ => Err(ValueError::Variant),
        }
    };
    Ok((field(0)?, field(1)?, field(2)?, field(3)?))
}

/// `TRect(...).TopLeft := ...` / `.BottomRight := ...`: the two columns of the
/// record value are written in place (the record value is shared with the
/// caller, as upstream's record pointer is).
fn write_rect(
    obj: &Option<Value>,
    top_left: Option<(i32, i32)>,
    bottom_right: Option<(i32, i32)>,
) -> Result<(), ValueError> {
    let Some(Value::Record(record)) = obj else {
        return Err(ValueError::Variant);
    };
    let mut record = record.borrow_mut();
    let mut write = |index: usize, value: i32| -> Result<(), ValueError> {
        match record.values.get_mut(index) {
            Some(slot @ Value::Integer(_)) => {
                *slot = Value::Integer(value);
                Ok(())
            }
            _ => Err(ValueError::Variant),
        }
    };
    if let Some((x, y)) = top_left {
        write(0, x)?;
        write(1, y)?;
    }
    if let Some((x, y)) = bottom_right {
        write(2, x)?;
        write(3, y)?;
    }
    Ok(())
}

/// The two integers of a `TPoint` value (`Var2Point`).
fn point_fields(value: &Value) -> Result<(i32, i32), ValueError> {
    let Value::Record(record) = value else {
        return Err(ValueError::Variant);
    };
    let record = record.borrow();
    let field = |index: usize| -> Result<i32, ValueError> {
        match record.values.get(index) {
            Some(Value::Integer(value)) => Ok(*value),
            _ => Err(ValueError::Variant),
        }
    };
    Ok((field(0)?, field(1)?))
}
