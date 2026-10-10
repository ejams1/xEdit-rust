// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! The parser corners the front end keeps from `JvInterpreter.pas`: the
//! shapes of `for`, `case`, `try`, nested routines, routine headers with
//! directives and defaults, the unit-file grammar, the expression stack's
//! associativity, adjacent string constants, sets, argument lists, and the
//! syntax errors with their interpreter codes.

use xedit_script::ast::*;
use xedit_script::parse;

fn unit(source: &str) -> Module {
    parse(source.as_bytes()).unwrap_or_else(|error| panic!("parse failed: {} (at {})", error.message, error.pos))
}

fn fail(source: &str) -> xedit_script::Error {
    parse(source.as_bytes()).expect_err("parse should fail")
}

fn first_routine(module: &Module) -> &RoutineDecl {
    module
        .items
        .iter()
        .find_map(|item| match &item.node {
            ItemNode::Routine(routine) => Some(&**routine),
            _ => None,
        })
        .expect("a routine")
}

fn body(module: &Module) -> &Block {
    first_routine(module).body.as_ref().expect("a body")
}

fn assignment(stmt: &Stmt) -> (&Expr, &Expr) {
    match &stmt.node {
        StmtNode::Assign { target, value } => (target, value),
        other => panic!("not an assignment: {other:?}"),
    }
}

fn ident(expr: &Expr) -> &str {
    match &expr.node {
        ExprNode::Ident { name, .. } => name,
        other => panic!("not an identifier: {other:?}"),
    }
}

fn int(expr: &Expr) -> &[u8] {
    match &expr.node {
        ExprNode::Int(text) => text,
        other => panic!("not an integer: {other:?}"),
    }
}

fn binary(expr: &Expr) -> (BinOp, &Expr, &Expr) {
    match &expr.node {
        ExprNode::Binary { op, left, right } => (*op, left, right),
        other => panic!("not a binary expression: {other:?}"),
    }
}

#[test]
fn assignment_with_member_target() {
    let module = unit("unit u;\nprocedure P;\nbegin\n  a.b := 1;\nend;\nend.\n");
    let block = body(&module);
    assert_eq!(block.stmts.len(), 1);
    let (target, value) = assignment(&block.stmts[0]);
    match &target.node {
        ExprNode::Member { base, name, .. } => {
            assert_eq!(name, "b");
            assert_eq!(ident(base), "a");
        }
        other => panic!("not a member: {other:?}"),
    }
    assert_eq!(int(value), b"1");
}

#[test]
fn call_with_open_array_and_named_argument() {
    let module = unit("unit u;\nprocedure P;\nbegin\n  AddMessage(s, [1, 2]);\nend;\nend.\n");
    let stmt = &body(&module).stmts[0];
    let StmtNode::Expr(expr) = &stmt.node else {
        panic!("not an expression statement")
    };
    let ExprNode::Ident { name, args, indexed } = &expr.node else {
        panic!("not a call")
    };
    assert_eq!(name, "AddMessage");
    assert!(!indexed);
    assert_eq!(args.len(), 2);
    assert_eq!(args[0].name.as_deref(), Some("s"));
    assert!(matches!(&args[0].value, ArgValue::Expr(e) if ident(e) == "s"));
    match &args[1].value {
        ArgValue::OpenArray(items) => {
            assert_eq!(items.len(), 2);
            assert_eq!(int(&items[0]), b"1");
            assert_eq!(int(&items[1]), b"2");
        }
        other => panic!("not an open array: {other:?}"),
    }

    // The empty C-style argument list.
    let module = unit("unit u;\nprocedure P;\nbegin\n  f();\nend;\nend.\n");
    let StmtNode::Expr(expr) = &body(&module).stmts[0].node else {
        panic!()
    };
    assert!(matches!(&expr.node, ExprNode::Ident { args, .. } if args.is_empty()));
}

#[test]
fn member_chain_with_call() {
    let module = unit("unit u;\nprocedure P;\nbegin\n  x := f(a).b;\nend;\nend.\n");
    let (_, value) = assignment(&body(&module).stmts[0]);
    let ExprNode::Member { base, name, .. } = &value.node else {
        panic!("not a member")
    };
    assert_eq!(name, "b");
    let ExprNode::Ident { name, args, .. } = &base.node else {
        panic!("not a call")
    };
    assert_eq!(name, "f");
    assert_eq!(args.len(), 1);
}

#[test]
fn for_to_and_downto() {
    let module = unit("unit u;\nprocedure P;\nbegin\n  for i := 1 to 10 do AddMessage('i');\nend;\nend.\n");
    let StmtNode::For { var, down, .. } = &body(&module).stmts[0].node else {
        panic!("not a for")
    };
    assert_eq!(var, "i");
    assert!(!down);

    let module = unit("unit u;\nprocedure P;\nbegin\n  for i := 10 downto 1 do AddMessage('i');\nend;\nend.\n");
    let StmtNode::For { down, .. } = &body(&module).stmts[0].node else {
        panic!("not a for")
    };
    assert!(down);
}

#[test]
fn case_shapes() {
    let module = unit(
        "unit u;\nprocedure P;\nbegin\n  case x of\n    0..9, 12: a;\n    15: b;\n  else c;\n  end;\nend;\nend.\n",
    );
    let StmtNode::Case {
        arms,
        else_arm,
        selector,
    } = &body(&module).stmts[0].node
    else {
        panic!("not a case")
    };
    assert_eq!(ident(selector), "x");
    assert_eq!(arms.len(), 2);
    assert_eq!(arms[0].labels.len(), 2);
    assert_eq!(int(&arms[0].labels[0].lo), b"0");
    assert_eq!(int(arms[0].labels[0].hi.as_ref().unwrap()), b"9");
    assert!(arms[0].labels[1].hi.is_none());
    assert_eq!(int(&arms[0].labels[1].lo), b"12");
    assert_eq!(arms[1].labels.len(), 1);
    assert!(else_arm.is_some());

    // Without an else arm; the last arm needs no ';' here (the interpreter's
    // skipping grammar is stricter about it, see interpreter.rs).
    let module = unit("unit u;\nprocedure P;\nbegin\n  case x of\n    1: a;\n    2: b;\n  end;\nend;\nend.\n");
    let StmtNode::Case { arms, else_arm, .. } = &body(&module).stmts[0].node else {
        panic!()
    };
    assert_eq!(arms.len(), 2);
    assert!(else_arm.is_none());
}

#[test]
fn while_repeat_break_continue_exit() {
    let module = unit(
        "unit u;\nprocedure P;\nbegin\n  while i < 10 do begin\n    i := i + 1;\n    if i = 3 then Continue;\n    if i = 5 then Break;\n    if i = 7 then Exit;\n  end;\n  repeat\n    i := i - 1;\n  until i = 0;\nend;\nend.\n",
    );
    let block = body(&module);
    assert_eq!(block.stmts.len(), 2);
    assert!(matches!(block.stmts[0].node, StmtNode::While { .. }));
    let StmtNode::Repeat { body, cond } = &block.stmts[1].node else {
        panic!("not a repeat")
    };
    assert_eq!(body.stmts.len(), 1);
    assert_eq!(binary(cond).0, BinOp::Equ);
    let StmtNode::While { body, .. } = &block.stmts[0].node else {
        panic!()
    };
    let StmtNode::Block(inner) = &body.node else {
        panic!("not a block")
    };
    let kinds: Vec<&StmtNode> = inner.stmts.iter().map(|s| &s.node).collect();
    assert!(matches!(kinds[1], StmtNode::If { .. }));
    let StmtNode::If { then_branch, .. } = kinds[1] else {
        panic!()
    };
    assert!(matches!(then_branch.node, StmtNode::Continue));
    let StmtNode::If { then_branch, .. } = kinds[2] else {
        panic!()
    };
    assert!(matches!(then_branch.node, StmtNode::Break));
    let StmtNode::If { then_branch, .. } = kinds[3] else {
        panic!()
    };
    assert!(matches!(then_branch.node, StmtNode::Exit));
}

#[test]
fn try_finally_and_except_forms() {
    let module = unit("unit u;\nprocedure P;\nbegin\n  try\n    a := 1;\n  finally\n    b := 2;\n  end;\nend;\nend.\n");
    let StmtNode::Try { handler, .. } = &body(&module).stmts[0].node else {
        panic!("not a try")
    };
    let Handler::Finally(block) = handler else {
        panic!("not a finally")
    };
    assert_eq!(block.stmts.len(), 1);

    let module =
        unit("unit u;\nprocedure P;\nbegin\n  try\n    a := 1;\n  except\n    AddMessage('e');\n  end;\nend;\nend.\n");
    let StmtNode::Try { handler, .. } = &body(&module).stmts[0].node else {
        panic!()
    };
    let Handler::Except {
        ons,
        body: plain,
        else_arm,
    } = handler
    else {
        panic!("not an except")
    };
    assert!(ons.is_empty());
    assert_eq!(plain.as_ref().unwrap().stmts.len(), 1);
    assert!(else_arm.is_none());

    let module = unit(
        "unit u;\nprocedure P;\nbegin\n  try\n    a := 1;\n  except\n    on E: Exception do AddMessage(E.Message);\n    on EAbort do b := 2;\n  else\n    c := 3;\n  end;\nend;\nend.\n",
    );
    let StmtNode::Try { handler, .. } = &body(&module).stmts[0].node else {
        panic!()
    };
    let Handler::Except { ons, body, else_arm } = handler else {
        panic!()
    };
    assert!(body.is_none());
    assert_eq!(ons.len(), 2);
    assert_eq!(ons[0].var.as_deref(), Some("E"));
    assert_eq!(ons[0].class, "Exception");
    assert_eq!(ons[1].var, None);
    assert_eq!(ons[1].class, "EAbort");
    assert!(else_arm.is_some());
}

#[test]
fn raise_forms() {
    let module = unit("unit u;\nprocedure P;\nbegin\n  raise;\nend;\nend.\n");
    assert!(matches!(body(&module).stmts[0].node, StmtNode::Raise { value: None }));
    let module = unit("unit u;\nprocedure P;\nbegin\n  raise Exception.Create('x');\nend;\nend.\n");
    let StmtNode::Raise { value: Some(value) } = &body(&module).stmts[0].node else {
        panic!("not a raise with a value")
    };
    assert!(matches!(&value.node, ExprNode::Member { name, .. } if name == "Create"));
}

#[test]
fn nested_routine_in_a_block() {
    let module = unit(
        "unit u;\nprocedure Outer;\nbegin\n  procedure Inner;\n  begin\n    AddMessage('i');\n  end;\n  Inner;\nend;\nend.\n",
    );
    let StmtNode::LocalRoutine(inner) = &body(&module).stmts[0].node else {
        panic!("not a local routine")
    };
    assert_eq!(inner.header.name, "Inner");
    assert!(inner.body.is_some());
    assert_eq!(body(&module).stmts.len(), 2);
}

#[test]
fn expression_associativity_and_unary() {
    let module = unit("unit u;\nprocedure P;\nbegin\n  x := 2 + 3 * 4 - 5;\nend;\nend.\n");
    let (_, value) = assignment(&body(&module).stmts[0]);
    let (op, left, right) = binary(value);
    assert_eq!(op, BinOp::Minus);
    assert_eq!(int(right), b"5");
    let (op, left2, right2) = binary(left);
    assert_eq!(op, BinOp::Plus);
    assert_eq!(int(left2), b"2");
    let (op, left3, right3) = binary(right2);
    assert_eq!(op, BinOp::Mul);
    assert_eq!(int(left3), b"3");
    assert_eq!(int(right3), b"4");

    let module = unit("unit u;\nprocedure P;\nbegin\n  x := -a * b;\nend;\nend.\n");
    let (_, value) = assignment(&body(&module).stmts[0]);
    let (op, left, _) = binary(value);
    assert_eq!(op, BinOp::Mul);
    assert!(matches!(&left.node, ExprNode::Unary { op: UnOp::Minus, .. }));

    let module = unit("unit u;\nprocedure P;\nbegin\n  x := not a and b;\nend;\nend.\n");
    let (_, value) = assignment(&body(&module).stmts[0]);
    let (op, left, _) = binary(value);
    assert_eq!(op, BinOp::And);
    assert!(matches!(&left.node, ExprNode::Unary { op: UnOp::Not, .. }));

    let module = unit("unit u;\nprocedure P;\nbegin\n  x := (1 + 2) * 3;\nend;\nend.\n");
    let (_, value) = assignment(&body(&module).stmts[0]);
    let (op, left, right) = binary(value);
    assert_eq!(op, BinOp::Mul);
    assert_eq!(int(right), b"3");
    assert_eq!(binary(left).0, BinOp::Plus);
}

#[test]
fn adjacent_string_constants_concatenate() {
    let module = unit("unit u;\nprocedure P;\nbegin\n  s := 'a''b' 'c';\nend;\nend.\n");
    let (_, value) = assignment(&body(&module).stmts[0]);
    match &value.node {
        ExprNode::Str(bytes) => assert_eq!(bytes, b"a'bc"),
        other => panic!("not a string: {other:?}"),
    }
}

#[test]
fn set_literals() {
    let module = unit("unit u;\nprocedure P;\nbegin\n  s := [caUnknown, 3];\n  t := [];\nend;\nend.\n");
    let block = body(&module);
    let (_, value) = assignment(&block.stmts[0]);
    let ExprNode::Set { items } = &value.node else {
        panic!("not a set")
    };
    assert_eq!(items.len(), 2);
    assert_eq!(ident(&items[0]), "caUnknown");
    assert_eq!(int(&items[1]), b"3");
    let (_, value) = assignment(&block.stmts[1]);
    assert!(matches!(&value.node, ExprNode::Set { items } if items.is_empty()));
}

#[test]
fn string_and_hex_and_float_literals() {
    let module = unit("unit u;\nprocedure P;\nbegin\n  a := $FF;\n  b := 1.5e+3;\n  c := true;\nend;\nend.\n");
    let block = body(&module);
    let (_, value) = assignment(&block.stmts[0]);
    assert_eq!(int(value), b"$FF");
    let (_, value) = assignment(&block.stmts[1]);
    assert!(matches!(&value.node, ExprNode::Float(text) if text == b"1.5e+3"));
    let (_, value) = assignment(&block.stmts[2]);
    assert!(matches!(&value.node, ExprNode::Bool(true)));
}

#[test]
fn unit_grammar_with_interface_and_implementation() {
    let module = unit(
        "unit api;\n\
         interface\n\
         uses SysUtils, Classes;\n\
         const LowInteger = Low(Integer);\n\
         type\n\
           IThing = IInterface;\n\
           TMode = (mOne, mTwo, mThree);\n\
           TFoo = class(TObject)\n\
             FCount: integer;\n\
             procedure Run(x: integer);\n\
           end;\n\
         implementation\n\
         function Add(a, b: integer = 1): integer; overload;\n\
         begin\n\
           Result := a + b;\n\
         end;\n\
         end.\n",
    );
    assert_eq!(module.name, "api");
    assert_eq!(module.kind, ModuleKind::Unit);
    assert_eq!(module.items.len(), 6);
    // uses
    let ItemNode::Uses(uses) = &module.items[0].node else {
        panic!("not a uses")
    };
    assert_eq!(uses.units.len(), 2);
    assert_eq!(uses.units[0].name, "SysUtils");
    assert_eq!(module.items[0].section, Section::Interface);
    // const
    let ItemNode::Const(decl) = &module.items[1].node else {
        panic!("not a const")
    };
    assert_eq!(decl.name, "LowInteger");
    // type alias / enum / class
    let ItemNode::Type(alias) = &module.items[2].node else {
        panic!("not a type")
    };
    assert!(matches!(&alias.def, TypeDef::Alias(TypeRef::Named(name)) if name == "IInterface"));
    let ItemNode::Type(enumeration) = &module.items[3].node else {
        panic!()
    };
    match &enumeration.def {
        TypeDef::Enum { values } => assert_eq!(values, &["mOne", "mTwo", "mThree"]),
        other => panic!("not an enum: {other:?}"),
    }
    let ItemNode::Type(class) = &module.items[4].node else {
        panic!()
    };
    match &class.def {
        TypeDef::Class(class) => {
            assert_eq!(class.parent, "TObject");
            assert_eq!(class.members.len(), 2);
            let ClassMember::Field(field) = &class.members[0] else {
                panic!("not a field")
            };
            assert_eq!(field.names, &["FCount"]);
            let ClassMember::Method(method) = &class.members[1] else {
                panic!("not a method")
            };
            assert_eq!(method.name, "Run");
            assert_eq!(method.params.len(), 1);
            assert_eq!(method.params[0].name, "x");
            assert_eq!(method.params[0].type_name, "integer");
        }
        other => panic!("not a class: {other:?}"),
    }
    // implementation routine
    let routine = first_routine(&module);
    assert_eq!(module.items[5].section, Section::Implementation);
    assert_eq!(routine.header.name, "Add");
    assert_eq!(routine.header.params.len(), 2);
    assert_eq!(routine.header.params[0].name, "a");
    assert_eq!(routine.header.params[1].name, "b");
    assert_eq!(routine.header.params[0].type_name, "integer");
    assert!(matches!(
        routine.header.result,
        Some(TypeRef::Named(ref name)) if name == "integer"
    ));
    assert_eq!(routine.directives, vec!["overload".to_owned()]);
    assert!(routine.body.is_some());
}

#[test]
fn var_and_const_sections_hold_several_declarations() {
    let module = unit(
        "unit u;\nvar\n  a, b: integer;\n  c: string;\nconst\n  x = 1;\n  y = 2;\nprocedure P;\nbegin\nend;\nend.\n",
    );
    let vars: Vec<&VarGroup> = module
        .items
        .iter()
        .filter_map(|item| match &item.node {
            ItemNode::Var(group) => Some(group),
            _ => None,
        })
        .collect();
    assert_eq!(vars.len(), 2);
    assert_eq!(vars[0].names, &["a", "b"]);
    assert_eq!(vars[1].names, &["c"]);
    let consts: Vec<&ConstDecl> = module
        .items
        .iter()
        .filter_map(|item| match &item.node {
            ItemNode::Const(decl) => Some(decl),
            _ => None,
        })
        .collect();
    assert_eq!(consts.len(), 2);
    assert_eq!(int(&consts[1].value), b"2");
}

#[test]
fn local_var_and_const_sections() {
    let module = unit(
        "unit u;\nprocedure P;\nvar\n  a: integer;\nconst\n  b = 1;\nvar\n  c: string;\nbegin\n  a := b;\nend;\nend.\n",
    );
    let routine = first_routine(&module);
    assert_eq!(routine.locals.len(), 3);
    assert!(matches!(routine.locals[0], LocalDecl::Var(_)));
    assert!(matches!(routine.locals[1], LocalDecl::Const(_)));
    assert!(matches!(routine.locals[2], LocalDecl::Var(_)));
}

#[test]
fn class_dot_method_header() {
    let module = unit("unit u;\ninterface\nimplementation\nprocedure TFoo.Bar(x: integer);\nbegin\nend;\nend.\n");
    let routine = first_routine(&module);
    assert_eq!(routine.header.class.as_deref(), Some("TFoo"));
    assert_eq!(routine.header.name, "Bar");
}

#[test]
fn array_types_in_var_and_result() {
    let module = unit(
        "unit u;\nvar\n  a: array [0..9] of integer;\n  b: array of string;\n  c: array [1..2, 3..4] of integer;\nprocedure P;\nbegin\nend;\nend.\n",
    );
    let vars: Vec<&VarGroup> = module
        .items
        .iter()
        .filter_map(|item| match &item.node {
            ItemNode::Var(group) => Some(group),
            _ => None,
        })
        .collect();
    match &vars[0].ty {
        TypeRef::Array { ranges, element } => {
            assert_eq!(ranges, &[(0, 9)]);
            assert!(matches!(&**element, TypeRef::Named(name) if name == "integer"));
        }
        other => panic!("not an array: {other:?}"),
    }
    match &vars[1].ty {
        TypeRef::Array { ranges, .. } => assert_eq!(ranges, &[(0, -1)]),
        other => panic!("not an array: {other:?}"),
    }
    match &vars[2].ty {
        TypeRef::Array { ranges, element } => {
            assert_eq!(ranges, &[(1, 2), (3, 4)]);
            assert!(matches!(&**element, TypeRef::Named(name) if name == "integer"));
        }
        other => panic!("not an array: {other:?}"),
    }
    // `ReadFunctionHeader` requires an identifier before the result type
    // (`:7931`), so an array result does not parse.
    let error = fail("unit u;\nfunction P: array [1..2] of integer;\nbegin\nend;\nend.\n");
    assert_eq!(error.message, "Identifier expected but 'array' found");
}

#[test]
fn empty_then_branch() {
    // `InterpretStatement` treats `else` in statement position as a no-op
    // (`:6667`), which makes an empty then branch valid.
    let module = unit("unit u;\nprocedure P;\nbegin\n  if a then\n  else AddMessage('x');\nend;\nend.\n");
    let StmtNode::If {
        then_branch,
        else_branch,
        ..
    } = &body(&module).stmts[0].node
    else {
        panic!("not an if")
    };
    assert!(matches!(then_branch.node, StmtNode::Empty));
    assert!(else_branch.is_some());
}

#[test]
fn compile_mode_scans_bodies_without_parsing_them() {
    // The compile (`InterpretFunction`: FindToken(ttBegin); SkipToEnd) finds
    // the body's balanced `end` and does not parse the statements, so a
    // statement-level error of a shipped script still compiles; the full
    // parser (the runtime grammar) rejects the same source.
    let source =
        "unit u;\nprocedure P;\nbegin\n  for i := 0 to Pred(ElementCount(ents) do\n    AddMessage('x');\nend;\nend.\n";
    let module = xedit_script::interpreter::parse_compile(source.as_bytes()).expect("compiles");
    assert!(first_routine(&module).body.is_none());
    assert!(parse(source.as_bytes()).is_err());
}

#[test]
fn program_form() {
    let module = unit("program p;\nvar x: integer;\nbegin\n  x := 1\nend.\n");
    assert_eq!(module.kind, ModuleKind::Program);
    assert_eq!(module.name, "p");
    let block = module.body.as_ref().expect("a body");
    assert_eq!(block.stmts.len(), 1);
}

#[test]
fn syntax_errors_carry_interpreter_codes() {
    // Missing operator or semicolon (ieMissingOperator).
    let error = fail("unit u;\nprocedure P;\nbegin\n  x := 1 y;\nend;\nend.\n");
    assert_eq!(error.code, 110);
    assert_eq!(error.message, "Missing operator or semicolon");

    // An `else` the `;` left outside the `if` (ieExpected, "Statement").
    let error = fail("unit u;\nprocedure P;\nbegin\n  if a then b; else c;\nend;\nend.\n");
    assert_eq!(error.code, 103);
    assert_eq!(error.message, "Statement expected but 'else' found");

    // A file that is no unit.
    let error = fail("procedure P;\nbegin\nend.\n");
    assert_eq!(error.code, 103);
    assert_eq!(error.message, "'unit' expected but 'procedure' found");

    // A unit without its final '.'.
    let error = fail("unit u;\nend\n");
    assert_eq!(error.message, "'.' expected but End of File found");

    // A bad remark (ieBadRemark from the tokenizer).
    let error = fail("unit u; { open\nend.\n");
    assert_eq!(error.code, 101);
    assert_eq!(error.message, "Error in remark");

    // A procedure without a result type is checked the other way.
    let error = fail("unit u;\nfunction f;\nbegin\nend;\nend.\n");
    assert_eq!(error.message, "':' expected but ';' found");

    // A `case` without `of`.
    let error = fail("unit u;\nprocedure P;\nbegin\n  case x;\n  end;\nend;\nend.\n");
    assert_eq!(error.message, "'of' expected but ';' found");
}

#[test]
fn error_lines_count_carriage_returns() {
    // The line of the error: #13 characters before the position, plus one.
    let source = "unit u;\r\nprocedure P;\r\nbegin\r\n  x := 1 y;\r\nend;\r\nend.\r\n";
    let error = fail(source);
    assert_eq!(xedit_script::error::line_of(source.as_bytes(), error.pos), 4);
    // A LF-only file is one line, as GetLineByPos counts it.
    let source = "unit u;\nprocedure P;\nbegin\n  x := 1 y;\nend;\nend.\n";
    let error = fail(source);
    assert_eq!(xedit_script::error::line_of(source.as_bytes(), error.pos), 1);
}

#[test]
fn comments_and_strings_do_not_confuse_the_tokenizer() {
    let module = unit(
        "unit u;\n{ a remark with begin end; }\n(* another (* nested is not allowed upstream *)\nprocedure P;\nbegin\n  // a line remark with ; and ' strings\n  AddMessage('begin end; { } // ''x''');\nend;\nend.\n",
    );
    let StmtNode::Expr(expr) = &body(&module).stmts[0].node else {
        panic!()
    };
    let ExprNode::Ident { args, .. } = &expr.node else {
        panic!()
    };
    match &args[0].value {
        ArgValue::Expr(Expr {
            node: ExprNode::Str(bytes),
            ..
        }) => assert_eq!(bytes, b"begin end; { } // 'x'"),
        other => panic!("not a string: {other:?}"),
    }
}

#[test]
fn strip_namespaces_removes_delphi_prefixes() {
    let stripped = xedit_script::check::strip_namespaces(b"uses SysUtils, Classes, UITypes, Vcl.Graphics;\nx");
    assert_eq!(stripped, b"uses SysUtils, Classes, UITypes, Graphics;\nx");
    let stripped =
        xedit_script::check::strip_namespaces(b"interface\nimplementation\nuses xEditAPI, Classes, System.SysUtils;\n");
    assert_eq!(
        stripped,
        b"interface\nimplementation\nuses xEditAPI, Classes, SysUtils;\n"
    );
    // A string constant at a line start is stripped too, as the regex does.
    let stripped = xedit_script::check::strip_namespaces(b"s := 1;\nuses System.Foo;\n");
    assert_eq!(stripped, b"s := 1;\nuses Foo;\n");
}

#[test]
fn check_resolves_used_units_from_the_folder() {
    let dir = std::env::temp_dir().join(format!("xedit-script-check-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(
        dir.join("Helper.pas"),
        "unit Helper;\nprocedure Help;\nbegin\nend;\nend.\n",
    )
    .unwrap();
    std::fs::write(
        dir.join("Good.pas"),
        "unit Good;\nuses Helper, SysUtils;\nprocedure P;\nbegin\n  Help;\nend;\nend.\n",
    )
    .unwrap();
    std::fs::write(
        dir.join("Bad.pas"),
        // A compile-level failure: the unit misses its final `end.` (an error
        // inside a routine body would compile -- bodies are scanned).
        "unit Bad;\nuses Helper;\nprocedure P;\nbegin\nend;\n",
    )
    .unwrap();
    let good = xedit_script::check::check_file(&dir.join("Good.pas"), &dir);
    assert!(good.ok(), "{:?}", good.errors);
    let bad = xedit_script::check::check_file(&dir.join("Bad.pas"), &dir);
    assert!(!bad.ok());
    let _ = std::fs::remove_dir_all(&dir);
}
