// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! The phase 6 step 2 gate: every `.pas` of the scripts folder parses, with
//! the units a script uses resolved from the folder (the oracle's
//! `Edit Scripts` at `XEDIT_SCRIPTS`, else below `XEDIT_ORACLE_DIR`). The
//! test does nothing when neither is set.

use std::path::{Path, PathBuf};

fn scripts_folder() -> Option<PathBuf> {
    if let Some(folder) = std::env::var_os("XEDIT_SCRIPTS") {
        let folder = PathBuf::from(folder);
        if folder.is_dir() {
            return Some(folder);
        }
    }
    let oracle = std::env::var_os("XEDIT_ORACLE_DIR")?;
    let folder = PathBuf::from(oracle).join("Edit Scripts");
    folder.is_dir().then_some(folder)
}

fn pas_files(folder: &Path) -> Vec<PathBuf> {
    let mut files: Vec<PathBuf> = std::fs::read_dir(folder)
        .unwrap_or_else(|error| panic!("reading {}: {error}", folder.display()))
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| {
            path.extension()
                .is_some_and(|extension| extension.to_string_lossy().eq_ignore_ascii_case("pas"))
        })
        .collect();
    files.sort();
    files
}

#[test]
fn every_corpus_script_parses() {
    let Some(folder) = scripts_folder() else {
        eprintln!("skipped: set XEDIT_SCRIPTS (or XEDIT_ORACLE_DIR) to run the corpus gate");
        return;
    };
    let files = pas_files(&folder);
    let mut failures = Vec::new();
    let mut failed_files = 0usize;
    for file in &files {
        let check = xedit_script::check::check_file(file, &folder);
        if !check.ok() {
            failed_files += 1;
        }
        for error in &check.errors {
            failures.push(format!("{}:{}: {}", error.file, error.line, error.message));
        }
    }
    eprintln!(
        "{} of {} scripts parse (folder {})",
        files.len() - failed_files,
        files.len(),
        folder.display()
    );
    assert!(
        files.len() >= 150,
        "corpus of {} files is smaller than the 150 of the gate ({})",
        files.len(),
        folder.display()
    );
    assert!(
        failures.is_empty(),
        "{} of {} scripts do not parse:\n{}",
        failed_files,
        files.len(),
        failures.join("\n")
    );
    // The declaration-only API reference and the Bookmark helper units are the
    // files the scripts' `uses` resolve to; keep them pinned.
    for name in ["xEditAPI.pas", "Bookmark.pas", "Bookmark1.pas"] {
        let path = folder.join(name);
        if path.is_file() {
            let check = xedit_script::check::check_file(&path, &folder);
            assert!(check.ok(), "{} does not parse: {:?}", name, check.errors);
        }
    }
}
