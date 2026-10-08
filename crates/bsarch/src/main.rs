// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

// Ported from xEdit: BSArch.dpr

//! `bsarch`: packer and unpacker for Bethesda Game Studios archive files,
//! with the arguments and the console output of `BSArch.exe`.
//!
//! ```text
//! bsarch pack <source1+source2+...> <archive> [parameters]
//! bsarch unpack <archive> [folder] [parameters]
//! bsarch <archive> [-list] [-dump]
//! ```
//!
//! Run it without arguments for the full list of parameters. The packing is
//! done on all CPUs and the archive is the same for every count, and the
//! same as the one `BSArch.exe -mt:no` writes.

use std::io::{BufWriter, Stdout, Write};
use std::path::Path;
use std::process::ExitCode;
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

use xedit_io::archive::packer::unpack_archive;
use xedit_io::archive::{ArchiveType, format_size, include_trailing_path_delimiter};
use xedit_io::{Archive, ArchiveError, CompressionType, MultiSourcePacker};

#[global_allocator]
static GLOBAL: mimalloc::MiMalloc = mimalloc::MiMalloc;

/// What `BSArch.exe` prints without arguments, byte for byte: the banner
/// (up to and including the blank line after it) and the usage.
const USAGE: &[u8] = include_bytes!("usage.txt");

/// Delphi `GB`.
const GB: i64 = 1024 * 1024 * 1024;

/// The end of a line in the output (`WriteLn`).
#[cfg(windows)]
const NEWLINE: &str = "\r\n";
#[cfg(not(windows))]
const NEWLINE: &str = "\n";

/// A failure that ends the program: the class and the message that
/// `Writeln(E.ClassName, ': ', E.ToString)` prints.
struct Failure {
    class: &'static str,
    message: String,
}

impl Failure {
    fn invalid_arguments(message: &str) -> Self {
        Self {
            class: "EInvalidArguments",
            message: message.to_owned(),
        }
    }

    fn general(message: String) -> Self {
        Self {
            class: "Exception",
            message,
        }
    }
}

impl From<ArchiveError> for Failure {
    /// The class of the exception that upstream raises for the message.
    fn from(error: ArchiveError) -> Self {
        let class = if error.0.starts_with("Cannot open file") {
            "EFOpenError"
        } else if error.0.starts_with("Cannot create file") {
            "EFCreateError"
        } else if error.0 == "Stream read error" {
            "EReadError"
        } else {
            "Exception"
        };
        Self {
            class,
            message: error.0,
        }
    }
}

fn output() -> &'static Mutex<BufWriter<Stdout>> {
    static OUT: OnceLock<Mutex<BufWriter<Stdout>>> = OnceLock::new();
    OUT.get_or_init(|| Mutex::new(BufWriter::with_capacity(1 << 16, std::io::stdout())))
}

/// `Write`.
fn write(text: &str) {
    let mut out = output().lock().expect("output lock");
    let _ = out.write_all(text.as_bytes());
}

/// `WriteLn`.
fn write_line(text: &str) {
    let mut out = output().lock().expect("output lock");
    let _ = out.write_all(line_ends(text).as_bytes());
    let _ = out.write_all(NEWLINE.as_bytes());
}

fn flush() {
    let _ = output().lock().expect("output lock").flush();
}

/// The characters that start a switch (`SwitchChars`).
fn is_switch(argument: &str) -> bool {
    argument.starts_with('-') || (cfg!(windows) && argument.starts_with('/'))
}

/// Port of `FindCmdLineSwitch`: whether a parameter is the switch, without a value.
fn find_switch(args: &[String], switch: &str) -> bool {
    args.iter().skip(1).any(|argument| {
        let mut chars = argument.chars();
        matches!(chars.next(), Some('-') | Some('/')) && chars.as_str().eq_ignore_ascii_case(switch)
    })
}

/// Port of `wbFindCmdLineParam`: whether a parameter is `switch` or
/// `switch:value`, and the value.
fn find_param(args: &[String], switch: &str) -> Option<String> {
    for argument in args.iter().skip(1) {
        if !is_switch(argument) {
            continue;
        }
        let rest = &argument[1..];
        let prefix = format!("{switch}:");
        if rest.len() >= prefix.len()
            && rest.is_char_boundary(prefix.len())
            && rest[..prefix.len()].eq_ignore_ascii_case(&prefix)
        {
            return Some(rest[prefix.len()..].to_owned());
        }
        if rest.eq_ignore_ascii_case(switch) {
            return Some(String::new());
        }
    }
    None
}

/// Port of `HexToInt`.
fn hex_to_int(text: &str) -> Result<u32, Failure> {
    let text = if text.len() >= 2 && text[..2].eq_ignore_ascii_case("0x") {
        &text[2..]
    } else {
        text
    };
    u32::from_str_radix(text, 16).map_err(|_| Failure {
        class: "EConvertError",
        message: format!("'${text}' is not a valid integer value"),
    })
}

/// Delphi `StrToIntDef(text, 0)`.
fn str_to_int_def(text: &str) -> i64 {
    let text = text.trim();
    let (negative, digits) = match text.strip_prefix('-') {
        Some(rest) => (true, rest),
        None => (false, text.strip_prefix('+').unwrap_or(text)),
    };
    let value = if let Some(hex) = digits.strip_prefix('$').or_else(|| digits.strip_prefix("0x")) {
        i64::from_str_radix(hex, 16).ok()
    } else {
        digits.parse::<i64>().ok()
    };
    match value {
        Some(value) if (-(1i64 << 31)..(1i64 << 31)).contains(&if negative { -value } else { value }) => {
            if negative {
                -value
            } else {
                value
            }
        }
        _ => 0,
    }
}

/// Delphi `Round`: to the nearest integer, halves to the even one.
fn round_half_even(value: f64) -> i64 {
    let floor = value.floor();
    let difference = value - floor;
    let rounded = if difference > 0.5 || (difference == 0.5 && floor % 2.0 != 0.0) {
        floor + 1.0
    } else {
        floor
    };
    rounded as i64
}

/// `ShowProgress`: after `processed` of `count` steps, writes the percent
/// every hundredth step (at least every tenth) and after the last one.
fn show_progress(processed: usize, count: usize) {
    let step = (count / 100).max(10);
    if processed.is_multiple_of(step) || processed + 1 == count {
        write(&format!(
            "\r{}%",
            round_half_even((processed + 1) as f64 / count as f64 * 100.0)
        ));
        flush();
    }
}

/// Delphi `TTimeSpan.ToString`: `[d.]hh:mm:ss[.fffffff]`.
fn format_elapsed(elapsed: Duration) -> String {
    let ticks = elapsed.as_nanos() / 100;
    let seconds_total = ticks / 10_000_000;
    let fraction = ticks % 10_000_000;
    let days = seconds_total / 86_400;
    let hours = seconds_total / 3600 % 24;
    let minutes = seconds_total / 60 % 60;
    let seconds = seconds_total % 60;
    let mut text = String::new();
    if days != 0 {
        text.push_str(&format!("{days}."));
    }
    text.push_str(&format!("{hours:02}:{minutes:02}:{seconds:02}"));
    if fraction != 0 {
        text.push_str(&format!(".{fraction:07}"));
    }
    text
}

/// `Split(['+'], '"')`: the parts of the text between `+`, a `+` inside
/// quotes not splitting; the quotes are dropped.
fn split_sources(text: &str) -> Vec<String> {
    let mut parts = vec![String::new()];
    let mut quoted = false;
    for c in text.chars() {
        match c {
            '"' => quoted = !quoted,
            '+' if !quoted => parts.push(String::new()),
            _ => parts.last_mut().expect("a part").push(c),
        }
    }
    parts
}

/// Delphi `ExtractFilePath`: up to and including the last `\`, `/` or `:`.
fn extract_file_path(path: &str) -> &str {
    path.rfind(['\\', '/', ':']).map_or("", |at| &path[..=at])
}

/// `DoInfo`.
fn do_info(args: &[String]) {
    let mut archive = Archive::new();
    match archive.load_from_file(&args[1]) {
        Ok(()) => {
            write_line(&archive.info());
            for warning in archive.warnings() {
                write_line(&format!("\tWarning: {warning}"));
            }
            let dump = find_switch(args, "dump");
            if !(find_switch(args, "list") || dump) {
                return;
            }
            write_line("");
            for file in archive.files() {
                write_line(&file.name);
                if dump {
                    write_line(&archive.file_info(file));
                    write_line("");
                }
            }
        }
        Err(error) => write_line(&format!("Error: {}", error.0)),
    }
}

/// `DoPack`. Returns the exit code.
fn do_pack(args: &[String]) -> Result<u8, Failure> {
    let argument = |index: usize| args.get(index).map_or("", String::as_str);
    let sources = argument(2);
    if sources.is_empty() || is_switch(sources) {
        return Err(Failure::invalid_arguments(
            "No source files/folders/archives provided for packing",
        ));
    }
    let archive_name = argument(3);
    if archive_name.is_empty() || is_switch(archive_name) {
        return Err(Failure::invalid_arguments("No archive name provided for packing"));
    }

    let kind = if find_switch(args, "tes3") {
        ArchiveType::Tes3
    } else if find_switch(args, "tes4") {
        ArchiveType::Tes4
    } else if find_switch(args, "fo3") || find_switch(args, "fnv") || find_switch(args, "tes5") {
        ArchiveType::Fo3
    } else if find_switch(args, "sse") {
        ArchiveType::Sse
    } else if find_switch(args, "fo4") {
        ArchiveType::Fo4
    } else if find_switch(args, "fo4dds") {
        ArchiveType::Fo4Dds
    } else if find_switch(args, "sf1") {
        ArchiveType::Sf
    } else if find_switch(args, "sf1dds") {
        ArchiveType::SfDds
    } else {
        return Err(Failure::invalid_arguments(
            "No archive type (game) provided for packing",
        ));
    };

    let mut packer = MultiSourcePacker::new();
    if kind != ArchiveType::Tes3
        && let Some(value) = find_param(args, "z")
    {
        if !value.is_empty() {
            let compression = CompressionType::type_by_name(&value).unwrap_or(CompressionType::None);
            if compression == CompressionType::None {
                return Err(Failure::invalid_arguments(&format!("Unknown compression type {value}")));
            }
            if !kind.supports_compression(compression) {
                return Err(Failure::invalid_arguments(&format!(
                    "{} archives don't support {value} compression",
                    kind.format_name()
                )));
            }
            packer.set_compression_type(compression);
        } else {
            packer.set_compression_type(kind.default_compression());
        }
        packer.set_compress(true);
    }

    match find_param(args, "split").filter(|value| !value.is_empty()) {
        Some(value) => {
            let split = str_to_int_def(&value).min(8);
            packer.set_split_size(split * GB);
        }
        None => {
            if kind < ArchiveType::Fo4 {
                packer.set_split_size(xedit_io::archive::BSA_MAX_OFFSET);
            }
        }
    }

    if let Some(value) = find_param(args, "f").filter(|value| !value.is_empty()) {
        let filters: Vec<String> = value.split(',').map(str::to_owned).collect();
        packer.set_filters(&filters);
    }

    packer.set_share_data(!find_param(args, "share").is_some_and(|value| value.eq_ignore_ascii_case("no")));
    let multi_threaded = !find_param(args, "mt").is_some_and(|value| value.eq_ignore_ascii_case("no"));
    // EXTENSION: `-threads:N` sets the number of threads; the archives do not depend on it.
    let threads = find_param(args, "threads").and_then(|value| value.trim().parse::<usize>().ok());
    packer.set_threads(if multi_threaded { threads.unwrap_or(0) } else { 1 });

    if let Some(value) = find_param(args, "af").filter(|value| !value.is_empty()) {
        packer.set_archive_flags(hex_to_int(&value)?);
    }
    if let Some(value) = find_param(args, "ff").filter(|value| !value.is_empty()) {
        packer.set_file_flags(hex_to_int(&value)?);
    }

    write_line(&format!(
        "Packing {} archive: Split: {},  Compress: {},  Share: {}",
        kind.format_name(),
        if packer.split_size() != 0 {
            format_size(packer.split_size())
        } else {
            "No".to_owned()
        },
        if packer.compress() {
            packer.compression_type().name().to_owned()
        } else {
            "No".to_owned()
        },
        if packer.share_data() { "Yes" } else { "No" }
    ));

    for source in split_sources(sources) {
        write(&format!("Adding source: {source}"));
        let count = packer.add_source(&source)?;
        write_line(&format!("  {count} file(s)"));
    }

    if packer.source_files_count() == 0 {
        return Err(Failure::invalid_arguments("No valid source file(s) found."));
    }

    packer.create_archive(archive_name, kind)?;

    write_line(&format!(
        "{}threaded packing: {} file(s)...",
        if packer.multi_threaded() { "Multi" } else { "Single" },
        packer.source_files_count()
    ));
    flush();

    let started = Instant::now();
    let count = packer.process_count();
    let result = packer.process(&mut |done| show_progress(done, count));
    if let Err(message) = result {
        write_line("");
        write_line(&message);
        return Ok(1);
    }

    packer.save().map_err(Failure::general)?;
    let elapsed = started.elapsed();

    write_line("");
    write_line(&format!("Done in {}.", format_elapsed(elapsed)));
    write_line("");
    write_line("Created archives:");
    for archive in packer.archives() {
        let mut line = format!(
            "{}   {}  {} files",
            archive.file_name(),
            format_size(archive.archive_size()),
            archive.count()
        );
        if archive.archive_shared_files() != 0 {
            line.push_str(&format!(
                "  {} shared saving {}",
                archive.archive_shared_files(),
                format_size(archive.archive_shared_size())
            ));
        }
        write_line(&line);
        for warning in archive.warnings() {
            write_line(&format!("\tWarning: {warning}"));
        }
    }
    write_line("");
    Ok(0)
}

/// `DoUnpack`. Returns the exit code.
fn do_unpack(args: &[String]) -> Result<u8, Failure> {
    let argument = |index: usize| args.get(index).map_or("", String::as_str);
    let archive_name = argument(2);
    if archive_name.is_empty() || is_switch(archive_name) {
        return Err(Failure::invalid_arguments("No archive file provided for unpacking"));
    }
    let third = argument(3);
    let folder = if third.is_empty() || is_switch(third) {
        extract_file_path(archive_name).to_owned()
    } else {
        let folder = include_trailing_path_delimiter(third);
        if !Path::new(&folder).is_dir() {
            return Err(Failure::invalid_arguments(&format!("Folder does not exist: {folder}")));
        }
        folder
    };

    let mut archive = Archive::new();
    archive.load_from_file(archive_name)?;
    let multi_threaded = !find_param(args, "mt").is_some_and(|value| value.eq_ignore_ascii_case("no"));

    let started = Instant::now();
    write_line(&format!("Unpacking archive \"{archive_name}\" into \"{folder}\""));
    write_line(&format!(
        "{}threaded unpacking: {} file(s)...",
        if multi_threaded { "Multi" } else { "Single" },
        archive.count()
    ));
    flush();

    let count = archive.count();
    let result = unpack_archive(&archive, &folder, if multi_threaded { 0 } else { 1 }, &|done| {
        show_progress(done, count)
    });
    let elapsed = started.elapsed();
    write_line("");
    match result {
        Err(message) => {
            write_line(&message);
            Ok(1)
        }
        Ok(()) => {
            write_line(&format!("Done in {}.", format_elapsed(elapsed)));
            Ok(0)
        }
    }
}

const CRLF: &str = "\r\n";
const LF: &str = "\n";

/// The marker after which the banner of `BSArch.exe` ends and its usage begins.
const BANNER_END: &str = "TES5Edit/TES5Edit\n\n";

/// The text `BSArch.exe` prints before anything else. Its first line break
/// is a bare LF, as in the output of the original, the others are `WriteLn`s.
fn banner() -> String {
    let text = usage_text();
    let end = text.find(BANNER_END).map_or(0, |at| at + BANNER_END.len());
    let rest = line_ends(&text[1..end]);
    format!("\n{rest}")
}

/// The usage file with LF line ends, whatever the checkout made of them.
fn usage_text() -> String {
    String::from_utf8_lossy(USAGE).replace(CRLF, LF)
}

fn usage() -> String {
    let text = usage_text();
    let start = text.find(BANNER_END).map_or(0, |at| at + BANNER_END.len());
    line_ends(&text[start..])
}

/// The text with the line ends of the platform (CR LF on Windows).
fn line_ends(text: &str) -> String {
    text.replace("\r\n", "\n").replace('\n', NEWLINE)
}

/// `Main`. Returns the exit code.
fn run(args: &[String]) -> Result<u8, Failure> {
    write(&banner());
    // At least one parameter and it is not a switch.
    if args.len() > 1 && !is_switch(&args[1]) {
        return if args[1].eq_ignore_ascii_case("pack") {
            do_pack(args)
        } else if args[1].eq_ignore_ascii_case("unpack") {
            do_unpack(args)
        } else if Path::new(&args[1]).is_file() {
            do_info(args);
            Ok(0)
        } else {
            Err(Failure::invalid_arguments(
                "The first parameter must be \"pack\", \"unpack\" or existing archive file",
            ))
        };
    }

    write(&usage());
    Ok(0)
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args_os()
        .map(|argument| argument.to_string_lossy().into_owned())
        .collect();
    let code = match run(&args) {
        Ok(code) => code,
        Err(failure) => {
            write_line(&format!("{}: {}", failure.class, failure.message));
            1
        }
    };
    flush();
    ExitCode::from(code)
}
