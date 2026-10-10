// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

// Ported from xEdit: External/jvcl/jvcl/run/JvInterpreter.pas (EJvInterpreterError,
// LoadStr2, the ie* codes) and External/jvcl/jvcl/run/JvResources.pas (the English
// resourcestring texts) and External/jvcl/jvcl/run/JvJCLUtils.pas (GetLineByPos)

//! The errors of the front end.
//!
//! `EJvInterpreterError.Create` formats its message with
//! `Format(LoadStr2(ErrCode), [ErrName1, ErrName2])` (`JvInterpreter.pas:1478`),
//! so a message is built from the code's English text and the names the raiser
//! passes. The texts are the `RsEInterpreter*` resourcestrings of
//! `JvResources.pas:1317-1379`. A position is a 0-based byte offset into the
//! source; [`get_line_by_pos`] is the port of `GetLineByPos`
//! (`JvJCLUtils.pas:1358`), which counts `#13` characters, so the line of an
//! error is the number of CRs before the position plus one.

use std::fmt;

/// `ieInternal` (`JvInterpreter.pas:1229`).
pub const IE_INTERNAL: i32 = 2;
/// `ieErrorPos` -- the `Error in unit '%0:s' on line %1:d : %2:s` format.
pub const IE_ERROR_POS: i32 = 5;
/// `ieUnitNotFound`.
pub const IE_UNIT_NOT_FOUND: i32 = 56;
/// `ieBadRemark` -- raised by the tokenizer for a bad remark.
pub const IE_BAD_REMARK: i32 = 101;
/// `ieExpected`.
pub const IE_EXPECTED: i32 = 103;
/// `ieUnknownIdentifier`.
pub const IE_UNKNOWN_IDENTIFIER: i32 = 104;
/// `ieBooleanRequired`.
pub const IE_BOOLEAN_REQUIRED: i32 = 105;
/// `ieClassRequired`.
pub const IE_CLASS_REQUIRED: i32 = 106;
/// `ieNotAllowedBeforeElse`.
pub const IE_NOT_ALLOWED_BEFORE_ELSE: i32 = 107;
/// `ieIntegerRequired`.
pub const IE_INTEGER_REQUIRED: i32 = 108;
/// `ieROCRequired`.
pub const IE_ROC_REQUIRED: i32 = 109;
/// `ieMissingOperator`.
pub const IE_MISSING_OPERATOR: i32 = 110;
/// `ieIdentifierRedeclared`.
pub const IE_IDENTIFIER_REDECLARED: i32 = 111;
/// `ieArrayIndexOutOfBounds`.
pub const IE_ARRAY_INDEX_OUT_OF_BOUNDS: i32 = 171;
/// `ieArrayTooManyParams`.
pub const IE_ARRAY_TOO_MANY_PARAMS: i32 = 172;
/// `ieArrayNotEnoughParams`.
pub const IE_ARRAY_NOT_ENOUGH_PARAMS: i32 = 173;
/// `ieArrayBadDimension`.
pub const IE_ARRAY_BAD_DIMENSION: i32 = 174;
/// `ieArrayBadRange`.
pub const IE_ARRAY_BAD_RANGE: i32 = 175;
/// `ieArrayRequired`.
pub const IE_ARRAY_REQUIRED: i32 = 176;
/// `ieTypeMistmatch` (upstream spells it so).
pub const IE_TYPE_MISMATCH: i32 = 53;
/// `ieRaise` -- the internal code of a bare `raise;` (`JvInterpreter.pas:7663`).
pub const IE_RAISE: i32 = 4;
/// `ieExternal` -- a non-interpreter exception, reported with its own text.
pub const IE_EXTERNAL: i32 = 6;
/// `ieExpressionStackOverflow`.
pub const IE_EXPRESSION_STACK_OVERFLOW: i32 = 8;
/// `ieRecordNotDefined` -- raised for a record value whose type is unknown
/// (`JvInterpreter.pas:6115`).
pub const IE_RECORD_NOT_DEFINED: i32 = 31;
/// `ieStackOverFlow` -- declared upstream, never raised.
pub const IE_STACK_OVERFLOW: i32 = 52;
/// `ieMainUndefined` -- `TJvInterpreterUnit.Run` without a `main` routine.
pub const IE_MAIN_UNDEFINED: i32 = 55;
/// `ieEventNotRegistered`.
pub const IE_EVENT_NOT_REGISTERED: i32 = 57;
/// `ieTooManyParams` (`CheckArgs`, `JvInterpreter.pas:3899`).
pub const IE_TOO_MANY_PARAMS: i32 = 181;
/// `ieNotEnoughParams` (`CheckArgs`, `JvInterpreter.pas:3901`).
pub const IE_NOT_ENOUGH_PARAMS: i32 = 182;
/// `ieIncompatibleTypes` -- declared upstream, never raised.
pub const IE_INCOMPATIBLE_TYPES: i32 = 183;

/// `RsENotImplemented` (`JvResources.pas:98`): concatenated to a message by
/// `NotImplemented` (`JvInterpreter.pas:1620`) without a separating space.
pub const RS_NOT_IMPLEMENTED: &str = "not implemented";
/// `RsArrayToArrayAssignment` (`JvResources.pas:1305`).
pub const RS_ARRAY_TO_ARRAY_ASSIGNMENT: &str = "Array to array assignment";
/// `RsOleAutomationCall` (`JvResources.pas:1300`).
pub const RS_OLE_AUTOMATION_CALL: &str = "Ole automation call";
/// `RsEUnknownRecordType` (`JvResources.pas:1303`).
pub const RS_UNKNOWN_RECORD_TYPE: &str = "Unknown RecordType";
/// `RsERangeCheckError` (`JvResources.pas:1304`).
pub const RS_RANGE_CHECK_ERROR: &str = "Range check error";
/// `RsESorryForOneDimensionalArraysOnly` (`JvResources.pas:106`), raised as
/// an `EJVCLException` by the array helpers for a multi-dimensional array in
/// a one-dimensional context (`JvInterpreter.pas:2300`).
pub const RS_SORRY_FOR_ONE_DIMENSIONAL_ARRAYS_ONLY: &str = "Sorry, for one-dimensional arrays only";

/// The English text of an error code (`LoadStr2`, `JvInterpreter.pas:1449`;
/// the texts of `JvResources.pas:1317-1379`). An unknown code gives `''`, as
/// `LoadStr2` leaves `Result` empty for an ID the table does not hold.
pub fn message_text(code: i32) -> &'static str {
    match code {
        1 => "Unknown error",
        2 => "Internal interpreter error: %s",
        3 => "User break",
        4 => "Re-raising an exception only allowed in exception handler",
        5 => "Error in unit '%0:s' on line %1:d : %2:s",
        6 => "External error in unit '%0:s' on line %1:d : %2:s",
        7 => "Access denied to '%s'",
        8 => "Expression is too complex - overflow",
        31 => "Record '%s' not defined",
        52 => "Stack overflow",
        56 => "Unit '%s' not found",
        57 => "Event '%s' not registered",
        58 => "DFM '%s' not found",
        101 => "Error in remark",
        103 => "%0:s expected but %1:s found",
        104 => "Undeclared Identifier '%s'",
        105 => "Type of expression must be boolean",
        106 => "Class type required",
        107 => " not allowed before else",
        108 => "Type of expression must be integer",
        109 => "Record, object or class type required",
        110 => "Missing operator or semicolon",
        111 => "Identifier redeclared: '%s'",
        171 => "Array index out of bounds",
        172 => "Too many array bounds",
        173 => "Not enough array bounds",
        174 => "Invalid array dimension",
        175 => "Invalid array range",
        176 => "Array type required",
        53 => "Type mismatch",
        55 => "Function 'main' undefined",
        // The table of `JvInterpreterConst.pas:58-118` has no entry for 54
        // (`ieIntegerOverflow`), so `LoadStr2` leaves the message empty.
        181 => "Too many actual parameters",
        182 => "Not enough parameters",
        183 => "Incompatible types: '%0:s' and '%1:s'",
        184 => "Error loading library '%s'",
        185 => "Invalid type of argument in call to function '%s'",
        186 => "Invalid type of result in call to function '%s'",
        187 => "Can't get proc address for function '%s'",
        188 => "Invalid type of argument in call to function '%s'",
        189 => "Invalid type of result in call to function '%s'",
        190 => "Invalid calling convention for function '%s'",
        201 => "Calling '%0:s' failed: '%1:s'",
        301 => "Expression",
        302 => "Identifier",
        303 => "Declaration",
        304 => "End of File",
        305 => "Class Declaration",
        306 => "Integer Constant'",
        307 => "Integer Value",
        308 => "String Constant",
        309 => "Statement",
        401 => "Implementation of unit not found",
        402 => "Array and Record types are not allowed as procedure/function parameter",
        _ => "",
    }
}

/// The message `Format(LoadStr2(code), [name1, name2])` gives for the codes
/// the interpreter raises (`JvInterpreter.pas:1487`). The `%s` codes use
/// `name1`; `%0:s` and `%1:s` take `name1` and `name2`; `%1:d` takes a number
/// the caller formats itself (see [`format_error_pos`]).
pub fn format_message(code: i32, name1: &str, name2: &str) -> String {
    match code {
        2 => format!("Internal interpreter error: {name1}"),
        5 => format!("Error in unit '{name1}' on line {name2} : "),
        31 => format!("Record '{name1}' not defined"),
        56 => format!("Unit '{name1}' not found"),
        57 => format!("Event '{name1}' not registered"),
        58 => format!("DFM '{name1}' not found"),
        103 => format!("{name1} expected but {name2} found"),
        104 => format!("Undeclared Identifier '{name1}'"),
        111 => format!("Identifier redeclared: '{name1}'"),
        183 => format!("Incompatible types: '{name1}' and '{name2}'"),
        184..=190 => message_text(code).replacen("%s", name1, 1),
        201 => format!("Calling '{name1}' failed: '{name2}'"),
        _ => message_text(code).to_owned(),
    }
}

/// `Format(LoadStr2(ieErrorPos), [ErrUnitName, ErrLine, ErrMessage])`
/// (`UpdateExceptionPos.NoName`, `JvInterpreter.pas:5377`): the message an
/// error carries once its unit and line are known.
pub fn format_error_pos(unit: &str, line: i64, message: &str) -> String {
    format!("Error in unit '{unit}' on line {line} : {message}")
}

/// `Format(LoadStr2(ieExternal), [...])` (`JvInterpreter.pas:5393`): the
/// message a non-interpreter exception is reported with.
pub fn format_external_error_pos(unit: &str, line: i64, message: &str) -> String {
    format!("External error in unit '{unit}' on line {line} : {message}")
}

/// An interpreter error: the code, the source position and the formatted message.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Error {
    /// The `ie*` code.
    pub code: i32,
    /// 0-based byte offset into the source; -1 when the raiser had no position.
    pub pos: i64,
    pub message: String,
}

impl Error {
    pub fn new(code: i32, pos: i64, name1: &str, name2: &str) -> Self {
        Self {
            code,
            pos,
            message: format_message(code, name1, name2),
        }
    }
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.message)
    }
}

impl std::error::Error for Error {}

/// The port of `GetLineByPos` (`JvJCLUtils.pas:1358`): the number of `#13`
/// characters at or before `pos` (the Delphi routine reads `S[1..Pos]`, so with
/// 0-based offsets this counts the bytes strictly before `pos`); -1 when the
/// source is shorter than `pos`. A `LF`-only file therefore has one line, as
/// upstream.
pub fn get_line_by_pos(source: &[u8], pos: i64) -> i64 {
    if (source.len() as i64) < pos {
        return -1;
    }
    let mut count = 0i64;
    let mut i = 0i64;
    while i < pos {
        if source[i as usize] == b'\r' {
            count += 1;
        }
        i += 1;
    }
    count
}

/// The line of an error as `UpdateExceptionPos` computes it
/// (`JvInterpreter.pas:5376`): `GetLineByPos(source, pos) + BaseErrLine + 1`
/// with `BaseErrLine` 0. The first line has number 1.
pub fn line_of(source: &[u8], pos: i64) -> i64 {
    get_line_by_pos(source, pos) + 1
}
