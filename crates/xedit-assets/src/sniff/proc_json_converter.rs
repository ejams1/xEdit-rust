// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

// Ported from xEdit: Sniff/Proc/ProcJsonConverter.pas

//! `Convert to and from JSON`: writes NIF and KF files as JSON, or builds
//! them from their JSON.

use std::sync::atomic::Ordering;

use xedit_io::encoding::{ansi_bytes, string_list_text};

use crate::data_format::{DfError, FLOAT_DECIMAL_DIGITS, R};
use crate::data_format_nif::NifFile;
use crate::data_format_nif_types::ROTATION_EULER;
use crate::proc_base;
use crate::sniff::processor::{
    GameType, Proc, ProcBase, ProcContext, ProcFileObject, Storage, change_file_ext, extract_file_ext, same_text,
};
use crate::variant::str_to_int;

pub struct ProcJsonConverter {
    base: ProcBase,
    /// `rbToJson`.
    to_json_checked: bool,
    /// `edExtension`.
    extension_text: String,
    /// `edDigits`.
    digits_text: String,
    /// `cmbRotation`.
    rotation: i32,
    to_json: bool,
    extension: String,
    digits_old: usize,
    rotation_euler_old: bool,
}

impl ProcJsonConverter {
    pub fn new() -> ProcJsonConverter {
        ProcJsonConverter {
            base: ProcBase::new("Convert to and from JSON", GameType::ALL, &["nif", "kf", "json"]),
            to_json_checked: true,
            extension_text: "nif".to_owned(),
            digits_text: String::new(),
            rotation: 0,
            to_json: true,
            extension: String::new(),
            digits_old: 6,
            rotation_euler_old: false,
        }
    }
}

impl Proc for ProcJsonConverter {
    proc_base!();

    fn on_show(&mut self, storage: &Storage) {
        self.digits_old = FLOAT_DECIMAL_DIGITS.load(Ordering::Relaxed);
        self.rotation_euler_old = ROTATION_EULER.load(Ordering::Relaxed);
        self.to_json_checked = storage.get_bool("bToJson", true);
        self.extension_text = storage.get_string("sExtension", "nif");
        self.digits_text = storage.get_string("sDigits", &self.digits_old.to_string());
        self.rotation = storage.get_integer("iRotation", 0);
    }

    fn on_hide(&mut self) {
        FLOAT_DECIMAL_DIGITS.store(self.digits_old, Ordering::Relaxed);
        ROTATION_EULER.store(self.rotation_euler_old, Ordering::Relaxed);
    }

    fn on_start(&mut self) -> R<()> {
        self.to_json = self.to_json_checked;
        self.extension = self.extension_text.clone();
        ROTATION_EULER.store(self.rotation == 1, Ordering::Relaxed);

        if self.to_json {
            if self.digits_text.is_empty() {
                self.digits_text = self.digits_old.to_string();
            }
            let digits = str_to_int(&self.digits_text).unwrap_or(0);
            if !(6..=16).contains(&digits) {
                return Err(DfError::new("Decimal digits can vary from 6 to 16"));
            }
            FLOAT_DECIMAL_DIGITS.store(digits as usize, Ordering::Relaxed);
        }

        if !self.to_json && self.extension.is_empty() {
            return Err(DfError::new("Default extension can not be empty"));
        }
        Ok(())
    }

    fn process_file(&self, file: &mut ProcFileObject, _: &mut ProcContext) -> R<Vec<u8>> {
        let mut nif = NifFile::new()?;
        if self.to_json {
            if same_text(extract_file_ext(&file.file_name), ".json") {
                return Ok(Vec::new());
            }
            nif.load_from_data(&file.get_data()?)?;
            // A `TStringStream` holds the text in the ANSI code page.
            let result = ansi_bytes(&nif.to_json(false)?);
            file.file_name.push_str(".json");
            Ok(result)
        } else {
            if !same_text(extract_file_ext(&file.file_name), ".json") {
                return Ok(Vec::new());
            }
            // Not supported for archives.
            if file.in_archive() {
                return Ok(Vec::new());
            }
            let path = format!("{}{}", file.input.input_directory, file.file_name);
            let bytes = std::fs::read(&path).map_err(|error| DfError::new(format!("Cannot open file \"{path}\". {error}")))?;
            nif.from_json(&string_list_text(&bytes))?;
            let result = nif.save_to_data()?;
            file.file_name = change_file_ext(&file.file_name, "");
            if extract_file_ext(&file.file_name).is_empty() {
                file.file_name = format!("{}.{}", file.file_name, self.extension);
            }
            Ok(result)
        }
    }
}
