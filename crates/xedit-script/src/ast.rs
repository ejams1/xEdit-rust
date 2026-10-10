// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

// Ported from xEdit: External/jvcl/jvcl/run/JvInterpreter.pas (the syntax the
// parser of the front end accepts; the tree replaces the positions the
// interpreter records and re-parses at run time).

//! The syntax tree of the JvInterpreter dialect.
//!
//! The interpreter has no separate parse step: `InterpretUnit` and friends
//! tokenize and *execute* the unit in one pass, recording the source position
//! of every routine so `ExecFunction` can re-parse its body later. This crate
//! turns that pass into a parser that builds this tree instead. Every node
//! carries the byte [`Span`] of the source it came from, so the evaluator
//! (phase 6 step 3) can report the positions the interpreter reports
//! (`PosBeg`/`CurPos` of `JvInterpreter.pas`).
//!
//! ## Lowering interface (for the evaluator)
//!
//! * A [`Module`] is what `TJvInterpreterUnit.Compile` reads: `unit`/`program`
//!   header, the uses clauses and the items in source order. Items keep the
//!   [`Section`] they were declared in; the interpreter only scans for bodies
//!   outside `interface`, so declarations without a body (`body: None`) carry
//!   the sig to register, and the `implementation` (or unit-less, `Section::None`,
//!   as every script in the corpus) routine of the same name carries it.
//! * A [`RoutineDecl`] with a body is what `ExecFunction` runs: locals in
//!   declaration order, then the [`Block`]. Parameters carry the upstream name,
//!   mode and the *first* token of the type (the only part of a type
//!   `ReadFunctionHeader` keeps, `JvInterpreter.pas:7827`).
//! * Statements and expressions are trees in the shape the interpreter's
//!   `Expression`/`Interpret*` routines walk while parsing; operator calls are
//!   already precedence-resolved the way the interpreter's expression stack
//!   resolves them, so the evaluator evaluates a [`Binary`](ExprNode::Binary)
//!   node directly instead of reimplementing the priority table.
//! * Names are **not** resolved here. Which unit or local a
//!   [`Ident`](ExprNode::Ident) refers to, and every type question
//!   (`varBoolean`, `ieIntegerRequired`, ...) belongs to the evaluator and the
//!   script host; the parser only reports syntax errors.
//!
//! # This module
//!
//! Values of upstream that have no tree shape of their own are kept on the
//! node that owns them: an argument's `FVarNames` entry is [`Arg::name`]
//! (recorded when the argument's first token is an identifier, exactly as
//! `ReadArgs` records it, `JvInterpreter.pas:5965`), `FCurrArgs.Indexed` is
//! [`ExprNode::Ident::indexed`], and the `open array` argument form of
//! `ReadArgs.ReadOpenArray` is [`ArgValue::OpenArray`].

/// A byte span of the parsed source: `start` inclusive, `end` exclusive.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Span {
    pub start: usize,
    pub end: usize,
}

impl Span {
    pub fn new(start: usize, end: usize) -> Self {
        Self { start, end }
    }
}

/// The section a declaration was read in (`TUnitSection`, `JvInterpreter.pas:1009`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Section {
    /// No `interface`/`implementation` keyword seen yet (every corpus script).
    None,
    Interface,
    Implementation,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ModuleKind {
    Unit,
    Program,
}

/// One source file: the script or a unit it uses.
#[derive(Clone, Debug)]
pub struct Module {
    pub kind: ModuleKind,
    pub name: String,
    pub items: Vec<Item>,
    /// The `begin ... end.` of a `program` module; `None` for a unit.
    pub body: Option<Block>,
    pub span: Span,
}

#[derive(Clone, Debug)]
pub struct Item {
    pub section: Section,
    pub span: Span,
    pub node: ItemNode,
}

#[derive(Clone, Debug)]
pub enum ItemNode {
    Uses(UsesClause),
    Const(ConstDecl),
    Var(VarGroup),
    Type(TypeDecl),
    Routine(Box<RoutineDecl>),
}

#[derive(Clone, Debug)]
pub struct UsesClause {
    pub units: Vec<UsesUnit>,
}

#[derive(Clone, Debug)]
pub struct UsesUnit {
    pub name: String,
    /// The string of the `in '...'` clause, when one is written (`InterpretUses`
    /// skips it, `JvInterpreter.pas:8072`).
    pub in_file: Option<Vec<u8>>,
    pub span: Span,
}

#[derive(Clone, Debug)]
pub struct ConstDecl {
    pub name: String,
    pub value: Expr,
    pub span: Span,
}

/// `a, b: T;` -- one statement of a `var` section (`InterpretVar`,
/// `JvInterpreter.pas:7369`).
#[derive(Clone, Debug)]
pub struct VarGroup {
    pub names: Vec<String>,
    pub ty: TypeRef,
    pub span: Span,
}

#[derive(Clone, Debug)]
pub struct TypeDecl {
    pub name: String,
    pub def: TypeDef,
    pub span: Span,
}

/// The right side of `X = ...;` of a `type` section.
///
/// `InterpretType` (`JvInterpreter.pas:8141`) understands only `class` and
/// `record`; the corpus needs `Alias` and `Enum` too, because `xEditAPI.pas`
/// is a declaration-only reference unit (`IwbElement = IInterface;`,
/// `TwbGameMode = (gmTES3, ...)`) that the checking front end must read, see
/// `interpreter.rs`. The interpreter itself never parses that file at run
/// time: the script host answers `xEditAPI` with the empty unit
/// `unit xEditAPI; end.` (`xejviScriptHost.pas`,
/// `JvInterpreterProgramGetUnitSource`).
#[derive(Clone, Debug)]
pub enum TypeDef {
    Alias(TypeRef),
    Enum { values: Vec<String> },
    Class(ClassDef),
    Record(RecordDef),
}

#[derive(Clone, Debug)]
pub struct ClassDef {
    pub parent: String,
    /// Field and method declarations in source order; visibility markers
    /// (`private` ... `published`) are consumed and dropped as upstream drops
    /// them (`InterpretClass`, `JvInterpreter.pas:8164`).
    pub members: Vec<ClassMember>,
    pub span: Span,
}

#[derive(Clone, Debug)]
pub enum ClassMember {
    Field(VarGroup),
    /// A method declaration only; `InterpretClass` reads the header and reads
    /// no body.
    Method(Box<RoutineHeader>),
}

#[derive(Clone, Debug)]
pub struct RecordDef {
    pub fields: Vec<VarGroup>,
    pub span: Span,
}

/// A routine declaration or definition (`InterpretFunction`,
/// `JvInterpreter.pas:7948`, and `ReadFunctionHeader`, `:7827`).
#[derive(Clone, Debug)]
pub struct RoutineDecl {
    pub header: RoutineHeader,
    /// `var`/`const` sections between the header and `begin` (`InFunction`,
    /// `JvInterpreter.pas:6553`).
    pub locals: Vec<LocalDecl>,
    /// `None` for an `interface` declaration; `Some` for a definition.
    pub body: Option<Block>,
    /// The `external 'dll' [name 'F' | index N]` directive.
    pub external: Option<ExternalDecl>,
    /// `overload`, `forward`, a calling convention, ... -- directive
    /// identifiers after the header. The interpreter understands only
    /// `external`; a second directive makes it fail in an `interface` section
    /// and is skipped by `FindToken(ttBegin)` in an implementation. The front
    /// end accepts them, which `xEditAPI.pas` needs.
    pub directives: Vec<String>,
    pub span: Span,
}

#[derive(Clone, Debug)]
pub enum LocalDecl {
    Const(ConstDecl),
    Var(VarGroup),
}

#[derive(Clone, Debug)]
pub struct RoutineHeader {
    pub name: String,
    /// The `TClass.` prefix of a method declaration (`FClassIdentifier`).
    pub class: Option<String>,
    /// Flattened parameter list, one entry per name, in source order, with the
    /// type and mode of its group applied the way `ReadParams` applies them
    /// (`JvInterpreter.pas:7832`).
    pub params: Vec<Param>,
    /// The result type of a `function` (`ParseDataType` runs on it too); a
    /// `procedure` has none.
    pub result: Option<TypeRef>,
    pub span: Span,
}

#[derive(Clone, Debug)]
pub struct Param {
    pub name: String,
    pub mode: ParamMode,
    /// The first identifier token of the type (`FParamTypeNames`); parameters
    /// with a type the interpreter only skips (a default value, `array of`)
    /// keep that first token, as upstream does.
    pub type_name: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ParamMode {
    Value,
    Var,
    Const,
}

#[derive(Clone, Debug)]
pub struct ExternalDecl {
    /// The DLL: a string literal, or an identifier the interpreter resolves
    /// with `GetValue` at parse time.
    pub dll: Expr,
    pub name: Option<Vec<u8>>,
    pub index: Option<i64>,
    pub span: Span,
}

/// A type reference (`ParseDataType`, `JvInterpreter.pas:7689`).
#[derive(Clone, Debug)]
pub enum TypeRef {
    Named(String),
    /// `array [a..b, ...] of T`. The `array of T` form records the upstream
    /// dimension 1, range `0 .. -1`, as `ParseDataType` stores it.
    Array {
        ranges: Vec<(i64, i64)>,
        element: Box<TypeRef>,
    },
}

#[derive(Clone, Debug)]
pub struct Block {
    pub span: Span,
    pub stmts: Vec<Stmt>,
}

#[derive(Clone, Debug)]
pub struct Stmt {
    pub span: Span,
    pub node: StmtNode,
}

#[derive(Clone, Debug)]
pub enum StmtNode {
    /// `target := value` (`InterpretIdentifier`, `JvInterpreter.pas:7015`).
    Assign {
        target: Expr,
        value: Expr,
    },
    /// An identifier statement: a call, a method call, a bare variable.
    Expr(Expr),
    Block(Block),
    If {
        cond: Expr,
        then_branch: Box<Stmt>,
        else_branch: Option<Box<Stmt>>,
    },
    While {
        cond: Expr,
        body: Box<Stmt>,
    },
    Repeat {
        body: Block,
        cond: Expr,
    },
    For {
        var: String,
        from: Expr,
        to: Expr,
        down: bool,
        body: Box<Stmt>,
    },
    Case {
        selector: Expr,
        arms: Vec<CaseArm>,
        else_arm: Option<Box<Stmt>>,
    },
    Try {
        body: Block,
        handler: Handler,
    },
    Raise {
        /// The exception object expression; `None` re-raises.
        value: Option<Expr>,
    },
    Break,
    Continue,
    Exit,
    /// An empty statement. `InterpretStatement` has no-op arms for `;`, `end`
    /// and `else` (`JvInterpreter.pas:6658-6667`), so `if c then else x;` is a
    /// valid statement whose then branch does nothing.
    Empty,
    /// A routine declared inside a block. The interpreter rejects a
    /// `function`/`procedure` token in `InterpretStatement` (`:6646`); the
    /// front end accepts it so `script check` sees every routine of a file.
    LocalRoutine(Box<RoutineDecl>),
}

#[derive(Clone, Debug)]
pub struct CaseArm {
    pub labels: Vec<CaseLabel>,
    pub body: Box<Stmt>,
    pub span: Span,
}

#[derive(Clone, Debug)]
pub struct CaseLabel {
    pub lo: Expr,
    /// The upper bound of a `a..b` label.
    pub hi: Option<Expr>,
    pub span: Span,
}

#[derive(Clone, Debug)]
pub enum Handler {
    Finally(Block),
    Except {
        /// The `on E: T do ...` clauses, in order.
        ons: Vec<OnClause>,
        /// The plain `except <statements> end` form (`InterpretExcept` parses
        /// it with `InterpretBegin`, `JvInterpreter.pas:7553`).
        body: Option<Block>,
        /// The `else <statement>` arm after `on` clauses.
        else_arm: Option<Box<Stmt>>,
    },
}

#[derive(Clone, Debug)]
pub struct OnClause {
    /// `on E: T do` binds `E`; `on T do` binds nothing.
    pub var: Option<String>,
    pub class: String,
    pub body: Box<Stmt>,
    pub span: Span,
}

#[derive(Clone, Debug)]
pub struct Expr {
    pub span: Span,
    pub node: ExprNode,
}

#[derive(Clone, Debug)]
pub enum ExprNode {
    /// A name with the arguments and the `[...]` form that were attached to it
    /// (`InternalGetValue`, `JvInterpreter.pas:6053`).
    Ident {
        name: String,
        args: Vec<Arg>,
        /// The arguments came from `Name[...]` (`FCurrArgs.Indexed`).
        indexed: bool,
    },
    /// `base.name` and its arguments; member access fetches through the
    /// value's pointer upstream.
    Member {
        base: Box<Expr>,
        name: String,
        args: Vec<Arg>,
        indexed: bool,
    },
    /// Integer constant: the raw token text (`123`, `$FF`), as `Val` reads it.
    Int(Vec<u8>),
    /// Float constant: the raw token text, as `TextToFloat` reads it.
    Float(Vec<u8>),
    /// String constant: the value between the quotes with `''` collapsed
    /// (`ParseToken`, `JvInterpreter.pas:5592`). Adjacent literals are one
    /// node concatenated at parse time, as the `ttString` case of `Expression`
    /// concatenates them (`:5676`).
    Str(Vec<u8>),
    Bool(bool),
    /// `[a, b, c]` (`SetExpression1`, `JvInterpreter.pas:5927`); the items are
    /// identifiers or integers only, as upstream accepts.
    Set {
        items: Vec<Expr>,
    },
    Unary {
        op: UnOp,
        operand: Box<Expr>,
    },
    Binary {
        op: BinOp,
        left: Box<Expr>,
        right: Box<Expr>,
    },
}

#[derive(Clone, Debug)]
pub struct Arg {
    /// The identifier the argument started with when it started with one
    /// (`FVarNames`); it names a `var`/`out` parameter the call writes back to.
    pub name: Option<String>,
    pub span: Span,
    pub value: ArgValue,
}

#[derive(Clone, Debug)]
pub enum ArgValue {
    Expr(Expr),
    /// The `[a, b]` form in an argument list (`ReadArgs.ReadOpenArray`,
    /// `JvInterpreter.pas:5967`); a range is not accepted there.
    OpenArray(Vec<Expr>),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum UnOp {
    Plus,
    Minus,
    Not,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BinOp {
    Plus,
    Minus,
    Mul,
    Div,
    IntDiv,
    Mod,
    And,
    Or,
    Xor,
    Shl,
    Shr,
    Equ,
    NotEqu,
    Greater,
    Less,
    EquGreater,
    EquLess,
}
