// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

// Ported from xEdit: External/jvcl/jvcl/run/JvInterpreterParser.pas and
// External/jvcl/jvcl/run/JvInterpreter.pas (the parser front end) and
// xEdit/JvI/xejviScriptHost.pas (the uses resolution of the check)

//! The JvInterpreter Pascal interpreter: the tokenizer of
//! `JvInterpreterParser.pas` ported as-is, the grammar of
//! `JvInterpreter.pas` as an AST builder, and the runtime that compiles and
//! executes the tree.
//!
//! Module map:
//!
//! * [`interpreter_parser`]: the tokenizer, the token kinds and the
//!   operator priority table, byte for byte after the Pascal.
//! * [`interpreter`]: the grammar -- expressions, statements, routine
//!   headers, `uses`, the unit-file grammar -- with [`parse`] as the entry
//!   point, and [`interpreter::parse_compile`], the acceptance of
//!   `Compile` (routine bodies scanned for their balanced `end`, not
//!   parsed) that `script check` reports; [`interpreter::parse_body`] is
//!   the `ExecFunction` re-parse of one routine body.
//! * [`ast`]: the syntax tree and the lowering interface the runtime
//!   builds on; its module doc is the contract.
//! * [`values`]: the `Variant` value model of `JvInterpreter.pas` -- the
//!   extended variant types, the conversion and copy helpers, the array
//!   record and the variant operators.
//! * [`adapter`]: `TJvInterpreterAdapter` -- the registration model in full
//!   and the `GetValue`/`SetValue`/`GetElement`/`SetElement` dispatch with
//!   the sorted identifier lists.
//! * [`eval`]: `TJvInterpreterExpression`/`Function`/`Unit` -- `compile`,
//!   `call_function`/`call_function_ex`, the `Interpret*` statements, the
//!   statement hook and `GetLastErrorLocation`.
//! * [`error`]: the `ie*` codes, the English message texts of
//!   `JvResources.pas` and the `GetLineByPos` line counting.
//! * [`check`]: checking a script with the units it uses resolved from the
//!   scripts folder, and the `uses` namespace stripping of the script host.
//!
//! The crate has no dependency on the session layer: it parses text into
//! trees and executes them against adapters; identifiers, units and types
//! resolve through the adapter model the host fills.

pub mod adapter;
pub mod ast;
pub mod check;
pub mod error;
pub mod eval;
pub mod interpreter;
pub mod interpreter_parser;
pub mod values;

pub use error::Error;
pub use eval::{ExecError, Interpreter};
pub use interpreter::parse;
