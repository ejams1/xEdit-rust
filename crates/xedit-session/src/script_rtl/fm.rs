// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

// Ported from xEdit: External/jvcl/jvcl/run/JvInterpreterFm.pas (its four
// `AddFunction` registrations: JvInterpreterRunFormModal,
// JvInterpreterRunForm, JvInterpreterMakeForm and JvInterpreterRunUnit)

//! `JvInterpreterFm.pas`'s own registrations.
//!
//! All four build or run a form: `MakeForm` (through `TJvInterpreterFm`),
//! `RunForm` (the form shown), `RunFormModal` (shown modally) and
//! `RunUnit`, which compiles a unit's source in a fresh interpreter and runs
//! its `main` routine -- the form interpreter's own entry point. They route
//! through the [`Vcl`](super::Vcl) seam; the headless VCL model implements
//! them (issue #81: `script_vcl::interactive::VclModel` with the text DFM
//! reader of `script_vcl::dfm`), while the seam's [`NoVcl`](super::NoVcl)
//! default, used by callers without a model, answers with the clear
//! `<function>: forms are not available without a GUI` error.

use std::cell::RefCell;
use std::rc::Rc;

use xedit_script::adapter::Adapter;
use xedit_script::values::Value;

use super::{STRING, Vcl, arg_text};

/// `cJvInterpreterFm = 'JvInterpreterFm'` (`JvInterpreterFm.pas:734`).
const UNIT: &str = "JvInterpreterFm";

/// Registers the four functions on `adapter`.
pub fn register(adapter: &mut Adapter, vcl: Rc<RefCell<dyn Vcl>>) {
    {
        let vcl = vcl.clone();
        super::function(
            adapter,
            UNIT,
            "JvInterpreterRunFormModal",
            &[STRING],
            move |value, args| {
                let file_name = arg_text(args, 0)?;
                *value = Value::Integer(vcl.borrow_mut().run_form_modal(&file_name)? as i32);
                Ok(())
            },
        );
    }
    {
        let vcl = vcl.clone();
        super::function(adapter, UNIT, "JvInterpreterRunForm", &[STRING], move |value, args| {
            let file_name = arg_text(args, 0)?;
            *value = vcl.borrow_mut().run_form(&file_name)?;
            Ok(())
        });
    }
    {
        let vcl = vcl.clone();
        super::function(adapter, UNIT, "JvInterpreterMakeForm", &[STRING], move |value, args| {
            let file_name = arg_text(args, 0)?;
            *value = vcl.borrow_mut().make_form(&file_name)?;
            Ok(())
        });
    }
    super::function(adapter, UNIT, "JvInterpreterRunUnit", &[STRING], move |value, args| {
        let file_name = arg_text(args, 0)?;
        *value = vcl.borrow_mut().run_unit(&file_name)?;
        Ok(())
    });
}
