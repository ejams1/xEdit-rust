// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

// Ported from xEdit: External/jvcl/jvcl/run/JvInterpreter.pas (CallDll,
// CallDllIns, TJvInterpreterExtFunction.CallDll: the call of a function a
// script declares with an `external` directive, AddExtFun of
// TJvInterpreterUnit.InterpretFunction)

//! The functions a script declares `external` (issue #25):
//!
//! ```pascal
//! function MulDiv(a, b, c: Integer): Integer; external 'kernel32.dll' name 'MulDiv';
//! ```
//!
//! `CallDll` loads the library by name (`SafeLoadLibrary`), looks the
//! function up by the `name` it was declared with (`GetProcAddress`; a
//! function declared by `index` has no name and is never found, as
//! upstream), passes the arguments by their declared types and reads the
//! result by its declared type, then frees the library and restores the
//! function's `GetLastError`. The 64-bit build calls through
//! `System.Rtti.Invoke` with the platform's one calling convention, which
//! the port's call through a function pointer of the same arity is.
//!
//! Argument types: `Integer`/`LongInt`/`DWord` and `Boolean`/`LongBool`/
//! `WordBool` pass the variant as an `Integer` (so `True` is -1),
//! `SmallInt`/`Word` as a `Word`, `string`/`PChar`/`Char` as a pointer to
//! the UTF-16 text, and a `var` argument of the first two kinds as a pointer
//! to the variable's value, written back after the call. Result types:
//! `SmallInt`/`Word`, `Integer`, `Boolean` and none (a procedure, whose call
//! gives `Null`).
//!
//! UPSTREAM-BUG: any other argument or result type is refused inside
//! `CallDllIns`'s `try` block, whose `on E: EJvInterpreterError do raise E`
//! re-raises an exception object the handler then frees: the 4.1.5q release
//! dies on an access violation there (probes `host-extfun-double`,
//! `host-extfun-result`). The port raises the refusal upstream means,
//! `ieDllInvalidArgument`/`ieDllInvalidResult` with the function's name.
//! A function that faults inside the library takes the process down (the
//! release turns the fault into an `EJVCLException`).

use xedit_script::adapter::{Args, ExtFunction};
use xedit_script::error::{
    IE_DLL_ERROR_LOAD_LIBRARY, IE_DLL_FUNCTION_NOT_FOUND, IE_DLL_INVALID_ARGUMENT, IE_DLL_INVALID_RESULT,
};
use xedit_script::values::{Value, ValueError, VarType};

/// The most arguments the port passes (the 64-bit `Invoke` has no limit;
/// no Windows API function the scripts reach takes more).
const MAX_ARGS: usize = 16;

/// One argument as the call passes it.
enum Arg {
    /// A value in a register or a stack slot.
    Value(u64),
    /// A `var` argument: the value the callee may write, read back after the
    /// call into the argument at `index`.
    ByRef { cell: Box<u64>, index: usize, small: bool },
    /// A text as a NUL-terminated UTF-16 buffer.
    Text(Vec<u16>),
}

/// `CallDll(DllName, FunctionName, Args, ParamTypes, ResTyp)`.
#[cfg(windows)]
pub(crate) fn call_dll(function: &ExtFunction, args: &mut Args) -> Result<Value, ValueError> {
    use windows_sys::Win32::Foundation::{FreeLibrary, GetLastError, SetLastError};
    use windows_sys::Win32::System::Diagnostics::Debug::{SEM_NOOPENFILEERRORBOX, SetErrorMode};
    use windows_sys::Win32::System::LibraryLoader::{GetProcAddress, LoadLibraryW};

    let dll_name =
        String::from_utf8_lossy(&xedit_script::values::to_str(&function.dll).unwrap_or_default()).into_owned();
    let wide: Vec<u16> = dll_name.encode_utf16().chain(std::iter::once(0)).collect();
    // `SafeLoadLibrary`: no "file not found" box while it loads.
    // SAFETY: the name is NUL-terminated and lives for the call; the error
    // mode is restored right after.
    let instance = unsafe {
        let previous = SetErrorMode(SEM_NOOPENFILEERRORBOX);
        let instance = LoadLibraryW(wide.as_ptr());
        SetErrorMode(previous);
        instance
    };
    if instance.is_null() {
        return Err(ValueError::interp_n(IE_DLL_ERROR_LOAD_LIBRARY, dll_name));
    }
    let result = call_dll_ins(instance, function, args);
    // SAFETY: the library was loaded above; the error code of the call is
    // kept across `FreeLibrary`, as upstream does.
    unsafe {
        let last_error = GetLastError();
        FreeLibrary(instance);
        SetLastError(last_error);
    }
    let _ = GetProcAddress;
    result
}

/// `CallDllIns(Ins, FuncName, Args, ParamDesc, ResTyp)`.
#[cfg(windows)]
fn call_dll_ins(
    instance: windows_sys::Win32::Foundation::HMODULE,
    function: &ExtFunction,
    args: &mut Args,
) -> Result<Value, ValueError> {
    use windows_sys::Win32::System::LibraryLoader::GetProcAddress;

    let name: Vec<u8> = function.function_name.bytes().chain(std::iter::once(0)).collect();
    // SAFETY: the module is loaded and the name NUL-terminated.
    let address = unsafe { GetProcAddress(instance, name.as_ptr()) };
    let Some(address) = address else {
        return Err(ValueError::interp(IE_DLL_FUNCTION_NOT_FOUND));
    };
    let invalid_argument = || ValueError::interp_n(IE_DLL_INVALID_ARGUMENT, function.function_name.clone());
    let mut passed: Vec<Arg> = Vec::new();
    for (index, value) in args.values.iter().enumerate() {
        let param = function.param_types.get(index).copied().unwrap_or_default();
        let arg = if !param.by_ref {
            match param.typ {
                VarType::Integer | VarType::Bool => {
                    // `AInt := Args.Values[I]`.
                    let int = xedit_script::values::to_i32(value)?;
                    Arg::Value(u64::from(int as u32))
                }
                VarType::Smallint => {
                    // `AWord := Word(Args.Values[I])`.
                    let word = xedit_script::values::to_i32(value)? as u16;
                    Arg::Value(u64::from(word))
                }
                VarType::Str => {
                    let text = String::from_utf8_lossy(&xedit_script::values::to_str(value)?).into_owned();
                    let text = xedit_io::encoding::ansi_string(&xedit_io::encoding::ansi_bytes(&text));
                    Arg::Text(text.encode_utf16().chain(std::iter::once(0)).collect())
                }
                _ => return Err(invalid_argument()),
            }
        } else {
            match param.typ {
                VarType::Integer | VarType::Bool => Arg::ByRef {
                    cell: Box::new(u64::from(xedit_script::values::to_i32(value).unwrap_or(0) as u32)),
                    index,
                    small: false,
                },
                VarType::Smallint => Arg::ByRef {
                    cell: Box::new(u64::from(xedit_script::values::to_i32(value).unwrap_or(0) as u16)),
                    index,
                    small: true,
                },
                _ => return Err(invalid_argument()),
            }
        };
        passed.push(arg);
    }
    if !matches!(
        function.res_typ,
        VarType::Smallint | VarType::Integer | VarType::Bool | VarType::Empty
    ) {
        return Err(ValueError::interp_n(
            IE_DLL_INVALID_RESULT,
            function.function_name.clone(),
        ));
    }
    if passed.len() > MAX_ARGS {
        return Err(invalid_argument());
    }
    let words: Vec<u64> = passed
        .iter_mut()
        .map(|arg| match arg {
            Arg::Value(word) => *word,
            Arg::ByRef { cell, .. } => cell.as_mut() as *mut u64 as u64,
            Arg::Text(text) => text.as_ptr() as u64,
        })
        .collect();
    // SAFETY: the address is the export the script declared, called with
    // as many integer-class arguments as it declared, in the platform's one
    // 64-bit calling convention (what upstream's `Rtti.Invoke` does); the
    // buffers the pointers refer to live until after the call. Whether the
    // declaration matches the export is the script's responsibility, as it
    // is upstream's.
    let returned = unsafe { invoke(address as usize, &words) };
    for arg in &passed {
        if let Arg::ByRef { cell, index, small } = arg {
            let slot = &mut args.values[*index];
            *slot = if *small {
                Value::Smallint(**cell as u16 as i16)
            } else {
                match slot {
                    Value::Smallint(_) => Value::Smallint(**cell as u16 as i16),
                    _ => Value::Integer(**cell as u32 as i32),
                }
            };
        }
    }
    Ok(match function.res_typ {
        VarType::Smallint => Value::Smallint(returned as u16 as i16),
        VarType::Integer => Value::Integer(returned as u32 as i32),
        // `Boolean(Ord(... .AsType<Boolean>()))`: the low byte.
        VarType::Bool => Value::Bool(returned as u8 != 0),
        _ => Value::Null,
    })
}

/// Calls `address` with the integer-class arguments `words`.
///
/// # Safety
///
/// `address` must be a function taking `words.len()` integer or pointer
/// arguments in the platform's C calling convention.
#[cfg(windows)]
unsafe fn invoke(address: usize, words: &[u64]) -> u64 {
    macro_rules! call {
        ($($arg:ident),*) => {{
            // SAFETY: the caller guarantees the arity and the argument kinds.
            let f: unsafe extern "system" fn($($arg: u64),*) -> u64 = unsafe { std::mem::transmute(address) };
            #[allow(unused_mut, unused_variables)]
            let mut iter = words.iter().copied();
            $(let $arg = iter.next().unwrap_or(0);)*
            unsafe { f($($arg),*) }
        }};
    }
    match words.len() {
        0 => call!(),
        1 => call!(a),
        2 => call!(a, b),
        3 => call!(a, b, c),
        4 => call!(a, b, c, d),
        5 => call!(a, b, c, d, e),
        6 => call!(a, b, c, d, e, f6),
        7 => call!(a, b, c, d, e, f6, g),
        8 => call!(a, b, c, d, e, f6, g, h),
        9 => call!(a, b, c, d, e, f6, g, h, i),
        10 => call!(a, b, c, d, e, f6, g, h, i, j),
        11 => call!(a, b, c, d, e, f6, g, h, i, j, k),
        12 => call!(a, b, c, d, e, f6, g, h, i, j, k, l),
        13 => call!(a, b, c, d, e, f6, g, h, i, j, k, l, m),
        14 => call!(a, b, c, d, e, f6, g, h, i, j, k, l, m, n),
        15 => call!(a, b, c, d, e, f6, g, h, i, j, k, l, m, n, o),
        _ => call!(a, b, c, d, e, f6, g, h, i, j, k, l, m, n, o, p),
    }
}

#[cfg(not(windows))]
pub(crate) fn call_dll(function: &ExtFunction, _args: &mut Args) -> Result<Value, ValueError> {
    let dll_name =
        String::from_utf8_lossy(&xedit_script::values::to_str(&function.dll).unwrap_or_default()).into_owned();
    Err(ValueError::interp_n(IE_DLL_ERROR_LOAD_LIBRARY, dll_name))
}
