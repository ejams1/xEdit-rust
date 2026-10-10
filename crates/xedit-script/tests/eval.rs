// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! The phase 6 step 3 gate for the interpreter runtime: scripts parse and
//! execute end to end against test adapters. Arithmetic and conversions,
//! `for`/`while`/`repeat`/`case`, `var`/const parameters and the ignored
//! default values, recursion, the dialect's missing `with`, arrays and
//! records, exceptions with their positions, a `var` parameter written by an
//! adapter function, the sorted dispatch's case-insensitivity, the
//! `for`/`case` 64-bit behaviour of the upstream `whatsnew.md` note, the
//! statement hook, `CallFunctionEx`, and three scripts of the 4.1.5q corpus
//! executed verbatim (`Apply filter for cleaning.pas`, `List loaded plugins
//! and their masters.pas`, `Check for errors.pas`).
//!
//! Notes on positions: the interpreter counts lines by `#13` characters
//! (`GetLineByPos`, `JvJCLUtils.pas:1358`), so the scripts that assert a
//! line number are written with CRLF, as the corpus files are. An error that
//! travels out of a routine has its message prefixed with
//! `Error in unit '...' on line n : ...` by `UpdateExceptionPos`
//! (`JvInterpreter.pas:5377`); a raised exception object keeps its own
//! message and reports the location on `last_error`.

// The interpreter's error is a value the tests move around; boxing it in the
// helper results would not make them clearer.
#![allow(clippy::result_large_err)]

use std::any::Any;
use std::cell::RefCell;
use std::rc::Rc;

use xedit_script::adapter::Args;
use xedit_script::values::{ClassDef, ParamType, RecordField, RecordInstance, ScriptObject, Value, VarType};
use xedit_script::{ExecError, Interpreter};

//=== Helpers =================================================================

/// A script object of the tests: a named object with a class and its data.
struct TestObject {
    class: Rc<ClassDef>,
    data: RefCell<TestData>,
}

#[derive(Default)]
struct TestData {
    name: String,
    error: String,
    children: Vec<Rc<RefCell<TestObject>>>,
}

impl ScriptObject for TestObject {
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

fn object_name(value: &Value) -> String {
    let Value::Object(object) = value else {
        panic!("not an object: {value:?}");
    };
    let object = object.borrow();
    let data = object
        .as_any()
        .downcast_ref::<TestObject>()
        .expect("TestObject")
        .data
        .borrow();
    data.name.clone()
}

/// An exception object with a message, the shape `Exception.Create` makes.
struct TestException {
    class: Rc<ClassDef>,
    message: String,
}

impl ScriptObject for TestException {
    fn class(&self) -> Rc<ClassDef> {
        self.class.clone()
    }

    fn as_any(&self) -> &dyn Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }

    fn message(&self) -> Option<&str> {
        Some(&self.message)
    }
}

/// Registers `Exception` with its `Create` constructor and `Message`
/// property, the `SysUtils` surface a script's `raise Exception.Create(...)`
/// and `on E: Exception` need.
fn register_exception(interpreter: &mut Interpreter) -> Rc<ClassDef> {
    let class = interpreter.adapter.add_class("SysUtils", "Exception", None);
    interpreter.adapter.add_get(
        class.clone(),
        "Create",
        {
            let class = class.clone();
            Rc::new(move |value: &mut Value, args: &mut Args| {
                let message = match args.values.first() {
                    Some(Value::Str(bytes)) => String::from_utf8_lossy(bytes).into_owned(),
                    _ => String::new(),
                };
                *value = Value::Object(Rc::new(RefCell::new(TestException {
                    class: class.clone(),
                    message,
                })));
                Ok(())
            })
        },
        1,
        vec![ParamType::value(VarType::Str)],
        VarType::Object,
    );
    interpreter.adapter.add_get(
        class.clone(),
        "Message",
        Rc::new(|value: &mut Value, args: &mut Args| {
            let object = args.object().expect("receiver").clone();
            let object = object.borrow();
            let message = object.message().unwrap_or_default().to_owned();
            *value = Value::Str(message.into_bytes());
            Ok(())
        }),
        0,
        Vec::new(),
        VarType::Str,
    );
    class
}

/// Compiles one unit script and calls its `Initialize`.
fn run_initialize(interpreter: &mut Interpreter, source: &str) -> Result<Value, ExecError> {
    interpreter.compile(source.as_bytes())?;
    interpreter.call_function("Initialize", &[])
}

//=== Arithmetic, strings, conversions ========================================

#[test]
fn arithmetic_strings_and_type_conversions() {
    let mut interpreter = Interpreter::new();
    let source = "\
unit u;
function Initialize: integer;
var s, t: string;
  d: double;
  n: integer;
  b: boolean;
begin
  Result := 7 div 2 + 7 mod 2 * 3;
  s := 'a' + 1;
  d := 1 + 2.5;
  b := (s = 'a1') and (d = 3.5);
  if b then Result := Result + 100;
  t := 'xyz';
  t[2] := 'Q';
  if t = 'xQz' then Result := Result + 1000;
  n := '5';
  s := 12;
  Result := Result + n;
  if s = '12' then Result := Result * 10;
end;
end.
";
    // 6 -> +100 -> +1000 -> +5 (the string '5' coerced by the typed
    // variable) -> *10.
    assert_eq!(run_initialize(&mut interpreter, source).unwrap(), Value::Integer(11110));
}

//=== Control flow ============================================================

#[test]
fn for_while_repeat_and_case() {
    let mut interpreter = Interpreter::new();
    let source = "\
unit u;
function Initialize: integer;
var i, total: integer;
begin
  total := 0;
  for i := 1 to 10 do total := total + i;
  for i := 10 downto 6 do total := total + i;
  for i := 3 to 1 do total := total + 100;
  i := 0;
  while i < 5 do begin i := i + 1; total := total + 1; end;
  repeat i := i - 1; until i = 0;
  case total of
    0..9: total := -1;
    100: total := total + 1000;
    else total := -2;
  end;
  Result := total + i;
end;
end.
";
    // 55 + 40 + 0 (the empty range) + 5 -> 100 -> 1100, and i back to 0.
    assert_eq!(run_initialize(&mut interpreter, source).unwrap(), Value::Integer(1100));
}

//=== Parameters ==============================================================

#[test]
fn var_parameters_defaults_and_the_out_prefix() {
    let mut interpreter = Interpreter::new();
    let source = "\
unit u;
procedure Swap(var a, b: integer);
var t: integer;
begin
  t := a; a := b; b := t;
end;
function AddTo(var acc: integer; a: integer): integer;
begin
  acc := acc + a;
  Result := acc;
end;
function Outy(out x: integer): integer;
begin
  Result := x * 2;
end;
function Initialize: integer;
var x, y, acc: integer;
begin
  x := 1; y := 2;
  Swap(x, y);
  Result := x * 100 + y * 10;
  acc := 0;
  Result := Result + AddTo(acc, 5);
  Result := Result + Outy(21);
end;
end.
";
    // `Swap` writes both `var` parameters back (x = 2, y = 1 -> 210),
    // `AddTo` writes its accumulator (215), and `out x: integer` is one
    // by-value parameter named `x` -- the dialect has no `out`, `ReadParams`
    // overwrites the name slot (`JvInterpreter.pas:7861`) -- giving 42.
    assert_eq!(run_initialize(&mut interpreter, source).unwrap(), Value::Integer(257));
}

#[test]
fn a_defaulted_parameter_is_not_a_default() {
    // `ReadParams` skips everything after the type name up to the `;`
    // (`JvInterpreter.pas:7870`), so `b: integer = 5` counts as a parameter
    // and the call with one argument is `ieNotEnoughParams`.
    let mut interpreter = Interpreter::new();
    let source = "unit u;\r\n\
function Padded(a: integer; b: integer = 5): integer;\r\n\
begin\r\n\
  Result := a + b;\r\n\
end;\r\n\
function Initialize: integer;\r\n\
begin\r\n\
  Result := Padded(1);\r\n\
end;\r\n\
end.\r\n";
    let error = run_initialize(&mut interpreter, source).unwrap_err();
    assert_eq!(error.code, 182);
    assert_eq!(error.message, "Error in unit 'u' on line 8 : Not enough parameters");
}

#[test]
fn recursion() {
    let mut interpreter = Interpreter::new();
    let source = "\
unit u;
function Fib(n: integer): integer;
begin
  if n < 2 then Result := n else Result := Fib(n - 1) + Fib(n - 2);
end;
function Initialize: integer;
begin
  Result := Fib(10);
end;
end.
";
    assert_eq!(run_initialize(&mut interpreter, source).unwrap(), Value::Integer(55));
}

#[test]
fn with_is_not_part_of_the_dialect() {
    // `JvInterpreterParser.pas` has no `ttWith`: `with x do ...` lexes as a
    // call to an identifier named `with`, and the statement terminator check
    // wants `';'` where `x` stands (`JvInterpreter.pas:6654`). `Compile`
    // scans bodies without parsing them, so the error surfaces at the call.
    let mut interpreter = Interpreter::new();
    let source = "unit u;\r\nfunction Initialize: integer;\r\nbegin\r\n  with x do y := 1;\r\nend;\r\nend.\r\n";
    interpreter.compile(source.as_bytes()).unwrap();
    let error = interpreter.call_function("Initialize", &[]).unwrap_err();
    assert_eq!(error.code, 103);
    assert_eq!(
        error.message,
        "Error in unit 'u' on line 4 : ';' expected but 'x' found"
    );
}

//=== Arrays and records ======================================================

#[test]
fn arrays_store_by_their_item_type() {
    let mut interpreter = Interpreter::new();
    let source = "\
unit u;
function Initialize: integer;
var a: array [1..4] of integer;
  s: array [0..1] of string;
begin
  a[1] := 10; a[4] := 40;
  Result := a[1] + a[4];
  s[0] := 'foo';
  if s[0] = 'foo' then Result := Result + 1;
  a[2] := 2.5;
  Result := Result + a[2];
  a[3] := '9';
  Result := Result + a[3];
end;
end.
";
    // `PInteger(P)^ := Value` rounds the double (half to even: 2.5 -> 2),
    // and the string element parses (`VarAsType`). 50 + 1 + 2 + 9.
    assert_eq!(run_initialize(&mut interpreter, source).unwrap(), Value::Integer(62));
}

#[test]
fn array_bounds_are_checked() {
    let mut interpreter = Interpreter::new();
    let source = "\
unit u;
function Initialize: integer;
var a: array [1..4] of integer;
begin
  Result := a[9];
end;
end.
";
    let error = run_initialize(&mut interpreter, source).unwrap_err();
    assert_eq!(error.code, 171);
    assert!(
        error.message.ends_with(": Array index out of bounds"),
        "{}",
        error.message
    );
}

#[test]
fn arrays_are_not_accepted_as_parameters() {
    // `CheckNotSupportedFunctionParameters` (`JvInterpreter.pas:6543`).
    let mut interpreter = Interpreter::new();
    let source = "\
unit u;
function Take(a: integer): integer;
begin
  Result := a;
end;
function Initialize: integer;
var a: array [1..2] of integer;
begin
  Result := Take(a);
end;
end.
";
    let error = run_initialize(&mut interpreter, source).unwrap_err();
    assert_eq!(error.code, 2);
    // `NotImplemented` concatenates `RsENotImplemented` to the message with
    // no separating space (`JvInterpreter.pas:1622`): "parameternot
    // implemented".
    assert!(
        error
            .message
            .ends_with(": Internal interpreter error: Array and Record types are not allowed as procedure/function parameternot implemented"),
        "{}",
        error.message
    );
}

#[test]
fn script_records_hold_typed_fields() {
    let mut interpreter = Interpreter::new();
    let source = "\
unit u;
type
  TPoint = record
    X, Y: integer;
  end;
function Initialize: integer;
var p: TPoint;
  q: TPoint;
begin
  p.X := 3;
  p.Y := 4;
  Result := p.X * p.Y;
  q := p;
  q.X := 10;
  Result := Result + p.X + q.X;
end;
end.
";
    // A record variable initializes through its `DataType`, a field write
    // goes through the declared type, and `q := p` copies the fields
    // (`JvInterpreterVarAssignment`, `JvInterpreter.pas:2512`).
    assert_eq!(run_initialize(&mut interpreter, source).unwrap(), Value::Integer(25));
}

#[test]
fn adapter_records_and_their_fields() {
    let mut interpreter = Interpreter::new();
    let point = interpreter.adapter.add_rec(
        "Types",
        "TPoint",
        8,
        vec![
            RecordField {
                name: "X".to_owned(),
                offset: 0,
                typ: VarType::Integer,
                data_type: None,
            },
            RecordField {
                name: "Y".to_owned(),
                offset: 4,
                typ: VarType::Integer,
                data_type: None,
            },
        ],
    );
    interpreter.adapter.add_function(
        "Types",
        "Point",
        {
            let point = point.clone();
            Rc::new(move |value: &mut Value, args: &mut Args| {
                let x = match args.values[0] {
                    Value::Integer(x) => x,
                    _ => 0,
                };
                let y = match args.values[1] {
                    Value::Integer(y) => y,
                    _ => 0,
                };
                let instance = RecordInstance {
                    def: point.clone(),
                    values: vec![Value::Integer(x), Value::Integer(y)],
                };
                *value = Value::Record(Rc::new(RefCell::new(instance)));
                Ok(())
            })
        },
        2,
        vec![ParamType::value(VarType::Integer), ParamType::value(VarType::Integer)],
        VarType::Record,
    );
    let source = "\
unit u;
function Initialize: integer;
var p: TPoint;
begin
  p := Point(3, 4);
  Result := p.X * p.Y;
  p.Y := 5;
  Result := Result + p.Y;
end;
end.
";
    assert_eq!(run_initialize(&mut interpreter, source).unwrap(), Value::Integer(17));
}

#[test]
fn open_array_arguments_arrive_as_variant_arrays() {
    let mut interpreter = Interpreter::new();
    interpreter.adapter.add_function(
        "",
        "Sum",
        Rc::new(|value: &mut Value, args: &mut Args| {
            // The port of `Args.OpenArray`/`V2OA` (`JvInterpreter.pas:5313`).
            let values = args.open_array(0)?;
            let mut sum = 0i32;
            for item in values {
                if let Value::Integer(v) = item {
                    sum += v;
                }
            }
            *value = Value::Integer(sum);
            Ok(())
        }),
        1,
        vec![ParamType::value(VarType::Empty)],
        VarType::Integer,
    );
    let source = "\
unit u;
function Initialize: integer;
begin
  Result := Sum([1, 2, 3]);
end;
end.
";
    assert_eq!(run_initialize(&mut interpreter, source).unwrap(), Value::Integer(6));
}

//=== Exceptions ==============================================================

#[test]
fn raise_and_except_with_the_bound_object() {
    let mut interpreter = Interpreter::new();
    register_exception(&mut interpreter);
    let source = "\
unit u;
function Raising: integer;
begin
  raise Exception.Create('boom');
end;
function Caught: integer;
begin
  Result := 0;
  try
    Raising;
  except
    on E: Exception do begin
      Result := 7;
      if E.Message = 'boom' then Result := 77;
    end;
  end;
end;
function Reraised: integer;
begin
  Result := 1;
  try
    try
      Raising;
    except
      on E: Exception do begin
        Result := 2;
        raise;
      end;
    end;
  except
    on E: Exception do Result := 3;
  end;
end;
end.
";
    interpreter.compile(source.as_bytes()).unwrap();
    assert_eq!(interpreter.call_function("Caught", &[]).unwrap(), Value::Integer(77));
    // The bare `raise;` re-raises the original, which the outer handler
    // catches.
    assert_eq!(interpreter.call_function("Reraised", &[]).unwrap(), Value::Integer(3));
}

#[test]
fn an_uncaught_raise_reports_the_object_and_its_location() {
    let mut interpreter = Interpreter::new();
    register_exception(&mut interpreter);
    let source = "unit u;\r\n\
function Raising: integer;\r\n\
begin\r\n\
  raise Exception.Create('boom');\r\n\
end;\r\n\
end.\r\n";
    interpreter.compile(source.as_bytes()).unwrap();
    let error = interpreter.call_function("Raising", &[]).unwrap_err();
    // The host sees the exception object's own message
    // (`raise V2O(V)`, `JvInterpreter.pas:7670`); the location lives on
    // `FLastError` (`UpdateExceptionPos`, `:5390`).
    assert_eq!(error.message, "boom");
    assert_eq!(error.code, 6);
    assert_eq!(interpreter.last_error().unit_name, "u");
    assert_eq!(interpreter.last_error().line, 4);
    assert_eq!(interpreter.last_error_location(), "unit u line 4");
    assert_eq!(
        interpreter.last_error().message,
        "External error in unit 'u' on line 4 : boom"
    );
}

#[test]
fn interpreter_errors_carry_their_position_and_line() {
    let mut interpreter = Interpreter::new();
    let source = "unit positions;\r\n\
function Missing: integer;\r\n\
begin\r\n\
  Result := Undeclared;\r\n\
end;\r\n\
end.\r\n";
    interpreter.compile(source.as_bytes()).unwrap();
    let error = interpreter.call_function("Missing", &[]).unwrap_err();
    assert_eq!(error.code, 104);
    assert_eq!(
        error.message,
        "Error in unit 'positions' on line 4 : Undeclared Identifier 'Undeclared'"
    );
    // `JvInterpreterErrorN(ieUnknownIdentifier, PosBeg, Identifier)`
    // (`JvInterpreter.pas:6128`) with `PosBeg` the start of the token after
    // the name: the `;`.
    let semicolon = (source.find("Undeclared;").unwrap() + "Undeclared".len()) as i64;
    assert_eq!(error.pos, Some(semicolon));
    assert_eq!(interpreter.last_error().line, 4);
    assert_eq!(interpreter.last_error_location(), "unit positions line 4");
}

#[test]
fn an_exception_a_class_does_not_match_propagates() {
    let mut interpreter = Interpreter::new();
    let exception = register_exception(&mut interpreter);
    // `EAbort` is registered and is not an ancestor of the raised
    // `Exception`: no clause matches and there is no `else`, so
    // `ReRaiseException` re-raises (`JvInterpreter.pas:7533`).
    interpreter.adapter.add_class("SysUtils", "EAbort", Some(exception));
    let source = "\
unit u;
function Raising: integer;
begin
  raise Exception.Create('boom');
end;
function Initialize: integer;
begin
  Result := 0;
  try
    Raising;
  except
    on E: EAbort do Result := 1;
  end;
end;
end.
";
    interpreter.compile(source.as_bytes()).unwrap();
    let error = interpreter.call_function("Initialize", &[]).unwrap_err();
    assert_eq!(error.message, "boom");
}

#[test]
fn an_unresolvable_on_class_swallows_the_exception() {
    // `On1`'s `GetValue` of the class name fails with `ieUnknownIdentifier`
    // (`JvInterpreter.pas:7480`), and the `DoFinallyExcept` wrapper catches
    // every `EJvInterpreterError` of the handler -- only `ieRaise` re-raises
    // (`:7587`) -- so the original exception counts as handled and the
    // lookup error vanishes.
    let mut interpreter = Interpreter::new();
    register_exception(&mut interpreter);
    let source = "\
unit u;
function Initialize: integer;
begin
  Result := 5;
  try
    raise Exception.Create('boom');
  except
    on E: NoSuchClass do Result := 1;
  end;
end;
end.
";
    assert_eq!(run_initialize(&mut interpreter, source).unwrap(), Value::Integer(5));
}

#[test]
fn a_bare_raise_outside_an_except_surfaces_as_ie_raise() {
    let mut interpreter = Interpreter::new();
    let source = "\
unit u;
function Initialize: integer;
begin
  Result := 1;
  raise;
end;
end.
";
    let error = run_initialize(&mut interpreter, source).unwrap_err();
    assert_eq!(error.code, 4);
    assert_eq!(
        error.message,
        "Re-raising an exception only allowed in exception handler"
    );
}

#[test]
fn an_error_inside_an_except_handler_is_swallowed() {
    // The same wrapper: an undeclared identifier inside an `except` arm
    // vanishes and the original exception counts as handled.
    let mut interpreter = Interpreter::new();
    register_exception(&mut interpreter);
    let source = "\
unit u;
function Raising: integer;
begin
  raise Exception.Create('boom');
end;
function Initialize: integer;
begin
  Result := 5;
  try
    Raising;
  except
    on E: Exception do Result := Undeclared;
  end;
end;
end.
";
    assert_eq!(run_initialize(&mut interpreter, source).unwrap(), Value::Integer(5));
}

#[test]
fn division_by_zero_is_the_external_error() {
    let mut interpreter = Interpreter::new();
    let source = "unit u;\r\nfunction Initialize: integer;\r\nbegin\r\n  Result := 1 / 0;\r\nend;\r\nend.\r\n";
    interpreter.compile(source.as_bytes()).unwrap();
    let error = interpreter.call_function("Initialize", &[]).unwrap_err();
    assert_eq!(error.code, 6);
    assert_eq!(error.message, "External error in unit 'u' on line 4 : Division by zero");
    assert_eq!(interpreter.last_error().line, 4);
}

//=== Adapter-written var parameters =========================================

#[test]
fn an_adapter_function_writes_a_var_parameter_back() {
    let mut interpreter = Interpreter::new();
    interpreter.adapter.add_function(
        "",
        "Bump",
        Rc::new(|value: &mut Value, args: &mut Args| {
            if let Some(Value::Integer(current)) = args.values.first_mut() {
                *current += 1;
            }
            *value = Value::Empty;
            Ok(())
        }),
        1,
        vec![ParamType::by_ref(VarType::Integer)],
        VarType::Empty,
    );
    let source = "\
unit u;
function Initialize: integer;
var n: integer;
begin
  n := 41;
  Bump(n);
  Result := n * 10;
end;
end.
";
    // `CheckArgs` sees the `var` parameter of the named variable, and after
    // the call `UpdateVarParams` writes the value back through `SetValue`
    // (`JvInterpreter.pas:6072`).
    assert_eq!(run_initialize(&mut interpreter, source).unwrap(), Value::Integer(420));
}

//=== Lookup dispatch =========================================================

#[test]
fn the_sorted_dispatch_is_case_insensitive() {
    let mut interpreter = Interpreter::new();
    for (name, result) in [("Gamma", 3), ("alpha", 1), ("Beta", 2)] {
        interpreter.adapter.add_function(
            "",
            name,
            Rc::new(move |value: &mut Value, _args: &mut Args| {
                *value = Value::Integer(result);
                Ok(())
            }),
            0,
            Vec::new(),
            VarType::Integer,
        );
    }
    interpreter.adapter.add_const("", "MaxCount", Value::Integer(9));
    let source = "\
unit u;
function Initialize: integer;
var total: integer;
begin
  total := ALPHA() + beta + GAMMA();
  Result := total + maxcount;
end;
end.
";
    // `alpha`, `Beta` and `Gamma` are found case-insensitively through the
    // sorted `Find` (`AnsiStrIComp`, `JvInterpreter.pas:2924`), and a
    // constant answers a bare name of any case.
    assert_eq!(run_initialize(&mut interpreter, source).unwrap(), Value::Integer(15));
}

#[test]
fn a_constant_wins_over_a_function_of_the_same_name() {
    // `GetValue`'s order: classes, constants, functions
    // (`JvInterpreter.pas:4536-4542`).
    let mut interpreter = Interpreter::new();
    interpreter.adapter.add_function(
        "",
        "Thing",
        Rc::new(|value: &mut Value, _args: &mut Args| {
            *value = Value::Integer(7);
            Ok(())
        }),
        0,
        Vec::new(),
        VarType::Integer,
    );
    interpreter.adapter.add_const("", "Thing", Value::Integer(5));
    let source = "\
unit u;
function Initialize: integer;
begin
  Result := Thing;
end;
end.
";
    assert_eq!(run_initialize(&mut interpreter, source).unwrap(), Value::Integer(5));
}

#[test]
fn set_values_and_their_comparisons() {
    // Sets: enum-style *integer* constants build `1 shl value` bits
    // (`SetExpression1`, `JvInterpreter.pas:5945`); a set-valued identifier
    // is `ieIntegerRequired` because `VarIsOrdinal(varSet)` is false
    // (`:5943`, the custom variant type is not ordinal). The comparison of
    // a `varSet` is the raw one (`:5792`).
    let mut interpreter = Interpreter::new();
    interpreter.adapter.add_const("", "One", Value::Integer(1));
    interpreter.adapter.add_const("", "Two", Value::Integer(2));
    let source = "\
unit u;
function Initialize: integer;
var s: variant;
begin
  s := [One, Two];
  Result := 0;
  if s = [One, Two] then Result := 11;
  if s = [One] then Result := -1;
end;
end.
";
    assert_eq!(run_initialize(&mut interpreter, source).unwrap(), Value::Integer(11));
}

//=== The 64-bit integer behaviour ============================================

#[test]
fn the_64_bit_integer_literals_of_for_and_case() {
    // `ParseToken` turns an integer literal that does not fit an `Integer`
    // into a `varInt64` (`Val(FTokenStr, ValueInt64, Stub)`,
    // `JvInterpreter.pas:5565`), and `Expression2` accepts the listed
    // integer kinds only when the value fits an `Integer` (`:5905-5913`):
    // the upstream `whatsnew.md` fix ("certain scripts with `for` loops or
    // `case` statements might fail with `Integer required` error in 64
    // bit"). At the boundary it works; the arithmetic keeps `Int64` values
    // in range accepted.
    let mut interpreter = Interpreter::new();
    let source = "\
unit q;
function Loop: integer;
var i, x: integer;
begin
  Result := 0;
  for i := 2147483647 to 2147483647 do Result := Result + 1;
  case 2147483647 of
    2147483647: Result := Result + 10;
  end;
  x := 2147483646;
  for i := x to x do Result := Result + 1;
end;
end.
";
    interpreter.compile(source.as_bytes()).unwrap();
    assert_eq!(interpreter.call_function("Loop", &[]).unwrap(), Value::Integer(12));
}

#[test]
fn an_out_of_range_64_bit_literal_is_integer_required() {
    let mut interpreter = Interpreter::new();
    let source = "unit q;\nfunction Loop: integer;\nvar i: integer;\nbegin\n  for i := 3000000000 to 3000000002 do Result := i;\nend;\nend.\n";
    interpreter.compile(source.as_bytes()).unwrap();
    let error = interpreter.call_function("Loop", &[]).unwrap_err();
    assert_eq!(error.code, 108);
    assert!(
        error.message.ends_with(": Type of expression must be integer"),
        "{}",
        error.message
    );
    // `Expression2`'s `ErrPos := PosBeg` at its entry (`:5896`): the first
    // token of the bound.
    let expected = source.find("3000000000").unwrap() as i64;
    assert_eq!(error.pos, Some(expected));
}

#[test]
fn an_out_of_range_64_bit_case_selector_is_integer_required() {
    let mut interpreter = Interpreter::new();
    let source = "\
unit q;
function Pick: integer;
begin
  case 3000000000 of
    1: Result := 1;
  end;
end;
end.
";
    interpreter.compile(source.as_bytes()).unwrap();
    let error = interpreter.call_function("Pick", &[]).unwrap_err();
    assert_eq!(error.code, 108);
    assert!(
        error.message.ends_with(": Type of expression must be integer"),
        "{}",
        error.message
    );
    let expected = source.find("3000000000").unwrap() as i64;
    assert_eq!(error.pos, Some(expected));
}

#[test]
fn an_int64_result_in_range_works_out_of_range_refuses() {
    let mut interpreter = Interpreter::new();
    // `$80000000` is above `High(Integer)`, so the literal is a `varInt64`;
    // `$80000000 - 1` stays an `Int64` in range, which `Expression2`
    // accepts. An untyped variable keeps whatever kind it is assigned.
    let source = "\
unit q;
function InRange: integer;
var i, x: variant;
begin
  x := $80000000 - 1;
  Result := 0;
  for i := x - 1 to x do Result := Result + 1;
end;
function OutOfRange: integer;
var i, x: variant;
begin
  x := $80000000;
  Result := 0;
  for i := x - 1 to x do Result := Result + 1;
end;
end.
";
    interpreter.compile(source.as_bytes()).unwrap();
    assert_eq!(interpreter.call_function("InRange", &[]).unwrap(), Value::Integer(2));
    let error = interpreter.call_function("OutOfRange", &[]).unwrap_err();
    assert_eq!(error.code, 108);
    assert!(
        error.message.ends_with(": Type of expression must be integer"),
        "{}",
        error.message
    );
}

//=== The statement hook ======================================================

#[test]
fn the_statement_hook_fires_per_statement_and_separator() {
    let mut interpreter = Interpreter::new();
    let log: Rc<RefCell<Vec<usize>>> = Rc::new(RefCell::new(Vec::new()));
    let log_ref = log.clone();
    interpreter.on_statement = Some(Box::new(move |pos| {
        log_ref.borrow_mut().push(pos);
        Ok(())
    }));
    let source = "unit u;\nfunction Initialize: integer;\nvar a, b: integer;\nbegin\n  a := 1; b := 2\nend;\nend.\n";
    interpreter.compile(source.as_bytes()).unwrap();
    assert_eq!(interpreter.call_function("Initialize", &[]).unwrap(), Value::Integer(0));
    // `InterpretStatement` hooks a statement once, and `InterpretBegin`
    // hooks every separating `;` on its own (`JvInterpreter.pas:6648`,
    // `:7056`): a, ';', b.
    let a = source.find("a := 1").unwrap();
    let semicolon = source.find("; b").unwrap();
    let b = source.find("b := 2").unwrap();
    assert_eq!(*log.borrow(), vec![a, semicolon, b]);
}

#[test]
fn the_statement_hook_can_abort_the_script() {
    let mut interpreter = Interpreter::new();
    interpreter.on_statement = Some(Box::new(|_pos| Err(ExecError::new(3, None, "", ""))));
    let source = "\
unit u;
function Initialize: integer;
begin
  Result := 1;
end;
end.
";
    let error = run_initialize(&mut interpreter, source).unwrap_err();
    assert_eq!(error.code, 3);
    assert!(error.message.ends_with(": User break"), "{}", error.message);
}

//=== CallFunctionEx and errors of the call ==================================

#[test]
fn call_function_ex_writes_host_arguments_back() {
    let mut interpreter = Interpreter::new();
    let source = "\
unit u;
procedure SetIt(var n: integer);
begin
  n := n + 1;
end;
function Initialize: integer;
begin
  Result := 0;
end;
end.
";
    interpreter.compile(source.as_bytes()).unwrap();
    let mut args = Args::new();
    args.values.push(Value::Integer(10));
    let result = interpreter.call_function_ex(None, "SetIt", &mut args).unwrap();
    // A procedure leaves the previous result (Empty); the `var` parameter
    // reads back through `LeaveFunction` (`JvInterpreter.pas:6505`).
    assert_eq!(result, Value::Empty);
    assert_eq!(args.values[0], Value::Integer(11));
    // An unknown name is `ieUnknownIdentifier` at the call itself
    // (`:8482`), without a position or a unit.
    let error = interpreter.call_function("Nope", &[]).unwrap_err();
    assert_eq!(error.code, 104);
    assert_eq!(error.message, "Undeclared Identifier 'Nope'");
}

#[test]
fn unknown_identifiers_at_runtime_report_the_script_position() {
    let mut interpreter = Interpreter::new();
    let source = "unit u;\nfunction Initialize: integer;\nbegin\n  Result := Nope(1);\nend;\nend.\n";
    interpreter.compile(source.as_bytes()).unwrap();
    let error = interpreter.call_function("Initialize", &[]).unwrap_err();
    assert_eq!(error.code, 104);
    assert!(
        error.message.ends_with(": Undeclared Identifier 'Nope'"),
        "{}",
        error.message
    );
    // `PosBeg` after `ReadArgs` consumed the token following the `)`: the
    // `;`.
    let semicolon = (source.find(");").unwrap() + 1) as i64;
    assert_eq!(error.pos, Some(semicolon));
}

#[test]
fn a_var_typed_local_coerces_and_a_redeclaration_is_an_error() {
    let mut interpreter = Interpreter::new();
    let source = "\
unit u;
function Initialize: integer;
var n: integer;
begin
  n := 5;
  n := '12';
  Result := n;
end;
function Broken: integer;
var n: integer;
  n: integer;
begin
  Result := 0;
end;
end.
";
    interpreter.compile(source.as_bytes()).unwrap();
    assert_eq!(
        interpreter.call_function("Initialize", &[]).unwrap(),
        Value::Integer(12)
    );
    // `AddVar`'s `FindVar` check (`JvInterpreter.pas:2619`).
    let error = interpreter.call_function("Broken", &[]).unwrap_err();
    assert_eq!(error.code, 111);
    assert!(
        error.message.ends_with(": Identifier redeclared: 'n'"),
        "{}",
        error.message
    );
}

#[test]
fn a_compile_only_body_error_surfaces_at_the_call() {
    // `Compile` scans bodies for their balanced `end` (`FindToken(ttBegin);
    // SkipToEnd`, `JvInterpreter.pas:8014`): a malformed body compiles --
    // the shipped `Assets manager.pas` proves it -- and fails when the
    // routine first runs; a sibling routine is unaffected.
    let mut interpreter = Interpreter::new();
    let source = "unit u;\nfunction Broken: integer;\nbegin\n  Result := 1 + ;\nend;\nfunction Fine: integer;\nbegin\n  Result := 2;\nend;\nend.\n";
    interpreter.compile(source.as_bytes()).unwrap();
    let error = interpreter.call_function("Broken", &[]).unwrap_err();
    assert!(error.message.contains("expected"), "{}", error.message);
    assert_eq!(interpreter.call_function("Fine", &[]).unwrap(), Value::Integer(2));
}

#[test]
fn uses_resolve_through_the_host_hook() {
    let mut interpreter = Interpreter::new();
    interpreter.set_unit_source(|name| match name {
        "Helper" => Some(
            b"unit Helper;\nfunction Twice(n: integer): integer;\nbegin\n  Result := n * 2;\nend;\nend.\n".to_vec(),
        ),
        // A built-in: the host answers with the empty stub.
        "xEditAPI" => Some(b"unit xEditAPI; end.\n".to_vec()),
        _ => None,
    });
    let source = "\
unit u;
uses xEditAPI, Helper;
function Initialize: integer;
begin
  Result := Twice(21);
end;
end.
";
    // `FindFunDesc` falls back to a classless global of any unit
    // (`JvInterpreter.pas:3933`).
    assert_eq!(run_initialize(&mut interpreter, source).unwrap(), Value::Integer(42));
    // A unit without a source is `ieUnitNotFound` (`:8041`).
    let source = "unit u;\nuses Missing;\nfunction Initialize: integer;\nbegin\n  Result := 0;\nend;\nend.\n";
    let error = run_initialize(&mut interpreter, source).unwrap_err();
    assert_eq!(error.code, 56);
    assert!(
        error.message.ends_with(": Unit 'Missing' not found"),
        "{}",
        error.message
    );
}

#[test]
fn values_of_a_typed_variable_take_their_conversions() {
    // `var_as_type`'s numeric rounding (half to even) and range checks,
    // exercised through the interpreter's own variable writes.
    let mut interpreter = Interpreter::new();
    let source = "\
unit u;
function Initialize: integer;
var n: integer;
  b: byte;
begin
  n := 2.5;
  b := 7;
  Result := n * 10 + b;
end;
end.
";
    assert_eq!(run_initialize(&mut interpreter, source).unwrap(), Value::Integer(27));
    assert_eq!(
        xedit_script::values::var_as_type(&Value::Double(2.5), VarType::Integer).unwrap(),
        Value::Integer(2)
    );
}

//=== The corpus scripts, executed ============================================

/// `Edit Scripts\Apply filter for cleaning.pas` of the 4.1.5q oracle,
/// verbatim: the host's `Filter*` globals are written through `OnSetValue`
/// (`xejviScriptHost.pas:64`) and `ApplyFilter` is a host procedure.
#[test]
fn corpus_apply_filter_for_cleaning() {
    const SCRIPT: &str = r#"{
  Apply filter for cleaning implemented via script.

  Hotkey: Ctrl+Shift+C
}
unit userscript;

function Initialize: Integer;
begin
  FilterConflictAll := False;
  //FilterConflictAllSet := [caUnknown, caOnlyOne, caNoConflict, caConflictBenign, caOverride, caConflict, caConflictCritical];
  FilterConflictThis := False;
  //FilterConflictThisSet := [ctUnknown, ctIgnored, ctNotDefined, ctIdenticalToMaster, ctOnlyOne, ctHiddenByModGroup, ctMaster, ctConflictBenign, ctOverride, ctIdenticalToMasterWinsConflict, ctConflictWins, ctConflictLoses];
  FilterByInjectStatus := False;
  FilterInjectStatus := False;
  FilterByNotReachableStatus := False;
  FilterNotReachableStatus := False;
  FilterByReferencesInjectedStatus := False;
  FilterReferencesInjectedStatus := False;
  FilterByEditorID := False;
  FilterEditorID := '';
  FilterByName := False;
  FilterName := '';
  FilterByBaseEditorID := False;
  FilterBaseEditorID := '';
  FilterByBaseName := False;
  FilterBaseName := '';
  FilterScaledActors := False;
  FilterByPersistent := False;
  FilterPersistent := False;
  FilterUnnecessaryPersistent := False;
  FilterMasterIsTemporary := False;
  FilterIsMaster := False;
  FilterPersistentPosChanged := False;
  FilterDeleted := False;
  FilterByVWD := False;
  FilterVWD := False;
  FilterByHasVWDMesh := False;
  FilterHasVWDMesh := False;
  FilterBySignature := False;
  FilterSignatures := '';
  //FilterSignatures := 'ARMO,AMMO,WEAP';
  FilterByBaseSignature := False;
  FilterBaseSignatures := '';
  FlattenBlocks := False;
  FlattenCellChilds := False;
  AssignPersWrldChild := False;
  InheritConflictByParent := True; // *********************

  ApplyFilter;

  Result := 1;
end;

end.
"#;
    let mut interpreter = Interpreter::new();
    let written: Rc<RefCell<Vec<(String, Value)>>> = Rc::new(RefCell::new(Vec::new()));
    let written_ref = written.clone();
    interpreter.on_set_value = Some(Box::new(move |identifier, value, _args| {
        written_ref.borrow_mut().push((identifier.to_owned(), value.clone()));
        Ok(true)
    }));
    let applied = Rc::new(RefCell::new(false));
    let applied_ref = applied.clone();
    interpreter.adapter.add_function(
        "",
        "ApplyFilter",
        Rc::new(move |_value: &mut Value, _args: &mut Args| {
            *applied_ref.borrow_mut() = true;
            Ok(())
        }),
        0,
        Vec::new(),
        VarType::Empty,
    );
    assert_eq!(run_initialize(&mut interpreter, SCRIPT).unwrap(), Value::Integer(1));
    assert!(*applied.borrow());
    let written = written.borrow();
    assert_eq!(written.len(), 36);
    assert_eq!(written[0].0, "FilterConflictAll");
    assert_eq!(written[0].1, Value::Bool(false));
    assert_eq!(written[9].0, "FilterEditorID");
    assert_eq!(written[9].1, Value::Str(Vec::new()));
    assert_eq!(written[35].0, "InheritConflictByParent");
    assert_eq!(written[35].1, Value::Bool(true));
}

/// `Edit Scripts\List loaded plugins and their masters.pas` of the 4.1.5q
/// oracle, verbatim: the file functions are host adapter functions over
/// object values.
#[test]
fn corpus_list_loaded_plugins_and_their_masters() {
    const SCRIPT: &str = r#"{
  Example on how to access the current list of loaded plugins.
  Lists plugins and their masters.
}
unit UserScript;

function Initialize: integer;
var
  i, j: integer;
  plugin, master: IInterface;
begin
  for i := 0 to FileCount - 1 do begin
    plugin := FileByIndex(i);
    AddMessage(Name(plugin)); // or GetFileName()
    for j := 0 to MasterCount(plugin) - 1 do begin
      master := MasterByIndex(plugin, j);
      AddMessage('      ' + Name(master));
    end;
  end;

  // nothing else to do, terminate
  Result := 1;
end;

end.
"#;
    let mut interpreter = Interpreter::new();
    let class = interpreter.adapter.add_class("", "TwbFile", None);
    let log: Rc<RefCell<Vec<String>>> = Rc::new(RefCell::new(Vec::new()));
    let log_ref = log.clone();
    interpreter.adapter.add_function(
        "",
        "AddMessage",
        Rc::new(move |_value: &mut Value, args: &mut Args| {
            if let Some(Value::Str(bytes)) = args.values.first() {
                log_ref.borrow_mut().push(String::from_utf8_lossy(bytes).into_owned());
            }
            Ok(())
        }),
        1,
        vec![ParamType::value(VarType::Empty)],
        VarType::Empty,
    );
    let make = |name: &str, children: Vec<Rc<RefCell<TestObject>>>| {
        Rc::new(RefCell::new(TestObject {
            class: class.clone(),
            data: RefCell::new(TestData {
                name: name.to_owned(),
                error: String::new(),
                children,
            }),
        }))
    };
    let files: Rc<Vec<Rc<RefCell<TestObject>>>> = Rc::new(vec![
        make("Skyrim.esm", Vec::new()),
        make(
            "Update.esm",
            vec![make("Skyrim.esm", Vec::new()), make("Dawnguard.esm", Vec::new())],
        ),
    ]);
    let files_count = files.clone();
    interpreter.adapter.add_function(
        "",
        "FileCount",
        Rc::new(move |value: &mut Value, _args: &mut Args| {
            *value = Value::Integer(files_count.len() as i32);
            Ok(())
        }),
        0,
        Vec::new(),
        VarType::Integer,
    );
    let files_index = files.clone();
    interpreter.adapter.add_function(
        "",
        "FileByIndex",
        Rc::new(move |value: &mut Value, args: &mut Args| {
            if let Some(Value::Integer(index)) = args.values.first()
                && let Some(file) = files_index.get(*index as usize)
            {
                *value = Value::Object(file.clone());
            }
            Ok(())
        }),
        1,
        vec![ParamType::value(VarType::Integer)],
        VarType::Object,
    );
    interpreter.adapter.add_function(
        "",
        "Name",
        Rc::new(|value: &mut Value, args: &mut Args| {
            let name = args.values.first().map(object_name).unwrap_or_default();
            *value = Value::Str(name.into_bytes());
            Ok(())
        }),
        1,
        vec![ParamType::value(VarType::Empty)],
        VarType::Str,
    );
    interpreter.adapter.add_function(
        "",
        "MasterCount",
        Rc::new(|value: &mut Value, args: &mut Args| {
            let count = args
                .values
                .first()
                .and_then(Value::as_object)
                .map(|object| {
                    object
                        .borrow()
                        .as_any()
                        .downcast_ref::<TestObject>()
                        .expect("TestObject")
                        .data
                        .borrow()
                        .children
                        .len() as i32
                })
                .unwrap_or(0);
            *value = Value::Integer(count);
            Ok(())
        }),
        1,
        vec![ParamType::value(VarType::Empty)],
        VarType::Integer,
    );
    interpreter.adapter.add_function(
        "",
        "MasterByIndex",
        Rc::new(|value: &mut Value, args: &mut Args| {
            let child = args.values.first().and_then(Value::as_object).and_then(|object| {
                let object = object.borrow();
                let data = object.as_any().downcast_ref::<TestObject>().expect("TestObject");
                let data = data.data.borrow();
                let index = match args.values.get(1) {
                    Some(Value::Integer(index)) => *index as usize,
                    _ => return None,
                };
                data.children.get(index).cloned()
            });
            if let Some(child) = child {
                *value = Value::Object(child);
            }
            Ok(())
        }),
        2,
        vec![ParamType::value(VarType::Empty), ParamType::value(VarType::Integer)],
        VarType::Object,
    );
    assert_eq!(run_initialize(&mut interpreter, SCRIPT).unwrap(), Value::Integer(1));
    assert_eq!(
        *log.borrow(),
        vec![
            "Skyrim.esm".to_owned(),
            "Update.esm".to_owned(),
            "      Skyrim.esm".to_owned(),
            "      Dawnguard.esm".to_owned(),
        ]
    );
}

/// `Edit Scripts\Check for errors.pas` of the 4.1.5q oracle, verbatim:
/// recursion, `downto`, `<>`, and the `or` accumulation over a small
/// element tree.
#[test]
fn corpus_check_for_errors() {
    const SCRIPT: &str = r#"{
  Does the same as internal "Check for errors" function in xEdit
}
unit CheckForErrorsScript;

function CheckForErrors(aIndent: Integer; aElement: IInterface): Boolean;
var
  Error : string;
  i     : Integer;
begin
  Error := Check(aElement);
  Result := Error <> '';
  if Result then begin
    Error := Check(aElement);
    AddMessage(StringOfChar(' ', aIndent * 2) + Name(aElement) + ' -> ' + Error);
  end;

  for i := ElementCount(aElement) - 1 downto 0 do
    Result := CheckForErrors(aIndent + 1, ElementByIndex(aElement, i)) or Result;

  if Result and (Error = '') then
    AddMessage(StringOfChar(' ', aIndent * 2) + 'Above errors were found in :' + Name(aElement));
end;

function Process(e: IInterface): integer;
begin
  CheckForErrors(0, e);
end;

end.
"#;
    let mut interpreter = Interpreter::new();
    let class = interpreter.adapter.add_class("", "TwbElement", None);
    let make = |name: &str, error: &str, children: Vec<Rc<RefCell<TestObject>>>| {
        Rc::new(RefCell::new(TestObject {
            class: class.clone(),
            data: RefCell::new(TestData {
                name: name.to_owned(),
                error: error.to_owned(),
                children,
            }),
        }))
    };
    let sub = make("Sub", "bad", Vec::new());
    let ok = make("Ok", "", Vec::new());
    let root = make("Record", "", vec![sub, ok]);
    let log: Rc<RefCell<Vec<String>>> = Rc::new(RefCell::new(Vec::new()));
    let log_ref = log.clone();
    interpreter.adapter.add_function(
        "",
        "AddMessage",
        Rc::new(move |_value: &mut Value, args: &mut Args| {
            if let Some(Value::Str(bytes)) = args.values.first() {
                log_ref.borrow_mut().push(String::from_utf8_lossy(bytes).into_owned());
            }
            Ok(())
        }),
        1,
        vec![ParamType::value(VarType::Empty)],
        VarType::Empty,
    );
    interpreter.adapter.add_function(
        "",
        "StringOfChar",
        Rc::new(|value: &mut Value, args: &mut Args| {
            let count = match args.values.get(1) {
                Some(Value::Integer(count)) => (*count).max(0) as usize,
                _ => 0,
            };
            *value = Value::Str(vec![b' '; count]);
            Ok(())
        }),
        2,
        vec![ParamType::value(VarType::Empty), ParamType::value(VarType::Integer)],
        VarType::Str,
    );
    interpreter.adapter.add_function(
        "",
        "Check",
        Rc::new(|value: &mut Value, args: &mut Args| {
            let error = args
                .values
                .first()
                .and_then(Value::as_object)
                .map(|object| {
                    object
                        .borrow()
                        .as_any()
                        .downcast_ref::<TestObject>()
                        .expect("TestObject")
                        .data
                        .borrow()
                        .error
                        .clone()
                })
                .unwrap_or_default();
            *value = Value::Str(error.into_bytes());
            Ok(())
        }),
        1,
        vec![ParamType::value(VarType::Empty)],
        VarType::Str,
    );
    interpreter.adapter.add_function(
        "",
        "Name",
        Rc::new(|value: &mut Value, args: &mut Args| {
            let name = args.values.first().map(object_name).unwrap_or_default();
            *value = Value::Str(name.into_bytes());
            Ok(())
        }),
        1,
        vec![ParamType::value(VarType::Empty)],
        VarType::Str,
    );
    interpreter.adapter.add_function(
        "",
        "ElementCount",
        Rc::new(|value: &mut Value, args: &mut Args| {
            let count = args
                .values
                .first()
                .and_then(Value::as_object)
                .map(|object| {
                    object
                        .borrow()
                        .as_any()
                        .downcast_ref::<TestObject>()
                        .expect("TestObject")
                        .data
                        .borrow()
                        .children
                        .len() as i32
                })
                .unwrap_or(0);
            *value = Value::Integer(count);
            Ok(())
        }),
        1,
        vec![ParamType::value(VarType::Empty)],
        VarType::Integer,
    );
    interpreter.adapter.add_function(
        "",
        "ElementByIndex",
        Rc::new(|value: &mut Value, args: &mut Args| {
            let child = args.values.first().and_then(Value::as_object).and_then(|object| {
                let object = object.borrow();
                let data = object.as_any().downcast_ref::<TestObject>().expect("TestObject");
                let data = data.data.borrow();
                let index = match args.values.get(1) {
                    Some(Value::Integer(index)) => *index as usize,
                    _ => return None,
                };
                data.children.get(index).cloned()
            });
            if let Some(child) = child {
                *value = Value::Object(child);
            }
            Ok(())
        }),
        2,
        vec![ParamType::value(VarType::Empty), ParamType::value(VarType::Integer)],
        VarType::Object,
    );
    interpreter.compile(SCRIPT.as_bytes()).unwrap();
    let mut args = Args::new();
    args.values.push(Value::Object(root));
    // `Process` has a result but never assigns it: the initialized 0.
    assert_eq!(
        interpreter.call_function_ex(None, "Process", &mut args).unwrap(),
        Value::Integer(0)
    );
    assert_eq!(
        *log.borrow(),
        vec![
            "  Sub -> bad".to_owned(),
            "Above errors were found in :Record".to_owned()
        ]
    );
}
