// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

// Ported from xEdit: xEdit/JvI/xejviScriptAdapterMiscRegEx.pas (the
// TPerlRegEx class and its registrations) over
// `xedit_core::perl_regex::PerlRegEx`, which is the class of Delphi's
// `System.RegularExpressionsCore` (`TPerlRegEx` by Jan Goyvaerts: Match,
// MatchAgain, Replace, ReplaceAll, ComputeReplacement, Split, SplitCapture,
// StoreGroups, Study, Compile, EscapeRegExChars, the Start/Stop window and
// the group accessors) on top of the port's PCRE engine
// (`xedit_core::pcre`). `System.RegularExpressionsConsts` gives the
// `ERegularExpressionError` texts.

//! `TPerlRegEx` for scripts.
//!
//! The engine is PCRE 8, as the release's run-time library uses it: the
//! call shape, the option and state flags, the offset bases, the error
//! texts and the behaviour are the upstream ones, and the check is
//! `cargo xtask parity script --script "Regex probe" --phase run`.

use std::rc::Rc;

use xedit_core::perl_regex::{
    E_REGEX_CLASS, OPTION_ANCHORED, OPTION_CASE_LESS, OPTION_EXTENDED, OPTION_MULTI_LINE,
    OPTION_NO_AUTO_CAPTURE, OPTION_SINGLE_LINE, OPTION_UN_GREEDY, PerlRegEx, RegexError,
    STATE_NOT_BOL, STATE_NOT_EOL, STATE_NOT_EMPTY,
};
use xedit_script::adapter::{Adapter, Args, PR_ARGS_NO_CHECK};
use xedit_script::values::{ClassDef, ParamType, ScriptObject, Value, ValueError, VarType};

use crate::script_classes::ScriptClasses;
use crate::script_strings::{borrow_list_mut, list_of_or_error, receiver, text_arg};

pub use xedit_core::perl_regex::{
    E_REGEX_CLASS as E_REGEX, S_REGEX_MATCH_REQUIRED as S_MATCH_REQUIRED,
    S_REGEX_STRINGS_REQUIRED as S_STRINGS_REQUIRED,
};

/// An `ERegularExpressionError` of the RTL's class.
fn regex_error(error: RegexError) -> ValueError {
    ValueError::external_class(E_REGEX_CLASS, error.message())
}

/// One `TPerlRegEx` script object: the class value it was created from and
/// the class of the RTL around the PCRE engine.
pub struct PerlRegExObject {
    class: Rc<ClassDef>,
    regex: PerlRegEx,
}

impl PerlRegExObject {
    pub fn new(class: Rc<ClassDef>) -> PerlRegExObject {
        PerlRegExObject {
            class,
            regex: PerlRegEx::new(),
        }
    }

    // Every accessor the adapter's registrations use, with the RTL's error
    // class.

    fn compile(&mut self) -> Result<(), ValueError> {
        self.regex.compile().map_err(regex_error)
    }

    fn study(&mut self) -> Result<(), ValueError> {
        self.regex.study().map_err(regex_error)
    }

    fn match_(&mut self) -> Result<bool, ValueError> {
        self.regex.match_().map_err(regex_error)
    }

    fn match_again(&mut self) -> Result<bool, ValueError> {
        self.regex.match_again().map_err(regex_error)
    }

    fn replace(&mut self) -> Result<String, ValueError> {
        self.regex.replace().map_err(regex_error)
    }

    fn replace_all(&mut self) -> Result<bool, ValueError> {
        self.regex.replace_all().map_err(regex_error)
    }

    fn compute_replacement(&self) -> Result<String, ValueError> {
        self.regex.compute_replacement().map_err(regex_error)
    }

    fn groups(&self, index: i64) -> Result<String, ValueError> {
        self.regex.groups(index).map_err(regex_error)
    }

    fn group_offsets(&self, index: i64) -> Result<i64, ValueError> {
        self.regex.group_offsets(index).map_err(regex_error)
    }

    fn group_lengths(&self, index: i64) -> Result<i64, ValueError> {
        self.regex.group_lengths(index).map_err(regex_error)
    }

    fn store_groups(&mut self) -> Result<(), ValueError> {
        self.regex.store_groups().map_err(regex_error)
    }

    fn split(&mut self, list: &mut Vec<String>, limit: i32) -> Result<(), ValueError> {
        self.regex.split(list, limit).map_err(regex_error)
    }

    fn split_capture(&mut self, list: &mut Vec<String>, limit: i32) -> Result<(), ValueError> {
        self.regex.split_capture(list, limit).map_err(regex_error)
    }

    fn found_match(&self) -> bool {
        self.regex.found_match()
    }

    fn group_count(&self) -> i64 {
        self.regex.group_count()
    }

    fn compiled(&self) -> bool {
        self.regex.compiled()
    }

    fn studied(&self) -> bool {
        self.regex.studied()
    }

    fn named_group(&self, name: &str) -> i64 {
        self.regex.named_group(name)
    }

    fn subject_left(&self) -> String {
        self.regex.subject_left()
    }

    fn subject_right(&self) -> String {
        self.regex.subject_right()
    }

    fn escape_regex_chars(text: &str) -> String {
        PerlRegEx::escape_regex_chars(text)
    }
}

impl ScriptObject for PerlRegExObject {
    fn class(&self) -> Rc<ClassDef> {
        self.class.clone()
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn std::any::Any {
        self
    }
}

/// The `TPerlRegEx` of an `Args.Obj` receiver.
fn regex_of(value: &Value) -> Option<Rc<std::cell::RefCell<dyn ScriptObject>>> {
    let object = value.as_object()?;
    if object.borrow().as_any().is::<PerlRegExObject>() {
        Some(object.clone())
    } else {
        None
    }
}

fn regex_obj(value: &Value) -> Result<Rc<std::cell::RefCell<dyn ScriptObject>>, ValueError> {
    regex_of(value).ok_or_else(|| {
        ValueError::external_class("EInvalidCast", "Invalid cast: the value is not a TPerlRegEx".to_owned())
    })
}

fn borrow_regex(object: &Rc<std::cell::RefCell<dyn ScriptObject>>) -> std::cell::Ref<'_, PerlRegExObject> {
    std::cell::Ref::map(object.borrow(), |object| {
        object
            .as_any()
            .downcast_ref::<PerlRegExObject>()
            .expect("a TPerlRegEx object")
    })
}

fn borrow_regex_mut(object: &Rc<std::cell::RefCell<dyn ScriptObject>>) -> std::cell::RefMut<'_, PerlRegExObject> {
    std::cell::RefMut::map(object.borrow_mut(), |object| {
        object
            .as_any_mut()
            .downcast_mut::<PerlRegExObject>()
            .expect("a TPerlRegEx object")
    })
}

fn receiver_regex(args: &Args) -> Result<Rc<std::cell::RefCell<dyn ScriptObject>>, ValueError> {
    match &args.obj {
        Some(object) => regex_obj(object),
        None => Err(ValueError::external_class(
            "EInvalidCast",
            "Invalid cast: the value is not a TPerlRegEx".to_owned(),
        )),
    }
}

fn value_param() -> ParamType {
    ParamType::value(VarType::Empty)
}

/// Registers `xejviScriptAdapterMiscRegEx.pas`.
pub fn register(adapter: &mut Adapter, classes: &ScriptClasses) {
    //=== The constants =======================================================

    for (name, value) in [
        ("preCaseLess", OPTION_CASE_LESS),
        ("preMultiLine", OPTION_MULTI_LINE),
        ("preSingleLine", OPTION_SINGLE_LINE),
        ("preExtended", OPTION_EXTENDED),
        ("preAnchored", OPTION_ANCHORED),
        ("preUnGreedy", OPTION_UN_GREEDY),
        ("preNoAutoCapture", OPTION_NO_AUTO_CAPTURE),
        ("preNotBOL", STATE_NOT_BOL),
        ("preNotEOL", STATE_NOT_EOL),
        ("preNotEmpty", STATE_NOT_EMPTY),
    ] {
        adapter.add_const("RegularExpressionsCore", name, Value::Integer(value));
    }

    let class = classes.t_perl_regex.clone();

    // `TPerlRegEx_Create`: called on the class value as upstream.
    {
        let class = class.clone();
        adapter.add_get(
            classes.t_perl_regex.clone(),
            "Create",
            Rc::new(move |value: &mut Value, _args: &mut Args| {
                *value = Value::Object(Rc::new(std::cell::RefCell::new(PerlRegExObject::new(class.clone()))));
                Ok(())
            }),
            0,
            Vec::new(),
            VarType::Empty,
        );
    }

    adapter.add_get(
        class.clone(),
        "Compile",
        Rc::new(|_value: &mut Value, args: &mut Args| {
            let object = receiver_regex(args)?;
            borrow_regex_mut(&object).compile()
        }),
        0,
        Vec::new(),
        VarType::Empty,
    );
    adapter.add_get(
        class.clone(),
        "Compiled",
        Rc::new(|value: &mut Value, args: &mut Args| {
            let object = receiver_regex(args)?;
            *value = Value::Bool(borrow_regex(&object).compiled());
            Ok(())
        }),
        0,
        Vec::new(),
        VarType::Empty,
    );
    adapter.add_get(
        class.clone(),
        "Study",
        Rc::new(|_value: &mut Value, args: &mut Args| {
            let object = receiver_regex(args)?;
            borrow_regex_mut(&object).study()
        }),
        0,
        Vec::new(),
        VarType::Empty,
    );
    adapter.add_get(
        class.clone(),
        "Studied",
        Rc::new(|value: &mut Value, args: &mut Args| {
            let object = receiver_regex(args)?;
            *value = Value::Bool(borrow_regex(&object).studied());
            Ok(())
        }),
        0,
        Vec::new(),
        VarType::Empty,
    );
    adapter.add_get(
        class.clone(),
        "Match",
        Rc::new(|value: &mut Value, args: &mut Args| {
            let object = receiver_regex(args)?;
            let matched = borrow_regex_mut(&object).match_()?;
            *value = Value::Bool(matched);
            Ok(())
        }),
        0,
        Vec::new(),
        VarType::Empty,
    );
    adapter.add_get(
        class.clone(),
        "MatchAgain",
        Rc::new(|value: &mut Value, args: &mut Args| {
            let object = receiver_regex(args)?;
            let matched = borrow_regex_mut(&object).match_again()?;
            *value = Value::Bool(matched);
            Ok(())
        }),
        0,
        Vec::new(),
        VarType::Empty,
    );
    adapter.add_get(
        class.clone(),
        "FoundMatch",
        Rc::new(|value: &mut Value, args: &mut Args| {
            let object = receiver_regex(args)?;
            *value = Value::Bool(borrow_regex(&object).found_match());
            Ok(())
        }),
        0,
        Vec::new(),
        VarType::Empty,
    );
    adapter.add_get(
        class.clone(),
        "GroupCount",
        Rc::new(|value: &mut Value, args: &mut Args| {
            let object = receiver_regex(args)?;
            let regex = borrow_regex(&object);
            if !regex.found_match() {
                return Err(regex_error(RegexError::MatchRequired));
            }
            *value = Value::Integer(regex.group_count() as i32);
            Ok(())
        }),
        0,
        Vec::new(),
        VarType::Empty,
    );
    adapter.add_iget(
        class.clone(),
        "Groups",
        Rc::new(|value: &mut Value, args: &mut Args| {
            let index = xedit_script::values::to_i32(&args.values[0])?;
            let object = receiver_regex(args)?;
            *value = Value::Str(borrow_regex(&object).groups(i64::from(index))?.into_bytes());
            Ok(())
        }),
        1,
        vec![value_param()],
        VarType::Empty,
    );
    adapter.add_iget(
        class.clone(),
        "GroupOffsets",
        Rc::new(|value: &mut Value, args: &mut Args| {
            let index = xedit_script::values::to_i32(&args.values[0])?;
            let object = receiver_regex(args)?;
            *value = Value::Integer(borrow_regex(&object).group_offsets(i64::from(index))? as i32);
            Ok(())
        }),
        1,
        vec![value_param()],
        VarType::Empty,
    );
    adapter.add_iget(
        class.clone(),
        "GroupLengths",
        Rc::new(|value: &mut Value, args: &mut Args| {
            let index = xedit_script::values::to_i32(&args.values[0])?;
            let object = receiver_regex(args)?;
            *value = Value::Integer(borrow_regex(&object).group_lengths(i64::from(index))? as i32);
            Ok(())
        }),
        1,
        vec![value_param()],
        VarType::Empty,
    );
    adapter.add_get(
        class.clone(),
        "MatchedText",
        Rc::new(|value: &mut Value, args: &mut Args| {
            let object = receiver_regex(args)?;
            *value = Value::Str(borrow_regex(&object).groups(0)?.into_bytes());
            Ok(())
        }),
        0,
        Vec::new(),
        VarType::Empty,
    );
    adapter.add_get(
        class.clone(),
        "MatchedOffset",
        Rc::new(|value: &mut Value, args: &mut Args| {
            let object = receiver_regex(args)?;
            *value = Value::Integer(borrow_regex(&object).group_offsets(0)? as i32);
            Ok(())
        }),
        0,
        Vec::new(),
        VarType::Empty,
    );
    adapter.add_get(
        class.clone(),
        "MatchedLength",
        Rc::new(|value: &mut Value, args: &mut Args| {
            let object = receiver_regex(args)?;
            *value = Value::Integer(borrow_regex(&object).group_lengths(0)? as i32);
            Ok(())
        }),
        0,
        Vec::new(),
        VarType::Empty,
    );
    adapter.add_get(
        class.clone(),
        "NamedGroup",
        Rc::new(|value: &mut Value, args: &mut Args| {
            let name = text_arg(&args.values[0]);
            let object = receiver_regex(args)?;
            *value = Value::Integer(borrow_regex(&object).named_group(&name) as i32);
            Ok(())
        }),
        1,
        vec![value_param()],
        VarType::Empty,
    );
    adapter.add_get(
        class.clone(),
        "SubjectLeft",
        Rc::new(|value: &mut Value, args: &mut Args| {
            let object = receiver_regex(args)?;
            *value = Value::Str(borrow_regex(&object).subject_left().into_bytes());
            Ok(())
        }),
        0,
        Vec::new(),
        VarType::Empty,
    );
    adapter.add_get(
        class.clone(),
        "SubjectRight",
        Rc::new(|value: &mut Value, args: &mut Args| {
            let object = receiver_regex(args)?;
            *value = Value::Str(borrow_regex(&object).subject_right().into_bytes());
            Ok(())
        }),
        0,
        Vec::new(),
        VarType::Empty,
    );
    adapter.add_get(
        class.clone(),
        "EscapeRegExChars",
        Rc::new(|value: &mut Value, args: &mut Args| {
            *value = Value::Str(PerlRegExObject::escape_regex_chars(&text_arg(&args.values[0])).into_bytes());
            Ok(())
        }),
        1,
        vec![value_param()],
        VarType::Empty,
    );
    adapter.add_get(
        class.clone(),
        "StoreGroups",
        Rc::new(|_value: &mut Value, args: &mut Args| {
            let object = receiver_regex(args)?;
            borrow_regex_mut(&object).store_groups()
        }),
        0,
        Vec::new(),
        VarType::Empty,
    );
    adapter.add_get(
        class.clone(),
        "ComputeReplacement",
        Rc::new(|value: &mut Value, args: &mut Args| {
            let object = receiver_regex(args)?;
            *value = Value::Str(borrow_regex(&object).compute_replacement()?.into_bytes());
            Ok(())
        }),
        0,
        Vec::new(),
        VarType::Empty,
    );
    adapter.add_get(
        class.clone(),
        "Replace",
        Rc::new(|value: &mut Value, args: &mut Args| {
            let object = receiver_regex(args)?;
            *value = Value::Str(borrow_regex_mut(&object).replace()?.into_bytes());
            Ok(())
        }),
        0,
        Vec::new(),
        VarType::Empty,
    );
    adapter.add_get(
        class.clone(),
        "ReplaceAll",
        Rc::new(|value: &mut Value, args: &mut Args| {
            let object = receiver_regex(args)?;
            *value = Value::Bool(borrow_regex_mut(&object).replace_all()?);
            Ok(())
        }),
        0,
        Vec::new(),
        VarType::Empty,
    );

    // `Split(Strings, Limit)`/`SplitCapture(Strings, Limit)` -- the
    // registration allows the 1-argument form PCRE-style call shape
    // (`Args.Count` 2), so `PR_ARGS_NO_CHECK` is not used: the port follows
    // the upstream `AddGet(..., 2, [varEmpty, varEmpty])`.
    adapter.add_get(
        class.clone(),
        "Split",
        Rc::new(|_value: &mut Value, args: &mut Args| {
            let list = list_of_or_error(&args.values[0])?;
            let limit = xedit_script::values::to_i32(&args.values[1])?;
            let object = receiver_regex(args)?;
            let mut regex = borrow_regex_mut(&object);
            let mut pieces = Vec::new();
            regex.split(&mut pieces, limit)?;
            let mut list = borrow_list_mut(&list);
            for piece in pieces {
                list.append(piece)?;
            }
            Ok(())
        }),
        2,
        vec![value_param(), value_param()],
        VarType::Empty,
    );
    adapter.add_get(
        class.clone(),
        "SplitCapture",
        Rc::new(|_value: &mut Value, args: &mut Args| {
            let list = list_of_or_error(&args.values[0])?;
            let limit = xedit_script::values::to_i32(&args.values[1])?;
            let object = receiver_regex(args)?;
            let mut regex = borrow_regex_mut(&object);
            let mut pieces = Vec::new();
            regex.split_capture(&mut pieces, limit)?;
            let mut list = borrow_list_mut(&list);
            for piece in pieces {
                list.append(piece)?;
            }
            Ok(())
        }),
        2,
        vec![value_param(), value_param()],
        VarType::Empty,
    );

    // The properties.
    adapter.add_get(
        class.clone(),
        "RegEx",
        Rc::new(|value: &mut Value, args: &mut Args| {
            let object = receiver_regex(args)?;
            *value = Value::Str(borrow_regex(&object).regex.regex().as_bytes().to_vec());
            Ok(())
        }),
        0,
        Vec::new(),
        VarType::Empty,
    );
    adapter.add_set(
        class.clone(),
        "RegEx",
        Rc::new(|value: &Value, args: &mut Args| {
            let text = text_arg(value);
            let object = receiver_regex(args)?;
            borrow_regex_mut(&object).regex.set_regex(text);
            Ok(())
        }),
        0,
        vec![value_param()],
    );
    adapter.add_get(
        class.clone(),
        "Replacement",
        Rc::new(|value: &mut Value, args: &mut Args| {
            let object = receiver_regex(args)?;
            *value = Value::Str(borrow_regex(&object).regex.replacement().as_bytes().to_vec());
            Ok(())
        }),
        0,
        Vec::new(),
        VarType::Empty,
    );
    adapter.add_set(
        class.clone(),
        "Replacement",
        Rc::new(|value: &Value, args: &mut Args| {
            let text = text_arg(value);
            let object = receiver_regex(args)?;
            borrow_regex_mut(&object).regex.set_replacement(text);
            Ok(())
        }),
        0,
        vec![value_param()],
    );
    adapter.add_get(
        class.clone(),
        "Subject",
        Rc::new(|value: &mut Value, args: &mut Args| {
            let object = receiver_regex(args)?;
            *value = Value::Str(borrow_regex(&object).regex.subject().as_bytes().to_vec());
            Ok(())
        }),
        0,
        Vec::new(),
        VarType::Empty,
    );
    adapter.add_set(
        class.clone(),
        "Subject",
        Rc::new(|value: &Value, args: &mut Args| {
            let text = text_arg(value);
            let object = receiver_regex(args)?;
            borrow_regex_mut(&object).regex.set_subject(text);
            Ok(())
        }),
        0,
        vec![value_param()],
    );
    adapter.add_get(
        class.clone(),
        "Start",
        Rc::new(|value: &mut Value, args: &mut Args| {
            let object = receiver_regex(args)?;
            *value = Value::Integer(borrow_regex(&object).regex.start() as i32);
            Ok(())
        }),
        0,
        Vec::new(),
        VarType::Empty,
    );
    adapter.add_set(
        class.clone(),
        "Start",
        Rc::new(|value: &Value, args: &mut Args| {
            let start = xedit_script::values::to_i32(value)?;
            let object = receiver_regex(args)?;
            borrow_regex_mut(&object).regex.set_start(i64::from(start));
            Ok(())
        }),
        0,
        vec![value_param()],
    );
    adapter.add_get(
        class.clone(),
        "Stop",
        Rc::new(|value: &mut Value, args: &mut Args| {
            let object = receiver_regex(args)?;
            *value = Value::Integer(borrow_regex(&object).regex.stop() as i32);
            Ok(())
        }),
        0,
        Vec::new(),
        VarType::Empty,
    );
    adapter.add_set(
        class.clone(),
        "Stop",
        Rc::new(|value: &Value, args: &mut Args| {
            let stop = xedit_script::values::to_i32(value)?;
            let object = receiver_regex(args)?;
            borrow_regex_mut(&object).regex.set_stop(i64::from(stop));
            Ok(())
        }),
        0,
        vec![value_param()],
    );
    adapter.add_get(
        class.clone(),
        "Options",
        Rc::new(|value: &mut Value, args: &mut Args| {
            let object = receiver_regex(args)?;
            // `S2V(Byte(Options))`.
            *value = Value::Integer(borrow_regex(&object).regex.options());
            Ok(())
        }),
        0,
        Vec::new(),
        VarType::Empty,
    );
    adapter.add_set(
        class.clone(),
        "Options",
        Rc::new(|value: &Value, args: &mut Args| {
            // `Options := TPerlRegExOptions(Byte(V2S(Value)))`.
            let options = xedit_script::values::to_i32(value)?;
            let object = receiver_regex(args)?;
            borrow_regex_mut(&object).regex.set_options(options);
            Ok(())
        }),
        0,
        vec![value_param()],
    );

    let _ = PR_ARGS_NO_CHECK;
    let _ = receiver;
}

#[cfg(test)]
mod tests {
    use super::*;

    fn regex(pattern: &str) -> PerlRegExObject {
        let mut object = PerlRegExObject::new(Rc::new(ClassDef {
            unit: String::new(),
            name: "TPerlRegEx".to_owned(),
            parent: None,
        }));
        object.regex.set_regex(pattern.to_owned());
        object
    }

    #[test]
    fn matches_with_offsets_and_groups() {
        let mut object = regex("(b)(c)?");
        object.regex.set_subject("abd".to_owned());
        assert!(object.match_().unwrap());
        assert_eq!(object.group_count(), 1, "only group 1 participated");
        assert_eq!(object.groups(0).unwrap(), "b");
        assert_eq!(object.groups(1).unwrap(), "b");
        assert_eq!(object.groups(2).unwrap(), "", "past the group count");
        assert_eq!(object.group_offsets(0).unwrap(), 2, "1-based character offset");
        assert_eq!(object.group_lengths(0).unwrap(), 1);
        assert_eq!(
            object.group_offsets(5).unwrap_err(),
            ValueError::ExternalClass {
                class: E_REGEX_CLASS,
                message: "Index out of bounds (5)".to_owned(),
            }
        );
        assert!(!object.match_again().unwrap());
        assert_eq!(object.subject_left(), "a");
        assert_eq!(object.subject_right(), "d");
    }

    #[test]
    fn match_again_walks_the_subject() {
        let mut object = regex("\\d+");
        object.regex.set_subject("a12 b345".to_owned());
        let mut found = Vec::new();
        while object.match_again().unwrap() {
            found.push(object.groups(0).unwrap());
        }
        assert_eq!(found, ["12", "345"]);
    }

    #[test]
    fn replace_all_substitutes_every_match() {
        let mut object = regex("a");
        object.regex.set_subject("banana".to_owned());
        object.regex.set_replacement("o".to_owned());
        assert!(object.replace_all().unwrap());
        assert_eq!(object.regex.subject(), "bonono");
        assert!(!object.found_match());
    }

    #[test]
    fn replacement_backreferences() {
        let mut object = regex("(\\w+)=(\\w+)");
        object.regex.set_subject("key=value".to_owned());
        assert!(object.match_().unwrap());
        object.regex.set_replacement("$2:$1".to_owned());
        assert_eq!(object.compute_replacement().unwrap(), "value:key");
        object.regex.set_replacement("${1}\\U2".to_owned());
        assert_eq!(object.compute_replacement().unwrap(), "keyVALUE");
        object.regex.set_replacement("$0 $& $+".to_owned());
        assert_eq!(
            object.compute_replacement().unwrap(),
            "key=value key=value value"
        );
    }

    #[test]
    fn split_appends_pieces() {
        let mut object = regex(",");
        object.regex.set_subject("a,b,,c".to_owned());
        let mut pieces = Vec::new();
        object.split(&mut pieces, 0).unwrap();
        assert_eq!(pieces, ["a", "b", "", "c"]);
        let mut pieces = Vec::new();
        object.split_capture(&mut pieces, 0).unwrap();
        assert_eq!(pieces, ["a", "b", "", "c"], "no capturing group to add");
    }

    #[test]
    fn escape_and_named_groups() {
        assert_eq!(
            PerlRegExObject::escape_regex_chars("a.b(c)"),
            "a\\.b\\(c\\)"
        );
        let mut object = regex("(?P<first>a)(b)");
        object.regex.set_subject("ab".to_owned());
        assert!(object.match_().unwrap());
        assert_eq!(object.named_group("first"), 1);
        assert_eq!(
            object.named_group("nope"),
            -7,
            "pcre_get_stringnumber's PCRE_ERROR_NOSUBSTRING"
        );
    }

    #[test]
    fn anchored_and_not_empty_options() {
        let mut object = regex("a");
        object.regex.set_options(1 << OPTION_ANCHORED);
        object.regex.set_subject("ba".to_owned());
        assert!(!object.match_().unwrap(), "an anchored match must start at 0");
        // Without the option the same pattern matches at 1.
        object.regex.set_options(0);
        assert!(object.match_().unwrap());
        assert_eq!(object.group_offsets(0).unwrap(), 2);
        // `preNotEmpty` skips the empty matches `a*` finds.
        object.regex.set_regex("a*".to_owned());
        object.regex.set_subject("ba".to_owned());
        object.regex.set_state(1 << STATE_NOT_EMPTY);
        assert!(object.match_().unwrap());
        assert_eq!(object.groups(0).unwrap(), "a");
    }

    /// A pattern PCRE refuses raises the RTL's error with PCRE's offset and
    /// text, as `Error in regular expression at offset N: <text>`.
    #[test]
    fn compile_errors_are_the_rtl_ones() {
        let mut object = regex("(?<=a+)b");
        let error = object.compile().expect_err("a compile error");
        assert_eq!(
            error,
            ValueError::ExternalClass {
                class: E_REGEX_CLASS,
                message: "Error in regular expression at offset 6: lookbehind assertion is not fixed length"
                    .to_owned(),
            }
        );
        let mut empty = PerlRegExObject::new(Rc::new(ClassDef {
            unit: String::new(),
            name: "TPerlRegEx".to_owned(),
            parent: None,
        }));
        assert_eq!(
            empty.compile().expect_err("a compile error"),
            ValueError::ExternalClass {
                class: E_REGEX_CLASS,
                message: "A regular expression specified in RegEx is required".to_owned(),
            }
        );
    }
}
