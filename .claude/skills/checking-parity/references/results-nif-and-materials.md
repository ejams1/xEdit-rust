# NIF and materials



```
cargo xtask parity nif [--game <game>]... [--archive <name part>]... [--file <path part>] [--threads <n>] [--keep]
cargo xtask parity nif --text [--sample <n>] ...
cargo xtask parity nif --from-json [--sample <n>] ...
```

Run it from a release build (`cargo build --release -p xtask`, then `target/release/xtask parity nif ...`): the port side runs in the harness process.

It reads every archive of the selected games' data folders with the port's archive reader (texture and sound archives hold none of the files and are skipped) and checks every `.nif`, `.kf`, `.bgsm` and `.bgem` in them against `Sniff.exe` of the release in its automation mode (`-OP:<operation> -I:<archive> -O:<folder> -S:<settings ini> -LOG:<file> -skip:yes -threads:<n>`; the settings ini holds the operation's options under its storage section, the title without spaces).

Two Sniff runs per archive are the oracle:

- the dump: `Convert to and from JSON` writes each NIF and KF as `ToJSON` with six decimals and rotations as angle and axis (`[ConverttoandfromJSON] bToJson=1 sDigits=6 iRotation=0`). Sniff writes the text with a `TStringStream`, which encodes it in the system's ANSI code page, so the port's JSON is compared after the same conversion (characters outside it become `?`).
- the save: `Universal tweaker` with `sBlocks=NiHeader sPath=Num Blocks sValue=99999` changes a value that `SaveToData` recomputes (`UpdateHeader`), so the written NIF is a plain load and save.

A material has no `Num Blocks`; the tweaker still counts it as changed and writes it, because `TdfElement.GetEditValues` leaves its string result unassigned for a missing element and Delphi passes the caller's variable as that result: the old value is read into an empty string and the new one into the variable that holds the new text.

The oracle's outputs are reduced to an FNV-1a hash per file (and the exception message of a file Sniff skipped) and cached as `<cache>/<tag>/nif-oracle/<game>/<archive>-<size>-<hash>.tsv`, keyed by the archive's name, size and a hash of its first and last megabyte.

Sniff's threads share state: rarely a file fails with an access violation among the others and reads fine alone, so a cached crash is run again on its own (`-P:<path> -threads:1`) before it counts.

A file whose output differs is written to `<scratch>/<tag>/nif-diff/<game>/<archive>/` (`<name>.port.json`, `<name>.port`); `--file <path part>` with a fresh Sniff run of the archive (delete its `.tsv`, or run Sniff by hand with `-P:`) gives the oracle's side.

The outcomes per file are `equal` (dump and save), `equal-error` (both refuse the file with the same message: Fallout 76 and Starfield meshes, `Unknown NIF version`), `dump-different`, `save-different`, `port-failed`, `oracle-failed` and `oracle-missing`; the report is `<scratch>/<tag>/nif-report.txt`.

The run exits with an error when a fatal outcome is counted (`dump-different`, `save-different`, `port-failed`); `oracle-failed` (Sniff's own crash, which repeats alone) and `oracle-missing` (Sniff wrote nothing for the file and did not skip it: its threads share state and can lose an output) do not fail it.

`--text` covers what Sniff does not: the GUI build runs `crates/xtask/oracle/dataformat.pas` through its script adapter of the data format units (in FO4 mode on an empty plugin the harness writes), which writes `ToText` and `SaveToFile` of every material, LOD settings and tree LOD file of the corpus and of the first `--sample` (default 20) NIF and FUZ files of each archive, and `dfCalcHash` of every element name of the definitions; the port's text is compared as `TStringList.SaveToFile` writes it (CRLF lines, ANSI). `--from-json` builds NIFs back from the port's JSON dumps of the first `--sample` NIFs of each archive, with Sniff's `Convert to and from JSON` in the other direction and with the port's `FromJSON`, and compares the bytes; a dump both refuse with the same message (Sniff logs `Skipped: <file>: <message>`) is `equal-error`.

Rotations are text there (angle and axis), so this is the check of the `Sin` and `Cos` of `AxisAngleToM33`; `--from-json` alone covers few angles, so a change there is also checked on dumps with many generated rotations through the same Sniff command line.
