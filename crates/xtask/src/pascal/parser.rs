// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Recursive descent parser for the Object Pascal that the xEdit units use.
//!
//! Type definitions and constants that are not plain expressions are kept as
//! tokens. Routine headers, statements and expressions are parsed completely.

use anyhow::{Result, anyhow, bail};

use super::ast::*;
use super::lexer::{Token, TokenKind, tokenize};

/// Words that cannot be the start of an expression operand.
const RESERVED: &[&str] = &[
    "and",
    "array",
    "as",
    "asm",
    "begin",
    "case",
    "class",
    "const",
    "constructor",
    "destructor",
    "div",
    "do",
    "downto",
    "else",
    "end",
    "except",
    "finalization",
    "finally",
    "for",
    "goto",
    "if",
    "implementation",
    "in",
    "initialization",
    "interface",
    "is",
    "label",
    "mod",
    "not",
    "of",
    "or",
    "record",
    "repeat",
    "shl",
    "shr",
    "then",
    "threadvar",
    "to",
    "try",
    "type",
    "unit",
    "until",
    "uses",
    "var",
    "while",
    "with",
    "xor",
];

/// Words that end a list of declarations in a `const`, `var` or `type` section.
const SECTION_WORDS: &[&str] = &[
    "const",
    "var",
    "threadvar",
    "type",
    "function",
    "procedure",
    "constructor",
    "destructor",
    "class",
    "implementation",
    "initialization",
    "finalization",
    "begin",
    "end",
    "label",
    "resourcestring",
    "exports",
    "asm",
    "interface",
];

const ROUTINE_DIRECTIVES: &[&str] = &[
    "overload",
    "inline",
    "virtual",
    "override",
    "abstract",
    "reintroduce",
    "stdcall",
    "cdecl",
    "register",
    "pascal",
    "safecall",
    "static",
    "forward",
    "dynamic",
    "final",
    "assembler",
    "platform",
    "deprecated",
    "external",
    "message",
    "varargs",
    "export",
    "unsafe",
];

pub struct Parser {
    tokens: Vec<Token>,
    pos: usize,
    /// Name and line of the routine that was started last, for error messages.
    last_routine: String,
}

/// Parses a unit, program or library.
pub fn parse_unit(source: &str, defines: &[&str]) -> Result<Unit> {
    let mut parser = Parser::new(tokenize(source, defines)?);
    parser.unit()
}

/// Parses one expression.
#[cfg(test)]
pub fn parse_expr(source: &str) -> Result<Expr> {
    let mut parser = Parser::new(tokenize(source, &[])?);
    let expr = parser.expr()?;
    parser.expect_eof()?;
    Ok(expr)
}

impl Parser {
    pub fn new(tokens: Vec<Token>) -> Self {
        Self {
            tokens,
            pos: 0,
            last_routine: String::new(),
        }
    }

    fn peek(&self) -> &Token {
        &self.tokens[self.pos.min(self.tokens.len() - 1)]
    }

    fn peek_at(&self, offset: usize) -> &Token {
        &self.tokens[(self.pos + offset).min(self.tokens.len() - 1)]
    }

    fn advance(&mut self) -> Token {
        let token = self.peek().clone();
        if self.pos < self.tokens.len() - 1 {
            self.pos += 1;
        }
        token
    }

    fn at_word(&self, word: &str) -> bool {
        self.peek().is_word(word)
    }

    fn at_any_word(&self, words: &[&str]) -> bool {
        words.iter().any(|word| self.at_word(word))
    }

    fn at_symbol(&self, symbol: &str) -> bool {
        self.peek().is_symbol(symbol)
    }

    fn at_eof(&self) -> bool {
        self.peek().kind == TokenKind::Eof
    }

    fn eat_word(&mut self, word: &str) -> bool {
        let found = self.at_word(word);
        if found {
            self.advance();
        }
        found
    }

    fn eat_symbol(&mut self, symbol: &str) -> bool {
        let found = self.at_symbol(symbol);
        if found {
            self.advance();
        }
        found
    }

    fn error<T>(&self, message: &str) -> Result<T> {
        let token = self.peek();
        let place = if self.last_routine.is_empty() {
            String::new()
        } else {
            format!(" (last routine: {})", self.last_routine)
        };
        Err(anyhow!("line {}: {message}, found {:?}{place}", token.line, token.kind))
    }

    fn expect_word(&mut self, word: &str) -> Result<()> {
        if self.eat_word(word) {
            Ok(())
        } else {
            self.error(&format!("expected `{word}`"))
        }
    }

    fn expect_symbol(&mut self, symbol: &str) -> Result<()> {
        if self.eat_symbol(symbol) {
            Ok(())
        } else {
            self.error(&format!("expected `{symbol}`"))
        }
    }

    fn expect_ident(&mut self) -> Result<String> {
        match self.peek().ident() {
            Some(name) => {
                // An escaped identifier such as &type is used without its `&`.
                let name = name.trim_start_matches('&').to_owned();
                self.advance();
                Ok(name)
            }
            None => self.error("expected an identifier"),
        }
    }

    #[cfg(test)]
    fn expect_eof(&self) -> Result<()> {
        if self.at_eof() {
            Ok(())
        } else {
            self.error("expected the end of the input")
        }
    }

    /// An identifier with unit or class prefixes: `System.SysUtils`.
    fn dotted_name(&mut self) -> Result<String> {
        let mut name = self.expect_ident()?;
        while self.at_symbol(".") && self.peek_at(1).ident().is_some() {
            self.advance();
            name.push('.');
            name.push_str(&self.expect_ident()?);
        }
        Ok(name)
    }

    // ----- units -----

    fn unit(&mut self) -> Result<Unit> {
        let mut unit = Unit::default();
        for kind in ["unit", "program", "library"] {
            if self.eat_word(kind) {
                unit.kind = kind.to_owned();
            }
        }
        if unit.kind.is_empty() {
            return self.error("expected `unit`, `program` or `library`");
        }
        unit.name = self.dotted_name()?;
        self.expect_symbol(";")?;
        if unit.kind == "unit" {
            self.expect_word("interface")?;
            unit.interface_uses = self.uses()?;
            unit.interface = self.decls(false)?;
            self.expect_word("implementation")?;
        }
        unit.implementation_uses = self.uses()?;
        unit.implementation = self.decls(true)?;
        if self.eat_word("initialization") {
            unit.initialization = self.statements(&["finalization", "end"])?;
            if self.eat_word("finalization") {
                unit.finalization = self.statements(&["end"])?;
            }
            self.expect_word("end")?;
        } else if self.eat_word("begin") {
            unit.initialization = self.statements(&["end"])?;
            self.expect_word("end")?;
        } else {
            self.expect_word("end")?;
        }
        self.expect_symbol(".")?;
        Ok(unit)
    }

    fn uses(&mut self) -> Result<Vec<String>> {
        let mut units = Vec::new();
        if !self.eat_word("uses") {
            return Ok(units);
        }
        loop {
            units.push(self.dotted_name()?);
            if self.eat_word("in") {
                self.advance();
            }
            if !self.eat_symbol(",") {
                break;
            }
        }
        self.expect_symbol(";")?;
        Ok(units)
    }

    // ----- declarations -----

    fn at_section_word(&self) -> bool {
        self.at_any_word(SECTION_WORDS)
    }

    /// The declarations of a section. Routines have bodies when `with_bodies`.
    fn decls(&mut self, with_bodies: bool) -> Result<Vec<Decl>> {
        let mut decls = Vec::new();
        loop {
            if self.at_word("const") || self.at_word("resourcestring") {
                self.advance();
                while !self.at_section_word() && !self.at_eof() && !self.at_symbol("[") {
                    decls.push(Decl::Const(self.var_decl()?));
                }
            } else if self.at_word("var") || self.at_word("threadvar") {
                self.advance();
                while !self.at_section_word() && !self.at_eof() && !self.at_symbol("[") {
                    decls.push(Decl::Var(self.var_decl()?));
                }
            } else if self.at_word("type") {
                self.advance();
                while !self.at_section_word() && !self.at_eof() {
                    if self.at_symbol("[") {
                        self.skip_balanced("[", "]")?;
                        continue;
                    }
                    decls.push(Decl::Type(self.type_decl()?));
                }
            } else if self.at_word("label") || self.at_word("exports") {
                while !self.eat_symbol(";") {
                    self.advance();
                }
            } else if self.at_symbol("[") {
                self.skip_balanced("[", "]")?;
            } else if self.at_routine_start() {
                decls.push(Decl::Routine(self.routine(with_bodies)?));
            } else {
                return Ok(decls);
            }
        }
    }

    fn at_routine_start(&self) -> bool {
        const KINDS: &[&str] = &["function", "procedure", "constructor", "destructor"];
        self.at_any_word(KINDS)
            || (self.at_word("class")
                && (KINDS.iter().any(|kind| self.peek_at(1).is_word(kind)) || self.peek_at(1).is_word("operator")))
    }

    fn skip_balanced(&mut self, open: &str, close: &str) -> Result<()> {
        self.expect_symbol(open)?;
        let mut depth = 1;
        while depth > 0 {
            if self.at_eof() {
                return self.error(&format!("`{open}` is not closed"));
            }
            if self.at_symbol(open) {
                depth += 1;
            } else if self.at_symbol(close) {
                depth -= 1;
            }
            self.advance();
        }
        Ok(())
    }

    /// The tokens up to a `;` that is not inside parentheses, brackets or a
    /// `record ... end` block. Does not consume the `;`.
    fn tokens_to_semicolon(&mut self) -> Result<Vec<Token>> {
        let mut tokens = Vec::new();
        let mut depth = 0;
        loop {
            if self.at_eof() {
                return self.error("expected `;`");
            }
            if depth == 0 && self.at_symbol(";") {
                return Ok(tokens);
            }
            if self.at_symbol("(") || self.at_symbol("[") || self.at_word("record") {
                depth += 1;
            } else if self.at_symbol(")") || self.at_symbol("]") || self.at_word("end") {
                depth -= 1;
            }
            tokens.push(self.advance());
        }
    }

    /// One entry of a `const` or `var` section: `a, b: T = value;`.
    fn var_decl(&mut self) -> Result<VarDecl> {
        let line = self.peek().line;
        let mut names = vec![self.expect_ident()?];
        while self.eat_symbol(",") {
            names.push(self.expect_ident()?);
        }
        let type_ref = if self.eat_symbol(":") {
            Some(self.type_ref()?)
        } else {
            None
        };
        let mut value = None;
        let mut value_tokens = Vec::new();
        if self.eat_symbol("=") {
            value_tokens = self.tokens_to_semicolon()?;
            let mut sub = Parser::new(with_eof(&value_tokens, line));
            if let Ok(expr) = sub.expr()
                && sub.at_eof()
            {
                value = Some(expr);
            }
        } else {
            // Directives such as `absolute x` or `deprecated`.
            self.tokens_to_semicolon()?;
        }
        self.expect_symbol(";")?;
        Ok(VarDecl {
            names,
            type_ref,
            value,
            value_tokens,
            line,
        })
    }

    /// One entry of a `type` section. The definition is kept as tokens.
    fn type_decl(&mut self) -> Result<TypeDecl> {
        let line = self.peek().line;
        let name = self.expect_ident()?;
        if self.at_symbol("<") {
            self.skip_balanced("<", ">")?;
        }
        self.expect_symbol("=")?;
        let start = self.pos;
        self.eat_word("packed");
        self.eat_word("type");
        let mut enum_values = Vec::new();
        if self.at_symbol("(") {
            // An enumeration: the identifiers at the top level of the parentheses.
            let mut offset = 1;
            let mut expect_name = true;
            loop {
                let token = self.peek_at(offset);
                if token.is_symbol(")") || token.kind == TokenKind::Eof {
                    break;
                }
                if expect_name && let Some(value) = token.ident() {
                    enum_values.push(value.to_owned());
                }
                expect_name = token.is_symbol(",");
                offset += 1;
            }
        }
        if self.at_block_type_start() {
            self.skip_block_type()?;
        }
        self.tokens_to_semicolon()?;
        let tokens = self.tokens[start..self.pos].to_vec();
        self.expect_symbol(";")?;
        // The calling convention of a procedural type.
        while self.at_any_word(&["stdcall", "cdecl", "register", "safecall", "pascal"]) {
            self.advance();
            self.expect_symbol(";")?;
        }
        Ok(TypeDecl {
            name,
            enum_values,
            tokens,
            line,
        })
    }

    /// Whether a `record`, `class`, `interface` or `object` type with a body
    /// starts here. `class;`, `class of T` and `class(TParent);` have no body.
    fn at_block_type_start(&self) -> bool {
        if self.at_word("record") || self.at_word("object") {
            return true;
        }
        if !(self.at_word("class") || self.at_word("interface") || self.at_word("dispinterface")) {
            return false;
        }
        let mut offset = 1;
        if self.peek_at(offset).is_word("of") {
            return false;
        }
        while self.peek_at(offset).is_word("abstract") || self.peek_at(offset).is_word("sealed") {
            offset += 1;
        }
        if self.peek_at(offset).is_symbol("(") {
            let mut depth = 0;
            loop {
                let token = self.peek_at(offset);
                if token.is_symbol("(") {
                    depth += 1;
                } else if token.is_symbol(")") {
                    depth -= 1;
                } else if token.kind == TokenKind::Eof {
                    return false;
                }
                offset += 1;
                if depth == 0 {
                    break;
                }
            }
        }
        !self.peek_at(offset).is_symbol(";")
    }

    /// Skips a type with a body up to and including its `end`.
    fn skip_block_type(&mut self) -> Result<()> {
        self.advance();
        let mut depth = 1;
        while depth > 0 {
            if self.at_eof() {
                return self.error("type definition is not closed");
            }
            if self.at_word("end") {
                depth -= 1;
            } else if self.at_word("record") {
                depth += 1;
            } else if self.at_symbol("=") {
                // A nested type declaration.
                self.advance();
                self.eat_word("packed");
                if self.at_block_type_start() {
                    depth += 1;
                    self.advance();
                }
                continue;
            }
            self.advance();
        }
        Ok(())
    }

    /// A type in a parameter, variable or result position.
    fn type_ref(&mut self) -> Result<TypeRef> {
        if self.at_word("array") && self.peek_at(1).is_word("of") {
            self.advance();
            self.advance();
            if self.eat_word("const") {
                return Ok(TypeRef::ArrayOfConst);
            }
            return Ok(TypeRef::ArrayOf(Box::new(self.type_ref()?)));
        }
        let simple = self.peek().ident().is_some_and(|word| {
            ![
                "array",
                "record",
                "set",
                "class",
                "function",
                "procedure",
                "reference",
                "packed",
                "file",
                "interface",
            ]
            .iter()
            .any(|reserved| word.eq_ignore_ascii_case(reserved))
        });
        if simple {
            let name = self.dotted_name()?;
            if self.at_symbol("<") {
                let args = self.generic_args()?;
                // A type nested in a generic type: TList<T>.TEnumerator.
                let mut name = name;
                while self.eat_symbol(".") {
                    name.push('.');
                    name.push_str(&self.expect_ident()?);
                }
                return Ok(TypeRef::Generic { name, args });
            }
            if self.at_symbol("[") {
                // A short string type: string[10].
                self.skip_balanced("[", "]")?;
            }
            return Ok(TypeRef::Named(name));
        }
        // Anything else: the tokens up to the end of the type.
        let mut tokens = Vec::new();
        let mut depth = 0;
        loop {
            if self.at_eof() {
                return self.error("type is not complete");
            }
            if depth == 0 && (self.at_symbol(";") || self.at_symbol("=") || self.at_symbol(")") || self.at_symbol(","))
            {
                break;
            }
            if self.at_symbol("(") || self.at_symbol("[") || self.at_word("record") {
                depth += 1;
            } else if self.at_symbol(")") || self.at_symbol("]") || self.at_word("end") {
                depth -= 1;
            }
            tokens.push(self.advance());
        }
        Ok(TypeRef::Other(tokens))
    }

    fn generic_args(&mut self) -> Result<Vec<TypeRef>> {
        self.expect_symbol("<")?;
        let mut args = vec![self.type_ref()?];
        while self.eat_symbol(",") {
            args.push(self.type_ref()?);
        }
        self.expect_symbol(">")?;
        Ok(args)
    }

    // ----- routines -----

    /// Parses a routine header without body, up to and including its directives.
    pub fn parse_routine_header(&mut self) -> Result<Routine> {
        self.routine(false)
    }

    fn params(&mut self) -> Result<Vec<Param>> {
        let mut params = Vec::new();
        if !self.eat_symbol("(") {
            return Ok(params);
        }
        while !self.at_symbol(")") {
            if self.at_symbol("[") {
                self.skip_balanced("[", "]")?;
            }
            let mut modifier = String::new();
            for word in ["const", "var", "out", "constref"] {
                if self.at_word(word) {
                    modifier = word.to_owned();
                    self.advance();
                    break;
                }
            }
            let mut names = vec![self.expect_ident()?];
            while self.eat_symbol(",") {
                names.push(self.expect_ident()?);
            }
            let type_ref = if self.eat_symbol(":") {
                Some(self.type_ref()?)
            } else {
                None
            };
            let default = if self.eat_symbol("=") { Some(self.expr()?) } else { None };
            for name in names {
                params.push(Param {
                    modifier: modifier.clone(),
                    name,
                    type_ref: type_ref.clone(),
                    default: default.clone(),
                });
            }
            if !self.eat_symbol(";") {
                break;
            }
        }
        self.expect_symbol(")")?;
        Ok(params)
    }

    /// A routine name: `Name`, `TClass.Method` or `TGeneric<T>.Method`.
    fn routine_name(&mut self) -> Result<String> {
        let mut name = self.expect_ident()?;
        loop {
            if self.at_symbol("<") {
                let start = self.pos;
                self.skip_balanced("<", ">")?;
                name.push('<');
                for token in &self.tokens[start + 1..self.pos - 1] {
                    match &token.kind {
                        TokenKind::Ident(word) => name.push_str(word),
                        TokenKind::Symbol(symbol) => name.push_str(symbol),
                        _ => {}
                    }
                }
                name.push('>');
            }
            if self.eat_symbol(".") {
                name.push('.');
                name.push_str(&self.expect_ident()?);
            } else {
                return Ok(name);
            }
        }
    }

    fn routine(&mut self, with_body: bool) -> Result<Routine> {
        let line = self.peek().line;
        self.eat_word("class");
        let kind = match self.expect_ident()?.to_ascii_lowercase().as_str() {
            "function" => RoutineKind::Function,
            "procedure" => RoutineKind::Procedure,
            "constructor" => RoutineKind::Constructor,
            "destructor" => RoutineKind::Destructor,
            "operator" => RoutineKind::Operator,
            _ => return self.error("expected a routine"),
        };
        let name = self.routine_name()?;
        self.last_routine = format!("{name} at line {line}");
        let params = self.params()?;
        let return_type = if self.eat_symbol(":") {
            Some(self.type_ref()?)
        } else {
            None
        };
        self.expect_symbol(";")?;
        let mut directives = Vec::new();
        while let Some(word) = self.peek().ident() {
            let word = word.to_ascii_lowercase();
            if !ROUTINE_DIRECTIVES.contains(&word.as_str()) {
                break;
            }
            // A directive can have arguments, as `external 'lib' name 'x'`. The
            // compiler accepts a directive without `;` before the next declaration.
            self.advance();
            while !self.eat_symbol(";") {
                if self.at_eof() {
                    return self.error("directive is not closed");
                }
                if self.at_section_word() {
                    break;
                }
                self.advance();
            }
            directives.push(word);
        }
        let has_body = with_body && !directives.iter().any(|d| d == "forward" || d == "external");
        let body = if has_body {
            let body = self.body()?;
            self.expect_symbol(";")?;
            Some(body)
        } else {
            None
        };
        Ok(Routine {
            kind,
            name,
            params,
            return_type,
            directives,
            body,
            line,
        })
    }

    /// Local declarations followed by `begin ... end` or `asm ... end`.
    fn body(&mut self) -> Result<Body> {
        let decls = self.decls(true)?;
        if self.eat_word("asm") {
            while !self.eat_word("end") {
                if self.at_eof() {
                    return self.error("`asm` is not closed");
                }
                self.advance();
            }
            return Ok(Body {
                decls,
                statements: Vec::new(),
                is_asm: true,
            });
        }
        self.expect_word("begin")?;
        let statements = self.statements(&["end"])?;
        self.expect_word("end")?;
        Ok(Body {
            decls,
            statements,
            is_asm: false,
        })
    }

    // ----- statements -----

    /// Statements separated by `;` up to one of the words in `until`.
    fn statements(&mut self, until: &[&str]) -> Result<Vec<Stmt>> {
        let mut statements = Vec::new();
        loop {
            while self.eat_symbol(";") {}
            if self.at_any_word(until) || self.at_eof() {
                return Ok(statements);
            }
            statements.push(self.statement()?);
            if !self.at_symbol(";") && !self.at_any_word(until) {
                return self.error("expected `;`");
            }
        }
    }

    fn statement(&mut self) -> Result<Stmt> {
        let line = self.peek().line;
        // A label before a statement.
        if matches!(self.peek().kind, TokenKind::Int { .. } | TokenKind::Ident(_)) && self.peek_at(1).is_symbol(":") {
            self.advance();
            self.advance();
        }
        if self.at_symbol(";") || self.at_any_word(&["end", "else", "until", "except", "finally"]) {
            return Ok(Stmt::Empty);
        }
        if self.eat_word("begin") {
            let statements = self.statements(&["end"])?;
            self.expect_word("end")?;
            return Ok(Stmt::Block(statements));
        }
        if self.eat_word("if") {
            let condition = self.expr()?;
            self.expect_word("then")?;
            let then_branch = Box::new(self.statement()?);
            let else_branch = if self.eat_word("else") {
                Some(Box::new(self.statement()?))
            } else {
                None
            };
            return Ok(Stmt::If {
                condition,
                then_branch,
                else_branch,
            });
        }
        if self.eat_word("for") {
            self.eat_word("var");
            let variable = self.expect_ident()?;
            if self.eat_symbol(":") {
                self.type_ref()?;
            }
            if self.eat_word("in") {
                let collection = self.expr()?;
                self.expect_word("do")?;
                return Ok(Stmt::ForIn {
                    variable,
                    collection,
                    body: Box::new(self.statement()?),
                });
            }
            self.expect_symbol(":=")?;
            let from = self.expr()?;
            let down = self.eat_word("downto");
            if !down {
                self.expect_word("to")?;
            }
            let to = self.expr()?;
            self.expect_word("do")?;
            return Ok(Stmt::For {
                variable,
                from,
                to,
                down,
                body: Box::new(self.statement()?),
            });
        }
        if self.eat_word("while") {
            let condition = self.expr()?;
            self.expect_word("do")?;
            return Ok(Stmt::While {
                condition,
                body: Box::new(self.statement()?),
            });
        }
        if self.eat_word("repeat") {
            let body = self.statements(&["until"])?;
            self.expect_word("until")?;
            return Ok(Stmt::Repeat {
                body,
                until: self.expr()?,
            });
        }
        if self.eat_word("case") {
            return self.case_statement();
        }
        if self.eat_word("with") {
            let mut targets = vec![self.expr()?];
            while self.eat_symbol(",") {
                targets.push(self.expr()?);
            }
            self.expect_word("do")?;
            return Ok(Stmt::With {
                targets,
                body: Box::new(self.statement()?),
            });
        }
        if self.eat_word("try") {
            return self.try_statement();
        }
        if self.eat_word("raise") {
            let mut value = None;
            if !self.at_symbol(";") && !self.at_any_word(&["end", "else", "until"]) {
                value = Some(self.expr()?);
                if self.eat_word("at") {
                    self.expr()?;
                }
            }
            return Ok(Stmt::Raise(value));
        }
        if self.at_word("var") || self.at_word("const") {
            let is_const = self.at_word("const");
            self.advance();
            let mut names = vec![self.expect_ident()?];
            while self.eat_symbol(",") {
                names.push(self.expect_ident()?);
            }
            let type_ref = if self.eat_symbol(":") {
                Some(self.type_ref()?)
            } else {
                None
            };
            let value = if self.eat_symbol(":=") || (is_const && self.eat_symbol("=")) {
                Some(self.expr()?)
            } else {
                None
            };
            return Ok(Stmt::Var(VarDecl {
                names,
                type_ref,
                value,
                value_tokens: Vec::new(),
                line,
            }));
        }
        if self.eat_word("goto") {
            self.advance();
            return Ok(Stmt::Empty);
        }
        let target = self.expr()?;
        if self.eat_symbol(":=") {
            let value = self.expr()?;
            return Ok(Stmt::Assign { target, value, line });
        }
        Ok(Stmt::Expr { expr: target, line })
    }

    fn case_statement(&mut self) -> Result<Stmt> {
        let selector = self.expr()?;
        self.expect_word("of")?;
        let mut arms = Vec::new();
        let mut else_branch = None;
        loop {
            while self.eat_symbol(";") {}
            if self.eat_word("end") {
                break;
            }
            if self.eat_word("else") {
                else_branch = Some(self.statements(&["end"])?);
                self.expect_word("end")?;
                break;
            }
            let mut labels = Vec::new();
            loop {
                let label = self.expr()?;
                if self.eat_symbol("..") {
                    labels.push(Expr::Range(Box::new(label), Box::new(self.expr()?)));
                } else {
                    labels.push(label);
                }
                if !self.eat_symbol(",") {
                    break;
                }
            }
            self.expect_symbol(":")?;
            arms.push(CaseArm {
                labels,
                body: self.statement()?,
            });
        }
        Ok(Stmt::Case {
            selector,
            arms,
            else_branch,
        })
    }

    fn try_statement(&mut self) -> Result<Stmt> {
        let body = self.statements(&["except", "finally"])?;
        let mut handlers = Vec::new();
        let mut except = None;
        let mut finally = None;
        if self.eat_word("finally") {
            finally = Some(self.statements(&["end"])?);
        } else {
            self.expect_word("except")?;
            if self.at_word("on") {
                loop {
                    while self.eat_symbol(";") {}
                    if !self.eat_word("on") {
                        break;
                    }
                    let mut variable = String::new();
                    if self.peek_at(1).is_symbol(":") {
                        variable = self.expect_ident()?;
                        self.advance();
                    }
                    let type_ref = self.type_ref()?;
                    self.expect_word("do")?;
                    handlers.push(ExceptHandler {
                        variable,
                        type_ref,
                        body: self.statement()?,
                    });
                }
                if self.eat_word("else") {
                    except = Some(self.statements(&["end"])?);
                }
            } else {
                except = Some(self.statements(&["end"])?);
            }
        }
        self.expect_word("end")?;
        Ok(Stmt::Try {
            body,
            handlers,
            except,
            finally,
        })
    }

    // ----- expressions -----

    pub fn expr(&mut self) -> Result<Expr> {
        let mut left = self.additive()?;
        loop {
            let op = if let Some(symbol) = ["=", "<>", "<=", ">=", "<", ">"]
                .iter()
                .find(|symbol| self.at_symbol(symbol))
            {
                (*symbol).to_owned()
            } else if self.at_word("in") || self.at_word("is") {
                self.peek().ident().unwrap_or_default().to_ascii_lowercase()
            } else {
                return Ok(left);
            };
            self.advance();
            let right = self.additive()?;
            left = Expr::Binary {
                op,
                left: Box::new(left),
                right: Box::new(right),
            };
        }
    }

    fn additive(&mut self) -> Result<Expr> {
        let mut left = self.multiplicative()?;
        loop {
            let op = if self.at_symbol("+") || self.at_symbol("-") {
                match &self.peek().kind {
                    TokenKind::Symbol(symbol) => (*symbol).to_owned(),
                    _ => unreachable!(),
                }
            } else if self.at_word("or") || self.at_word("xor") {
                self.peek().ident().unwrap_or_default().to_ascii_lowercase()
            } else {
                return Ok(left);
            };
            self.advance();
            let right = self.multiplicative()?;
            left = Expr::Binary {
                op,
                left: Box::new(left),
                right: Box::new(right),
            };
        }
    }

    fn multiplicative(&mut self) -> Result<Expr> {
        let mut left = self.unary()?;
        loop {
            let op = if self.at_symbol("*") || self.at_symbol("/") {
                match &self.peek().kind {
                    TokenKind::Symbol(symbol) => (*symbol).to_owned(),
                    _ => unreachable!(),
                }
            } else if self.at_any_word(&["div", "mod", "and", "shl", "shr", "as"]) {
                self.peek().ident().unwrap_or_default().to_ascii_lowercase()
            } else {
                return Ok(left);
            };
            self.advance();
            let right = self.unary()?;
            left = Expr::Binary {
                op,
                left: Box::new(left),
                right: Box::new(right),
            };
        }
    }

    fn unary(&mut self) -> Result<Expr> {
        for op in ["-", "+", "@"] {
            if self.eat_symbol(op) {
                return Ok(Expr::Unary {
                    op: op.to_owned(),
                    operand: Box::new(self.unary()?),
                });
            }
        }
        if self.eat_word("not") {
            return Ok(Expr::Unary {
                op: "not".to_owned(),
                operand: Box::new(self.unary()?),
            });
        }
        self.postfix()
    }

    fn postfix(&mut self) -> Result<Expr> {
        let mut expr = self.primary()?;
        loop {
            if self.eat_symbol("(") {
                let mut args = Vec::new();
                while !self.at_symbol(")") {
                    args.push(self.expr()?);
                    // Width and precision of Write arguments: x:5:2.
                    while self.eat_symbol(":") {
                        self.expr()?;
                    }
                    if !self.eat_symbol(",") {
                        break;
                    }
                }
                self.expect_symbol(")")?;
                expr = Expr::Call {
                    callee: Box::new(expr),
                    args,
                };
            } else if self.eat_symbol("[") {
                let mut args = vec![self.expr()?];
                while self.eat_symbol(",") {
                    args.push(self.expr()?);
                }
                self.expect_symbol("]")?;
                expr = Expr::Index {
                    base: Box::new(expr),
                    args,
                };
            } else if self.at_symbol(".") && self.peek_at(1).ident().is_some() {
                self.advance();
                let name = self.expect_ident()?;
                // The type arguments of a generic method call are not kept.
                if self.at_symbol("<") {
                    self.try_generic_args();
                }
                expr = Expr::Member {
                    base: Box::new(expr),
                    name,
                };
            } else if self.eat_symbol("^") {
                expr = Expr::Deref(Box::new(expr));
            } else {
                return Ok(expr);
            }
        }
    }

    /// Tries `<T, U>` after an identifier. A generic type in an expression is
    /// followed by `.` or `(`; anything else is a comparison.
    fn try_generic_args(&mut self) -> Option<Vec<TypeRef>> {
        let start = self.pos;
        let args = self.generic_args();
        match args {
            Ok(args) if self.at_symbol(".") || self.at_symbol("(") => Some(args),
            _ => {
                self.pos = start;
                None
            }
        }
    }

    fn primary(&mut self) -> Result<Expr> {
        let token = self.peek().clone();
        match &token.kind {
            TokenKind::Int { digits, hex } => {
                self.advance();
                Ok(Expr::Int {
                    digits: digits.clone(),
                    hex: *hex,
                })
            }
            TokenKind::Float(text) => {
                self.advance();
                Ok(Expr::Float(text.clone()))
            }
            TokenKind::Str(text) => {
                self.advance();
                Ok(Expr::Str(text.clone()))
            }
            TokenKind::Symbol("(") => {
                self.advance();
                let inner = self.expr()?;
                self.expect_symbol(")")?;
                Ok(Expr::Paren(Box::new(inner)))
            }
            TokenKind::Symbol("[") => {
                self.advance();
                let mut items = Vec::new();
                while !self.at_symbol("]") {
                    let item = self.expr()?;
                    if self.eat_symbol("..") {
                        items.push(Expr::Range(Box::new(item), Box::new(self.expr()?)));
                    } else {
                        items.push(item);
                    }
                    if !self.eat_symbol(",") {
                        break;
                    }
                }
                self.expect_symbol("]")?;
                Ok(Expr::List(items))
            }
            TokenKind::Ident(word) => {
                let lower = word.to_ascii_lowercase();
                match lower.as_str() {
                    "nil" => {
                        self.advance();
                        Ok(Expr::Nil)
                    }
                    "inherited" => {
                        self.advance();
                        let is_call = self
                            .peek()
                            .ident()
                            .is_some_and(|next| !RESERVED.contains(&next.to_ascii_lowercase().as_str()));
                        if is_call {
                            let name = self.expect_ident()?;
                            Ok(Expr::Inherited(Some(Box::new(Expr::Ident(name)))))
                        } else {
                            Ok(Expr::Inherited(None))
                        }
                    }
                    "function" | "procedure" => {
                        self.advance();
                        let kind = if lower == "function" {
                            RoutineKind::Function
                        } else {
                            RoutineKind::Procedure
                        };
                        let params = self.params()?;
                        let return_type = if self.eat_symbol(":") {
                            Some(self.type_ref()?)
                        } else {
                            None
                        };
                        let body = self.body()?;
                        Ok(Expr::Anonymous(Box::new(Routine {
                            kind,
                            name: String::new(),
                            params,
                            return_type,
                            directives: Vec::new(),
                            body: Some(body),
                            line: token.line,
                        })))
                    }
                    _ if RESERVED.contains(&lower.as_str()) => self.error("expected an expression"),
                    _ => {
                        self.advance();
                        if self.at_symbol("<")
                            && let Some(args) = self.try_generic_args()
                        {
                            return Ok(Expr::Generic {
                                name: word.clone(),
                                args,
                            });
                        }
                        Ok(Expr::Ident(word.clone()))
                    }
                }
            }
            _ => self.error("expected an expression"),
        }
    }
}

/// The tokens with an end marker, for a parser over part of a file.
fn with_eof(tokens: &[Token], line: u32) -> Vec<Token> {
    let mut tokens = tokens.to_vec();
    let line = tokens.last().map_or(line, |token| token.line);
    tokens.push(Token {
        kind: TokenKind::Eof,
        line,
    });
    tokens
}

/// Parses every `.pas` and `.dpr` file given and reports the first error of each.
pub fn check_files(paths: &[String]) -> Result<()> {
    let mut failed = 0;
    for path in paths {
        let bytes = std::fs::read(path)?;
        // The upstream sources are in code page 1252 without byte order mark.
        let source: String = match String::from_utf8(bytes) {
            Ok(text) => text,
            Err(error) => error.into_bytes().iter().map(|&byte| char::from(byte)).collect(),
        };
        match parse_unit(&source, &["WIN64", "MSWINDOWS", "CPUX64"]) {
            Ok(unit) => println!(
                "ok   {path}: {} interface and {} implementation declarations",
                unit.interface.len(),
                unit.implementation.len()
            ),
            Err(error) => {
                failed += 1;
                println!("FAIL {path}: {error}");
            }
        }
    }
    if failed > 0 {
        bail!("{failed} file(s) did not parse");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn unit(source: &str) -> Unit {
        parse_unit(source, &["WIN64"]).unwrap()
    }

    fn ident(name: &str) -> Expr {
        Expr::Ident(name.to_owned())
    }

    #[test]
    fn expressions() {
        assert_eq!(
            parse_expr("a + b * 2").unwrap(),
            Expr::Binary {
                op: "+".to_owned(),
                left: Box::new(ident("a")),
                right: Box::new(Expr::Binary {
                    op: "*".to_owned(),
                    left: Box::new(ident("b")),
                    right: Box::new(Expr::Int {
                        digits: "2".to_owned(),
                        hex: false
                    }),
                }),
            }
        );
        // `not` binds tighter than `and`, and comparison is lowest.
        let parsed = parse_expr("not a and (b = c)").unwrap();
        assert!(
            matches!(&parsed, Expr::Binary { op, left, .. } if op == "and" && matches!(**left, Expr::Unary { .. }))
        );
        let call = parse_expr("wbInteger('Value', itS32, wbEnum(['A', 'B']), cpNormal, True)").unwrap();
        match call {
            Expr::Call { callee, args } => {
                assert_eq!(*callee, ident("wbInteger"));
                assert_eq!(args.len(), 5);
                assert_eq!(args[0], Expr::Str("Value".to_owned()));
                assert!(
                    matches!(&args[2], Expr::Call { args, .. } if matches!(&args[0], Expr::List(items) if items.len() == 2))
                );
            }
            other => panic!("{other:?}"),
        }
        assert!(matches!(
            parse_expr("aElement.Container.ElementByName['X'].NativeValue").unwrap(),
            Expr::Member { name, .. } if name == "NativeValue"
        ));
        assert!(matches!(
            parse_expr("TFromArray<Integer>.Get(a, 1)").unwrap(),
            Expr::Call { callee, .. } if matches!(&*callee, Expr::Member { base, .. } if matches!(**base, Expr::Generic { .. }))
        ));
        // A comparison is not a generic.
        assert!(matches!(parse_expr("a < b").unwrap(), Expr::Binary { op, .. } if op == "<"));
        assert!(matches!(parse_expr("[1..3, 5]").unwrap(), Expr::List(items) if matches!(items[0], Expr::Range(..))));
        assert!(matches!(parse_expr("PCardinal(p)^").unwrap(), Expr::Deref(_)));
        assert!(matches!(parse_expr("x as IwbFile").unwrap(), Expr::Binary { op, .. } if op == "as"));
        assert!(parse_expr("a +").is_err());
        assert!(parse_expr("a b").is_err());
    }

    #[test]
    fn unit_sections_and_routines() {
        let parsed = unit(
            "unit wbTest;
             interface
             uses System.SysUtils, wbInterface;
             type
               TKind = (kOne, kTwo = 5, kThree);
               IFoo = interface;
               TFoo = class(TObject)
               private
                 fValue: record a, b: Integer; end;
               public
                 class function Make(const aName: string = 'x'): TFoo; static;
                 property Value: Integer read GetValue;
               end;
               TCallback = reference to function(aInt: Int64; const aElement: IwbElement): string;
             const
               cOne = 1;
               cNames: array[0..1] of string = ('a', 'b');
             var
               wbEDID: IwbRecordMemberDef;
             function wbInteger(const aName: string; const aIntType: TwbIntType = itU32;
               aRequired: Boolean = False): IwbIntegerDef; overload;
             procedure DefineTest;
             implementation
             uses wbHelpers;
             function wbInteger(const aName: string; const aIntType: TwbIntType = itU32;
               aRequired: Boolean = False): IwbIntegerDef;
             begin
               Result := TwbIntegerDef.Create(cpNormal, aRequired, aName, aIntType);
             end;
             function Local(a: Integer): Integer; forward;
             procedure DefineTest;
             var
               i: Integer;
               function Nested: Boolean;
               begin
                 Result := True;
               end;
             begin
               wbEDID := wbString(EDID, 'Editor ID');
               for i := 0 to 3 do
                 if Nested then
                   Inc(i)
                 else begin
                   Dec(i);
                 end;
               case i of
                 0, 1: Exit;
                 2..4: begin end;
               else
                 i := 0;
               end;
               try
                 with wbEDID do
                   IncludeFlag(dfNoReport);
               except
                 on E: Exception do
                   raise;
               end;
               var lList := TList<string>.Create;
               repeat
                 i := i + 1
               until i > 10;
             end;
             function Local(a: Integer): Integer;
             asm
               mov eax, a
             end;
             initialization
               DefineTest;
             finalization
             end.",
        );
        assert_eq!(parsed.name, "wbTest");
        assert_eq!(parsed.interface_uses, ["System.SysUtils", "wbInterface"]);
        assert_eq!(parsed.implementation_uses, ["wbHelpers"]);
        let types: Vec<&TypeDecl> = parsed
            .interface
            .iter()
            .filter_map(|decl| match decl {
                Decl::Type(type_decl) => Some(type_decl),
                _ => None,
            })
            .collect();
        assert_eq!(types.len(), 4);
        assert_eq!(types[0].enum_values, ["kOne", "kTwo", "kThree"]);
        assert_eq!(types[2].name, "TFoo");
        let routines: Vec<&Routine> = parsed
            .interface
            .iter()
            .filter_map(|decl| match decl {
                Decl::Routine(routine) => Some(routine),
                _ => None,
            })
            .collect();
        assert_eq!(routines.len(), 2);
        let integer = routines[0];
        assert_eq!(integer.name, "wbInteger");
        assert_eq!(integer.directives, ["overload"]);
        assert!(integer.body.is_none());
        assert_eq!(integer.params.len(), 3);
        assert_eq!(integer.params[1].name, "aIntType");
        assert_eq!(integer.params[1].modifier, "const");
        assert_eq!(integer.params[1].default, Some(ident("itU32")));
        assert_eq!(integer.return_type, Some(TypeRef::Named("IwbIntegerDef".to_owned())));
        let consts: Vec<&VarDecl> = parsed
            .interface
            .iter()
            .filter_map(|decl| match decl {
                Decl::Const(var) => Some(var),
                _ => None,
            })
            .collect();
        assert!(consts[0].value.is_some());
        // An array constant is not an expression and is kept as tokens.
        assert!(consts[1].value.is_none() && !consts[1].value_tokens.is_empty());

        let bodies: Vec<&Routine> = parsed
            .implementation
            .iter()
            .filter_map(|decl| match decl {
                Decl::Routine(routine) => Some(routine),
                _ => None,
            })
            .collect();
        assert_eq!(bodies.len(), 4);
        assert!(bodies[1].body.is_none(), "forward declaration");
        let define = bodies[2].body.as_ref().unwrap();
        assert_eq!(define.decls.len(), 2);
        assert_eq!(define.statements.len(), 6);
        assert!(matches!(&define.statements[0], Stmt::Assign { .. }));
        assert!(
            matches!(&define.statements[2], Stmt::Case { arms, else_branch, .. } if arms.len() == 2 && else_branch.is_some())
        );
        assert!(matches!(&define.statements[3], Stmt::Try { handlers, .. } if handlers.len() == 1));
        assert!(matches!(&define.statements[4], Stmt::Var(_)));
        assert!(bodies[3].body.as_ref().unwrap().is_asm);
        assert_eq!(parsed.initialization.len(), 1);
    }

    #[test]
    fn anonymous_routines_and_programs() {
        let parsed = unit(
            "program xDump;
             uses wbInterface in 'Core\\wbInterface.pas';
             var Callback: TwbCountCallback;
             begin
               Callback :=
                 function(aBasePtr: Pointer; aEndPtr: Pointer; const aElement: IwbElement): Cardinal
                 begin
                   Result := 0;
                   if not Assigned(aBasePtr) then
                     Exit;
                 end;
               WriteLn(ErrOutput, 'x':5, 1);
             end.",
        );
        assert_eq!(parsed.kind, "program");
        assert_eq!(parsed.initialization.len(), 2);
        assert!(matches!(
            &parsed.initialization[0],
            Stmt::Assign { value: Expr::Anonymous(routine), .. } if routine.params.len() == 3
        ));
    }

    #[test]
    fn errors_name_the_line() {
        let error = parse_unit(
            "unit a;\ninterface\nimplementation\nprocedure P;\nbegin\n  x := ;\nend;\nend.",
            &[],
        )
        .unwrap_err()
        .to_string();
        assert!(error.starts_with("line 6:"), "{error}");
    }
}
