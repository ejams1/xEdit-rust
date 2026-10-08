// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! `cargo xtask parity nif --from-json`: NIF files built back from their
//! JSON dumps (`FromJSON`). The port dumps the first `--sample` NIF files of
//! each archive; Sniff's `Convert to and from JSON` builds a NIF from each
//! dump, and so does the port; the two must be the same bytes.

use std::fs;
use std::path::PathBuf;

use anyhow::Result;

use xedit_assets::data_format_nif::NifFile;
use xedit_io::archive::Archive;
use xedit_io::encoding::Encoding;

use super::{Options, fnv, run_sniff};
use crate::parity::{cache_dir, required_var};

pub fn run(tag: &str, options: &Options) -> Result<()> {
    let sniff = PathBuf::from(required_var("XEDIT_ORACLE_DIR")?).join("Sniff.exe");
    let scratch = match std::env::var_os("XEDIT_PARITY_SCRATCH") {
        Some(dir) => PathBuf::from(dir).join(tag),
        None => cache_dir()?.join(tag),
    };
    let work = scratch.join("from-json");
    let input = work.join("in");
    let output = work.join("out");
    let _ = fs::remove_dir_all(&work);
    fs::create_dir_all(&input)?;

    // The port's dumps and the files it builds from them.
    let mut cases: Vec<(String, Result<u64, String>)> = Vec::new();
    for &game in &options.games {
        let Some(data) = std::env::var_os(game.data_var).map(PathBuf::from) else {
            continue;
        };
        let mut archives: Vec<PathBuf> = fs::read_dir(&data)?
            .filter_map(|entry| entry.ok().map(|entry| entry.path()))
            .filter(|path| {
                let name = path.file_name().unwrap_or_default().to_string_lossy().to_lowercase();
                (name.ends_with(".bsa") || name.ends_with(".ba2"))
                    && (options.archives.is_empty() || options.archives.iter().any(|part| name.contains(part)))
            })
            .collect();
        archives.sort();
        for path in archives {
            let Ok(archive) = Archive::open(&path) else { continue };
            let mut names: Vec<String> = archive
                .files()
                .iter()
                .map(|entry| entry.name.to_lowercase().replace('/', "\\"))
                .filter(|name| name.ends_with(".nif"))
                .collect();
            names.sort();
            let archive_name = path.file_name().unwrap_or_default().to_string_lossy().replace(' ', "_");
            let mut taken = 0;
            for name in names {
                if taken >= options.sample {
                    break;
                }
                let data = archive.read(&name)?.unwrap_or_default();
                let Ok(json) = (|| {
                    let mut nif = NifFile::new()?;
                    nif.load_from_data(&data)?;
                    nif.to_json(false)
                })() else {
                    continue;
                };
                taken += 1;
                let relative = format!("{}\\{archive_name}\\{name}", game.name);
                let file = input.join(format!("{relative}.json").replace('\\', "/"));
                fs::create_dir_all(file.parent().unwrap())?;
                fs::write(&file, Encoding::Mbcs(0).get_bytes(&json))?;
                let built = (|| {
                    let mut nif = NifFile::new()?;
                    nif.from_json(&json)?;
                    nif.save_to_data()
                })()
                .map(|bytes| fnv(&bytes))
                .map_err(|error| error.0);
                cases.push((relative, built));
            }
        }
    }

    println!("oracle        {} JSON dumps through Sniff", cases.len());
    run_sniff_from_json(&sniff, &work, &input, &output, options.threads)?;
    let (mut equal, mut different) = (0, 0);
    for (relative, built) in &cases {
        let oracle = fs::read(output.join(relative.replace('\\', "/")))
            .ok()
            .map(|bytes| fnv(&bytes));
        match (built, oracle) {
            (Ok(port), Some(oracle)) if *port == oracle => equal += 1,
            _ => {
                different += 1;
                if different <= 20 {
                    println!("    different {relative}: port {built:?}, oracle {oracle:?}");
                }
            }
        }
    }
    println!(
        "total: {} built from JSON: {equal} equal, {different} different",
        cases.len()
    );
    Ok(())
}

fn run_sniff_from_json(
    sniff: &std::path::Path,
    work: &std::path::Path,
    input: &std::path::Path,
    output: &std::path::Path,
    threads: usize,
) -> Result<()> {
    run_sniff(sniff, work, input, "from-json", output, None, threads)?;
    Ok(())
}
