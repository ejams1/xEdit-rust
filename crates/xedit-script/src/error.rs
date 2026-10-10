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

/// The English text of an error code (`LoadStr2`, `JvInterpreter.pas:1449`;
/// the texts of `JvResources.pas:1317-1379`). An unknown code gives `''`, as
/// `LoadStr2` leaves `Result` empty for an ID the table does not hold.
pub fn message_text(code: i32) -> &'static str {
    match code {
        1 => "Unknown error",
        2 => "Internal interpreter error: %s",
        3 => "User break",
        5 => "Error in unit '%0:s' on line %1:d : %2:s",
        56 => "Unit '%s' not found",
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
        _ => "",
    }
}

/// The message `Format(LoadStr2(code), [name1, name2])` gives for the codes the
/// front end raises. The `%s` codes use `name1`; `%0:s` and `%1:s` take `name1`
/// and `name2`.
fn format_message(code: i32, name1: &str, name2: &str) -> String {
    match code {
        2 => format!("Internal interpreter error: {name1}"),
        5 => format!("Error in unit '{name1}' on line {name2} : "),
        56 => format!("Unit '{name1}' not found"),
        103 => format!("{name1} expected but {name2} found"),
        104 => format!("Undeclared Identifier '{name1}'"),
        111 => format!("Identifier redeclared: '{name1}'"),
        _ => message_text(code).to_owned(),
    }
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
