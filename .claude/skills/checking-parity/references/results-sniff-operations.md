# Sniff operations



```
cargo xtask parity sniff [--case <name or name part>]... [--game <game>]... [--archive <name part>]... [--sample <n>] [--threads <n>] [--show <n>] [--keep] [--refresh-oracle] [--list]
```

Each case (`CASES` in `crates/xtask/src/parity/sniff.rs`, `--list` prints them) is one Sniff operation with the settings of its ini section; an operation has a case for its defaults and one for each setting that takes another branch; `--case` with a full name runs that case, with another text every case whose name holds it.

For each archive of the games the operation supports that holds files it accepts, `Sniff.exe` runs in its automation mode (`-OP:<title> -I:<archive> -O:<folder> -S:<ini> -LOG:<file> -skip:yes -threads:<n>`) and the port runs in the harness process with the same ini (built in release, as for `parity nif`).

Compared are the output of each file (an FNV-1a hash), the error of each file Sniff skipped (two access violations are the same error), the other log lines as a multiset (Sniff's threads log in the order they finish), the counts of the summary line, and the log file a processor writes itself (`{log}` in a case's settings).

In a case's settings `{source}` is a source folder the port makes over the unpacked files themselves (`Prep` of the case: the controlled block priorities of `Copy anim priorities`, the controlled blocks renamed to `XYZ` for `Copy anim controlled blocks`, the transforms scaled by 1.5, the texture names of the shader texture sets set to `x.dds` and the transforms baked into the geometry of `Copy geometry blocks`, the first three with the universal tweaker and the last with `Apply transformation`).

A case whose operation reads files beside the input (`Add blocks from skeleton`) gets an input folder of its own with a `death.kf` of the archive and the `skeleton.nif` of its folder.

Sniff's threads share state: a crash with an access violation, and any file whose output or error differs from the port's (up to 40 per archive), is run again alone (`-P:<file> -threads:1`) before it counts (`DLC05ElevatorNavCut.nif` of `Remove nodes` was written differently among the others).

Some access violations repeat alone: `Remove nodes` on two Fallout 4 Creation Club meshes reads freed memory (`TdfElement.Root` in `Sniff.exe`, after `RemoveBranch` of several shapes), which depends on the heap and is not reproduced; they count as `oracle-failed`.

The oracle's results are cached in `<cache>/<tag>/sniff-oracle/<case>/<game>/`, keyed by the archive and the settings (`--refresh-oracle` runs Sniff again and replaces them); the report is `<scratch>/<tag>/sniff-report.txt` (`--show <n>` names that many files and log lines of each difference per archive, 3 by default).

After an archive is compared, its work folder `<scratch>/<tag>/sniff-work/<case>/<archive>` is removed when everything was equal, and only Sniff's outputs of the files that differ and the logs are kept otherwise; `--keep` keeps everything (a full run writes hundreds of GB otherwise).

Sniff, like the xEdit GUI of the oracle-save checks, starts hidden on a desktop of the harness's own (`parity/hidden.rs`: `SW_HIDE` and `CreateDesktopW`), so no window reaches the user's screen. `--sample <n>` unpacks the first `n` files of each archive into a folder and runs both on it (quick, for porting); the gate runs without it.

Outcomes per file: `equal`, `equal-error`, `unchanged` (neither writes it), `output-different`, `port-only-output`, `oracle-only-output`, `error-different`, `port-failed`, `oracle-failed`.

The run exits with an error when a fatal outcome is counted (`output-different`, `oracle-only-output`, `error-different`, `port-failed`); `oracle-failed` (Sniff's own crashes, which repeat alone) and `port-only-output` (an output Sniff's threads lost: the `group-shapes-split` case loses 714 of them at 16 threads, which is why it runs with `--threads 1`) do not fail it.

A log difference is printed and counted but does not fail the run either: on a file Sniff crashes on alone the port's `Updated:` line and the summary counts differ by construction (`Remove nodes` on `midfloortoroof01_carpet.nif` shows it), which the oracle's own crash explains.

To find the element of a difference, run both on the file alone and diff their `Convert to and from JSON` dumps with `sDigits=16`, and `cmp -l` the bytes.
