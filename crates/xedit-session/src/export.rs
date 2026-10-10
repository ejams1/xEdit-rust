// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

// Ported from xEdit: xDump.dpr (the tmExport mode: TExportFormat,
// AnchorProfile, UESPName, UESPType, AddProfile, FindProfile, MarkProfile,
// LockProfile, ExportElement, ExportContainer, ProfileElement,
// ProfileContainer, ProfileHeader, ProfileArray, ProfileChapters)

//! The export tool mode (`xDump.exe -export <RAW|UESPWIKI>`): the profile of
//! the record definitions of a game, written as a text file and printed as
//! the structure of the definitions.
//!
//! The export walks the definition tree five times (`epRead`, `epSimple`,
//! `epShared`, `epChapters`, `epRemaining`). The first pass counts every
//! element's path (`:Name=TypeName` pieces, joined, for example
//! `:Record Header=Record:Signature=SubRecord of Signature`); the later
//! passes mark the paths that are shared by several elements (>1), the
//! structures and unions that are, and the records and chapters, and write
//! the marked ones as the structure text. The marked paths make the profile
//! list, a sorted string list of the definitions a converter has to know
//! (`FO4ExportPlugins.txt` next to the program, as xDump writes it).

use std::path::PathBuf;

use schemars::JsonSchema;
use serde::Serialize;
use xedit_core::interface::constructors::find_record_def;
use xedit_core::interface::def::Def;
use xedit_core::interface::globals::{app_name, header_signature, wb_group_order};
use xedit_core::interface::types::{DefType, PascalEnum, dt_arrays, dt_non_values};
use xedit_io::collate::ansi_compare_text;

use crate::{CommandError, Session};

/// Upstream `TExportFormat`: the formats the export writes.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum ExportFormat {
    UespWiki,
    Raw,
}

impl ExportFormat {
    /// Upstream `StrToTExportFormat`: `RAW` and `UESPWIKI`, any case, and
    /// anything else is `RAW` as well (xDump refuses an unknown format
    /// before it gets here).
    pub fn parse(name: &str) -> Self {
        if name.eq_ignore_ascii_case("UESPWIKI") {
            ExportFormat::UespWiki
        } else {
            ExportFormat::Raw
        }
    }

    /// Whether `xDump.dpr`'s `isFormatValid` accepts the name.
    pub fn is_valid(name: &str) -> bool {
        name.eq_ignore_ascii_case("RAW") || name.eq_ignore_ascii_case("UESPWIKI")
    }
}

/// Upstream `TwbExportPass`, the five passes of the export.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Pass {
    Read,
    Simple,
    Shared,
    Chapters,
    Remaining,
}

/// The passes `ProfileHeader`, `ProfileArray` and `ProfileChapters` are run
/// with, in the order of the `for Pass := epRead to epRemaining` loop.
const PASSES: [Pass; 5] = [Pass::Read, Pass::Simple, Pass::Shared, Pass::Chapters, Pass::Remaining];

/// Upstream `UESPWikiTable`.
const UESP_WIKI_TABLE: &str = "{| class=\"wikitable\" border=\"1\" width=\"100%\"\r\n\
! width=\"3%\" | [[Tes5Mod:File Format Conventions|C]]\r\n\
! width=\"10%\" | SubRecord\r\n\
! width=\"15%\" | Name\r\n\
! width=\"15%\" | [[Tes5Mod:File Format Conventions|Type/Size]]\r\n\
! width=\"57%\" | Info";

/// Upstream `UESPWikiClose`.
const UESP_WIKI_CLOSE: &str = "|}\r\n";

/// Upstream `AddProfile`, `FindProfile`, `MarkProfile` and `LockProfile` on
/// `wbDefProfiles`: a `TStringList` with `Sorted := True` and
/// `Duplicates := dupIgnore`, whose `Objects[]` holds the count of an entry,
/// `-1` for a mark and `-2` for a lock.
///
/// A sorted `TStringList` compares with `CompareStrings`, which is
/// `AnsiCompareText` while `CaseSensitive` is false (the default), so the
/// list is ordered and searched without case.
struct Profiles {
    entries: Vec<(String, i32)>,
}

impl Profiles {
    fn new() -> Self {
        Self { entries: Vec::new() }
    }

    /// The position of `profile` in the sorted list, or `None`.
    fn position(&self, profile: &str) -> Option<usize> {
        self.entries
            .binary_search_by(|(entry, _)| ansi_compare_text(entry, profile))
            .ok()
    }

    /// Upstream `AddProfile`: the count of the entry goes up, a new entry
    /// starts at one.
    fn add(&mut self, profile: &str) {
        match self.position(profile) {
            Some(index) => self.entries[index].1 += 1,
            None => {
                let index = self
                    .entries
                    .binary_search_by(|(entry, _)| ansi_compare_text(entry, profile))
                    .unwrap_err();
                self.entries.insert(index, (profile.to_owned(), 1));
            }
        }
    }

    /// Upstream `FindProfile`: the count, or zero for an absent entry.
    fn find(&self, profile: &str) -> i32 {
        self.position(profile).map_or(0, |index| self.entries[index].1)
    }

    /// Upstream `MarkProfile`.
    fn mark(&mut self, profile: &str) {
        if let Some(index) = self.position(profile) {
            self.entries[index].1 = -1;
        }
    }

    /// Upstream `LockProfile`.
    fn lock(&mut self, profile: &str) {
        if let Some(index) = self.position(profile) {
            self.entries[index].1 = -2;
        }
    }

    /// Upstream `wbDefProfiles.SaveToFile`: the entries in their sorted
    /// order, each line terminated by CRLF.
    fn text(&self) -> String {
        let mut text = String::new();
        for (entry, _) in &self.entries {
            text.push_str(entry);
            text.push_str("\r\n");
        }
        text
    }
}

/// The export of one game.
struct Export {
    format: ExportFormat,
    /// The structure text, as xDump writes it to stdout.
    out: String,
    profiles: Profiles,
}

/// Upstream `UESPName`: the spaces of a name become underscores.
fn uesp_name(name: &str) -> String {
    name.replace(' ', "_")
}

/// Upstream `UESPType`: the type of the wiki table for a definition type
/// name.
fn uesp_type(type_name: &str) -> String {
    fn count_of(type_name: &str) -> String {
        let (count, rest) = match type_name.find('_') {
            Some(index) if index > 0 => (&type_name[..index], &type_name[index + 1..]),
            _ => ("", type_name),
        };
        format!("_{rest}[{count}]")
    }
    fn array_of(type_name: &str) -> String {
        let upper = type_name.to_ascii_uppercase();
        const ARRAY: &str = "_ARRAY";
        const OF: &str = "_OF_";
        let Some(index) = upper.find(ARRAY) else {
            return type_name.to_owned();
        };
        // `(i>0) and ((i+l-1) = Length(aType))`: the name has to end in the
        // array suffix, else upstream returns it unchanged.
        if index + ARRAY.len() != type_name.len() {
            return type_name.to_owned();
        }
        let rest = format!("{}{}", &type_name[..index], &type_name[index + ARRAY.len()..]);
        let upper = rest.to_ascii_uppercase();
        // UPSTREAM-QUIRK: with the array suffix gone and no `_of_` inside,
        // the function ends without assigning `Result`, so the name is
        // empty (Delphi's default of a string result).
        let Some(at) = upper.find(OF) else {
            return String::new();
        };
        if at == 0 {
            return String::new();
        }
        let head = &rest[..at];
        let tail = &rest[at + OF.len()..];
        format!("{head}{}", count_of(tail))
    }
    /// Upstream `UESPsingleType`: the standard name inside the type name is
    /// replaced by the C type, the text before and after it kept.
    fn single_type(type_name: &str, standard: &str, result: &str) -> String {
        let upper = type_name.to_ascii_uppercase();
        let Some(index) = upper.find(&standard.to_ascii_uppercase()) else {
            return type_name.to_owned();
        };
        let mut head = String::new();
        let mut tail = type_name.to_owned();
        if index > 0 {
            head.push_str(&type_name[..index]);
            tail = tail[index + standard.len()..].to_owned();
        } else {
            tail = tail[standard.len()..].to_owned();
        }
        format!("{head}{result}{tail}")
    }

    let mut result = uesp_name(type_name);
    result = array_of(&result);
    result = single_type(&result, "Unsigned_Bytes", "uint8");
    result = single_type(&result, "Signed_Bytes", "int8");
    result = single_type(&result, "Bytes", "int8");
    result = single_type(&result, "Unsigned_Byte", "uint8");
    result = single_type(&result, "Signed_Byte", "int8");
    result = single_type(&result, "Byte", "int8");
    result = single_type(&result, "Unsigned_DWord", "uint32");
    result = single_type(&result, "Signed_DWord", "int32");
    result = single_type(&result, "DWord", "int32");
    result = single_type(&result, "Unsigned_Word", "uint16");
    result = single_type(&result, "Signed_Word", "int16");
    result = single_type(&result, "Word", "int16");
    result = single_type(&result, "Float", "float32");
    result = single_type(&result, "FormID", "formid");
    result
}

/// Upstream `wbDefToName`: the name a definition is written under, the
/// signature and the name of a signature definition, the name of a named
/// definition, and the name of the definition type of anything else. The
/// characters below a space are written as `$(XX)`.
fn wb_def_to_name(def: &dyn Def) -> String {
    let text = if let Some(signature_def) = def.as_signature_def() {
        let signature = signature_def.get_default_signature();
        let name = signature_def.get_name();
        let bytes = signature.0;
        if bytes[0] == 0 {
            format!(
                "$(00){}{}{} - {name}",
                bytes[1] as char, bytes[2] as char, bytes[3] as char
            )
        } else {
            format!("{signature} - {name}")
        }
    } else if let Some(named) = def.as_named_def() {
        named.get_name().to_owned()
    } else {
        format!("<{}>", def.get_def_type().enum_name())
    };
    let mut result = String::with_capacity(text.len());
    for character in text.chars() {
        if character < ' ' {
            result.push_str(&format!("$({:02X})", character as u32));
        } else {
            result.push(character);
        }
    }
    result
}

/// Upstream `AnchorProfile`: the line an element is written with.
fn anchor_profile(
    format: ExportFormat,
    indent: &str,
    profile: &str,
    use_profile: bool,
    name: &str,
    type_name: &str,
) -> String {
    match format {
        ExportFormat::UespWiki => {
            if indent.is_empty() {
                format!(
                    "=== [[Tes5Mod:Save File Format/{profile}|{}]] ===\r\n{UESP_WIKI_TABLE}",
                    uesp_name(name)
                )
            } else {
                let mut result = format!("|-\r\n|{}\r\n|", uesp_name(name));
                if use_profile {
                    result.push_str(&format!(
                        "[[Tes5Mod:Save File Format/{profile}|{}]]",
                        uesp_type(type_name)
                    ));
                } else {
                    result.push_str(&uesp_type(type_name));
                }
                result.push_str("\r\n|");
                result
            }
        }
        ExportFormat::Raw => {
            let mut result = format!("{indent}{name} as {type_name}");
            if use_profile {
                result.push_str(&format!(" [{profile}]"));
            }
            result
        }
    }
}

impl Export {
    /// Whether the definition is a container (`dtNonValues`), and so has
    /// children the walk descends into.
    fn is_container(def: &dyn Def) -> bool {
        dt_non_values().contains(def.get_def_type())
    }

    /// Upstream `ExportElement`: the profile of the element (and its
    /// children) is appended to `profile`, the marked elements are written
    /// as the structure text, and the containers the walk descends into
    /// follow with `ExportContainer`.
    fn export_element(&mut self, def: &dyn Def, profile: &mut String, pass: Pass, indent: &str) {
        let name = wb_def_to_name(def);
        let type_name = def.get_def_type_name();
        profile.push_str(&format!(":{name}={type_name}"));

        let def_type = def.get_def_type();
        let mut do_it = false;
        let mut skip_first = false;
        let mut the_element: &dyn Def = def;
        let mut sub_record_value = None;
        if dt_arrays().contains(def_type) {
            match def_type {
                DefType::dtArray => {
                    if let Some(array) = def.as_array_def() {
                        let element = array.get_element();
                        do_it = Self::is_container(element.as_dyn_def());
                        // UPSTREAM-QUIRK: `theElement := Element` even when
                        // `doIt` is false, and the sub-record array below
                        // keeps its own element, as upstream writes it.
                        the_element = element.as_dyn_def();
                    }
                }
                DefType::dtSubRecordArray => {
                    if let Some(array) = def.as_sub_record_array_def() {
                        let element = array.get_element();
                        do_it = Self::is_container(element.as_dyn_def());
                    }
                }
                _ => {}
            }
        } else if def_type == DefType::dtSubRecord {
            if let Some(sub_record) = def.as_sub_record_def() {
                sub_record_value = sub_record.get_value();
            }
            if let Some(value) = &sub_record_value {
                do_it = Self::is_container(value.as_dyn_def());
                the_element = value.as_dyn_def();
            }
        } else if def_type == DefType::dtUnion {
            if let Some(union) = def.as_union_def()
                && union.get_member_count() > 0
            {
                do_it = true;
                skip_first = union.get_member(0).get_def_type_name() == "Null";
            }
        } else if Self::is_container(def) {
            do_it = true;
        }

        if do_it {
            let mut children = String::new();
            self.profile_container(the_element, &mut children, pass, indent);
            profile.push_str(&children);
        }

        self.out
            .push_str(&anchor_profile(self.format, indent, profile, do_it, &name, &type_name));
        if skip_first {
            self.out.push_str(" Present only if ...");
        }
        self.out.push('\n');

        let next_indent = format!("{indent}  ");
        if (indent.is_empty() || self.profiles.find(profile) != -1) && do_it {
            let mut children = String::new();
            self.export_container(the_element, &mut children, pass, &next_indent, skip_first);
        }

        if indent.is_empty() {
            if self.format == ExportFormat::UespWiki {
                self.out.push_str(UESP_WIKI_CLOSE);
            }
            self.out.push('\n');
        }
    }

    /// Upstream `ExportContainer`: the children of an element are written
    /// the same way, one level deeper.
    fn export_container(&mut self, def: &dyn Def, profile: &mut String, pass: Pass, indent: &str, skip_first: bool) {
        let def_type = def.get_def_type();
        match def_type {
            DefType::dtSubRecordStruct | DefType::dtSubRecordUnion | DefType::dtRecord => {
                if let Some(record) = def.as_record_def() {
                    for index in 0..record.get_member_count() as usize {
                        let mut piece = String::new();
                        self.export_element(record.get_member(index).as_dyn_def(), &mut piece, pass, indent);
                        profile.push_str(&piece);
                    }
                }
            }
            DefType::dtSubRecord => {
                if let Some(sub_record) = def.as_sub_record_def()
                    && let Some(value) = sub_record.get_value()
                {
                    let mut piece = String::new();
                    self.export_element(value.as_dyn_def(), &mut piece, pass, indent);
                    profile.push_str(&piece);
                }
            }
            DefType::dtString
            | DefType::dtLString
            | DefType::dtLenString
            | DefType::dtByteArray
            | DefType::dtInteger
            | DefType::dtIntegerFormater
            | DefType::dtFloat => {}
            DefType::dtSubRecordArray => {
                if let Some(array) = def.as_sub_record_array_def() {
                    let mut piece = String::new();
                    self.export_element(array.get_element().as_dyn_def(), &mut piece, pass, indent);
                    profile.push_str(&piece);
                }
            }
            DefType::dtArray => {
                if let Some(array) = def.as_array_def() {
                    let mut piece = String::new();
                    self.export_element(array.get_element().as_dyn_def(), &mut piece, pass, indent);
                    profile.push_str(&piece);
                }
            }
            DefType::dtStruct | DefType::dtStructChapter => {
                if let Some(structure) = def.as_struct_def() {
                    for index in 0..structure.get_member_count() as usize {
                        let mut piece = String::new();
                        self.export_element(structure.get_member(index).as_dyn_def(), &mut piece, pass, indent);
                        profile.push_str(&piece);
                    }
                }
            }
            DefType::dtUnion => {
                if let Some(union) = def.as_union_def() {
                    let first = usize::from(skip_first);
                    for index in first..union.get_member_count() as usize {
                        let mut piece = String::new();
                        self.export_element(union.get_member(index).as_dyn_def(), &mut piece, pass, indent);
                        profile.push_str(&piece);
                    }
                }
            }
            DefType::dtEmpty => {}
            _ => {}
        }
    }

    /// Upstream `ProfileContainer`.
    fn profile_container(&mut self, def: &dyn Def, profile: &mut String, pass: Pass, indent: &str) {
        let def_type = def.get_def_type();
        match def_type {
            DefType::dtSubRecordStruct | DefType::dtSubRecordUnion | DefType::dtRecord => {
                if let Some(record) = def.as_record_def() {
                    for index in 0..record.get_member_count() as usize {
                        let mut piece = String::new();
                        self.profile_element(record.get_member(index).as_dyn_def(), &mut piece, pass, indent);
                        profile.push_str(&piece);
                    }
                }
            }
            DefType::dtSubRecord => {
                if let Some(sub_record) = def.as_sub_record_def()
                    && let Some(value) = sub_record.get_value()
                {
                    let mut piece = String::new();
                    self.profile_element(value.as_dyn_def(), &mut piece, pass, indent);
                    profile.push_str(&piece);
                }
            }
            DefType::dtString
            | DefType::dtLString
            | DefType::dtLenString
            | DefType::dtByteArray
            | DefType::dtInteger
            | DefType::dtIntegerFormater
            | DefType::dtFloat => {}
            DefType::dtSubRecordArray => {
                if let Some(array) = def.as_sub_record_array_def() {
                    let mut piece = String::new();
                    self.profile_element(array.get_element().as_dyn_def(), &mut piece, pass, indent);
                    profile.push_str(&piece);
                }
            }
            DefType::dtArray => {
                if let Some(array) = def.as_array_def() {
                    let mut piece = String::new();
                    self.profile_element(array.get_element().as_dyn_def(), &mut piece, pass, indent);
                    profile.push_str(&piece);
                }
            }
            DefType::dtStruct | DefType::dtStructChapter => {
                if let Some(structure) = def.as_struct_def() {
                    for index in 0..structure.get_member_count() as usize {
                        let mut piece = String::new();
                        self.profile_element(structure.get_member(index).as_dyn_def(), &mut piece, pass, indent);
                        profile.push_str(&piece);
                    }
                }
            }
            DefType::dtUnion => {
                if let Some(union) = def.as_union_def() {
                    for index in 0..union.get_member_count() as usize {
                        let mut piece = String::new();
                        self.profile_element(union.get_member(index).as_dyn_def(), &mut piece, pass, indent);
                        profile.push_str(&piece);
                    }
                }
            }
            DefType::dtEmpty => {}
            _ => {}
        }
    }

    /// Upstream `ProfileElement`: the pass decides which paths are counted,
    /// marked and locked, and writes the structure of the marked ones.
    fn profile_element(&mut self, def: &dyn Def, profile: &mut String, pass: Pass, indent: &str) {
        let name = wb_def_to_name(def);
        let type_name = def.get_def_type_name();
        let mut local = format!(":{name}={type_name}");
        profile.push_str(&local);

        let def_type = def.get_def_type();
        let mut do_it = false;
        let mut double_it = false;
        let mut the_element: &dyn Def = def;
        let mut sub_record_value = None;
        if dt_arrays().contains(def_type) {
            match def_type {
                DefType::dtArray => {
                    if let Some(array) = def.as_array_def() {
                        let element = array.get_element();
                        do_it = Self::is_container(element.as_dyn_def());
                        double_it = do_it;
                        the_element = element.as_dyn_def();
                    }
                }
                DefType::dtSubRecordArray => {
                    if let Some(array) = def.as_sub_record_array_def() {
                        let element = array.get_element();
                        do_it = Self::is_container(element.as_dyn_def());
                        double_it = do_it;
                    }
                }
                _ => {}
            }
        } else if def_type == DefType::dtSubRecord {
            if let Some(sub_record) = def.as_sub_record_def() {
                sub_record_value = sub_record.get_value();
            }
            if let Some(value) = &sub_record_value {
                do_it = Self::is_container(value.as_dyn_def());
                the_element = value.as_dyn_def();
            }
        } else if Self::is_container(def) {
            // `dtUnion` is a container too and has no case of its own here,
            // as upstream.
            do_it = true;
        }
        if do_it {
            local.clear();
            self.profile_container(the_element, &mut local, pass, indent);
            profile.push_str(&local);
            if double_it {
                let mut second = String::new();
                self.profile_container(the_element, &mut second, pass, indent);
                profile.push_str(&second);
            }
        }
        self.check_pass(pass, def, profile, &mut local, indent);
    }

    /// Upstream `CheckPass` with `doFindSimpleProfile`, `doFindSharedProfile`,
    /// `doFindChaptersProfile` and `doFindProfile`.
    fn check_pass(&mut self, pass: Pass, def: &dyn Def, profile: &str, local: &mut String, indent: &str) {
        let def_type = def.get_def_type();
        match pass {
            Pass::Read => self.profiles.add(profile),
            Pass::Simple => {
                if !Self::is_container(def) && self.profiles.find(profile) > 1 {
                    self.profiles.lock(profile);
                }
            }
            Pass::Shared => {
                if matches!(def_type, DefType::dtStruct | DefType::dtSubRecordStruct) && self.profiles.find(profile) > 0
                {
                    self.profiles.mark(profile);
                    self.export_element(def, local, Pass::Shared, indent);
                }
                if matches!(def_type, DefType::dtUnion | DefType::dtSubRecordUnion) && self.profiles.find(profile) > 1 {
                    self.profiles.mark(profile);
                    self.export_element(def, local, Pass::Shared, indent);
                }
            }
            Pass::Chapters => {
                if matches!(def_type, DefType::dtRecord | DefType::dtStructChapter) {
                    self.profiles.mark(profile);
                    self.export_element(def, local, Pass::Chapters, indent);
                }
            }
            Pass::Remaining => {
                if self.profiles.find(profile) > 0 {
                    self.profiles.mark(profile);
                }
            }
        }
    }

    /// Upstream `ProfileHeader`: the file header definition (`wbHeaderSignature`).
    fn profile_header(&mut self, pass: Pass) {
        if let Some(def) = find_record_def(header_signature()) {
            let mut profile = String::new();
            self.profile_element(def.as_dyn_def(), &mut profile, pass, "");
        }
    }

    /// Upstream `ProfileArray`: every record definition in the order of the
    /// top level groups.
    fn profile_array(&mut self, pass: Pass) {
        for signature in wb_group_order() {
            if signature == header_signature() {
                continue;
            }
            if let Some(def) = find_record_def(signature) {
                let mut profile = String::new();
                self.profile_element(def.as_dyn_def(), &mut profile, pass, "");
            }
        }
    }
}

/// What the export produced.
#[derive(Serialize, JsonSchema)]
pub struct ExportResult {
    /// Path of the profile file, as xDump writes it next to the program.
    pub path: String,
    /// The structure text, as xDump writes it to stdout.
    pub text: String,
    /// Lines of the profile file.
    pub profiles: usize,
    /// Size of the profile file in bytes.
    pub bytes: u64,
    /// Whether the file was written.
    pub written: bool,
}

/// The export tool mode: the profile of the definitions of the loaded game.
/// `format` is `RAW` or `UESPWIKI`, and `output` is where the profile file
/// goes (`<AppName>Export<Source>.txt` next to the program when omitted, as
/// xDump writes it into its working directory).
pub fn export_definitions(
    session: &mut Session,
    format: &str,
    output: Option<&str>,
    dry_run: bool,
) -> Result<ExportResult, CommandError> {
    session.mode()?;
    if !ExportFormat::is_valid(format) {
        return Err(CommandError::new(
            "invalid_params",
            format!("Cannot handle the format \"{format}\". Please check the command line parameters."),
        ));
    }
    let format = ExportFormat::parse(format);
    let mut export = Export {
        format,
        out: String::new(),
        profiles: Profiles::new(),
    };
    // `for Pass := epRead to epRemaining do begin ProfileHeader; ProfileArray;
    // ProfileChapters; end;` — the save chapters (`wbFileChapters`) are only
    // walked in the saves source, which has no tool mode here.
    for pass in PASSES {
        export.profile_header(pass);
        export.profile_array(pass);
    }
    let text = export.profiles.text();
    let path = PathBuf::from(output.map_or_else(|| format!("{}ExportPlugins.txt", app_name()), str::to_owned));
    if !dry_run {
        std::fs::write(&path, text.as_bytes())
            .map_err(|error| CommandError::new("io", format!("{}: {error}", path.display())))?;
    }
    Ok(ExportResult {
        path: path.to_string_lossy().into_owned(),
        profiles: text.lines().count(),
        bytes: text.len() as u64,
        written: !dry_run,
        text: export.out,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_wiki_type_names_follow_uesp_type() {
        assert_eq!(uesp_type("Unsigned Byte"), "uint8");
        assert_eq!(uesp_type("FormID"), "formid");
        // The array suffix has to end the name, and without an  the
        // type name is empty (the unassigned result upstream leaves).
        assert_eq!(uesp_type("Array of Structure"), "Array_of_Structure");
        assert_eq!(uesp_type("2 Bytes Array"), "");
        assert_eq!(uesp_type("4 Word_ARRAY_OF_FormID"), "4_int16_ARRAY_OF_formid");
        assert_eq!(uesp_type("4_Word_OF_Bytes_ARRAY"), "4_int16_int8[]");
    }

    #[test]
    fn a_sorted_profile_list_counts_and_marks() {
        let mut profiles = Profiles::new();
        profiles.add(":A=Structure");
        profiles.add(":A=Structure");
        profiles.add(":B=Float");
        assert_eq!(profiles.find(":a=structure"), 2);
        profiles.mark(":A=Structure");
        assert_eq!(profiles.find(":A=Structure"), -1);
        profiles.lock(":B=Float");
        assert_eq!(profiles.find(":B=Float"), -2);
        assert_eq!(profiles.find(":C=Float"), 0);
        assert_eq!(profiles.text(), ":A=Structure\r\n:B=Float\r\n");
    }
}
