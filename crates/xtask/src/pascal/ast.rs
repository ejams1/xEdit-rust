// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Syntax tree of the parts of Object Pascal that the definition units use.

use super::lexer::Token;

#[derive(Debug, Clone, PartialEq)]
pub enum Expr {
    Ident(String),
    Int {
        digits: String,
        hex: bool,
    },
    Float(String),
    Str(String),
    Nil,
    /// `callee(args)`
    Call {
        callee: Box<Expr>,
        args: Vec<Expr>,
    },
    /// `base.name`
    Member {
        base: Box<Expr>,
        name: String,
    },
    /// `base[args]`
    Index {
        base: Box<Expr>,
        args: Vec<Expr>,
    },
    /// `base^`
    Deref(Box<Expr>),
    /// `not x`, `-x`, `+x`, `@x`
    Unary {
        op: String,
        operand: Box<Expr>,
    },
    /// An infix operator in lower case: `+`, `and`, `=`, `in`, `as`, ...
    Binary {
        op: String,
        left: Box<Expr>,
        right: Box<Expr>,
    },
    /// `[a, b, c]`: a set or an open array.
    List(Vec<Expr>),
    /// `a..b` inside a list or a case label.
    Range(Box<Expr>, Box<Expr>),
    /// `(a)`. Kept so that the output can keep the parentheses.
    Paren(Box<Expr>),
    /// `Name<Args>`: a generic type used in an expression.
    Generic {
        name: String,
        args: Vec<TypeRef>,
    },
    /// `inherited` alone or `inherited Name(args)`.
    Inherited(Option<Box<Expr>>),
    /// `function(params): T begin ... end` or the procedure form.
    Anonymous(Box<Routine>),
}

#[derive(Debug, Clone, PartialEq)]
pub enum TypeRef {
    /// A type name, with unit prefix when written: `IwbValueDef`, `System.Integer`.
    Named(String),
    /// `Name<Args>`
    Generic { name: String, args: Vec<TypeRef> },
    /// `array of T`
    ArrayOf(Box<TypeRef>),
    /// `array of const`
    ArrayOfConst,
    /// Any other type expression, as its tokens.
    Other(Vec<Token>),
}

#[derive(Debug, Clone, PartialEq)]
pub struct Param {
    /// `const`, `var`, `out` or empty.
    pub modifier: String,
    pub name: String,
    /// `None` for an untyped parameter.
    pub type_ref: Option<TypeRef>,
    pub default: Option<Expr>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RoutineKind {
    Function,
    Procedure,
    Constructor,
    Destructor,
    Operator,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Routine {
    pub kind: RoutineKind,
    /// The name as written, with class prefix for a method: `TwbDef.Create`.
    /// Empty for an anonymous routine.
    pub name: String,
    pub params: Vec<Param>,
    pub return_type: Option<TypeRef>,
    /// Lower-case directives after the header: `overload`, `inline`, ...
    pub directives: Vec<String>,
    /// `None` for a declaration without body.
    pub body: Option<Body>,
    pub line: u32,
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct Body {
    pub decls: Vec<Decl>,
    pub statements: Vec<Stmt>,
    /// The body is assembler, which is not parsed.
    pub is_asm: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub struct VarDecl {
    pub names: Vec<String>,
    pub type_ref: Option<TypeRef>,
    /// The initial value when it is an expression.
    pub value: Option<Expr>,
    /// The tokens of the initial value, for values that are not expressions
    /// (record and array constants).
    pub value_tokens: Vec<Token>,
    pub line: u32,
}

#[derive(Debug, Clone, PartialEq)]
pub struct TypeDecl {
    pub name: String,
    /// The values of an enumeration type.
    pub enum_values: Vec<String>,
    /// The tokens of the type definition.
    pub tokens: Vec<Token>,
    pub line: u32,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Decl {
    Const(VarDecl),
    Var(VarDecl),
    Type(TypeDecl),
    Routine(Routine),
}

#[derive(Debug, Clone, PartialEq)]
pub struct CaseArm {
    pub labels: Vec<Expr>,
    pub body: Stmt,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ExceptHandler {
    /// `on E: Exception do`: the variable, which may be empty, and the type.
    pub variable: String,
    pub type_ref: TypeRef,
    pub body: Stmt,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Stmt {
    Empty,
    /// `target := value`
    Assign {
        target: Expr,
        value: Expr,
        line: u32,
    },
    /// A call or another expression used as a statement.
    Expr {
        expr: Expr,
        line: u32,
    },
    Block(Vec<Stmt>),
    If {
        condition: Expr,
        then_branch: Box<Stmt>,
        else_branch: Option<Box<Stmt>>,
    },
    For {
        variable: String,
        from: Expr,
        to: Expr,
        down: bool,
        body: Box<Stmt>,
    },
    ForIn {
        variable: String,
        collection: Expr,
        body: Box<Stmt>,
    },
    While {
        condition: Expr,
        body: Box<Stmt>,
    },
    Repeat {
        body: Vec<Stmt>,
        until: Expr,
    },
    Case {
        selector: Expr,
        arms: Vec<CaseArm>,
        else_branch: Option<Vec<Stmt>>,
    },
    With {
        targets: Vec<Expr>,
        body: Box<Stmt>,
    },
    Try {
        body: Vec<Stmt>,
        handlers: Vec<ExceptHandler>,
        except: Option<Vec<Stmt>>,
        finally: Option<Vec<Stmt>>,
    },
    Raise(Option<Expr>),
    /// An inline `var` or `const` declaration.
    Var(VarDecl),
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct Unit {
    pub name: String,
    /// `unit`, `program` or `library`.
    pub kind: String,
    pub interface_uses: Vec<String>,
    pub implementation_uses: Vec<String>,
    pub interface: Vec<Decl>,
    pub implementation: Vec<Decl>,
    pub initialization: Vec<Stmt>,
    pub finalization: Vec<Stmt>,
}
