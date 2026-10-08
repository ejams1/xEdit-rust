// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! `bsarch` as a process: the usage, the modes and the messages of
//! `BSArch.exe`.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

fn bsarch(args: &[&str]) -> (String, Option<i32>) {
    let output = Command::new(env!("CARGO_BIN_EXE_bsarch"))
        .args(args)
        .output()
        .expect("running bsarch");
    (
        String::from_utf8_lossy(&output.stdout).replace("\r\n", "\n"),
        output.status.code(),
    )
}

fn folder(name: &str) -> PathBuf {
    let path = std::env::temp_dir().join(format!("bsarch-test-{name}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&path);
    fs::create_dir_all(&path).unwrap();
    path
}

fn text(path: &Path) -> &str {
    path.to_str().unwrap()
}

#[test]
fn without_arguments_it_prints_the_usage() {
    let (out, code) = bsarch(&[]);
    assert_eq!(code, Some(0));
    assert!(
        out.starts_with("\nBSArch v1.0 x64 by zilav, ElminsterAU, Sheson\n"),
        "{out}"
    );
    assert!(out.contains("PACK ARCHIVE\n  BSArch.exe pack <source1+source2+...> <archive> [parameters]"));
    assert!(out.contains("\nARCHIVE INFO\n"));
    assert!(out.ends_with("BSArch \"d:\\game\\mod - main.bsa\" -dump\n"), "{out:?}");
}

#[test]
fn bad_arguments_name_the_problem() {
    let (out, code) = bsarch(&["nothing.bsa"]);
    assert_eq!(code, Some(1));
    assert!(
        out.ends_with("EInvalidArguments: The first parameter must be \"pack\", \"unpack\" or existing archive file\n")
    );
    let (out, _) = bsarch(&["pack"]);
    assert!(out.ends_with("EInvalidArguments: No source files/folders/archives provided for packing\n"));
    let (out, _) = bsarch(&["pack", "source", "-sse"]);
    assert!(out.ends_with("EInvalidArguments: No archive name provided for packing\n"));
    let (out, _) = bsarch(&["pack", "source", "out.bsa"]);
    assert!(out.ends_with("EInvalidArguments: No archive type (game) provided for packing\n"));
    let (out, code) = bsarch(&["pack", "source", "out.bsa", "-sse", "-z:lz4"]);
    assert_eq!(code, Some(1));
    assert!(out.ends_with("EInvalidArguments: Skyrim AE, Skyrim SE archives don't support lz4 compression\n"));
    let (out, _) = bsarch(&["pack", "source", "out.bsa", "-sse", "-z:brotli"]);
    assert!(out.ends_with("EInvalidArguments: Unknown compression type brotli\n"));
    let (out, _) = bsarch(&["pack", "source", "out.bsa", "-sse", "-af:xyz"]);
    assert!(out.ends_with("EConvertError: '$xyz' is not a valid integer value\n"));
    let (out, _) = bsarch(&["unpack"]);
    assert!(out.ends_with("EInvalidArguments: No archive file provided for unpacking\n"));
    let (out, _) = bsarch(&["pack", "no-such-folder", "out.bsa", "-sse"]);
    assert!(out.contains("Adding source: no-such-folder  0 file(s)\n"));
    assert!(out.ends_with("EInvalidArguments: No valid source file(s) found.\n"));
}

#[test]
fn packs_lists_and_unpacks() {
    let root = folder("modes");
    let source = root.join("data");
    for (name, data) in [
        ("meshes/armor/a.nif", "mesh a".repeat(500)),
        ("meshes/armor/b.nif", "mesh a".repeat(500)),
        ("textures/t.dds", "texture".repeat(300)),
        ("sound/fx/s.wav", "sound".repeat(100)),
    ] {
        let path = source.join(name);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, data).unwrap();
    }
    let archive = root.join("test.bsa");
    let (out, code) = bsarch(&["pack", text(&source), text(&archive), "-sse", "-z", "-mt:no"]);
    assert_eq!(code, Some(0), "{out}");
    assert!(out.contains("Packing Skyrim AE, Skyrim SE archive: Split: 2 GB,  Compress: LZ4F,  Share: Yes\n"));
    assert!(
        out.contains("  4 file(s)\nSinglethreaded packing: 4 file(s)...\n"),
        "{out}"
    );
    assert!(out.contains("\nCreated archives:\n"));
    assert!(out.contains("  4 files  1 shared saving "), "{out}");

    let (out, code) = bsarch(&[text(&archive), "-list"]);
    assert_eq!(code, Some(0));
    assert!(
        out.contains("       Version: 0x69\n         Files: 4\n    Compressed: 3 (LZ4F)\n"),
        "{out}"
    );
    assert!(
        out.contains(" Archive Flags: 0x0097              File Flags: 0x000B\n"),
        "{out}"
    );
    assert!(out.contains("\nmeshes\\armor\\a.nif\n"));
    let (dump, _) = bsarch(&[text(&archive), "-dump"]);
    assert!(dump.contains("  DirHash: "));

    let target = root.join("unpacked");
    fs::create_dir_all(&target).unwrap();
    let (out, code) = bsarch(&["unpack", text(&archive), text(&target)]);
    assert_eq!(code, Some(0), "{out}");
    assert!(out.contains("threaded unpacking: 4 file(s)...\n"));
    assert_eq!(
        fs::read(target.join("meshes/armor/b.nif")).unwrap(),
        fs::read(source.join("meshes/armor/b.nif")).unwrap()
    );
    assert_eq!(
        fs::read(target.join("sound/fx/s.wav")).unwrap(),
        fs::read(source.join("sound/fx/s.wav")).unwrap()
    );

    // The same archive from any number of threads.
    for threads in ["1", "3"] {
        let again = root.join(format!("again-{threads}.bsa"));
        let thread_switch = format!("-threads:{threads}");
        let (_, code) = bsarch(&["pack", text(&source), text(&again), "-sse", "-z", &thread_switch]);
        assert_eq!(code, Some(0));
        assert_eq!(
            fs::read(&again).unwrap(),
            fs::read(&archive).unwrap(),
            "{threads} threads"
        );
    }
    fs::remove_dir_all(&root).unwrap();
}

#[test]
fn a_missing_folder_to_unpack_into_is_reported() {
    let root = folder("missing");
    let (out, code) = bsarch(&["unpack", "x.bsa", text(&root.join("nowhere"))]);
    assert_eq!(code, Some(1));
    assert!(out.contains("EInvalidArguments: Folder does not exist: "), "{out}");
    fs::remove_dir_all(&root).unwrap();
}
