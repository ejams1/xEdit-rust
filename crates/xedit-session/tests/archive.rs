// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! The `archive.*` commands on small generated folders: packing every
//! format and reading it back, the dry runs, the edit gate, and the bytes
//! not depending on the number of threads.

use std::fs;
use std::path::Path;

use serde_json::{Value, json};
use xedit_io::temp::TempDir;
use xedit_session::{Registry, Session};

/// A folder with a few files in a temporary place of its own, removed when
/// the test ends.
struct Folder(TempDir);

impl Folder {
    fn new(name: &str) -> Self {
        Folder(TempDir::new(format!(
            "xedit-archive-test-{name}-{}",
            std::process::id()
        )))
    }

    fn path(&self, relative: &str) -> String {
        self.0.join(relative).display().to_string()
    }

    fn write(&self, relative: &str, data: &[u8]) {
        let path = self.0.join(relative);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, data).unwrap();
    }
}

fn text(length: usize, seed: u8) -> Vec<u8> {
    (0..length)
        .map(|i| b'a' + ((i as u8).wrapping_mul(seed) % 11))
        .collect()
}

/// The files of the source folder.
fn source(tag: &str) -> Folder {
    let folder = Folder::new(&format!("source-{tag}"));
    folder.write("meshes/armor/a.nif", &text(4000, 3));
    folder.write("meshes/armor/b.nif", &text(6000, 5));
    folder.write("meshes/b2.nif", &text(6000, 5)); // the same data as b.nif
    folder.write("textures/t/c.dds", &text(9000, 7));
    folder.write("sound/fx/d.wav", &text(3000, 9));
    folder.write("meshes/empty.nif", b"");
    folder
}

fn call(edit: bool, name: &str, params: Value) -> Result<Value, xedit_session::CommandError> {
    let mut session = Session::default();
    session.allow_edit(edit);
    Registry::standard().call(&mut session, name, params)
}

fn files_below(root: &Path) -> Vec<(String, Vec<u8>)> {
    let mut files = Vec::new();
    let mut stack = vec![root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        for entry in fs::read_dir(&dir).unwrap() {
            let path = entry.unwrap().path();
            if path.is_dir() {
                stack.push(path);
            } else {
                let name = path
                    .strip_prefix(root)
                    .unwrap()
                    .to_string_lossy()
                    .replace('\\', "/")
                    .to_lowercase();
                files.push((name, fs::read(&path).unwrap()));
            }
        }
    }
    files.sort();
    files
}

#[test]
fn every_format_packs_lists_and_extracts() {
    let source = source("every_format");
    let work = Folder::new("work-formats");
    for (format, extension, compress) in [
        ("tes3", "bsa", None),
        ("tes4", "bsa", Some("zlib")),
        ("fo3", "bsa", Some("default")),
        ("sse", "bsa", Some("lz4f")),
        ("fo4", "ba2", None),
        ("fo4", "ba2", Some("default")),
        ("sf1", "ba2", Some("lz4")),
        ("sf1", "ba2", Some("zlib")),
    ] {
        let archive = work.path(&format!("{format}-{}.{extension}", compress.unwrap_or("plain")));
        let result = call(
            true,
            "archive.pack",
            json!({
                "archive": archive, "sources": [source.path("")], "format": format, "compress": compress,
            }),
        )
        .unwrap();
        assert_eq!(result["source_files"], 6, "{format}");
        assert_eq!(result["archives"][0]["files"], 6, "{format}");

        let listed = call(false, "archive.list", json!({ "archive": archive, "files": true })).unwrap();
        assert_eq!(listed["file_count"], 6, "{format}");
        assert_eq!(listed["entries"].as_array().unwrap().len(), 6, "{format}");
        assert!(listed["entries"].as_array().unwrap().iter().any(|entry| {
            entry["name"]
                .as_str()
                .unwrap()
                .eq_ignore_ascii_case("meshes\\armor\\a.nif")
        }));

        let output = Folder::new(&format!("out-{format}-{}", compress.unwrap_or("plain")));
        let extracted = call(
            true,
            "archive.extract",
            json!({ "archive": archive, "output": output.path("") }),
        )
        .unwrap();
        assert_eq!(extracted["files"], 6, "{format}");
        assert_eq!(files_below(&output.0), files_below(&source.0), "{format}");
    }
}

/// A DDS file of the format with a chain of mipmaps of generated data.
fn dds_file(format: xedit_io::dds::Dxgi, size: i32, mips: i32, seed: u32) -> Vec<u8> {
    use xedit_io::dds;
    let mut file = vec![0u8; dds::HEADER_SIZE + dds::HEADER_DX10_SIZE];
    dds::set_up_header(&mut file, format, size, size, mips, false, false);
    file.truncate(dds::header_size(&file));
    let bits = usize::from(dds::bits_per_pixel(format));
    let mut state = seed;
    let mut level = size as usize;
    for _ in 0..mips {
        let bytes = (level * level * bits / 8).max(16);
        // Half runs, half noise, so that zlib and lz4 both find something.
        for index in 0..bytes {
            state = state.wrapping_mul(1664525).wrapping_add(1013904223);
            file.push(if index % 3 == 0 {
                (state >> 24) as u8
            } else {
                (index / 64) as u8
            });
        }
        level = (level / 2).max(1);
    }
    file
}

#[test]
fn texture_archives_pack_list_and_extract() {
    use xedit_io::dds::Dxgi;
    let source = Folder::new("source-textures");
    let files = [
        ("textures/t/big.dds", dds_file(Dxgi::BC1_UNORM, 1024, 11, 1)),
        ("textures/t/seven.dds", dds_file(Dxgi::BC7_UNORM, 256, 9, 2)),
        ("textures/t/small.dds", dds_file(Dxgi::BC3_UNORM, 64, 7, 3)),
        ("textures/t/alike.dds", dds_file(Dxgi::BC3_UNORM, 64, 7, 3)),
        ("textures/t/plain.dds", dds_file(Dxgi::R8G8B8A8_UNORM, 128, 1, 4)),
    ];
    for (name, data) in &files {
        source.write(name, data);
    }
    let work = Folder::new("work-textures");
    for (format, compress) in [
        ("fo4dds", None),
        ("fo4dds", Some("zlib")),
        ("sf1dds", Some("lz4")),
        ("sf1dds", Some("zlib")),
    ] {
        let archive = work.path(&format!("{format}-{}.ba2", compress.unwrap_or("plain")));
        let packed = call(
            true,
            "archive.pack",
            json!({ "archive": archive, "sources": [source.path("")], "format": format, "compress": compress }),
        )
        .unwrap();
        assert_eq!(packed["archives"][0]["files"], 5, "{format}");
        // The identical texture shares all of its chunks, and counts once.
        assert_eq!(packed["archives"][0]["shared_files"], 1, "{format}");

        let listed = call(false, "archive.list", json!({ "archive": archive, "files": true })).unwrap();
        let entries = listed["entries"].as_array().unwrap();
        let big = entries
            .iter()
            .find(|entry| entry["name"].as_str().unwrap().ends_with("big.dds"))
            .unwrap();
        assert_eq!(
            (big["width"].as_u64(), big["height"].as_u64()),
            (Some(1024), Some(1024))
        );
        assert_eq!(big["format"], "BC1_UNORM");

        let output = Folder::new(&format!("out-textures-{format}-{}", compress.unwrap_or("plain")));
        call(
            true,
            "archive.extract",
            json!({ "archive": archive, "output": output.path("") }),
        )
        .unwrap();
        let extracted = files_below(&output.0);
        assert_eq!(extracted.len(), files.len());
        for (name, data) in &files {
            let found = extracted.iter().find(|(extracted, _)| extracted == name).unwrap();
            assert!(&found.1 == data, "{format} {name}");
        }
    }
}

#[test]
fn texture_archives_do_not_depend_on_the_threads() {
    use xedit_io::dds::Dxgi;
    let source = Folder::new("source-textures-threads");
    for index in 0..9u32 {
        source.write(
            &format!("textures/t/t{index}.dds"),
            &dds_file(
                if index % 2 == 0 {
                    Dxgi::BC1_UNORM
                } else {
                    Dxgi::BC7_UNORM
                },
                512,
                10,
                index % 4,
            ),
        );
    }
    let work = Folder::new("work-textures-threads");
    for split in [json!(0), json!(-1)] {
        let mut archives = Vec::new();
        for threads in [1, 2, 7] {
            let archive = work.path(&format!("t-{threads}-{split}.ba2"));
            call(
                true,
                "archive.pack",
                json!({
                    "archive": archive, "sources": [source.path("")], "format": "fo4dds",
                    "compress": "zlib", "threads": threads, "split": split,
                }),
            )
            .unwrap();
            archives.push(fs::read(&archive).unwrap());
        }
        assert!(
            archives[0] == archives[1] && archives[0] == archives[2],
            "split {split}"
        );
    }
}

#[test]
fn the_archive_does_not_depend_on_the_threads() {
    let source = source("the_archive_");
    let work = Folder::new("threads");
    let mut archives = Vec::new();
    for (format, extension) in [("sse", "bsa"), ("fo4", "ba2")] {
        for threads in [1, 2, 5] {
            let archive = work.path(&format!("{format}-{threads}.{extension}"));
            call(
                true,
                "archive.pack",
                json!({
                    "archive": archive, "sources": [source.path("")], "format": format,
                    "compress": "default", "threads": threads,
                }),
            )
            .unwrap();
            archives.push((format, fs::read(&archive).unwrap()));
        }
    }
    for pair in archives.chunks(3) {
        assert_eq!(pair[0].1, pair[1].1, "{}", pair[0].0);
        assert_eq!(pair[0].1, pair[2].1, "{}", pair[0].0);
    }
}

#[test]
fn filters_sharing_and_splitting_apply() {
    let source = source("filters_shar");
    let work = Folder::new("options");
    let shared = call(
        true,
        "archive.pack",
        json!({ "archive": work.path("shared.ba2"), "sources": [source.path("")], "format": "fo4" }),
    )
    .unwrap();
    assert_eq!(shared["archives"][0]["shared_files"], 1);
    let unshared = call(
        true,
        "archive.pack",
        json!({ "archive": work.path("unshared.ba2"), "sources": [source.path("")], "format": "fo4", "share": false }),
    )
    .unwrap();
    assert_eq!(unshared["archives"][0]["shared_files"], 0);
    assert!(unshared["archives"][0]["size"].as_i64() > shared["archives"][0]["size"].as_i64());

    let filtered = call(
        true,
        "archive.pack",
        json!({
            "archive": work.path("filtered.bsa"), "sources": [source.path("")], "format": "sse",
            "filters": ["*.nif"],
        }),
    )
    .unwrap();
    assert_eq!(filtered["source_files"], 4);

    // A negative split size puts every file in an archive of its own.
    let split = call(
        true,
        "archive.pack",
        json!({ "archive": work.path("split.bsa"), "sources": [source.path("")], "format": "sse", "split": -1 }),
    )
    .unwrap();
    let archives = split["archives"].as_array().unwrap();
    assert_eq!(archives.len(), 6);
    assert!(archives[1]["file"].as_str().unwrap().ends_with("split2.bsa"));
    assert!(archives.iter().all(|archive| archive["files"] == 1));
}

#[test]
fn a_dry_run_writes_nothing_and_the_gate_holds() {
    let source = source("a_dry_run_wr");
    let work = Folder::new("gate");
    let archive = work.path("gate.bsa");
    let params = json!({ "archive": archive, "sources": [source.path("")], "format": "tes4" });

    assert_eq!(
        call(false, "archive.pack", params.clone()).unwrap_err().code,
        "edit_required"
    );

    let mut dry = params.clone();
    dry["dry_run"] = json!(true);
    let result = call(false, "archive.pack", dry).unwrap();
    assert_eq!(result["dry_run"], true);
    assert_eq!(result["source_files"], 6);
    assert_eq!(result["archives"].as_array().unwrap().len(), 0);
    assert!(!Path::new(&archive).exists());

    call(true, "archive.pack", params).unwrap();
    let output = Folder::new("gate-out");
    let params = json!({ "archive": archive, "output": output.path("") });
    assert_eq!(
        call(false, "archive.extract", params.clone()).unwrap_err().code,
        "edit_required"
    );
    let mut dry = params;
    dry["dry_run"] = json!(true);
    let result = call(false, "archive.extract", dry).unwrap();
    assert_eq!(result["files"], 6);
    assert!(files_below(&output.0).is_empty());
}

#[test]
fn bad_requests_have_stable_errors() {
    let source = source("bad_requests");
    let work = Folder::new("errors");
    let pack = |extra: Value| {
        let mut params = json!({
            "archive": work.path("x.bsa"), "sources": [source.path("")], "format": "sse", "dry_run": true,
        });
        for (key, value) in extra.as_object().unwrap() {
            params[key] = value.clone();
        }
        call(false, "archive.pack", params).unwrap_err()
    };
    assert_eq!(pack(json!({ "format": "doom" })).code, "invalid_params");
    assert_eq!(pack(json!({ "sources": [] })).code, "invalid_params");
    assert_eq!(
        pack(json!({ "compress": "lz4" })).message,
        "Skyrim AE, Skyrim SE archives don't support lz4 compression"
    );
    assert_eq!(
        pack(json!({ "compress": "brotli" })).message,
        "Unknown compression type brotli"
    );
    assert_eq!(pack(json!({ "archive_flags": "xyz" })).code, "invalid_params");
    assert_eq!(
        pack(json!({ "sources": [work.path("missing")] })).message,
        "No valid source file(s) found."
    );
    // A texture archive takes DDS files only.
    let error = call(
        true,
        "archive.pack",
        json!({ "archive": work.path("t.ba2"), "sources": [source.path("")], "format": "fo4dds" }),
    )
    .unwrap_err();
    assert_eq!(error.code, "archive_failed");
    assert!(error.message.contains("Not a valid DDS file"), "{}", error.message);
    assert!(!Path::new(&work.path("t.ba2")).exists(), "a failed pack leaves nothing");

    let error = call(false, "archive.list", json!({ "archive": work.path("missing.bsa") })).unwrap_err();
    assert_eq!(error.code, "archive_failed");
    assert!(error.message.starts_with("Cannot open file"), "{}", error.message);
}

/// The XBox 360 target of `archive.pack` (`BSArchPro`'s "XBox 360
/// Archive"): a DDS archive refuses the textures of the other platform,
/// and a PC texture converts only through the `xtexconv.exe` the release
/// does not ship.
#[test]
fn the_xbox_target_refuses_the_other_platforms_textures() {
    use xedit_io::dds::{self, Dxgi};
    let source = Folder::new("source-xbox");
    let pc = dds_file(Dxgi::BC1_UNORM, 64, 1, 11);
    source.write("textures/t/pc.dds", &pc);
    let mut xbox = vec![0u8; dds::MAX_HEADER_SIZE];
    dds::set_up_header(&mut xbox, Dxgi::BC1_UNORM, 64, 64, 1, false, true);
    xbox.truncate(dds::header_size(&xbox));
    xbox.extend(&pc[dds::HEADER_SIZE + dds::HEADER_DX10_SIZE..]);
    let xbox_source = Folder::new("source-xbox-file");
    xbox_source.write("textures/t/xbox.dds", &xbox);

    let pack = |folder: &Folder, target: &str| -> Result<Value, xedit_session::CommandError> {
        call(
            true,
            "archive.pack",
            json!({
                "archive": folder.path("out.ba2"),
                "sources": [folder.path("textures")],
                "format": "fo4dds",
                "compress": "default",
                "target": target,
                "threads": 1,
            }),
        )
    };

    // A PC archive refuses the XBox texture, an XBox archive the PC one.
    let error = pack(&source, "xbox").unwrap_err();
    assert_eq!(error.code, "archive_failed");
    assert!(error.message.contains("DDS is not in XBox format"), "{}", error.message);
    let error = pack(&xbox_source, "pc").unwrap_err();
    assert!(error.message.contains("DDS is in XBox format"), "{}", error.message);
    // An unknown target.
    let error = pack(&source, "wii").unwrap_err();
    assert_eq!(error.code, "invalid_params");
    assert!(error.message.contains("unknown target wii"), "{}", error.message);

    // The matching target packs, and the default is the PC.
    assert!(pack(&source, "pc").is_ok());
    let packed = pack(&xbox_source, "xbox").unwrap();
    assert_eq!(packed["archives"][0]["files"], 1);
    let _ = fs::remove_file(source.path("out.ba2"));
    let _ = fs::remove_file(xbox_source.path("out.ba2"));
}

/// `dds2xbox` (`DDS2XBOX` of the GUI packer): the messages of a tool that
/// does not start and of one that fails. The workflow with a tool is
/// `cargo xtask parity bsarchpro` (the stub stands in for the real
/// `xtexconv.exe`, which is not in the release archive).
#[test]
fn dds2xbox_gives_upstream_messages() {
    use xedit_io::archive::xtexconv;
    use xedit_io::dds::Dxgi;
    let data = dds_file(Dxgi::BC1_UNORM, 64, 1, 12);
    let missing = std::env::temp_dir().join("xtexconv-that-is-not-there-1338.exe");
    let error = xtexconv::dds2xbox(&missing, &data).unwrap_err();
    assert_eq!(error, "Unable to start xtexconv.exe");
    // A tool that fails gives its exit code as eight hex digits, as
    // `ErrCode.ToHexString(8)` does.
    let fail = Folder::new("source-xtexconv-fail");
    fail.write("tool.cmd", b"@exit /b 3\r\n");
    let tool = std::path::PathBuf::from(fail.path("tool.cmd"));
    if let Err(error) = xtexconv::dds2xbox(&tool, &data) {
        assert_eq!(error, "xtexconv.exe error code 00000003");
    }
    // Nothing of the temporary file is left behind.
    let leftovers = fs::read_dir(std::env::temp_dir())
        .unwrap()
        .filter_map(|entry| entry.ok())
        .filter(|entry| entry.file_name().to_string_lossy().starts_with("xtexconv-"))
        .filter(|entry| {
            entry
                .file_name()
                .to_string_lossy()
                .contains(&std::process::id().to_string())
        })
        .count();
    assert_eq!(leftovers, 0);
}
