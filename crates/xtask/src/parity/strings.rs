// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! The string tables in `parity oracle-edit` (phase 4 step 7).
//!
//! A sequence with `"strings": true` runs on a staging folder of its own:
//! the plugins it loads (with their masters, renamed where `rename` says,
//! replaced by another sequence's oracle saves where `inputs_from` says) and,
//! in `Strings`, the string tables of every localized one, taken from the
//! game's `Data\Strings` folder or its archives. The GUI oracle gets the
//! tables in its private data folder and is closed after the script, so it
//! saves the tables the edits changed (`SaveChanged` of `FormClose`, which
//! names them in its log); the port loads from the staging folder and
//! `files.save` writes the tables next to the saved plugin. Each table one
//! of them saved is compared byte for byte.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};

use super::Game;
use super::oracle_save::{find_in, load_list};

/// The language `xeInit.pas` gives a game without a language in its ini,
/// which names its tables (`<plugin>_<language>.STRINGS`).
pub(super) fn default_language(mode: &str) -> &'static str {
    match mode.to_ascii_uppercase().as_str() {
        "FO4" | "FO4VR" | "FO76" | "SF1" => "En",
        _ => "English",
    }
}

const TABLE_EXTENSIONS: [&str; 3] = [".STRINGS", ".DLSTRINGS", ".ILSTRINGS"];

/// The staging folder of a sequence.
pub(super) struct Stage {
    /// Every plugin the oracle loads, masters first, in the staging folder.
    pub plugins: Vec<PathBuf>,
    /// The plugins the sequence names in `load`, in the staging folder (the
    /// port's `--load`).
    pub load: Vec<PathBuf>,
    /// The string tables, with their path relative to the data folder.
    pub tables: Vec<(PathBuf, String)>,
}

/// Whether the plugin's file header has the localized flag.
fn is_localized(path: &Path) -> Result<bool> {
    let mut header = [0u8; 12];
    use std::io::Read;
    fs::File::open(path)?.read_exact(&mut header)?;
    Ok(u32::from_le_bytes(header[8..12].try_into().unwrap()) & 0x80 != 0)
}

/// Puts `source` at `target`: a hard link when the volume allows it, else a
/// copy; a target of the same size is kept.
fn place(source: &Path, target: &Path) -> Result<()> {
    if let (Ok(a), Ok(b)) = (fs::metadata(source), fs::metadata(target))
        && a.len() == b.len()
        && a.modified().ok() == b.modified().ok()
    {
        return Ok(());
    }
    let _ = fs::remove_file(target);
    if fs::hard_link(source, target).is_err() {
        fs::copy(source, target).with_context(|| format!("copying {}", source.display()))?;
    }
    Ok(())
}

/// The bytes of a game table: `Data\Strings\<name>` when it is there, else
/// the first archive of the data folder that holds `strings\<name>` (the
/// interface and localization archives first).
fn game_table(data: &Path, name: &str) -> Result<Option<Vec<u8>>> {
    let loose = data.join("Strings").join(name);
    if loose.is_file() {
        return Ok(Some(fs::read(loose)?));
    }
    let mut archives: Vec<PathBuf> = fs::read_dir(data)?
        .filter_map(|entry| entry.ok().map(|entry| entry.path()))
        .filter(|path| {
            path.extension()
                .is_some_and(|e| e.eq_ignore_ascii_case("bsa") || e.eq_ignore_ascii_case("ba2"))
        })
        .collect();
    archives.sort_by_key(|path| {
        let lower = path.file_name().unwrap().to_string_lossy().to_ascii_lowercase();
        (!(lower.contains("interface") || lower.contains("localization")), lower)
    });
    let inner = format!("strings\\{name}");
    for archive in archives {
        let Ok(archive) = xedit_io::Archive::open(&archive) else {
            continue;
        };
        if let Ok(Some(bytes)) = archive.read(&inner) {
            return Ok(Some(bytes));
        }
    }
    Ok(None)
}

/// Builds the staging folder `dir` of a sequence.
pub(super) fn stage(
    game: &Game,
    data: &Path,
    dir: &Path,
    load: &[String],
    rename: &BTreeMap<String, String>,
    inputs: &BTreeMap<String, Vec<u8>>,
) -> Result<Stage> {
    fs::create_dir_all(dir.join("Strings"))?;
    // The plugins under their names in the data folder.
    let source_of = |name: &str| -> String {
        rename
            .iter()
            .find(|(new, _)| new.eq_ignore_ascii_case(name))
            .map_or_else(|| name.to_owned(), |(_, old)| old.clone())
    };
    let names: Vec<String> = load.iter().map(|name| source_of(name)).collect();
    let refs: Vec<&str> = names.iter().map(String::as_str).collect();
    let sources = load_list(game, data, &refs)?;
    let language = default_language(game.mode);
    let mut stage = Stage {
        plugins: Vec::new(),
        load: Vec::new(),
        tables: Vec::new(),
    };
    for source in &sources {
        let source_name = source.file_name().unwrap().to_string_lossy().into_owned();
        let name = rename
            .iter()
            .find(|(_, old)| old.eq_ignore_ascii_case(&source_name))
            .map_or_else(|| source_name.clone(), |(new, _)| new.clone());
        let target = dir.join(&name);
        match inputs.iter().find(|(input, _)| input.eq_ignore_ascii_case(&name)) {
            Some((_, bytes)) => fs::write(&target, bytes)?,
            None => place(source, &target)?,
        }
        if is_localized(&target)? {
            let source_stem = Path::new(&source_name)
                .file_stem()
                .unwrap()
                .to_string_lossy()
                .into_owned();
            let stem = Path::new(&name).file_stem().unwrap().to_string_lossy().into_owned();
            for extension in TABLE_EXTENSIONS {
                let source_table = format!("{source_stem}_{language}{extension}");
                let Some(bytes) = game_table(data, &source_table)? else {
                    continue;
                };
                let table = format!("{stem}_{language}{extension}");
                let path = dir.join("Strings").join(&table);
                if fs::read(&path).ok().as_deref() != Some(bytes.as_slice()) {
                    fs::write(&path, &bytes)?;
                }
                stage.tables.push((path, format!("Strings\\{table}")));
            }
        }
        stage.plugins.push(target);
    }
    for name in load {
        stage.load.push(find_in(dir, name)?);
    }
    Ok(stage)
}

/// The names of the tables the GUI saved on close, from its message log
/// (`Saving: Strings\<name>`, with `.save.<time>` when an older file was
/// there), and where each ended up in the private data folder: renamed over
/// the old file, or left under the save name when the rename was put off.
pub(super) fn saved_tables(log: &str, data: &Path) -> Result<Vec<(String, PathBuf)>> {
    let mut tables: Vec<(String, PathBuf)> = Vec::new();
    for line in log.lines() {
        let Some(position) = line.find("Saving: Strings\\") else {
            continue;
        };
        let saved = line[position + "Saving: Strings\\".len()..].trim();
        let name = match saved.find(".save.") {
            Some(end) => &saved[..end],
            None => saved,
        };
        let pending = data.join("Strings").join(saved);
        let path = if pending.is_file() {
            pending
        } else {
            data.join("Strings").join(name)
        };
        anyhow::ensure!(path.is_file(), "the GUI logged the save of {saved}, which is not there");
        tables.retain(|(known, _)| !known.eq_ignore_ascii_case(name));
        tables.push((name.to_owned(), path));
    }
    Ok(tables)
}

/// The outcome of one table of a sequence, `<sequence>/Strings/<table>`:
/// `equal`, or `different` when the bytes differ or only one side saved
/// the table. `oracle` is the cached (compressed) save of the GUI.
pub(super) fn compare_table(
    game: &'static str,
    stem: &str,
    name: &str,
    oracle: Option<&Path>,
    port: Option<&Path>,
) -> Result<super::Outcome> {
    let mut outcome = super::Outcome {
        game,
        file: format!("{stem}/Strings/{name}"),
        status: "different",
        detail: None,
        oracle_bytes: 0,
        port_bytes: 0,
        oracle_peak: None,
        port_peak: None,
    };
    match (oracle, port) {
        (Some(oracle), Some(port)) => {
            let mut expected = Vec::new();
            use std::io::Read;
            super::oracle_save::zstd_reader(oracle)?.read_to_end(&mut expected)?;
            let actual = fs::read(port)?;
            outcome.oracle_bytes = expected.len() as u64;
            outcome.port_bytes = actual.len() as u64;
            match super::oracle_save::first_difference(expected.as_slice(), actual.as_slice())? {
                None => {
                    outcome.status = "equal";
                    outcome.detail = Some(format!("  {} strings", table_count(&expected)));
                }
                Some(offset) => {
                    outcome.detail = Some(format!(
                        "  first difference at offset {offset} ({} and {} strings), the port's table is {}",
                        table_count(&expected),
                        table_count(&actual),
                        port.display()
                    ));
                }
            }
        }
        (Some(_), None) => outcome.detail = Some("  only the oracle saved the table".to_owned()),
        (None, Some(port)) => {
            outcome.detail = Some(format!("  only the port saved the table: {}", port.display()));
        }
        (None, None) => unreachable!("a table of neither side"),
    }
    Ok(outcome)
}

/// The string count of a table.
fn table_count(bytes: &[u8]) -> u32 {
    bytes
        .get(..4)
        .map_or(0, |count| u32::from_le_bytes(count.try_into().unwrap()))
}

/// The Pascal helpers of the localization steps, put into the script of a
/// sequence that uses them.
///
/// `mniNavLocalizationSwitchClick` is a menu handler with dialogs and the
/// script API has neither `AddValue` nor `NoTranslate`, so the scripts do
/// what it does through `SetEditValue`, which reaches the same code of
/// `TwbLStringDef.FromStringNative` and `wbLocalizationHandler`:
///
/// - Localize: the localized strings are gathered (`GatherLStrings`: an
///   element before its children, the children from the last one) and
///   their texts read; each is set to `STRINGID:00000000` (an ID, so the
///   element holds four bytes); the localized flag of the file header is set;
///   then each text is set again in the gathered order, which in a
///   localized file is `SetValue` of ID 0, so `AddValue`: the IDs and the
///   tables come out as the handler's `AddValue` and `STRINGID:` give them.
///   An empty text stays ID 0, as `AddValue('')` returns 0.
/// - Delocalize: the texts are gathered, the localized flag is cleared and
///   each text is set, which in a file that is not localized stores it in
///   the plugin as `NoTranslate` does. An empty text is set after `-`,
///   because ID 0 reads as an empty text once the flag is cleared and the
///   set would do nothing.
///
/// The script holds no element between the passes: a `TList` keeps no
/// reference, so an element of a record the GUI released meanwhile would be
/// another object (or none) by the time it is set. Each pass walks the file
/// again in the same order and finds the same elements, as the edits change
/// no element list.
pub(super) const LOCALIZATION_HELPERS: &str = r"var
  lvalues: TStringList;
  lcount: Integer;
  lpass: Integer;

// The pass of GatherLStrings: 0 reads the texts, 1 sets STRINGID:00000000,
// 2 sets the texts read, 3 sets them after '-' where they are empty.
procedure VisitLString(el: IInterface);
begin
  try
    case lpass of
      0: lvalues.Add(GetEditValue(el));
      1: SetEditValue(el, 'STRINGID:00000000');
      2: SetEditValue(el, lvalues[lcount]);
      3: begin
        if lvalues[lcount] = '' then
          SetEditValue(el, '-');
        SetEditValue(el, lvalues[lcount]);
      end;
    end;
  except
    log.Add('failed: ' + FullPath(el));
    raise;
  end;
  Inc(lcount);
end;

procedure GatherLStrings(el: IInterface);
var
  i: Integer;
begin
  if DefType(el) = dtLString then
    VisitLString(el);
  for i := Pred(ElementCount(el)) downto 0 do
    GatherLStrings(ElementByIndex(el, i));
end;

procedure LStringPass(f: IInterface; aPass: Integer);
begin
  lpass := aPass;
  lcount := 0;
  GatherLStrings(f);
  if lcount <> lvalues.Count then
    raise Exception.Create('pass ' + IntToStr(aPass) + ' found ' + IntToStr(lcount) + ' localized strings, the first ' + IntToStr(lvalues.Count));
end;

procedure SetLocalizedFlag(f: IInterface; aValue: Boolean);
var
  h: IInterface;
  flags: Cardinal;
begin
  h := ElementByIndex(f, 0);
  flags := GetElementNativeValues(h, 'Record Header\Record Flags');
  if aValue then
    flags := flags or $80
  else
    flags := flags and not $80;
  SetElementNativeValues(h, 'Record Header\Record Flags', flags);
end;

procedure SwitchLocalization(f: IInterface; aLocalize: Boolean);
begin
  lvalues := TStringList.Create;
  try
    lpass := 0;
    lcount := 0;
    GatherLStrings(f);
    log.Add('localizable strings: ' + IntToStr(lvalues.Count));
    if aLocalize then
      LStringPass(f, 1);
    SetLocalizedFlag(f, aLocalize);
    if aLocalize then
      LStringPass(f, 2)
    else
      LStringPass(f, 3);
  finally
    lvalues.Free;
  end;
end;
";

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_saved_tables_are_read_from_the_log() {
        let dir = std::env::temp_dir().join(format!("xtask-strings-{}", std::process::id()));
        fs::create_dir_all(dir.join("Strings")).unwrap();
        fs::write(dir.join("Strings").join("A_English.STRINGS"), b"a").unwrap();
        fs::write(
            dir.join("Strings").join("B_English.ILSTRINGS.save.2026_10_08_10_00_00"),
            b"b",
        )
        .unwrap();
        let log = "[00:01] Saving: A.esp.save.2026\r\n[00:01] Saving: Strings\\A_English.STRINGS.save.2026_10_08_10_00_00\r\n[00:02] Saving: Strings\\B_English.ILSTRINGS.save.2026_10_08_10_00_00\r\n";
        let tables = saved_tables(log, &dir).unwrap();
        assert_eq!(tables.len(), 2);
        assert_eq!(tables[0].0, "A_English.STRINGS");
        assert!(tables[0].1.ends_with("A_English.STRINGS"));
        assert!(tables[1].1.to_string_lossy().contains(".save."));
        let _ = fs::remove_dir_all(&dir);
    }
}
