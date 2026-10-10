// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

// Ported from xEdit: External/jvcl/jvcl/run/JvInterpreterParser.pas and
// External/jvcl/jvcl/run/JvInterpreter.pas (the parser front end) and
// xEdit/JvI/xejviScriptHost.pas (the uses resolution of the check)

//! The JvInterpreter Pascal front end: the tokenizer of
//! `JvInterpreterParser.pas` ported as-is and the grammar of
//! `JvInterpreter.pas` as an AST builder.
//!
//! Module map:
//!
//! * [`interpreter_parser`]: the tokenizer, the token kinds and the
//!   operator priority table, byte for byte after the Pascal.
//! * [`interpreter`]: the grammar -- expressions, statements, routine
//!   headers, `uses`, the unit-file grammar -- with [`parse`] as the entry
//!   point, and [`interpreter::parse_compile`], the acceptance of
//!   `Compile` (routine bodies scanned for their balanced `end`, not
//!   parsed) that `script check` reports.
//! * [`ast`]: the syntax tree and the lowering interface the evaluator
//!   (phase 6 step 3) builds on; its module doc is the contract.
//! * [`error`]: the `ie*` codes, the English message texts of
//!   `JvResources.pas` and the `GetLineByPos` line counting.
//! * [`check`]: checking a script with the units it uses resolved from the
//!   scripts folder, and the `uses` namespace stripping of the script host.
//!
//! The crate has no dependency on the session layer: it parses text into
//! trees and reports syntax errors; identifiers, units and types stay
//! unresolved.

pub mod ast;
pub mod check;
pub mod error;
pub mod interpreter;
pub mod interpreter_parser;

pub use error::Error;
pub use interpreter::parse;
