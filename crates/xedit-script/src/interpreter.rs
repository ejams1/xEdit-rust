// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

// Ported from xEdit: External/jvcl/jvcl/run/JvInterpreter.pas (the parser
// front end: InterpretUnit and the Interpret*/Expression/ReadArgs routines,
// ReadFunctionHeader, ParseDataType, InterpretVar/InterpretConst/InterpretType,
// InterpretUses and the unit-file grammar).

//! The syntax front end: the grammar of `JvInterpreter.pas` as an AST builder.
//!
//! The interpreter has no parse step of its own -- it executes while it
//! parses and records source positions of routine bodies for `ExecFunction`
//! to re-parse. This module keeps the grammar and the token choreography of
//! those routines (including their `Back`/`NextToken` sequences, so the
//! accepted token streams are the same) and builds [`crate::ast`] nodes
//! instead of evaluating.
//!
//! What the port does not do, on purpose:
//!
//! * nothing is resolved: identifiers, units and types are left to the
//!   evaluator and the script host (phase 6 steps 3 and 4), so the only errors
//!   here are syntax errors;
//! * `SkipStatement`/`SkipToEnd` (the interpreter's separate, looser grammar
//!   for the branches it does not execute at parse time) is not ported: every
//!   branch is parsed with the real grammar, which is stricter and never
//!   looser for a well-formed script;
//! * `InterpretType` understands only `class` and `record` (`:8141`); a type
//!   alias and an enum are accepted here because the declaration-only
//!   `xEditAPI.pas` of the corpus consists of them and of routine headers
//!   with directives and defaulted parameters (which upstream stores/forwards
//!   without looking at, `ReadParams` skips everything up to `;`/`)`). The
//!   interpreter never parses that file -- its script host answers the unit
//!   name with `unit xEditAPI; end.` -- so no behaviour of a running script
//!   changes;
//! * a `function`/`procedure` token inside a block is a local routine here
//!   ([`crate::ast::StmtNode::LocalRoutine`]); `InterpretStatement` rejects
//!   it, and no corpus script uses the form.

use crate::ast::*;
use crate::error::{
    Error, IE_ARRAY_BAD_RANGE, IE_CLASS_REQUIRED, IE_EXPECTED, IE_INTEGER_REQUIRED, IE_INTERNAL, IE_MISSING_OPERATOR,
    IE_TYPE_MISMATCH,
};
use crate::interpreter_parser::{
    PRIOR_AND, PRIOR_DIV, PRIOR_EQU, PRIOR_EQU_GREATER, PRIOR_EQU_LESS, PRIOR_GREATER, PRIOR_INT_DIV, PRIOR_LESS,
    PRIOR_MINUS, PRIOR_MOD, PRIOR_MUL, PRIOR_NOT_EQU, PRIOR_OR, PRIOR_PLUS, PRIOR_SHL, PRIOR_SHR, PRIOR_XOR, TT_AND,
    TT_ARRAY, TT_BEGIN, TT_BOOLEAN, TT_BREAK, TT_CASE, TT_CLASS, TT_COL, TT_COLON, TT_CONST, TT_CONTINUE, TT_DIV,
    TT_DO, TT_DOUBLE, TT_DOUBLE_POINT, TT_DOUBLE_QUOTE, TT_DOWNTO, TT_ELSE, TT_EMPTY, TT_END, TT_EQU, TT_EQU_GREATER,
    TT_EQU_LESS, TT_EXCEPT, TT_EXIT, TT_EXTERNAL, TT_FALSE, TT_FINALLY, TT_FOR, TT_FUNCTION, TT_GREATER, TT_IDENTIFIER,
    TT_IF, TT_IMPLEMENTATION, TT_IN, TT_INT_DIV, TT_INTEGER, TT_INTERFACE, TT_LB, TT_LESS, TT_LS, TT_MINUS, TT_MOD,
    TT_MUL, TT_NOT, TT_NOT_EQU, TT_OF, TT_ON, TT_OR, TT_PLUS, TT_POINT, TT_PRIVATE, TT_PROCEDURE, TT_PROGRAM,
    TT_PROTECTED, TT_PUBLIC, TT_PUBLISHED, TT_RAISE, TT_RB, TT_RECORD, TT_REPEAT, TT_RS, TT_SEMICOLON, TT_SHL, TT_SHR,
    TT_STRING, TT_THEN, TT_TO, TT_TRUE, TT_TRY, TT_TYPE, TT_UNIT, TT_UNKNOWN, TT_UNTIL, TT_USES, TT_VAR, TT_WHILE,
    TT_XOR, TTokenKind, Tokenizer, prior, token_kind,
};

// The `ir*` texts of JvResources.pas:301-309 (`ReadContext`), with upstream's
// stray quote in `irIntegerConstant` (`RsEInterpreter306 = 'Integer Constant''';`).
const IR_EXPRESSION: &str = "Expression";
const IR_IDENTIFIER: &str = "Identifier";
const IR_DECLARATION: &str = "Declaration";
const IR_END_OF_FILE: &str = "End of File";
const IR_INTEGER_VALUE: &str = "Integer Value";
const IR_STRING_CONSTANT: &str = "String Constant";
const IR_INTEGER_CONSTANT: &str = "Integer Constant'";
const IR_CLASS: &str = "Class Declaration";
const IR_STATEMENT: &str = "Statement";

/// Parses one source file the way `TJvInterpreterUnit.Compile` reads it
/// (`JvInterpreter.pas:8291`): the first token must be `unit` (or `program`,
/// which the interpreter's `TJvInterpreterProgram.Run` accepts for a
/// standalone program).
pub fn parse(source: &[u8]) -> Result<Module, Error> {
    let mut parser = Parser::new(source);
    parser.next_token()?;
    match parser.tok.kind {
        TT_UNIT => parser.parse_unit(),
        TT_PROGRAM => parser.parse_program(),
        _ => Err(parser.expected("'unit'")),
    }
}

/// Parses one source file the way `TJvInterpreterUnit.Compile`
/// (`JvInterpreter.pas:8291`) accepts it: everything of [`parse`] except that
/// a routine body is only scanned for its balanced `end` (`InterpretFunction`:
/// `FindToken(ttBegin); SkipToEnd`), so the returned routines have
/// `body: None` and `locals` empty. This is the acceptance `script check`
/// reports -- statement-level errors surface when a script runs, as they do in
/// the interpreter.
pub fn parse_compile(source: &[u8]) -> Result<Module, Error> {
    let mut parser = Parser::new(source);
    parser.compile = true;
    parser.next_token()?;
    match parser.tok.kind {
        TT_UNIT => parser.parse_unit(),
        _ => Err(parser.expected("'unit'")),
    }
}

/// Parses one routine body the way `ExecFunction` re-parses it at call time
/// (`JvInterpreter.pas:8412`: `CurPos := Fun.PosBeg; NextToken; InFunction`):
/// from `pos` -- the byte after the routine header's `;` that
/// [`crate::ast::RoutineDecl::body_pos`] records -- the `var`/`const`
/// sections of `InFunction` (`:6569`) and the `begin ... end` block. The
/// compile acceptance of [`parse_compile`] only scans a body for its balanced
/// `end` (`InterpretFunction`: `FindToken(ttBegin); SkipToEnd`), so a
/// statement-level error of a routine surfaces here, when the routine first
/// runs, as upstream.
pub fn parse_body(source: &[u8], pos: usize) -> Result<RoutineBody, Error> {
    let mut parser = Parser::new(source);
    parser.section = Section::Implementation;
    parser.lexer.set_pos(pos);
    parser.next_token()?;
    let mut locals = Vec::new();
    loop {
        match parser.tok.kind {
            TT_VAR => {
                let groups = parser.parse_var_section_fields()?;
                locals.extend(groups.into_iter().map(LocalDecl::Var));
            }
            TT_CONST => {
                let decls = parser.parse_const_section()?;
                locals.extend(decls.into_iter().map(LocalDecl::Const));
            }
            TT_BEGIN => break,
            // `InFunction` reads nothing else between the header and `begin`
            // (`:6573`), even where the compile pass's `FindToken(ttBegin)`
            // would have skipped a directive.
            _ => return Err(parser.expected("'begin'")),
        }
        parser.next_token()?;
    }
    let block = parser.parse_begin_block()?;
    Ok(RoutineBody { locals, block })
}

struct Tok {
    kind: TTokenKind,
    text: Vec<u8>,
    start: usize,
    end: usize,
}

/// The parser state of `TJvInterpreterExpression` (`JvInterpreter.pas:846`):
/// the current token, the previous token's kind (`FPrevTTyp`), the one-token
/// `Back` (`FBacked`) and `FAllowAssignment`.
struct Parser<'a> {
    lexer: Tokenizer<'a>,
    tok: Tok,
    prev_kind: TTokenKind,
    prev_end: usize,
    backed: bool,
    allow_assignment: bool,
    section: Section,
    /// `true` for [`parse_compile`]: routine bodies are scanned for their
    /// balanced `end` (`InterpretFunction`'s `SkipToEnd`), not parsed.
    compile: bool,
}

impl<'a> Parser<'a> {
    fn new(source: &'a [u8]) -> Self {
        Self {
            lexer: Tokenizer::new(source),
            tok: Tok {
                kind: TT_EMPTY,
                text: Vec::new(),
                start: 0,
                end: 0,
            },
            prev_kind: TT_EMPTY,
            prev_end: 0,
            backed: false,
            allow_assignment: true,
            section: Section::None,
            compile: false,
        }
    }

    fn text_string(&self) -> String {
        String::from_utf8_lossy(&self.tok.text).into_owned()
    }

    /// The value of a `ttString` token: `Copy(TokenStr, 2, Length - 2)`
    /// (`JvInterpreter.pas:5592`).
    fn string_value(&self) -> Vec<u8> {
        let text = &self.tok.text;
        if text.len() < 2 {
            Vec::new()
        } else {
            text[1..text.len() - 1].to_vec()
        }
    }

    /// `NextToken` (`JvInterpreter.pas:5530`) with `ParseToken` (`:5544`): the
    /// `Back` flag makes the call a no-op; the integer and double conversions
    /// raise the errors `ParseToken` raises (the values themselves are the
    /// evaluator's).
    fn next_token(&mut self) -> Result<(), Error> {
        if self.backed {
            self.backed = false;
            return Ok(());
        }
        self.prev_kind = self.tok.kind;
        self.prev_end = self.tok.end;
        let raw = self.lexer.token()?;
        let kind = token_kind(&raw.text);
        match kind {
            TT_INTEGER if !delphi_val_ok(&raw.text) => {
                return Err(Error::new(IE_INTEGER_REQUIRED, raw.end as i64, "", ""));
            }
            TT_DOUBLE if !delphi_text_to_float_ok(&raw.text) => {
                return Err(Error::new(IE_INTERNAL, -1, "", ""));
            }
            _ => {}
        }
        self.tok = Tok {
            kind,
            text: raw.text,
            start: raw.start,
            end: raw.end,
        };
        Ok(())
    }

    /// `Back` (`JvInterpreter.pas:5607`).
    fn back(&mut self) {
        self.backed = true;
    }

    /// The state the `interface` path of `InterpretFunction` leaves
    /// (`CurPos := FPosBeg; FTTyp := LastTTyp`): the current token is the
    /// routine header's `;` again, and the next `NextToken` re-reads the token
    /// after it.
    fn rewind_semicolon(&mut self, semicolon_end: usize) {
        self.lexer.set_pos(semicolon_end);
        self.tok = Tok {
            kind: TT_SEMICOLON,
            text: b";".to_vec(),
            start: semicolon_end.saturating_sub(1),
            end: semicolon_end,
        };
        self.backed = false;
    }

    /// `ErrorExpected` (`JvInterpreter.pas:5448`).
    fn expected(&self, exp: &str) -> Error {
        let text = self.text_string();
        if !text.is_empty() {
            Error::new(IE_EXPECTED, self.tok.start as i64, exp, &format!("'{text}'"))
        } else {
            Error::new(IE_EXPECTED, self.tok.start as i64, exp, IR_END_OF_FILE)
        }
    }

    /// `FindToken` (`JvInterpreter.pas:6848`).
    fn find_token(&mut self, kind: TTokenKind) -> Result<(), Error> {
        while self.tok.kind != kind && self.tok.kind != TT_EMPTY {
            self.next_token()?;
        }
        if self.tok.kind == TT_EMPTY {
            return Err(self.expected("'end'"));
        }
        Ok(())
    }

    /// `SkipToEnd` (`JvInterpreter.pas:6763`): on exit the current token is
    /// the one after the `end` that balances the open block.
    fn skip_to_end(&mut self) -> Result<(), Error> {
        loop {
            self.next_token()?;
            if self.tok.kind == TT_END {
                self.next_token()?;
                break;
            } else if matches!(self.tok.kind, TT_BEGIN | TT_TRY | TT_CASE) {
                self.skip_to_end()?;
            } else if self.tok.kind == TT_EMPTY {
                return Err(self.expected("'end'"));
            } else if self.tok.kind == TT_DOUBLE_QUOTE {
                self.next_token()?;
            } else {
                self.skip_statement()?;
            }
            if self.tok.kind == TT_END {
                self.next_token()?;
                break;
            }
        }
        Ok(())
    }

    /// `SkipToUntil` (`JvInterpreter.pas:6794`).
    fn skip_to_until(&mut self) -> Result<(), Error> {
        loop {
            self.next_token()?;
            if self.tok.kind == TT_UNTIL {
                self.next_token()?;
                break;
            } else if self.tok.kind == TT_EMPTY {
                return Err(self.expected("'until'"));
            } else {
                self.skip_statement()?;
            }
            if self.tok.kind == TT_UNTIL {
                self.next_token()?;
                break;
            }
        }
        Ok(())
    }

    /// `SkipIdentifier` (`JvInterpreter.pas:6819`): scans over a statement
    /// without parsing it.
    fn skip_identifier(&mut self) -> Result<(), Error> {
        loop {
            match self.tok.kind {
                TT_EMPTY => return Err(self.expected("'end'")),
                kind if (TT_IDENTIFIER..=TT_BOOLEAN).contains(&kind)
                    || matches!(
                        kind,
                        TT_LB | TT_RB | TT_COL | TT_POINT | TT_LS | TT_RS | TT_DOUBLE_QUOTE | TT_TRUE | TT_FALSE
                    )
                    || (TT_NOT..=TT_XOR).contains(&kind) =>
                {
                    self.next_token()?;
                }
                TT_SEMICOLON | TT_END | TT_ELSE | TT_UNTIL | TT_FINALLY | TT_EXCEPT | TT_DO | TT_OF => break,
                TT_COLON => {
                    // `case` or assignment.
                    self.next_token()?;
                    if self.tok.kind != TT_EQU {
                        self.back();
                        break;
                    }
                }
                _ => return Err(self.expected(IR_EXPRESSION)),
            }
        }
        Ok(())
    }

    /// `SkipStatement` (`JvInterpreter.pas:6694`): the interpreter's separate,
    /// looser grammar for the branches a parse-time evaluation does not walk.
    fn skip_statement(&mut self) -> Result<(), Error> {
        match self.tok.kind {
            TT_EMPTY => return Err(self.expected("'end'")),
            TT_IDENTIFIER => self.skip_identifier()?,
            TT_SEMICOLON | TT_END => self.next_token()?,
            TT_IF => {
                self.find_token(TT_THEN)?;
                self.next_token()?;
                self.skip_statement()?;
                if self.tok.kind == TT_ELSE {
                    self.next_token()?;
                    self.skip_statement()?;
                }
                return Ok(());
            }
            TT_ELSE => return Ok(()),
            TT_WHILE | TT_FOR => {
                self.find_token(TT_DO)?;
                self.next_token()?;
                self.skip_statement()?;
                return Ok(());
            }
            TT_REPEAT => {
                self.skip_to_until()?;
                self.skip_identifier()?;
                return Ok(());
            }
            TT_BREAK | TT_CONTINUE => self.next_token()?,
            TT_BEGIN | TT_TRY => {
                self.skip_to_end()?;
                return Ok(());
            }
            TT_FUNCTION | TT_PROCEDURE => return Err(self.expected("'end'")),
            TT_RAISE => {
                self.next_token()?;
                self.skip_identifier()?;
            }
            TT_EXIT => self.next_token()?,
            TT_CASE => {
                self.skip_to_end()?;
                return Ok(());
            }
            _ => {}
        }
        Ok(())
    }

    /// `Expression2` (`JvInterpreter.pas:5888`) at the grammar level: the same
    /// expression with `FAllowAssignment` off (its type checks are runtime).
    fn expression2(&mut self) -> Result<Expr, Error> {
        let old = self.allow_assignment;
        self.allow_assignment = false;
        let result = self.expression1();
        self.allow_assignment = old;
        result
    }

    /// `Expression1` (`JvInterpreter.pas:5626`): evaluates `Expression` and
    /// pops the top of the expression stack.
    fn expression1(&mut self) -> Result<Expr, Error> {
        let mut stack: Vec<Expr> = Vec::new();
        let _ = self.expression(TT_UNKNOWN, &mut stack)?;
        match stack.pop() {
            Some(value) => Ok(value),
            None => Err(Error::new(IE_INTERNAL, -1, "", "")),
        }
    }

    /// The operand a recursive `Expression` call must give: upstream evaluates
    /// an empty variant there, which ends as a variant error, reported as
    /// `ieTypeMismatch` by the `Expression1` handler (`:5883`).
    fn required(&self, value: Option<Expr>) -> Result<Expr, Error> {
        value.ok_or_else(|| Error::new(IE_TYPE_MISMATCH, self.tok.end as i64, "", ""))
    }

    /// One operator case of `Expression`: on a priority `>` the operator is
    /// applied to the popped operand and the right side is the recursive
    /// call (this keeps the associativity of the interpreter's stack); the
    /// return is `false` when the priority check fails and the caller exits.
    fn operator_step(
        &mut self,
        op: BinOp,
        op_prior: i32,
        op_kind: TTokenKind,
        stack: &mut Vec<Expr>,
        result: &mut Option<Expr>,
    ) -> Result<bool, Error> {
        if op_prior > prior(op_kind) {
            let left = match stack.pop() {
                Some(left) => left,
                None => return Err(Error::new(IE_INTERNAL, -1, "", "")),
            };
            let token_kind = self.tok.kind;
            let value = self.expression(token_kind, stack)?;
            let right = self.required(value)?;
            let span = Span::new(left.span.start, self.prev_end);
            *result = Some(Expr {
                span,
                node: ExprNode::Binary {
                    op,
                    left: Box::new(left),
                    right: Box::new(right),
                },
            });
            Ok(true)
        } else {
            Ok(false)
        }
    }

    /// `Expression` (`JvInterpreter.pas:5649`): `None` is the interpreter's
    /// `Unassigned` result.
    fn expression(&mut self, op_kind: TTokenKind, stack: &mut Vec<Expr>) -> Result<Option<Expr>, Error> {
        let mut result: Option<Expr> = None;
        if op_kind != TT_UNKNOWN {
            self.next_token()?;
        }
        let entry_kind = self.tok.kind;
        loop {
            match self.tok.kind {
                TT_INTEGER | TT_DOUBLE | TT_FALSE | TT_TRUE | TT_IDENTIFIER => {
                    if self.tok.kind == TT_IDENTIFIER {
                        result = Some(self.internal_get_value(None)?);
                    } else {
                        let start = self.tok.start;
                        let node = match self.tok.kind {
                            TT_INTEGER => ExprNode::Int(self.tok.text.clone()),
                            TT_DOUBLE => ExprNode::Float(self.tok.text.clone()),
                            TT_FALSE => ExprNode::Bool(false),
                            _ => ExprNode::Bool(true),
                        };
                        result = Some(Expr {
                            span: Span::new(start, self.tok.end),
                            node,
                        });
                    }
                    self.next_token()?;
                    if matches!(
                        self.tok.kind,
                        TT_INTEGER | TT_DOUBLE | TT_STRING | TT_FALSE | TT_TRUE | TT_IDENTIFIER
                    ) {
                        return Err(Error::new(IE_MISSING_OPERATOR, self.tok.end as i64, "", ""));
                    }
                    if prior(self.tok.kind) < prior(op_kind) {
                        return Ok(result);
                    }
                }
                TT_STRING => {
                    // Adjacent string constants concatenate (`:5676`).
                    let start = self.tok.start;
                    let mut bytes = Vec::new();
                    loop {
                        bytes.extend_from_slice(&self.string_value());
                        self.next_token()?;
                        if matches!(
                            self.tok.kind,
                            TT_INTEGER | TT_DOUBLE | TT_FALSE | TT_TRUE | TT_IDENTIFIER
                        ) {
                            return Err(Error::new(IE_MISSING_OPERATOR, self.tok.end as i64, "", ""));
                        }
                        if self.tok.kind != TT_STRING {
                            break;
                        }
                    }
                    result = Some(Expr {
                        span: Span::new(start, self.prev_end),
                        node: ExprNode::Str(bytes),
                    });
                    if prior(self.tok.kind) < prior(op_kind) {
                        return Ok(result);
                    }
                }
                TT_SHL => {
                    if !self.operator_step(BinOp::Shl, PRIOR_SHL, op_kind, stack, &mut result)? {
                        return Ok(result);
                    }
                }
                TT_SHR => {
                    if !self.operator_step(BinOp::Shr, PRIOR_SHR, op_kind, stack, &mut result)? {
                        return Ok(result);
                    }
                }
                TT_XOR => {
                    if !self.operator_step(BinOp::Xor, PRIOR_XOR, op_kind, stack, &mut result)? {
                        return Ok(result);
                    }
                }
                TT_MUL => {
                    if !self.operator_step(BinOp::Mul, PRIOR_MUL, op_kind, stack, &mut result)? {
                        return Ok(result);
                    }
                }
                TT_PLUS => {
                    // Unary plus binds like `not` (the highest priority).
                    if !matches!(
                        self.prev_kind,
                        TT_INTEGER | TT_DOUBLE | TT_STRING | TT_FALSE | TT_TRUE | TT_IDENTIFIER | TT_RB | TT_RS
                    ) {
                        let start = self.tok.start;
                        let value = self.expression(TT_NOT, stack)?;
                        let operand = self.required(value)?;
                        result = Some(Expr {
                            span: Span::new(start, self.prev_end),
                            node: ExprNode::Unary {
                                op: UnOp::Plus,
                                operand: Box::new(operand),
                            },
                        });
                    } else if !self.operator_step(BinOp::Plus, PRIOR_PLUS, op_kind, stack, &mut result)? {
                        return Ok(result);
                    }
                }
                TT_MINUS => {
                    if !matches!(
                        self.prev_kind,
                        TT_INTEGER | TT_DOUBLE | TT_STRING | TT_FALSE | TT_TRUE | TT_IDENTIFIER | TT_RB | TT_RS
                    ) {
                        let start = self.tok.start;
                        let value = self.expression(TT_NOT, stack)?;
                        let operand = self.required(value)?;
                        result = Some(Expr {
                            span: Span::new(start, self.prev_end),
                            node: ExprNode::Unary {
                                op: UnOp::Minus,
                                operand: Box::new(operand),
                            },
                        });
                    } else if !self.operator_step(BinOp::Minus, PRIOR_MINUS, op_kind, stack, &mut result)? {
                        return Ok(result);
                    }
                }
                TT_DIV => {
                    if !self.operator_step(BinOp::Div, PRIOR_DIV, op_kind, stack, &mut result)? {
                        return Ok(result);
                    }
                }
                TT_INT_DIV => {
                    if !self.operator_step(BinOp::IntDiv, PRIOR_INT_DIV, op_kind, stack, &mut result)? {
                        return Ok(result);
                    }
                }
                TT_MOD => {
                    if !self.operator_step(BinOp::Mod, PRIOR_MOD, op_kind, stack, &mut result)? {
                        return Ok(result);
                    }
                }
                TT_OR => {
                    if !self.operator_step(BinOp::Or, PRIOR_OR, op_kind, stack, &mut result)? {
                        return Ok(result);
                    }
                }
                TT_AND => {
                    if !self.operator_step(BinOp::And, PRIOR_AND, op_kind, stack, &mut result)? {
                        return Ok(result);
                    }
                }
                TT_NOT => {
                    // 'Not' has the highest priority, so no check (`:5782`).
                    let start = self.tok.start;
                    let value = self.expression(TT_NOT, stack)?;
                    let operand = self.required(value)?;
                    result = Some(Expr {
                        span: Span::new(start, self.prev_end),
                        node: ExprNode::Unary {
                            op: UnOp::Not,
                            operand: Box::new(operand),
                        },
                    });
                }
                TT_EQU => {
                    if !self.operator_step(BinOp::Equ, PRIOR_EQU, op_kind, stack, &mut result)? {
                        return Ok(result);
                    }
                }
                TT_NOT_EQU => {
                    if !self.operator_step(BinOp::NotEqu, PRIOR_NOT_EQU, op_kind, stack, &mut result)? {
                        return Ok(result);
                    }
                }
                TT_GREATER => {
                    if !self.operator_step(BinOp::Greater, PRIOR_GREATER, op_kind, stack, &mut result)? {
                        return Ok(result);
                    }
                }
                TT_LESS => {
                    if !self.operator_step(BinOp::Less, PRIOR_LESS, op_kind, stack, &mut result)? {
                        return Ok(result);
                    }
                }
                TT_EQU_LESS => {
                    if !self.operator_step(BinOp::EquLess, PRIOR_EQU_LESS, op_kind, stack, &mut result)? {
                        return Ok(result);
                    }
                }
                TT_EQU_GREATER => {
                    if !self.operator_step(BinOp::EquGreater, PRIOR_EQU_GREATER, op_kind, stack, &mut result)? {
                        return Ok(result);
                    }
                }
                TT_LB => {
                    let value = self.expression(TT_LB, stack)?;
                    result = Some(self.required(value)?);
                    if self.tok.kind != TT_RB {
                        return Err(self.expected("')'"));
                    }
                    self.next_token()?;
                }
                TT_LS => {
                    let start = self.tok.start;
                    self.next_token()?;
                    let items = self.set_expression1()?;
                    if self.tok.kind != TT_RS {
                        return Err(self.expected("']'"));
                    }
                    self.next_token()?;
                    result = Some(Expr {
                        span: Span::new(start, self.prev_end),
                        node: ExprNode::Set { items },
                    });
                }
                TT_RB | TT_RS => {
                    if result.is_none() && entry_kind != TT_IDENTIFIER {
                        return Err(self.expected(IR_EXPRESSION));
                    }
                    return Ok(result);
                }
                _ => {
                    if result.is_none() && entry_kind != TT_IDENTIFIER {
                        return Err(self.expected(IR_EXPRESSION));
                    }
                    return Ok(result);
                }
            }
            match &result {
                Some(value) => stack.push(value.clone()),
                None => return Err(Error::new(IE_INTERNAL, -1, "", "")),
            }
        }
    }

    /// `SetExpression1` (`JvInterpreter.pas:5927`): the items of `[...]`,
    /// identifiers or integers only.
    fn set_expression1(&mut self) -> Result<Vec<Expr>, Error> {
        let mut items = Vec::new();
        while let TT_IDENTIFIER | TT_INTEGER = self.tok.kind {
            if self.tok.kind == TT_INTEGER {
                items.push(Expr {
                    span: Span::new(self.tok.start, self.tok.end),
                    node: ExprNode::Int(self.tok.text.clone()),
                });
            } else {
                items.push(self.internal_get_value(None)?);
            }
            self.next_token()?; // skip ','
            if self.tok.kind == TT_COL {
                self.next_token()?;
            } else if self.tok.kind == TT_RS {
                break;
            } else {
                return Err(self.expected("']'"));
            }
        }
        Ok(items)
    }

    /// `ReadArgs` (`JvInterpreter.pas:5965`).
    fn read_args(&mut self) -> Result<Vec<Arg>, Error> {
        let sk = if self.tok.kind == TT_LB { TT_RB } else { TT_RS };
        let mut args = Vec::new();
        self.next_token()?;
        let arg_start = self.tok.start;
        let name = if self.tok.kind == TT_IDENTIFIER {
            Some(self.text_string())
        } else {
            None
        };
        let value = if self.tok.kind == TT_LS {
            self.read_open_array()?
        } else if self.tok.kind == TT_RB {
            // The C-style `Name()`, an empty argument list (`:6018`).
            self.next_token()?;
            return Ok(args);
        } else {
            ArgValue::Expr(self.expression1()?)
        };
        args.push(Arg {
            name,
            span: Span::new(arg_start, self.prev_end),
            value,
        });
        while self.tok.kind == TT_COL {
            self.next_token()?;
            let arg_start = self.tok.start;
            let name = if self.tok.kind == TT_IDENTIFIER {
                Some(self.text_string())
            } else {
                None
            };
            let value = if self.tok.kind == TT_LS {
                self.read_open_array()?
            } else {
                ArgValue::Expr(self.expression1()?)
            };
            args.push(Arg {
                name,
                span: Span::new(arg_start, self.prev_end),
                value,
            });
        }
        if self.tok.kind != sk {
            return Err(self.expected(if sk == TT_RB { "')'" } else { "']'" }));
        }
        self.next_token()?;
        Ok(args)
    }

    /// `ReadArgs.ReadOpenArray` (`JvInterpreter.pas:5967`): the `[a, b]` form
    /// in an argument list; a range is not accepted here.
    fn read_open_array(&mut self) -> Result<ArgValue, Error> {
        self.next_token()?;
        let mut items = vec![self.expression1()?];
        while self.tok.kind == TT_COL {
            self.next_token()?;
            items.push(self.expression1()?);
        }
        if self.tok.kind != TT_RS {
            return Err(self.expected("']'"));
        }
        self.next_token()?;
        Ok(ArgValue::OpenArray(items))
    }

    /// `InternalGetValue` (`JvInterpreter.pas:6053`): a name with its
    /// arguments and its `.member` chain. `base` is the value a member is
    /// fetched through.
    fn internal_get_value(&mut self, base: Option<Expr>) -> Result<Expr, Error> {
        if self.tok.kind != TT_IDENTIFIER {
            return Err(self.expected(IR_IDENTIFIER));
        }
        let start = self.tok.start;
        let name = self.text_string();
        self.internal_get_value_body(name, start, base)
    }

    /// `InternalGetValue` (`JvInterpreter.pas:6053`) after its
    /// `Identifier := Token`: the name is read and the current token is the
    /// one after it. The leading `NextToken` advances, or absorbs the pending
    /// `Back` of `InterpretIdentifier` without advancing -- the same
    /// choreography as upstream, whose `Identifier := Token` reads the
    /// still-current name there.
    fn internal_get_value_body(&mut self, name: String, start: usize, base: Option<Expr>) -> Result<Expr, Error> {
        self.next_token()?;
        let indexed = self.tok.kind == TT_LS;
        let args = if matches!(self.tok.kind, TT_LB | TT_LS) {
            self.read_args()?
        } else {
            Vec::new()
        };
        let node = match base {
            None => ExprNode::Ident { name, args, indexed },
            Some(base) => ExprNode::Member {
                base: Box::new(base),
                name,
                args,
                indexed,
            },
        };
        let expr = Expr {
            span: Span::new(start, self.prev_end),
            node,
        };
        if self.tok.kind == TT_COLON && self.allow_assignment {
            // The `:` of an assignment target (`:6089`): back up so the
            // statement level sees it.
            self.back();
            return Ok(expr);
        }
        if self.tok.kind == TT_POINT {
            self.next_token()?;
            if self.tok.kind != TT_IDENTIFIER {
                return Err(self.expected(IR_IDENTIFIER));
            }
            // The recursive call ends with its own Back, so the outer
            // NextToken/Back of upstream (`:6152`, `:6155`) are no-ops.
            return self.internal_get_value(Some(expr));
        }
        self.back();
        Ok(expr)
    }

    /// `InterpretIdentifier` (`JvInterpreter.pas:7015`): an assignment or an
    /// identifier statement.
    fn parse_identifier_statement(&mut self) -> Result<Stmt, Error> {
        let start = self.tok.start;
        let name = self.text_string();
        self.next_token()?;
        if self.tok.kind != TT_COLON {
            self.back();
            let target = self.internal_get_value_body(name, start, None)?;
            self.next_token()?;
            if self.tok.kind == TT_COLON {
                let value = self.parse_assignment_value()?;
                return Ok(Stmt {
                    span: Span::new(start, self.prev_end),
                    node: StmtNode::Assign { target, value },
                });
            }
            return Ok(Stmt {
                span: Span::new(start, self.prev_end),
                node: StmtNode::Expr(target),
            });
        }
        let target = Expr {
            span: Span::new(start, start + name.len()),
            node: ExprNode::Ident {
                name,
                args: Vec::new(),
                indexed: false,
            },
        };
        let value = self.parse_assignment_value()?;
        Ok(Stmt {
            span: Span::new(start, self.prev_end),
            node: StmtNode::Assign { target, value },
        })
    }

    /// The `:=` and the value of an assignment: entered with the current token
    /// on the `:`; the value is a full `Expression1`.
    fn parse_assignment_value(&mut self) -> Result<Expr, Error> {
        self.next_token()?;
        if self.tok.kind != TT_EQU {
            return Err(self.expected("'='"));
        }
        self.next_token()?;
        self.expression1()
    }

    /// `InterpretStatement` (`JvInterpreter.pas:6646`) with the statement
    /// terminators its identifier case checks.
    fn parse_statement(&mut self) -> Result<Stmt, Error> {
        let start = self.tok.start;
        let node = match self.tok.kind {
            TT_IDENTIFIER => {
                let stmt = self.parse_identifier_statement()?;
                if !matches!(
                    self.tok.kind,
                    TT_SEMICOLON | TT_END | TT_ELSE | TT_UNTIL | TT_FINALLY | TT_EXCEPT
                ) {
                    return Err(self.expected("';'"));
                }
                return Ok(stmt);
            }
            TT_BEGIN => StmtNode::Block(self.parse_begin_block()?),
            TT_IF => self.parse_if()?,
            TT_WHILE => self.parse_while()?,
            TT_REPEAT => self.parse_repeat()?,
            TT_FOR => self.parse_for()?,
            TT_BREAK => {
                self.next_token()?;
                StmtNode::Break
            }
            TT_CONTINUE => {
                self.next_token()?;
                StmtNode::Continue
            }
            TT_TRY => self.parse_try()?,
            TT_RAISE => self.parse_raise()?,
            TT_EXIT => {
                self.next_token()?;
                StmtNode::Exit
            }
            TT_CASE => self.parse_case()?,
            TT_FUNCTION | TT_PROCEDURE => StmtNode::LocalRoutine(Box::new(self.parse_routine()?)),
            TT_SEMICOLON | TT_END | TT_ELSE => {
                // The no-op statement arms of `InterpretStatement`
                // (`JvInterpreter.pas:6658-6667`): the token is not consumed.
                return Ok(Stmt {
                    span: Span::new(self.tok.start, self.tok.end),
                    node: StmtNode::Empty,
                });
            }
            _ => return Err(self.expected("';'")),
        };
        Ok(Stmt {
            span: Span::new(start, self.prev_end),
            node,
        })
    }

    /// `InterpretBegin` (`JvInterpreter.pas:7042`): entered with the current
    /// token on `begin`; consumes the matching `end`.
    fn parse_begin_block(&mut self) -> Result<Block, Error> {
        let start = self.tok.start;
        self.next_token()?;
        self.parse_block_body(start)
    }

    /// The body of `InterpretBegin` after its leading `NextToken`, entered
    /// with the current token on the first statement; consumes the `end`.
    fn parse_block_body(&mut self, start: usize) -> Result<Block, Error> {
        let mut stmts = Vec::new();
        loop {
            match self.tok.kind {
                TT_END => {
                    self.next_token()?;
                    break;
                }
                TT_ELSE | TT_DO => return Err(self.expected(IR_STATEMENT)),
                TT_SEMICOLON => {
                    self.next_token()?;
                }
                kind if is_statement_start(kind) => stmts.push(self.parse_statement()?),
                _ => return Err(self.expected("'end'")),
            }
        }
        Ok(Block {
            span: Span::new(start, self.prev_end),
            stmts,
        })
    }

    /// `InterpretIf` (`JvInterpreter.pas:7073`).
    fn parse_if(&mut self) -> Result<StmtNode, Error> {
        self.next_token()?;
        let cond = self.expression2()?;
        if self.tok.kind != TT_THEN {
            return Err(self.expected("'then'"));
        }
        self.next_token()?;
        let then_branch = Box::new(self.parse_statement()?);
        let else_branch = if self.tok.kind == TT_ELSE {
            self.next_token()?;
            Some(Box::new(self.parse_statement()?))
        } else {
            None
        };
        Ok(StmtNode::If {
            cond,
            then_branch,
            else_branch,
        })
    }

    /// `InterpretWhile` (`JvInterpreter.pas:7113`).
    fn parse_while(&mut self) -> Result<StmtNode, Error> {
        self.next_token()?;
        let cond = self.expression1()?;
        if self.tok.kind != TT_DO {
            return Err(self.expected("'do'"));
        }
        self.next_token()?;
        let body = Box::new(self.parse_statement()?);
        Ok(StmtNode::While { cond, body })
    }

    /// `InterpretRepeat` (`JvInterpreter.pas:7154`).
    fn parse_repeat(&mut self) -> Result<StmtNode, Error> {
        let start = self.tok.start;
        let mut stmts = Vec::new();
        loop {
            self.next_token()?;
            match self.tok.kind {
                TT_ELSE | TT_DO => return Err(self.expected(IR_STATEMENT)),
                TT_SEMICOLON => {}
                kind if is_statement_start(kind) => stmts.push(self.parse_statement()?),
                TT_UNTIL => break,
                _ => return Err(self.expected("'until'")),
            }
        }
        self.next_token()?;
        let cond = self.expression1()?;
        Ok(StmtNode::Repeat {
            body: Block {
                span: Span::new(start, self.prev_end),
                stmts,
            },
            cond,
        })
    }

    /// `InterpretFor` (`JvInterpreter.pas:7203`).
    fn parse_for(&mut self) -> Result<StmtNode, Error> {
        self.next_token()?;
        if self.tok.kind != TT_IDENTIFIER {
            return Err(self.expected(IR_IDENTIFIER));
        }
        let var = self.text_string();
        self.next_token()?;
        if self.tok.kind != TT_COLON {
            return Err(self.expected("':'"));
        }
        self.next_token()?;
        if self.tok.kind != TT_EQU {
            return Err(self.expected("'='"));
        }
        self.next_token()?;
        let from = self.expression2()?;
        if self.tok.kind != TT_TO && self.tok.kind != TT_DOWNTO {
            return Err(self.expected("'to' or 'downto'"));
        }
        let down = self.tok.kind == TT_DOWNTO;
        self.next_token()?;
        let to = self.expression2()?;
        if self.tok.kind != TT_DO {
            return Err(self.expected("'do'"));
        }
        self.next_token()?;
        let body = Box::new(self.parse_statement()?);
        Ok(StmtNode::For {
            var,
            from,
            to,
            down,
            body,
        })
    }

    /// `InterpretCase` (`JvInterpreter.pas:7283`).
    fn parse_case(&mut self) -> Result<StmtNode, Error> {
        self.next_token()?;
        let selector = self.expression2()?;
        if self.tok.kind != TT_OF {
            return Err(self.expected("'of'"));
        }
        let mut arms = Vec::new();
        let mut else_arm: Option<Box<Stmt>> = None;
        loop {
            self.next_token()?;
            match self.tok.kind {
                TT_IDENTIFIER | TT_INTEGER => arms.push(self.parse_case_arm()?),
                TT_ELSE => {
                    self.next_token()?;
                    else_arm = Some(Box::new(self.parse_statement()?));
                    if self.tok.kind == TT_SEMICOLON {
                        self.next_token()?;
                    }
                    if self.tok.kind != TT_END {
                        return Err(self.expected("'end'"));
                    }
                    self.next_token()?;
                    break;
                }
                TT_END => {
                    self.next_token()?;
                    break;
                }
                _ => return Err(self.expected("'end'")),
            }
        }
        Ok(StmtNode::Case {
            selector,
            arms,
            else_arm,
        })
    }

    fn parse_case_arm(&mut self) -> Result<CaseArm, Error> {
        let start = self.tok.start;
        let mut labels = Vec::new();
        loop {
            let lo = self.expression2()?;
            let hi = if self.tok.kind == TT_DOUBLE_POINT {
                self.next_token()?;
                Some(self.expression2()?)
            } else {
                None
            };
            let span = Span::new(lo.span.start, self.prev_end);
            labels.push(CaseLabel { lo, hi, span });
            if self.tok.kind == TT_COL {
                self.next_token()?;
                continue;
            }
            break;
        }
        if self.tok.kind != TT_COLON {
            return Err(self.expected("':'"));
        }
        self.next_token()?;
        let body = Box::new(self.parse_statement()?);
        Ok(CaseArm {
            labels,
            body,
            span: Span::new(start, self.prev_end),
        })
    }

    /// `InterpretTry` (`JvInterpreter.pas:7435`).
    fn parse_try(&mut self) -> Result<StmtNode, Error> {
        let start = self.tok.start;
        self.next_token()?;
        let mut stmts = Vec::new();
        loop {
            match self.tok.kind {
                TT_FINALLY | TT_EXCEPT => break,
                TT_SEMICOLON => {
                    self.next_token()?;
                }
                kind if is_statement_start(kind) => stmts.push(self.parse_statement()?),
                _ => return Err(self.expected("'finally'")),
            }
        }
        let body = Block {
            span: Span::new(start, self.prev_end),
            stmts,
        };
        let handler = if self.tok.kind == TT_FINALLY {
            self.next_token()?;
            Handler::Finally(self.parse_block_body(start)?)
        } else {
            self.parse_except()?
        };
        Ok(StmtNode::Try { body, handler })
    }

    /// `InterpretExcept` (`JvInterpreter.pas:7457`): entered with the current
    /// token on `except`; consumes the matching `end`.
    fn parse_except(&mut self) -> Result<Handler, Error> {
        let start = self.tok.start;
        self.next_token()?;
        if self.tok.kind != TT_ON {
            // The plain `except <statements> end` form (`:7553`).
            let body = self.parse_block_body(start)?;
            return Ok(Handler::Except {
                ons: Vec::new(),
                body: Some(body),
                else_arm: None,
            });
        }
        let mut ons = Vec::new();
        let mut else_arm = None;
        loop {
            // On1 (`:7463`).
            let on_start = self.tok.start;
            self.next_token()?;
            if self.tok.kind != TT_IDENTIFIER {
                return Err(self.expected(IR_IDENTIFIER));
            }
            let first = self.text_string();
            self.next_token()?;
            let (var, class) = if self.tok.kind == TT_COLON {
                self.next_token()?;
                if self.tok.kind != TT_IDENTIFIER {
                    return Err(self.expected(IR_IDENTIFIER));
                }
                let second = self.text_string();
                self.next_token()?;
                (Some(first), second)
            } else {
                (None, first)
            };
            if self.tok.kind != TT_DO {
                return Err(self.expected("'do'"));
            }
            self.next_token()?;
            let body = Box::new(self.parse_statement()?);
            ons.push(OnClause {
                var,
                class,
                body,
                span: Span::new(on_start, self.prev_end),
            });
            if self.tok.kind == TT_SEMICOLON {
                self.next_token()?;
            }
            match self.tok.kind {
                TT_ON => {}
                TT_ELSE => {
                    self.next_token()?;
                    else_arm = Some(Box::new(self.parse_statement()?));
                    if self.tok.kind == TT_SEMICOLON {
                        self.next_token()?;
                    }
                    if self.tok.kind != TT_END {
                        return Err(self.expected("'end'"));
                    }
                    self.next_token()?;
                    break;
                }
                TT_END => {
                    self.next_token()?;
                    break;
                }
                _ => return Err(self.expected("'end'")),
            }
        }
        Ok(Handler::Except {
            ons,
            body: None,
            else_arm,
        })
    }

    /// `InterpretRaise` (`JvInterpreter.pas:7655`).
    fn parse_raise(&mut self) -> Result<StmtNode, Error> {
        self.next_token()?;
        match self.tok.kind {
            TT_EMPTY | TT_SEMICOLON | TT_END | TT_BEGIN | TT_ELSE | TT_FINALLY | TT_EXCEPT => {
                Ok(StmtNode::Raise { value: None })
            }
            TT_IDENTIFIER => {
                let value = self.internal_get_value(None)?;
                Ok(StmtNode::Raise { value: Some(value) })
            }
            _ => Err(Error::new(IE_CLASS_REQUIRED, self.tok.start as i64, "", "")),
        }
    }

    /// `InterpretVar` (`JvInterpreter.pas:7369`) entered with the current
    /// token on `var` (`InterpretVar`'s own leading `NextToken` consumed it
    /// upstream). On exit the current token is the first token after the
    /// section with `Back` pending, as upstream leaves it.
    fn parse_var_section_fields(&mut self) -> Result<Vec<VarGroup>, Error> {
        self.next_token()?;
        self.parse_var_fields()
    }

    /// The statements of a `var` section, entered with the current token on
    /// the first name (``InterpretVar``'s inner `repeat`). At the top of every
    /// iteration a pending `Back` of the previous declaration is cleared, as
    /// upstream's loop-top `NextToken` clears it.
    fn parse_var_fields(&mut self) -> Result<Vec<VarGroup>, Error> {
        let mut groups = Vec::new();
        loop {
            if self.backed {
                self.backed = false;
            }
            if self.tok.kind != TT_IDENTIFIER {
                return Err(self.expected(IR_IDENTIFIER));
            }
            groups.push(self.parse_var_declaration()?);
            if self.tok.kind != TT_IDENTIFIER {
                break;
            }
        }
        Ok(groups)
    }

    /// One `a, b: T;` of a `var` section; on exit the current token is the
    /// first token after the `;` with `Back` pending (`:7401`).
    fn parse_var_declaration(&mut self) -> Result<VarGroup, Error> {
        let start = self.tok.start;
        let mut names = Vec::new();
        loop {
            if self.tok.kind != TT_IDENTIFIER {
                return Err(self.expected(IR_IDENTIFIER));
            }
            names.push(self.text_string());
            self.next_token()?;
            if self.tok.kind == TT_COL {
                self.next_token()?;
                continue;
            }
            break;
        }
        if self.tok.kind != TT_COLON {
            return Err(self.expected("':'"));
        }
        self.next_token()?;
        let ty = self.parse_data_type()?;
        self.next_token()?;
        if self.tok.kind != TT_SEMICOLON {
            return Err(self.expected("';'"));
        }
        self.next_token()?;
        self.back();
        Ok(VarGroup {
            names,
            ty,
            span: Span::new(start, self.prev_end),
        })
    }

    /// `InterpretConst` (`JvInterpreter.pas:7410`) entered with the current
    /// token on `const`; on exit the current token is the first token after
    /// the section with `Back` pending.
    fn parse_const_section(&mut self) -> Result<Vec<ConstDecl>, Error> {
        self.next_token()?;
        let mut decls = Vec::new();
        loop {
            if self.backed {
                self.backed = false;
            }
            if self.tok.kind != TT_IDENTIFIER {
                return Err(self.expected(IR_IDENTIFIER));
            }
            decls.push(self.parse_const_declaration()?);
            if self.tok.kind != TT_IDENTIFIER {
                break;
            }
        }
        Ok(decls)
    }

    /// One `Name = Value;` of a `const` section (`:7415`); on exit the
    /// current token is the first token after the `;` with `Back` pending.
    fn parse_const_declaration(&mut self) -> Result<ConstDecl, Error> {
        let start = self.tok.start;
        if self.tok.kind != TT_IDENTIFIER {
            return Err(self.expected(IR_IDENTIFIER));
        }
        let name = self.text_string();
        self.next_token()?;
        if self.tok.kind != TT_EQU {
            // Upstream passes `'='` unquoted here (`:7422`).
            return Err(self.expected("="));
        }
        self.next_token()?;
        let value = self.expression1()?;
        if self.tok.kind != TT_SEMICOLON {
            return Err(self.expected("';'"));
        }
        self.next_token()?;
        self.back();
        Ok(ConstDecl {
            name,
            value,
            span: Span::new(start, self.prev_end),
        })
    }

    /// `InterpretType` (`JvInterpreter.pas:8141`) with the extensions the
    /// module doc names: alias and enum; consumes consecutive declarations of
    /// one `type` section. On exit the current token is the first token after
    /// the section with `Back` pending.
    fn parse_type_section(&mut self) -> Result<Vec<TypeDecl>, Error> {
        self.next_token()?;
        if self.tok.kind != TT_IDENTIFIER {
            return Err(self.expected(IR_IDENTIFIER));
        }
        let mut decls = vec![self.parse_type_declaration()?];
        loop {
            self.next_token()?;
            if self.tok.kind != TT_IDENTIFIER {
                self.back();
                break;
            }
            decls.push(self.parse_type_declaration()?);
        }
        Ok(decls)
    }

    /// One `Name = ...;` of a `type` section; on exit the current token is the
    /// `;` (`:8145`).
    fn parse_type_declaration(&mut self) -> Result<TypeDecl, Error> {
        let start = self.tok.start;
        let name = self.text_string();
        self.next_token()?;
        if self.tok.kind != TT_EQU {
            return Err(self.expected("'='"));
        }
        self.next_token()?;
        let def = match self.tok.kind {
            TT_CLASS => self.parse_class()?,
            TT_RECORD => self.parse_record()?,
            TT_LB => {
                // `X = (a, b);` -- an enum (`xEditAPI.pas`).
                self.next_token()?;
                let mut values = Vec::new();
                loop {
                    if self.tok.kind != TT_IDENTIFIER {
                        return Err(self.expected(IR_IDENTIFIER));
                    }
                    values.push(self.text_string());
                    self.next_token()?;
                    if self.tok.kind == TT_COL {
                        self.next_token()?;
                        continue;
                    }
                    break;
                }
                if self.tok.kind != TT_RB {
                    return Err(self.expected("')'"));
                }
                self.next_token()?;
                if self.tok.kind != TT_SEMICOLON {
                    return Err(self.expected("';'"));
                }
                TypeDef::Enum { values }
            }
            TT_IDENTIFIER => {
                // `X = SomeType;` -- an alias (`xEditAPI.pas`).
                let ty = self.parse_data_type()?;
                self.next_token()?;
                if self.tok.kind != TT_SEMICOLON {
                    return Err(self.expected("';'"));
                }
                TypeDef::Alias(ty)
            }
            _ => return Err(self.expected(IR_CLASS)),
        };
        Ok(TypeDecl {
            name,
            def,
            span: Span::new(start, self.prev_end),
        })
    }

    /// `InterpretClass` (`JvInterpreter.pas:8164`): entered with the current
    /// token on `class`; on exit the current token is the `;`.
    fn parse_class(&mut self) -> Result<TypeDef, Error> {
        let start = self.tok.start;
        self.next_token()?;
        if self.tok.kind != TT_LB {
            return Err(self.expected("'('"));
        }
        self.next_token()?;
        if self.tok.kind != TT_IDENTIFIER {
            return Err(self.expected(IR_IDENTIFIER));
        }
        let parent = self.text_string();
        self.next_token()?;
        if self.tok.kind != TT_RB {
            return Err(self.expected("')'"));
        }
        let mut members = Vec::new();
        self.next_token()?;
        if self.tok.kind == TT_IDENTIFIER {
            // Fields may follow the class declaration directly (`:8184`).
            self.back();
            members.extend(self.parse_var_fields()?.into_iter().map(ClassMember::Field));
            self.next_token()?;
        }
        loop {
            match self.tok.kind {
                TT_EMPTY => return Err(self.expected("'end'")),
                TT_FUNCTION | TT_PROCEDURE => {
                    // An empty reading of the header (`:8195`).
                    members.push(ClassMember::Method(Box::new(self.read_function_header()?)));
                }
                TT_END => break,
                TT_PRIVATE | TT_PROTECTED | TT_PUBLIC | TT_PUBLISHED => {
                    self.next_token()?;
                    if self.tok.kind == TT_IDENTIFIER {
                        self.back();
                        members.extend(self.parse_var_fields()?.into_iter().map(ClassMember::Field));
                    }
                }
                _ => return Err(self.expected(IR_DECLARATION)),
            }
            self.next_token()?;
        }
        self.next_token()?;
        if self.tok.kind != TT_SEMICOLON {
            return Err(self.expected("';'"));
        }
        Ok(TypeDef::Class(ClassDef {
            parent,
            members,
            span: Span::new(start, self.prev_end),
        }))
    }

    /// `InterpretRecord` (`JvInterpreter.pas:8229`): entered with the current
    /// token on `record`; on exit the current token is the `;`.
    fn parse_record(&mut self) -> Result<TypeDef, Error> {
        let start = self.tok.start;
        self.next_token()?;
        let fields = self.parse_var_fields()?;
        self.next_token()?;
        if self.tok.kind != TT_END {
            return Err(self.expected("'end'"));
        }
        self.next_token()?;
        if self.tok.kind != TT_SEMICOLON {
            return Err(self.expected("';'"));
        }
        Ok(TypeDef::Record(RecordDef {
            fields,
            span: Span::new(start, self.prev_end),
        }))
    }

    /// `ParseDataType` (`JvInterpreter.pas:7689`): entered with the current
    /// token on the first token of the type; leaves it there for a named type
    /// and on the element type for an array, as upstream leaves it.
    fn parse_data_type(&mut self) -> Result<TypeRef, Error> {
        if self.tok.kind == TT_IDENTIFIER {
            return Ok(TypeRef::Named(self.text_string()));
        }
        if self.tok.kind != TT_ARRAY {
            return Err(self.expected(IR_IDENTIFIER));
        }
        self.next_token()?;
        let mut ranges = Vec::new();
        if self.tok.kind != TT_LS && self.tok.kind != TT_OF {
            return Err(self.expected("'[' or 'of'"));
        }
        if self.tok.kind == TT_LS {
            loop {
                self.next_token()?;
                let mut minus1 = false;
                if self.tok.text == b"-" {
                    minus1 = true;
                    self.next_token()?;
                }
                let begin = self.int_literal()?;
                let begin = if minus1 { -begin } else { begin };
                self.next_token()?;
                if self.tok.kind != TT_DOUBLE_POINT {
                    return Err(self.expected("'..'"));
                }
                self.next_token()?;
                let mut minus2 = false;
                if self.tok.text == b"-" {
                    minus2 = true;
                    self.next_token()?;
                }
                let end = self.int_literal()?;
                // Upstream multiplies the upper bound by -1 only in the
                // `except` arm of a `try` that cannot be reached with the
                // value assigned (`:7757`); the intended sign is applied here.
                let end = if minus2 { -end } else { end };
                if ranges.len() > 32 {
                    return Err(self.error_at(self.tok.end, crate::error::IE_ARRAY_BAD_DIMENSION));
                }
                if begin > end {
                    return Err(self.error_at(self.tok.end, IE_ARRAY_BAD_RANGE));
                }
                ranges.push((begin, end));
                self.next_token()?;
                if self.tok.kind != TT_COL {
                    break;
                }
            }
            if self.tok.kind != TT_RS {
                return Err(self.expected("']'"));
            }
            self.next_token()?;
            if self.tok.kind != TT_OF {
                return Err(self.expected("'of'"));
            }
        } else {
            // `array of T`: dimension 1 with the empty range 0 .. -1 (`:7784`).
            ranges.push((0, -1));
        }
        self.next_token()?;
        let element = self.parse_data_type()?;
        Ok(TypeRef::Array {
            ranges,
            element: Box::new(element),
        })
    }

    fn error_at(&self, pos: usize, code: i32) -> Error {
        Error::new(code, pos as i64, "", "")
    }

    /// The integer text of a `ParseDataType` range bound: the interpreter
    /// reads `FTokenStr` with `StrToInt` (which accepts a `$` prefix); a
    /// non-number text raises `EConvertError` upstream -- not an interpreter
    /// error -- and is reported here as `irIntegerValue` instead.
    fn int_literal(&mut self) -> Result<i64, Error> {
        let text = self.text_string();
        let parsed = if let Some(hex) = text.strip_prefix('$') {
            i64::from_str_radix(hex, 16).ok()
        } else {
            text.parse::<i64>().ok()
        };
        match parsed {
            Some(value) => Ok(value),
            None => Err(self.expected(IR_INTEGER_VALUE)),
        }
    }

    /// `ReadFunctionHeader` (`JvInterpreter.pas:7827`): entered with the
    /// current token on `function`/`procedure`; on exit it is the header's
    /// `;`.
    fn read_function_header(&mut self) -> Result<RoutineHeader, Error> {
        let start = self.tok.start;
        let is_function = self.tok.kind == TT_FUNCTION;
        self.next_token()?;
        if self.tok.kind != TT_IDENTIFIER {
            return Err(self.expected(IR_IDENTIFIER));
        }
        let mut name = self.text_string();
        self.next_token()?;
        let class = if self.tok.kind == TT_POINT {
            self.next_token()?;
            if self.tok.kind != TT_IDENTIFIER {
                return Err(self.expected(IR_IDENTIFIER));
            }
            let class = Some(name);
            name = self.text_string();
            self.next_token()?;
            class
        } else {
            None
        };
        let mut params = Vec::new();
        if self.tok.kind == TT_LB {
            self.read_params(&mut params)?;
            self.next_token()?;
        }
        let mut result = None;
        if is_function {
            if self.tok.kind != TT_COLON {
                return Err(self.expected("':'"));
            }
            self.next_token()?;
            if self.tok.kind != TT_IDENTIFIER {
                return Err(self.expected(IR_IDENTIFIER));
            }
            result = Some(self.parse_data_type()?);
            self.next_token()?;
        }
        if self.tok.kind != TT_SEMICOLON {
            return Err(self.expected("';'"));
        }
        Ok(RoutineHeader {
            name,
            class,
            params,
            result,
            span: Span::new(start, self.prev_end),
        })
    }

    /// `ReadFunctionHeader.ReadParams` (`JvInterpreter.pas:7832`): entered
    /// with the current token on `(`; exits with it on the `)`. Only the
    /// first identifier of a parameter's type is kept
    /// (`FParamTypeNames`); everything else up to the `;`/`)` -- a default
    /// value, `array of` -- is skipped, as upstream skips it. `const` does
    /// not consume its token (upstream has no `NextToken` there, `:7854`);
    /// the generic `NextToken` of the name loop passes over it.
    fn read_params(&mut self, out: &mut Vec<Param>) -> Result<(), Error> {
        loop {
            let mut var_param = false;
            let mut var_const = false;
            self.next_token()?;
            if self.tok.kind == TT_RB {
                return Ok(());
            }
            if self.tok.kind == TT_VAR {
                var_param = true;
                self.next_token()?;
            }
            if self.tok.kind == TT_CONST {
                var_const = true;
            }
            let mode = if var_param {
                ParamMode::Var
            } else if var_const {
                ParamMode::Const
            } else {
                ParamMode::Value
            };
            let group_start = out.len();
            // One name slot; only `,` advances to the next one, an identifier
            // overwrites the slot (so `out x` is one parameter named `x`,
            // as the upstream case statement leaves it).
            let mut names: Vec<String> = vec![String::new()];
            loop {
                match self.tok.kind {
                    TT_IDENTIFIER => {
                        *names.last_mut().unwrap() = self.text_string();
                    }
                    TT_SEMICOLON => break,
                    TT_RB => {
                        flush_params(out, &names, mode, "", group_start);
                        return Ok(());
                    }
                    TT_COLON => {
                        self.next_token()?;
                        if self.tok.kind != TT_IDENTIFIER {
                            return Err(self.expected(IR_IDENTIFIER));
                        }
                        let type_name = self.text_string();
                        while self.tok.kind != TT_RB && self.tok.kind != TT_SEMICOLON {
                            self.next_token()?;
                        }
                        if self.tok.kind == TT_RB {
                            // Upstream backs up here so the outer loop's
                            // `NextToken` is a no-op (`:7872`).
                            self.back();
                        }
                        flush_params(out, &names, mode, &type_name, group_start);
                        break;
                    }
                    TT_COL => {
                        names.push(String::new());
                    }
                    _ => {}
                }
                self.next_token()?;
            }
        }
    }

    /// `InterpretFunction` (`JvInterpreter.pas:7948`) with the body grammar of
    /// `InFunction` (`:6553`). Entered with the current token on
    /// `function`/`procedure`; on exit it is the token after the body's `end`
    /// (or the `;` for a declaration or an `external`).
    fn parse_routine(&mut self) -> Result<RoutineDecl, Error> {
        let start = self.tok.start;
        let header = self.read_function_header()?;
        let mut decl = RoutineDecl {
            header,
            locals: Vec::new(),
            body: None,
            body_pos: None,
            external: None,
            directives: Vec::new(),
            span: Span::new(start, self.prev_end),
        };
        let semicolon_end = self.tok.end;
        self.next_token()?;
        if self.tok.kind == TT_EXTERNAL {
            let external = self.parse_external()?;
            decl.external = Some(external);
            decl.span = Span::new(start, self.prev_end);
            return Ok(decl);
        }
        if self.tok.kind == TT_IDENTIFIER && is_directive(&self.tok.text) {
            decl.directives.push(self.text_string());
            self.next_token()?;
            if self.section == Section::Interface {
                decl.span = Span::new(start, self.prev_end);
                return Ok(decl);
            }
        } else if self.section == Section::Interface {
            // The interpreter resets to just after the header's `;` and
            // re-reads the following token on the next `NextToken`.
            self.rewind_semicolon(semicolon_end);
            decl.span = Span::new(start, self.prev_end);
            return Ok(decl);
        }
        // The header's `;` end is where `ExecFunction` re-parses the body
        // (`FunctionDesc.FPosBeg := CurPos`, `JvInterpreter.pas:7959`).
        decl.body_pos = Some(semicolon_end);
        if self.compile {
            // `InterpretFunction` (`JvInterpreter.pas:8014`): FindToken(ttBegin);
            // SkipToEnd -- compiling scans a body for its balanced `end` and
            // does not parse the statements; those run when `ExecFunction`
            // re-parses them at the recorded position.
            self.find_token(TT_BEGIN)?;
            self.skip_to_end()?;
            decl.span = Span::new(start, self.prev_end);
            return Ok(decl);
        }
        // FindToken(ttBegin) -- junk after the header is skipped, as upstream
        // skips it; `var`/`const` sections are read on the way.
        loop {
            match self.tok.kind {
                TT_BEGIN => break,
                TT_EMPTY => return Err(self.expected("'end'")),
                TT_VAR => {
                    let groups = self.parse_var_section_fields()?;
                    decl.locals.extend(groups.into_iter().map(LocalDecl::Var));
                }
                TT_CONST => {
                    let decls = self.parse_const_section()?;
                    decl.locals.extend(decls.into_iter().map(LocalDecl::Const));
                }
                _ => {}
            }
            self.next_token()?;
        }
        decl.body = Some(self.parse_begin_block()?);
        decl.span = Span::new(start, self.prev_end);
        Ok(decl)
    }

    /// The `external` directive of `InterpretFunction` (`:7962`): entered
    /// with the current token on `external`; on exit it is the directive's
    /// `;`.
    fn parse_external(&mut self) -> Result<ExternalDecl, Error> {
        let start = self.tok.start;
        self.next_token()?;
        let dll = match self.tok.kind {
            TT_STRING => Expr {
                span: Span::new(self.tok.start, self.tok.end),
                node: ExprNode::Str(self.string_value()),
            },
            TT_IDENTIFIER => {
                let name_span = Span::new(self.tok.start, self.tok.end);
                Expr {
                    span: name_span,
                    node: ExprNode::Ident {
                        name: self.text_string(),
                        args: Vec::new(),
                        indexed: false,
                    },
                }
            }
            _ => return Err(self.expected(IR_STRING_CONSTANT)),
        };
        self.next_token()?;
        if self.tok.kind != TT_IDENTIFIER {
            return Err(self.expected("'name' or 'index'"));
        }
        let mut name = None;
        let mut index = None;
        if self.tok.text.eq_ignore_ascii_case(b"name") {
            self.next_token()?;
            if self.tok.kind == TT_STRING {
                name = Some(self.string_value());
            } else {
                return Err(self.expected(IR_STRING_CONSTANT));
            }
        } else if self.tok.text.eq_ignore_ascii_case(b"index") {
            self.next_token()?;
            if self.tok.kind == TT_INTEGER {
                index = self.text_string().trim_start_matches('$').parse::<i64>().ok();
            } else {
                return Err(self.expected(IR_INTEGER_CONSTANT));
            }
        } else {
            return Err(self.expected("'name' or 'index'"));
        }
        self.next_token()?;
        Ok(ExternalDecl {
            dll,
            name,
            index,
            span: Span::new(start, self.prev_end),
        })
    }

    /// `InterpretUses` (`JvInterpreter.pas:8062`): entered with the current
    /// token on `uses`; on exit it is the `;`.
    fn parse_uses(&mut self) -> Result<UsesClause, Error> {
        let mut units = Vec::new();
        self.next_token()?;
        if !matches!(self.tok.kind, TT_IDENTIFIER | TT_STRING) {
            return Err(self.expected(IR_IDENTIFIER));
        }
        units.push(self.uses_unit()?);
        loop {
            self.next_token()?;
            if self.tok.kind == TT_IN {
                // `in 'file.pas'` is ignored except for the file it names
                // (`:8072`).
                self.next_token()?;
                if self.tok.kind == TT_STRING
                    && let Some(unit) = units.last_mut()
                {
                    unit.in_file = Some(self.string_value());
                }
                self.next_token()?;
            }
            if self.tok.kind == TT_SEMICOLON {
                break;
            }
            if self.tok.kind != TT_COL {
                return Err(self.expected("','"));
            }
            self.next_token()?;
            if !matches!(self.tok.kind, TT_IDENTIFIER | TT_STRING) {
                return Err(self.expected(IR_IDENTIFIER));
            }
            units.push(self.uses_unit()?);
        }
        Ok(UsesClause { units })
    }

    fn uses_unit(&mut self) -> Result<UsesUnit, Error> {
        let name = if self.tok.kind == TT_STRING {
            String::from_utf8_lossy(&self.string_value()).into_owned()
        } else {
            self.text_string()
        };
        Ok(UsesUnit {
            name,
            in_file: None,
            span: Span::new(self.tok.start, self.tok.end),
        })
    }

    /// `InterpretUnit` (`JvInterpreter.pas:8090`): entered with the current
    /// token on `unit`; consumes through `end.`.
    fn parse_unit(&mut self) -> Result<Module, Error> {
        let start = self.tok.start;
        self.next_token()?;
        if self.tok.kind != TT_IDENTIFIER {
            return Err(self.expected(IR_IDENTIFIER));
        }
        let name = self.text_string();
        self.next_token()?;
        if self.tok.kind != TT_SEMICOLON {
            return Err(self.expected("';'"));
        }
        self.next_token()?;
        let items = self.parse_unit_items(false)?;
        // `parse_unit_items` stopped on `end`.
        self.next_token()?;
        if self.tok.kind != TT_POINT {
            return Err(self.expected("'.'"));
        }
        Ok(Module {
            kind: ModuleKind::Unit,
            name,
            items,
            body: None,
            span: Span::new(start, self.prev_end),
        })
    }

    /// The declaration loop of `InterpretUnit` (`:8103`) and of
    /// `TJvInterpreterProgram.Run` (`:8552`); stops on `end` (a unit) or
    /// `begin` (a program).
    fn parse_unit_items(&mut self, is_program: bool) -> Result<Vec<Item>, Error> {
        let mut items = Vec::new();
        loop {
            match self.tok.kind {
                TT_EMPTY => return Err(self.expected("'end'")),
                TT_FUNCTION | TT_PROCEDURE => {
                    let start = self.tok.start;
                    let routine = self.parse_routine()?;
                    items.push(Item {
                        section: self.section,
                        span: Span::new(start, self.prev_end),
                        node: ItemNode::Routine(Box::new(routine)),
                    });
                    if self.tok.kind != TT_SEMICOLON {
                        return Err(self.expected("';'"));
                    }
                }
                TT_END => break,
                TT_BEGIN if is_program => break,
                TT_USES => {
                    let start = self.tok.start;
                    let uses = self.parse_uses()?;
                    items.push(Item {
                        section: self.section,
                        span: Span::new(start, self.prev_end),
                        node: ItemNode::Uses(uses),
                    });
                }
                TT_VAR => {
                    let start = self.tok.start;
                    let groups = self.parse_var_section_fields()?;
                    for group in groups {
                        items.push(Item {
                            section: self.section,
                            span: Span::new(start, self.prev_end),
                            node: ItemNode::Var(group),
                        });
                    }
                }
                TT_CONST => {
                    let start = self.tok.start;
                    let decls = self.parse_const_section()?;
                    for decl in decls {
                        items.push(Item {
                            section: self.section,
                            span: Span::new(start, self.prev_end),
                            node: ItemNode::Const(decl),
                        });
                    }
                }
                TT_INTERFACE => self.section = Section::Interface,
                TT_IMPLEMENTATION => self.section = Section::Implementation,
                TT_TYPE => {
                    let start = self.tok.start;
                    let decls = self.parse_type_section()?;
                    for decl in decls {
                        items.push(Item {
                            section: self.section,
                            span: Span::new(start, self.prev_end),
                            node: ItemNode::Type(decl),
                        });
                    }
                }
                TT_PROGRAM if is_program => {
                    // A `program Name;` after the first one (`:8577`).
                    self.next_token()?;
                    if self.tok.kind != TT_IDENTIFIER {
                        return Err(self.expected(IR_IDENTIFIER));
                    }
                    self.next_token()?;
                    if self.tok.kind != TT_SEMICOLON {
                        return Err(self.expected("';'"));
                    }
                }
                _ => return Err(self.expected(IR_DECLARATION)),
            }
            self.next_token()?;
        }
        Ok(items)
    }

    /// `TJvInterpreterProgram.Run` (`JvInterpreter.pas:8540`): the `program`
    /// form, for completeness; the script host compiles units only.
    fn parse_program(&mut self) -> Result<Module, Error> {
        let start = self.tok.start;
        self.next_token()?;
        if self.tok.kind != TT_IDENTIFIER {
            return Err(self.expected(IR_IDENTIFIER));
        }
        let name = self.text_string();
        self.next_token()?;
        if self.tok.kind != TT_SEMICOLON {
            return Err(self.expected("';'"));
        }
        self.next_token()?;
        let items = self.parse_unit_items(true)?;
        let body = if self.tok.kind == TT_BEGIN {
            Some(self.parse_begin_block()?)
        } else {
            // `end` without a `begin`; accepted here, where upstream fails
            // inside `InterpretBegin`.
            None
        };
        if self.tok.kind != TT_POINT {
            return Err(self.expected("'.'"));
        }
        Ok(Module {
            kind: ModuleKind::Program,
            name,
            items,
            body,
            span: Span::new(start, self.prev_end),
        })
    }
}

/// The statement starters of `InterpretStatement`/`InterpretBegin`
/// (`:6646`, `:7060`), with the local routine extension.
fn is_statement_start(kind: TTokenKind) -> bool {
    matches!(
        kind,
        TT_IDENTIFIER
            | TT_BEGIN
            | TT_IF
            | TT_WHILE
            | TT_FOR
            | TT_REPEAT
            | TT_BREAK
            | TT_CONTINUE
            | TT_TRY
            | TT_RAISE
            | TT_EXIT
            | TT_CASE
            | TT_FUNCTION
            | TT_PROCEDURE
    )
}

/// Appends the parameter slot of one group to `out`; the slot names that were
/// never written stay empty, as the upstream arrays leave them.
fn flush_params(out: &mut Vec<Param>, names: &[String], mode: ParamMode, type_name: &str, group_start: usize) {
    for name in names {
        out.push(Param {
            name: name.clone(),
            mode,
            type_name: type_name.to_owned(),
        });
    }
    let _ = group_start;
}

/// The directive identifiers accepted after a routine header (the interpreter
/// itself knows only `external`, which is parsed before this list). A directive
/// makes the interpreter fail in an `interface` section and is skipped by
/// `FindToken(ttBegin)` in an implementation; accepting it here is the front
/// end extension the module doc describes (`xEditAPI.pas` uses `overload`).
fn is_directive(text: &[u8]) -> bool {
    const DIRECTIVES: [&[u8]; 23] = [
        b"overload",
        b"forward",
        b"register",
        b"pascal",
        b"cdecl",
        b"stdcall",
        b"safecall",
        b"winapi",
        b"varargs",
        b"virtual",
        b"dynamic",
        b"abstract",
        b"reintroduce",
        b"deprecated",
        b"platform",
        b"inline",
        b"assembler",
        b"export",
        b"local",
        b"near",
        b"far",
        b"message",
        b"static",
    ];
    DIRECTIVES.iter().any(|directive| directive.eq_ignore_ascii_case(text))
}

/// `Val(FTokenStr, Int, Stub)` (`JvInterpreter.pas:5561`): a `$` prefixes a
/// hexadecimal value.
fn delphi_val_ok(text: &[u8]) -> bool {
    let (digits, radix) = match text.first() {
        Some(b'$') => (&text[1..], 16),
        _ => (text, 10),
    };
    let Ok(digits) = std::str::from_utf8(digits) else {
        return false;
    };
    !digits.is_empty() && i64::from_str_radix(digits, radix).is_ok()
}

/// `TextToFloat(FTokenStr, Dob, fvExtended)` (`JvInterpreter.pas:5577`): a
/// decimal form with one radix point and an optional exponent that has at
/// least one digit.
fn delphi_text_to_float_ok(text: &[u8]) -> bool {
    let mut i = 0usize;
    let mut digits = 0usize;
    while i < text.len() && text[i].is_ascii_digit() {
        i += 1;
        digits += 1;
    }
    if i < text.len() && text[i] == b'.' {
        i += 1;
        while i < text.len() && text[i].is_ascii_digit() {
            i += 1;
            digits += 1;
        }
    }
    if digits == 0 {
        return false;
    }
    if i < text.len() && (text[i] == b'E' || text[i] == b'e') {
        i += 1;
        if i < text.len() && (text[i] == b'+' || text[i] == b'-') {
            i += 1;
        }
        let mut exponent = 0usize;
        while i < text.len() && text[i].is_ascii_digit() {
            i += 1;
            exponent += 1;
        }
        if exponent == 0 {
            return false;
        }
    }
    i == text.len()
}
