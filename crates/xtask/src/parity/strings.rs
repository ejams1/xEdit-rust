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
    /// The archives linked into the staging folder.
    pub archives: Vec<PathBuf>,
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

/// Builds the staging folder `dir` of a sequence. `strings` is the
/// sequence's: without it the GUI gets no tables of its own, so it finds
/// those of the game's plugins in the game's archives under their own
/// names and none for a renamed plugin; the port's staging folder then
/// leaves out the tables of the renamed plugins as well (a read of one of
/// their strings, the reference build's for one, would otherwise load
/// tables the GUI never sees, and a copied string would be added after
/// their last ID instead of to new tables from ID 1).
#[allow(clippy::too_many_arguments)]
pub(super) fn stage(
    game: &Game,
    data: &Path,
    dir: &Path,
    load: &[String],
    rename: &BTreeMap<String, String>,
    inputs: &BTreeMap<String, Vec<u8>>,
    archives: &[String],
    strings: bool,
) -> Result<Stage> {
    fs::create_dir_all(dir.join("Strings"))?;
    // The archives the sequence names go next to the plugins; the tables
    // are then read from them, not put loose.
    let mut linked = Vec::new();
    for source in super::resources::archives_named(archives, data)? {
        let target = dir.join(source.file_name().unwrap());
        place(&source, &target)?;
        linked.push(target);
    }
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
        archives: linked,
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
        let renamed = !name.eq_ignore_ascii_case(&source_name);
        if !strings && renamed {
            // Tables an earlier run of the folder left.
            let stem = Path::new(&name).file_stem().unwrap().to_string_lossy().into_owned();
            for extension in TABLE_EXTENSIONS {
                let _ = fs::remove_file(dir.join("Strings").join(format!("{stem}_{language}{extension}")));
            }
        }
        if stage.archives.is_empty() && (strings || !renamed) && is_localized(&target)? {
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
    // The tables of `inputs` (another sequence's oracle saves) loose in
    // `Strings`, over any taken from the game.
    for (key, bytes) in inputs {
        let Some(table) = key.strip_prefix("Strings\\") else {
            continue;
        };
        let path = dir.join("Strings").join(table);
        fs::write(&path, bytes)?;
        let relative = format!("Strings\\{table}");
        stage.tables.retain(|(_, known)| !known.eq_ignore_ascii_case(&relative));
        stage.tables.push((path, relative));
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
    saved_files(log, &data.join("Strings"), |saved| saved.strip_prefix("Strings\\"))
}

/// The plugins the GUI saved on close, as [`saved_tables`] finds the
/// tables: `Saving: <plugin>.save.<time>` lines of the message log, the
/// save left under its name (the rename of a memory-mapped plugin is put
/// off to the shutdown, which the harness does not wait for) or renamed.
pub(super) fn saved_plugins(log: &str, data: &Path) -> Result<Vec<(String, PathBuf)>> {
    saved_files(log, data, |saved| (!saved.contains('\\')).then_some(saved))
}

/// The files `SaveChanged` logged (`] Saving: <path>`) whose path `select`
/// takes, by name, with where each is in `dir`.
fn saved_files(log: &str, dir: &Path, select: impl Fn(&str) -> Option<&str>) -> Result<Vec<(String, PathBuf)>> {
    let mut files: Vec<(String, PathBuf)> = Vec::new();
    for line in log.lines() {
        let Some(position) = line.find("] Saving: ") else {
            continue;
        };
        let Some(saved) = select(line[position + "] Saving: ".len()..].trim()) else {
            continue;
        };
        let name = match saved.find(".save.") {
            Some(end) => &saved[..end],
            None => saved,
        };
        let pending = dir.join(saved);
        let path = if pending.is_file() { pending } else { dir.join(name) };
        anyhow::ensure!(path.is_file(), "the GUI logged the save of {saved}, which is not there");
        files.retain(|(known, _)| !known.eq_ignore_ascii_case(name));
        files.push((name.to_owned(), path));
    }
    Ok(files)
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

/// The Pascal helper of the localization steps, put into the script of a
/// sequence that uses them.
///
/// The step runs the GUI's own handler, `mniNavLocalizationSwitchClick`
/// ("Localize plugin" / "Delocalize plugin" of the navigation tree's menu),
/// on the file's node, as a user does: `JumpTo` the file header (which
/// focuses its node and ends the `BeginUpdate` the script mode holds the
/// tree in), a Left key to the tree (`Perform(WM_KEYDOWN, VK_LEFT)`: on a
/// node without children it focuses the parent, the file), then a click of
/// the menu item. The handler's own dialogs are answered by the harness:
/// "Localize plugin" (`TfrmLocalizePlugin`, without translation) by its
/// Localize button; `EditWarn` is skipped by `-IKnowWhatImDoing`. At its
/// end the handler closes the main form ("immediately force a save changes
/// window and quit"): `FormClose` runs `SaveChanged`, without its dialog in
/// the script mode, so the plugin and the tables the handler changed are
/// saved into the private data folder and named in the message log, and
/// the GUI's file list is released. So a localization step is the last
/// step of its sequence, the script writes no plugin of its own, and the
/// harness takes the plugin the GUI saved (see `check_edit`). The helper
/// checks the localized flag of the file header afterwards: a handler that
/// returned early (the focused node not the file) changes nothing.
///
/// Parity #33 replaced an emulation through `SetEditValue` that could not
/// reproduce the handler: the handler sets each text with
/// `wbLocalizationHandler.NoTranslate` while the file is still localized,
/// which the script API cannot do (`sf1-strings-delocalize` differed in one
/// `INFO` whose response text the emulation trimmed).
pub(super) const LOCALIZATION_HELPERS: &str = r"procedure SwitchLocalization(f: IInterface; aLocalize: Boolean);
var
  h: IInterface;
  nav: TObject;
  localized: Boolean;
begin
  h := ElementByIndex(f, 0);
  JumpTo(h, False);
  nav := frmMain.FindComponent('vstNav');
  nav.Perform($0100, $25, 0);
  log.Add('switch ' + GetFileName(f));
  frmMain.FindComponent('mniNavLocalizationSwitch').Click;
  localized := (GetElementNativeValues(h, 'Record Header\Record Flags') and $80) <> 0;
  if localized <> aLocalize then
    raise Exception.Create('the localization switch did not run on ' + GetFileName(f));
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
        fs::write(dir.join("A.esp.save.2026"), b"p").unwrap();
        let plugins = saved_plugins(log, &dir).unwrap();
        assert_eq!(plugins.len(), 1);
        assert_eq!(plugins[0].0, "A.esp");
        assert!(plugins[0].1.ends_with("A.esp.save.2026"));
        let _ = fs::remove_dir_all(&dir);
    }
}
