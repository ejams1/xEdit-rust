# Phase 8 performance review

A static, read-only review of the port ahead of phase 8 (Performance) of [PLAN.md](PLAN.md). It names where time and memory go today, what can run in parallel, where SIMD kernels with runtime dispatch pay, and what can be simplified. Nothing here was measured; every item cites the code it describes and says how to keep the output identical, as the phase's rule requires ("Every optimisation keeps a scalar fallback and must pass the full parity harness. Output never depends on thread count or CPU features.").

Reviewed at `main` `2dea413` (all crates) and `phase-7-gui` `e739a19` (`crates/xedit-gui`). Line numbers refer to those commits.

## Contents

1. [What is already good](#1-what-is-already-good)
2. [Numbers on record](#2-numbers-on-record)
3. [The ten highest-payoff changes](#3-the-ten-highest-payoff-changes)
4. [Findings by gate workload](#4-findings-by-gate-workload)
5. [Findings by crate](#5-findings-by-crate)
6. [SIMD plan](#6-simd-plan)
7. [Concurrency plan](#7-concurrency-plan)
8. [Build profile and benchmarks](#8-build-profile-and-benchmarks)
9. [Leave alone](#9-leave-alone)
10. [Measure before acting](#10-measure-before-acting)
11. [Suggested order of work](#11-suggested-order-of-work)

## 1. What is already good

These are in place and should not be re-proposed.

- **Zero-copy read path.** Plugins are memory-mapped (`crates/xedit-io/src/mapped_file.rs`); sub-record and value elements keep `(DataBlock, start, end)` ranges into the map or the decompressed buffer (`crates/xedit-core/src/implementation/sub_record.rs:50-54`, `value.rs:47-51`); bytes are materialised only after an edit. Archive tables are parsed from the map (`crates/xedit-io/src/archive/mod.rs:385-462`) and compressed entries decompress straight into the result buffer.
- **Parallel scan with deterministic replay.** `scan.rs` splits a plugin into pieces, scans them on the rayon pool and replays the recorded `Attach/Progress/Register` events in file order (`crates/xedit-core/src/implementation/scan.rs:149-212`). The `InitOnce` + retire/epoch scheme in `threads.rs` lets many threads build records at once, with `init_cycles` so a caller can redo timing-dependent work serially.
- **Parallel dump, reference build, conflict scan, check and archive pack/unpack**, each with an ordered merge: dump batches of 512 records in file order (`crates/xedit-session/src/dump.rs:1010-1180`); reference collection in 4096-record batches applied in file order (`crates/xedit-core/src/implementation/refs.rs:712-760, 878-896`); conflict groups per FormID chain with the order-dependent groups (GMST/DFOB/NAVI/TES4) classified serially (`crates/xedit-analysis/src/conflict.rs:1300-1380`); check batches of 512 (`crates/xedit-analysis/src/check.rs:136-163`); the packer's prefetch pipeline (`crates/xedit-io/src/archive/packer/mod.rs:259-331`) and Sniff's bounded worker/commit pipeline (`crates/xedit-assets/src/sniff/main_form.rs:504-590`).
- **Fast library routes where bytes must match.** zlib inflate and CRC32 through libdeflate (which already dispatches PCLMUL/AVX-512 internally), deflate split between libdeflate level 12 and real zlib above 8 MiB for byte parity (`crates/xedit-io/src/compression.rs`, `hash.rs:18-23`). `mimalloc` in all binaries.
- **Sorted-vector FormID index** with `partition_point` and a cached fixed FormID per record (`crates/xedit-core/src/implementation/mod.rs:1231-1252, 3076-3104`); lock-free definition cells (`ArcSwapOption`, `interface/def.rs:76-100`).
- **Writer fast paths.** Unmodified groups and records are written as raw copies of the loaded bytes (`write.rs:485-495, 660-663`); the output `Vec` is pre-sized to the loaded length; save is temp file + rename.
- **One SIMD kernel with the right shape.** `crates/xedit-io/src/simd.rs` has a `Level` enum (Scalar/Ssse3/Avx2), runtime detection, `XEDIT_SIMD=off`, a scalar definition and an equivalence test over every input shape. New kernels should copy this shape. `crates/xedit-core/src/half_float.rs` has an F16C half/float kernel with scalar fallback that currently has no callers.
- **Script interpreter basics.** Sorted and binary-searched registrations, lazily parsed and cached routine bodies, byte-vector strings so Pascal 1-based `Copy`/`Pos` are O(1) slices, no global handle table.
- **GUI.** Row-virtualised tree and grid (`show_rows`), lazy child build, event-driven repaints, session moved to a worker thread for load and jobs, conflict colours fetched lazily in batches of 200.
- **NIF model** is an arena of `El(u32)` indices, not boxed objects; Sniff's file order and output do not depend on the thread count.

## 2. Numbers on record

The only timings in the repository, from the phase histories and the one criterion bench (`crates/xedit-session/benches/dump.rs`). They are the baseline the gate will be measured against.

| Workload | Port | Oracle (xEdit 4.1.5q) | Source |
| --- | --- | --- | --- |
| Load `Skyrim.esm` + `Update.esm` + hardcoded (SSE) | bench exists, no number recorded | 2.4 s | `benches/dump.rs` doc |
| Dump `Update.esm` (938 MB of text) | bench exists, no number recorded | 98 s | `benches/dump.rs` doc |
| Reference build, 5 SSE masters | 23 s on 16 threads, 60 s on 1; cache load under 1 s | not recorded | `docs/history/phase-4.md:77` |
| Reference build, FO3 masters | 4.6 s on 16 threads, 14 s on 1 | not recorded | same |
| `conflicts.list`, Skyrim SE | 3.8 s on 16 threads, 12.6 s on 1 | about 6 min for Fallout 4 | `docs/history/phase-4.md:49` |
| `conflicts.list`, Fallout 76 | 23 s on 16 threads, 48 s on 1 | not recorded | same |

Two things follow. The parallel speed-up on 16 threads is 2.6x for refs and 3.3x for conflicts, so both have a large serial share (sections 4 and 5 say where). And there is no benchmark at all for refs, conflicts, clean or archive pack, which are four of the five gate workloads (section 8).

## 3. The ten highest-payoff changes

Ranked by expected gain across the gate workloads, independence from other changes, and parity risk. Each is expanded in section 5.

| # | Change | Workloads | Category | Gain | Parity risk |
| --- | --- | --- | --- | --- | --- |
| 1 | **Add a `[profile.release]`** (fat LTO, `codegen-units = 1`); there is none today, so every parity and benchmark number comes from thin-local LTO and 16 codegen units. Keep `panic = "unwind"` (Sniff and the CLI rely on `catch_unwind`/`resume_unwind`). | all | build | medium, free | none |
| 2 | **Stop running the clean/filter conflict scan one record at a time.** For FO3, FNV, TES4, TES5 and SSE, `can_sort_info()` is true (`crates/xedit-session/src/save.rs:84-102`), so `apply_filter` takes the `statuses_as_walked` path (`crates/xedit-analysis/src/filter.rs:1559-1562, 1675-1676`): every FormID chain is its own one-element `pool.install`, zero parallelism, with the batch scan used only for the other games. Batch-classify every chain that holds no DIAL/INFO record; keep the per-record path for those. | clean, filter, conflicts via filter | concurrency | large | topic-sort timing; guarded by the FNV/FO3 filter and clean fixtures |
| 3 | **Byte-identical-to-master fast path in the conflict scan** with an AVX2/SSE2 `memcmp` kernel: `content_equals` exists (`conflict.rs:1613`) but is reachable only through `is_compare_to_same`, which is always false in the port. Most overrides in large load orders are byte-identical ITMs that today pay a full element build and sort-key walk of both records. | conflicts, clean, filter | SIMD, algorithmic | large | equal bytes do not always mean equal classification (partial forms, master-index mapping, `cpIgnore` rows); ship behind a dual-run diff mode and narrow conditions |
| 4 | **Memoise FormID resolution per file in the reference build** and borrow the master list under the lock instead of cloning it per FormID (`refs.rs:460-487`); add a contiguous `Vec<u32>` key array beside `fl_records` so the binary search stops pointer-chasing through 750-byte records (`mod.rs:1231-1250`). | refs, conflicts, dump | algorithmic, allocation | large | none |
| 5 | **ASCII fast path for Windows code-page decoding and encoding.** `get_string` does two `MultiByteToWideChar` calls and two allocations per string (`crates/xedit-io/src/encoding.rs:240-255`) for every EDID, FULL, path, archive name and string-table entry. Bytes below 0x80 are identity in every ANSI/DBCS code page. | load, dump, refs, conflicts, startup, GUI | allocation | large | test every code page xEdit uses against the Win32 result; keep Win32 as the non-ASCII path |
| 6 | **Per-thread bookkeeping instead of the global `INITIALIZED_RECORDS` and `PINNED_RECORDS` mutexes** taken on every record build and pin (`mod.rs:2050-2126, 3460-3470`). Three reviewers found this independently from refs, conflicts and dump. | refs, conflicts, dump, check | concurrency | medium, grows with cores | none (trim order affects only memory) |
| 7 | **Stop cloning element lists on every access.** `elements()` is a `RwLock` read plus a `Vec<Arc>` clone (`mod.rs:774-776`); `init_main_record` calls it once per sub-record (O(N²) atomics, `sub_record.rs:1876-1880`), `element_by_name` collects every child then allocates a `String` per child to compare (`mod.rs:381-399`), the writer clones per container (`write.rs:276-290, 485-498`), the NavTree build two or three times per container (`filter.rs:147-270`). Add `with_elements(|&[ElementRef]|)`, `element_at`, and a non-allocating `name_matches`. | load, refs, conflicts, dump, save, clean | allocation | medium, everywhere | none |
| 8 | **Two-phase record registration after the parallel scan.** The `Register` replay (SipHash `HashSet` under a mutex, master binary search, `add_override` write lock per record) and `sort_records` are serial (`scan.rs:173-178`, `mod.rs:1026-1058, 1386-1392`). Sort in parallel with a stable cached-key sort, find duplicates by adjacent scan, resolve masters in parallel, apply overrides serially in file order. | load | concurrency | medium-large on 1M-record masters | override-list and message order; keep serial path for `threads() == 1` |
| 9 | **NIF leaves: inline small values and lazy vertex arrays.** Every value copies its bytes into its own `Vec<u8>` and every union member is materialised (`crates/xedit-assets/src/data_format.rs:2094-2128, 2266-2286`): about 27 nodes and 20 allocations per BSTriShape vertex. Pair with typed bulk vertex readers using the existing F16C kernel instead of the per-component `format!("[{index}]")` + `FloatToStr` + `StrToFloat` route (`data_format_nif/geometry.rs:28-67`). | nif, sniff, lodgen | allocation, SIMD | large | bit-exact; guard the lazy path to member defs with no callbacks |
| 10 | **Script runtime: stop re-lexing the source per statement and per call** (`crates/xedit-script/src/eval.rs:1556-1558, 1973, 2583-2617`) and make `Value::Str` copy-on-write (`values.rs:1068-1113`). Together these are the inner loop of every per-record script. | script run | algorithmic, allocation | large | positions must come from the same tokenizer at parse time; value semantics via `Rc::make_mut` |

## 4. Findings by gate workload

The phase gate names five workloads. This maps the findings onto them, most valuable first; the section 5 anchors hold the detail.

**Load** (gate: faster than Delphi). Release profile (#1). Encoding fast path (#5). Two-phase registration (#8). `init_main_record` list clones and the skip-list clones per sub-record (#7, [core-load 7](#core-load-7)). `PrefetchVirtualMemory` on the map before the serial header walk: cold-cache load currently faults 4 KiB pages one at a time on one thread, and the Delphi build reads the file into memory ([core-load 8](#core-load-8)). `find_record_def` under two `RwLock`s and a SipHash per record ([core-load 12](#core-load-12)). Archive table parsing: six allocations and two syscalls per name, SipHash on keys that are already xxHash64 values ([io 4](#io-4), [io 6](#io-6)); resource containers opened for commands that never read a resource ([session 7](#session-7)). LZ4 frame decompression through `lz4_flex::FrameDecoder` with an internal 4 MiB buffer and a second copy, while `lz4-sys` is already linked ([io 1](#io-1), [io 2](#io-2)).

**Reference build** (gate). FormID memo and key array (#4). `BTreeSet<u32>` collector per record replaced by a reused `Vec` + sort + dedup ([core-write 2](#core-write-2)). Global mutexes (#6). `DataBlock` Arc clone per element, a shared counter all workers hammer ([core-load 5](#core-load-5)). `read_built` Weak-upgrade per child access ([core-load 9](#core-load-9)). Cache file encode/decode allocations ([core-write 9](#core-write-9)). `crc32_app` hashing the whole executable serially right before the first build ([core-write 10](#core-write-10)).

**Conflict scan** (gate). Filter path (#2). Identical-to-master fast path (#3). `overrides()` Vec allocation per record in the serial prologue over 23M records ([analysis 3](#analysis-3)). Sort keys computed up to three times per element and per-row `HashSet<String>` allocations ([analysis 4](#analysis-4)). Barrier every 1024 groups and the global mutexes ([analysis 7](#analysis-7)). `HashMap<i32, usize>` per `ViewContainer` ([analysis 8](#analysis-8)). Localized string lookup formatting the table path on every read ([core-write 6](#core-write-6), [core-load 11](#core-load-11)). `conflicts.list` allocating two `String`s per record for counters ([analysis 12](#analysis-12)).

**Clean** (gate). Everything under conflict scan, plus: NavTree built two or three times per container ([analysis 9](#analysis-9)); passes 2 and 3 recompute the whole tree and scan ([analysis 10](#analysis-10)); `references_injected` re-resolves every reference ([core-write 11](#core-write-11)); the save at the end initialises every CELL record serially for the INCC count ([core-write 7](#core-write-7)) and grows ONAM one element at a time through the edit machinery ([core-write 12](#core-write-12)).

**Archive pack** (gate). Compressor states allocated per chunk (libdeflate level 12 is on the order of a megabyte; LZ4HC mallocs ~256 KB internally) ([io 5](#io-5)); `lookup_hash_text` allocating twice per name ([io 4](#io-4)); up to 30 `format!` per file in `asset_type_by_folder` ([io 10](#io-10)); `unpack_archive` re-hashing names it already has ([io 11](#io-11)); DXT encode scalar per block and serial ([assets 5](#assets-5)) for the texture conversion route.

**Beyond the gate.** Dump: release profile, encoding, `read_built`, fixed four batches in flight ([session 6](#session-6)), stdout through the 1 KiB `LineWriter` ([session 3](#session-3)). Save: writer list clones, INCC, ONAM. Scripts: section [script](#xedit-script-and-the-script-host). Assets and LODGen: section [assets](#xedit-assets). Serve/MCP: unbuffered pipe writer, one `WriteFile` per JSON fragment ([session 2](#session-2)); MCP result serialised twice ([session 8](#session-8)). GUI: section [gui](#xedit-gui-phase-7-gui-branch). Startup: 230 JSON schemas built per process ([session 5](#session-5)).

## 5. Findings by crate

Format per finding: location, what happens now, the change, category, workload, gain, parity risk. Items that several reviewers found are listed once, under the crate that owns the code, with cross-references.

### xedit-io

<a id="io-1"></a>**io 1. LZ4 frame decompression takes the slow, double-buffered route.**
`crates/xedit-io/src/compression.rs:394-409` (also `:136`). `FrameDecoder::new(src); decoder.read_exact(dst)` with lz4_flex's default `safe-decode` decodes each block into internal buffers reserved to the frame's max block size (4 MiB for BSArch frames) and then copies into `dst`; the end-of-frame check does a second `read`. Change: use the linked C library, `LZ4F_createDecompressionContext` + `LZ4F_decompress` writing directly into `dst`, with a thread-local context, and the "processed bytes don't match" rule reproduced from the returned hint and consumed length; keep lz4_flex as the fallback behind a feature for the equivalence test. Category: io. Workload: load (every SSE/AE BSA read), nif/sniff, archive unpack. Gain: large for Skyrim SE workloads. Parity: deterministic by format; decode the parity corpus both ways and compare bytes and Ok/Err.

<a id="io-2"></a>**io 2. LZ4 block decompression uses lz4_flex safe-decode while lz4-sys is linked.**
`compression.rs:387-392`: `lz4_flex::block::decompress_into(src, dst)`. Change: `lz4_sys::LZ4_decompress_safe`, treating a negative result or `written != dst.len()` as the existing error. Workload: load (Starfield LZ4 records via `value.rs:899`, Starfield `.ba2`). Gain: medium, Starfield only. Parity: same bytes by definition; keep the fallback and a corpus test.

<a id="io-3"></a>**io 3. Windows text decoding and encoding: two kernel calls and three passes per string.** (Also found from the load path: [core-load 3](#core-load-3).)
`encoding.rs:240-255` (`get_string`), `:257-293` (`get_bytes`); hot callers `crates/xedit-core/src/interface/string.rs:237`, `len_string.rs:158`, `archive/mod.rs:440,446,460`, `localization.rs:364-386`. Change: `if bytes.is_ascii()` return `String::from_utf8` directly (std's `is_ascii` is already SWAR; an AVX2/AVX-512BW `movemask` version is optional); mirror in `get_bytes`. For the single-byte code pages 1250-1258, a 256-entry table built once from `MultiByteToWideChar` itself keeps best-fit behaviour identical; DBCS pages keep the Win32 call. Add a `memchr` zero scan for the terminator search (`string.rs:230`, `localization.rs:391`). Category: allocation. Workload: load, dump, refs, conflicts, startup, GUI. Gain: large. Parity: test every byte string of the corpus for code pages 1252, 1250, 1251, 932, 936, 949, 950, 65001 against the Win32 result.

<a id="io-4"></a>**io 4. `lookup_hash_text` allocates twice per call and the hash maps re-hash a hash.**
`hash.rs:174-185`; callers `archive/mod.rs:671,907`, `write.rs:543,930`, `packer/mod.rs:499`, `process.rs:104,356`; maps `archive/mod.rs:365,367,523-524`, `packer/mod.rs:352`, `process.rs:111`. `lower_case_ascii(text)` (alloc) then `encode_utf16().flat_map(to_le_bytes).collect()` (alloc) then `xxh64`; `by_hash`, `packed_data`, `source_by_hash` are `std::HashMap` with SipHash over 64-bit xxHash keys. Every `Archive::read` from xedit-core also pays `normalize_path`'s `replace` + `lower_case_ascii` (`mod.rs:1232-1234`). Change: a streaming `Xxh64` state fed from `encode_utf16()` with case folding on units below 0x80, no allocation; optional AVX2 kernel (`_mm256_cvtepu8_epi16` widen + range-compare/add 0x20) for ASCII runs; `BuildHasherDefault<IdentityHasher>` or `FxHasher` for the hash-keyed maps (none is iterated for output). Category: algorithmic, allocation. Workload: startup, load, archive pack. Gain: medium. Parity: unit test `streaming == lookup_hash_text` over random and Unicode strings.

<a id="io-5"></a>**io 5. Compression and decompression states allocated per call.**
`compression.rs:272` (`Compressor::new(level)` at level 12 per chunk), `:310` (`LZ4_compress_HC`, ~256 KB internal malloc), `:377` (`Decompressor::new()` per compressed record, `implementation/mod.rs:4012`), `:338-360` (`LZ4F_compressFrame` creates a cctx per call). Change: `thread_local!` `RefCell<Compressor>` / `RefCell<Decompressor>` keyed by level; `LZ4_compress_HC_extStateHC` with a thread-local aligned state; a cached cctx with `LZ4F_compressBegin/Update/End` and `auto_flush = 1` (already written at `:150-267`), which gives identical bytes. Category: allocation. Workload: archive pack (main), load, save. Gain: medium for pack. Parity: libdeflate compressors are stateless between calls, LZ4HC `extStateHC` re-inits; confirm with `parity bsarch`.

<a id="io-6"></a>**io 6. Archive table load does extra per-name work.**
`archive/mod.rs:877` (`string_len16()?.replace('/', "\\")`), `:777` (`format!("{}\\", folder.name)` then `push_str`), `:455-458` (scalar `position(|&b| b == 0)`), `:670-673`. A BA2 name costs `ansi_string` (two syscalls, two allocations) + `replace` + `lookup_hash_text` (two more) + SipHash insert. Change: replace `0x2F` with `0x5C` on the raw bytes before decoding (0x2F is never a DBCS trail byte), decode once with the ASCII fast path, build folder + name with `String::with_capacity`, `memchr::memchr(0, rest)` for `string_term` (the crate is already in `Cargo.lock`). With io 3 and io 4 this goes from ~6 allocations + 2 syscalls per name to ~1 allocation. Workload: startup. Gain: medium on FO4/SF (hundreds of thousands of names). Parity: the `name_hashes_equal_the_tables_of_the_game_archives` test (`mod.rs:1364`) plus a names-equal test over the corpus.

<a id="io-7"></a>**io 7. Uncompressed entries are copied out of the map.**
`archive/mod.rs:994-996, 1026, 1037`, `texture.rs:446`; API `mod.rs:946` (`read -> Option<Vec<u8>>`), consumer `crates/xedit-core/src/container_handler.rs:76-90,156-162`. Change: return `Cow<'_, [u8]>` (borrowed for uncompressed, owned for decompressed) from `unpack_entry`/`read`, and let `open_resource` hand out `Cow`/`Arc<[u8]>`; `unpack_archive` can `fs::write` the borrowed slice. Category: allocation. Workload: load, nif/sniff, archive unpack. Gain: small-medium, large for big uncompressed sounds and LOD. Parity: none, compile-time API change.

<a id="io-8"></a>**io 8. Locale comparison allocates two UTF-16 buffers per comparison.**
`collate.rs:36-37,61-62`, `encoding.rs:217-218`; sort callers `crates/xedit-session/src/script.rs:150`, `script_misc.rs:1774`, `script_rtl/sysutils.rs:314`, `script_host.rs:449`, `crates/xedit-analysis/src/merged_patch.rs:556`. An O(n log n) sort does two allocations per comparison. Change: stack buffer `[u16; 256]` with heap only beyond; for sorts, `LCMapStringW(LCMAP_SORTKEY | NORM_IGNORECASE)` once per string and sort by `memcmp` of keys (documented as giving the same order as `CompareString` with the same flags). Workload: GUI, scripting. Gain: small-medium on large script string lists. Parity: property test sort keys against `CompareStringW`; keep `CompareStringW` as fallback.

<a id="io-9"></a>**io 9. `files_by_folder` lower-cases every file name per call.**
`archive/mod.rs:927-935`; callers `container_handler.rs:212-216`, `crates/xedit-session/src/archive.rs:165`. Change: `file.name.as_bytes().get(..folder.len()).is_some_and(|p| p.eq_ignore_ascii_case(folder.as_bytes()))`, the same predicate. Gain: small; listings over 100k-entry BA2s become allocation-free.

<a id="io-10"></a>**io 10. Per-asset string churn in `asset_type_by_folder` and friends.**
`archive/asset.rs:225-233` (`lowered.starts_with(&format!("{}\\", asset.root))` inside a 30-entry loop), `:359` (`do_not_compress` lower-cases the whole name again), `write.rs:137-158` (`detect_flags` calls both per file), `get_asset_name` `:251-321` (several `Vec<char>` and `format!` per call). Change: `strip_prefix` + separator check, fold case once per file, `const` prefixes with the trailing backslash. Category: simplification. Workload: archive pack. Gain: small, noticeable on 100k-file packs. Parity: none.

<a id="io-11"></a>**io 11. Unpack paths re-derive what the entry already knows.**
`packer/process.rs:523` (`archive.unpack(&file.name)` instead of `unpack_entry(file)`), `texture.rs:402` (`archive.file_name.to_lowercase().contains("_xbox.")` per texture unpack), `write.rs:831` (`name.clone()` per chunk, used only for the embedded-name case). Change: call `unpack_entry(file)`, compute the `_xbox.` flag once per archive, borrow the name in `pack_chunk`. Gain: small. Parity: none.

Not worth doing in xedit-io: a SIMD CRC32 (libdeflate already dispatches), SIMD xxHash64 (a sequential 4-lane design), switching zlib implementations (bytes would change), SIMD UTF-8 validation (std's `from_utf8` already is).

### xedit-core: load and read path

Per-element footprint as read: every node is an `Arc<dyn Element>` whose `ElementBase` (`mod.rs:538-553`) holds a `RwLock<Option<Weak<dyn Element>>>` parent, a `RwLock<String>` name suffix and six atomics (~96 B); containers add `ContainerBase` (~64 B plus the child `Vec`); `ValueImpl` (`value.rs:43-78, 294`) comes to roughly 380 B, `SubRecordImpl` (`sub_record.rs:39-76`) roughly 360 B, and `MainRecordImpl` (`mod.rs:2672-2777`) roughly 750 B with 13 locks and five `RwLock<String>`-class name caches. Headers are copied into small structs; data bytes are borrowed from the map unless compressed or edited. A 1M-record plugin costs about 0.75-0.8 GB in main-record objects before any sub-record is built.

<a id="core-load-1"></a>**core-load 1. FormID lookup pointer-chases through 750-byte records.** (Top-ten #4.)
`mod.rs:1231-1250, 1386-1392`, `refs.rs:460-485`. `sorted.partition_point(|record| record.get_fixed_form_id().to_cardinal() < key)` over `Vec<Arc<MainRecordImpl>>`: each of ~20 probes touches a different record and does an Acquire load. Change: `fl_form_id_keys: RwLock<Vec<u32>>` mirrored from `fl_records` (rebuilt in `sort_records`, kept in sync in `add_main_record` and where `clear_fixed_form_id` runs), searched with a branchless binary search or an Eytzinger layout (an SSE2/AVX2 compare of 8 or 16 keys per step is optional), then index `fl_records`. Category: algorithmic. Workload: refs (dominant), conflicts, dump, GUI. Gain: large; the probe chain is memory-latency bound. Parity: equivalence test that the key array equals `records.map(get_fixed_form_id)` after every mutation; first-of-equal semantics preserved.

<a id="core-load-2"></a>**core-load 2. Record registration, duplicate check and sort are serial after the parallel scan.** (Top-ten #8.)
`scan.rs:173-178`, `mod.rs:1026-1058, 1040, 1386-1392`, `form_ids.rs:65-86`. Each replayed `Register` does `fl_scanned_form_ids.lock().unwrap().insert(..)` (SipHash `HashSet<u32>` under a mutex), a push, a master binary search and `add_override` (write lock on the master record); then `sort_records` on one thread. Events are also copied once per nesting level in `merge`. Change: two-phase build. Concatenate records in file order, `par_sort_by_cached_key` (stable) on the fixed FormID, mark duplicates by adjacent scan (first in file order wins, as the set does today), resolve each record's master in parallel into a `Vec<Option<Arc<..>>>`, apply `add_override`/`inject_main_record` serially in file order, emit the duplicate and injection progress messages from the recorded results. Category: concurrency. Workload: load. Gain: medium-large on big masters. Parity: override-list order (`add_override` inserts by load order, `mod.rs:3277-3291`), injected list order and message order must be reproduced; keep the serial path for `threads() == 1` and diff logs in the harness.

<a id="core-load-3"></a>**core-load 3. String decoding.** See [io 3](#io-3).

<a id="core-load-4"></a>**core-load 4. `element_by_name` allocates a `String` per child per lookup.** (Top-ten #7.)
`mod.rs:381-399, 448-449`, `sub_record.rs:2124-2129`, `value.rs:160-170`. Collects every child into a `Vec`, then `find(|e| e.get_name().eq_ignore_ascii_case(name))`, then a second pass on `get_display_name(true)`; `SubRecordImpl::get_name` is `format!("{} - {}", ..)`, `ValueBase::name` is `name.to_owned()`. Definition callbacks use this heavily (88 path/name lookups in `callbacks/common.rs`, 16 in `fo4.rs`, 18 in `tes5.rs`), as do `get_grid_cell` and the master-list read at load. Change: `fn name_matches(&self, name: &str) -> bool` on `Element` with non-allocating per-type implementations; `Cow<'_, str>` from `get_name` where the name is the def's; iterate under the read lock instead of collecting. Workload: load (callbacks during init), refs, conflicts, dump, GUI. Gain: medium. Parity: same comparison order (name, display name, flags).

<a id="core-load-5"></a>**core-load 5. Every element clones the file-bytes `Arc`: one shared cache line hammered by all workers.**
`value.rs:93, 47-50`, `sub_record.rs:44`, `mod.rs:365-369`. `DataBlock::File(Arc<FileBytes>)` is one `Arc` per plugin whose count every element built or dropped on every worker increments and decrements. Change: a non-counted handle (a raw pointer with a `SAFETY` argument bounded by the owning record's `bytes: Arc<FileBytes>`, `mod.rs:2677`, or a `u16` file index into a global table); keep the `Arc` on `MainRecordImpl` and for the decompressed-buffer case. Also fold the duplicated name suffix (`ElementBase` `RwLock<String>` plus `ValueBase` `String`) into one `Option<Box<str>>`. Category: allocation. Workload: dump, refs, conflicts. Gain: medium on many cores (false sharing on one counter), ~56 B per element. Parity: none.

<a id="core-load-6"></a>**core-load 6. Global `INITIALIZED_RECORDS` mutex on every record build.** (Top-ten #6; also [analysis 7](#analysis-7), [core-write 3](#core-write-3).)
`mod.rs:2050-2055, 3460-3470, 2090-2126, 2063-2088`. `INITIALIZED_RECORDS.lock().unwrap().push_back(..)` inside `build()`; `trim_initialized_records` drains under the same lock; `is_pinned` scans `PINNED_RECORDS` linearly under another global mutex per trimmed record; `pin_record`'s `Drop` does a linear `position` + `swap_remove`. Change: per-thread ring buffers (thread-local `VecDeque`; the dump already keys by `rayon::current_thread_index`) trimmed by the owning thread, or a sharded lock by thread token; an `AtomicU32` pin count on `MainRecordImpl` instead of the pinned list; in `collect_one` skip pinning when a per-thread queue removes the cross-worker trim. Category: concurrency. Workload: refs, dump, conflicts, check. Gain: medium with 16+ workers. Parity: trim order affects memory only (`mod.rs:2093-2095` says so); keep `KEPT_RECORDS_PER_THREAD` semantics.

<a id="core-load-7"></a>**core-load 7. `init_main_record` clones the element list on every sub-record iteration.**
`sub_record.rs:1876-1880, 2010, 1228, 1829, 1859, 1831-1863`. `loop { let elements = record.container.elements(); ... elements.get(current_rec_pos) }` per sub-record is O(N²) atomics per record; `do_process` (`:1228`) the same per array member; per record `ignore_records()` and per sub-record `sub_record_to_skip()` clone Vecs under RwLocks; `sub_records.push(get_display_signature())` allocates a `String` per sub-record for an error message almost never printed. Change: `element_at(pos)` (exists, `mod.rs:777`) or hold the read guard; read the skip lists once per file load; build `sub_records` lazily when `found_error`. Workload: load, refs, conflicts, dump. Gain: medium (NPC_/CELL/REFR/WEAP have tens of sub-records and are built several times per session). Parity: none; `insert_element` changes positions, so keep re-reading by index.

<a id="core-load-8"></a>**core-load 8. mmap faulted page by page with no prefetch.**
`crates/xedit-io/src/mapped_file.rs:21-31`, `scan.rs:227-277`. `split` walks every record header serially (first touch of every 4 KiB page of a 300 MB master on one thread) before workers start. Change: `PrefetchVirtualMemory` on Windows right after mapping (`windows-sys` is already a dependency; add `Win32_System_Memory`), `Advice::WillNeed`/`populate()` elsewhere; or for files above a threshold read into the existing `FileBytes::Owned` variant (`mod.rs:291-293`) with large sequential reads; optionally move `split`'s header walk onto the pool per top-level group. Category: io. Workload: load (cold cache), startup. Gain: large cold, near zero warm. Parity: none.

<a id="core-load-9"></a>**core-load 9. `read_built` upgrades a `Weak` and re-checks init on every element access.**
`mod.rs:3394-3407, 5441-5447`. `self_arc()` (a Weak→Arc CAS loop) then `do_init` (thread-token TLS) then the RwLock read, per `get_element(i)` and `get_element_count()`. Change: fast path `if self.mr_init.is_done() { read the list }` before touching `self_arc`/`do_init`; add `for_each_element`/`with_elements` on `Container` for the dump, conflict and refs walkers. Category: lock contention. Workload: dump, conflicts, refs, GUI. Gain: medium (three atomic RMWs per child visit removed). Parity: `is_done` is Acquire and the reset flips it under the same elements lock, so the fast path sees a full build or falls through.

<a id="core-load-10"></a>**core-load 10. `MainRecordImpl` carries ~200 B of rarely used fields and 13 locks.**
`mod.rs:2672-2777` (`mr_editor_id`, `mr_full_name`, `mr_display_name`, `mr_name`, `mr_short_name`, `mr_precombined`, `mr_has_vwd_mesh`, `mr_index_keys`, `mr_collapsed`, `mr_pinned_data`, `mr_grid_cell`). Change: cold fields into `OnceLock<Box<MainRecordCold>>`; editor ID and full name as `Arc<str>` in one `ArcSwapOption<Names>`; `mr_struct` as an atomic snapshot (`ArcSwap<MainRecordStruct>` or 3×`AtomicU64` with a generation) since `get_signature()`/`form_id()` are read far more than written. Category: simplification. Workload: load (memory), GUI. Gain: ≈300 B × records (≈300 MB on a 1M-record master). Parity: the official-file-only name caching stays observable.

<a id="core-load-11"></a>**core-load 11. Localized value lookup.** See [core-write 6](#core-write-6).

<a id="core-load-12"></a>**core-load 12. `find_record_def` takes two `RwLock`s and a SipHash per main record created.**
`interface/constructors.rs:906-907, 937-940`, `mod.rs:2787`. Change: freeze after `init_records` into an `ArcSwap<Vec<(u32, Arc<MainRecordDef>)>>` sorted by signature (binary search over ~200 entries) or a perfect hash; in `scan_serially` cache the last `(signature, def)` since records in a group share a signature. Workload: load. Gain: small-medium. Parity: none.

<a id="core-load-13"></a>**core-load 13. `Storage`, `range` and `sr_def` reads lock on every data access.**
`edit.rs:41-70`, `value.rs:110-112, 125`, `sub_record.rs:822-829`. `data_raw` does `storage.current()` (Acquire + Mutex when present) then `*self.range.read().unwrap()`; `SubRecordImpl::def()` clones an `Arc<dyn RecordMemberDef>` under a RwLock each call. Change: `range` as two `AtomicUsize` (the end moves once), `sr_def`/`sr_value_def` as `ArcSwapOption` like `DefCell`, `Storage::current` from a `OnceLock`-style last pointer. Workload: dump, conflicts, refs. Gain: small-medium. Parity: none.

<a id="core-load-14"></a>**core-load 14. `split` and `scan_serially` re-parse every header; `MainRecordStruct::parse` re-reads globals per record.**
`scan.rs:227-277`, `structs.rs:201-237`. Change: resolve `HeaderLayout {size, tes3}` once per scan; parse with one `get(offset..offset+24)` and fixed-offset `from_le_bytes`; compare signatures as `u32`. Category: simplification. Workload: load. Gain: small. Parity: none.

<a id="core-load-15"></a>**core-load 15. Hot `format!` in per-record naming and FormID display.**
`mod.rs:5023-5050, 3380-3386, 120-130`, `interface/form_id_formater.rs:822-860`. `get_name` builds `short_name_internal` then `format!("{result} ({info})")`; `get_full_path` recurses with `format!` and a linear `index_of_id` per level; `to_sort_key` formats `"{position:02X}:{result}"`. Change: write into a reusable buffer passed down; avoid `get_full_path` except on error paths; cache `short_name` for non-official unmodified records, invalidated by `set_modified`. Workload: dump, conflicts (sort keys), GUI. Gain: small-medium. Parity: the upstream caching rule (official files only, `mr_name` not cleared on FormID change) must stay observable: cache only derived strings whose inputs `e_generation` tracks.

Startup and definitions: definitions are built once per game through the generated builder calls; nothing in the load path showed them as a hot spot, so no change is proposed there without a measurement (section 10).

### xedit-core: write, references, edit, regex

How the reference build works today: `build_or_load_refs` (`refs.rs:770-870`) classifies files as cache-load, build or rebuild-changed; records of all build files are concatenated in file order and handed to `collect_all` (`:712-760`), 4096-record batches on the pool with `with_max_len(8)`; each `collect_one` (`:683-708`) pins the record, builds its elements, walks them with `can_contain_form_ids` pushing FormIDs into a thread-local `BTreeSet<u32>`, resolves every FormID (`resolve_reference`, `:460-487`), resets the record and trims the global queue; if `init_cycles` moved the batch is redone on one thread; results are applied serially in record order (`apply_collected`, `:878-896`) onto the target's `mr_referenced_by` `Mutex<Vec>`, sorted lazily on first read with `sort_by_cached_key`. Cache files (`refcache.rs`) are an LZ4 frame around a per-record stream.

<a id="core-write-1"></a>**core-write 1. Memoise FormID resolution per file during the reference build.** (Top-ten #4.)
`refs.rs:460-487, 692-695, 815-828`; `mod.rs:1289-1310, 3300-3322` (`highest_override_visible_for_file`). Every FormID clones `file.masters()` (N atomic increments), binary-searches, and for an override chain clones the masters again and searches each in reverse. Change: a per-worker `FxHashMap<(file ptr, u32), Option<Weak<MainRecordImpl>>>` filled lazily inside `collect_all` and dropped after the batch; borrow the master list under the `RwLock` read guard. The cache load path (`:815-828`) benefits identically. Category: algorithmic, allocation. Workload: refs, conflicts, clean injected. Gain: large. Parity: none; the tree is not edited during the build; memoise only the final result so the fallback order (`find_form_id` → injected → master chain) is unchanged.

<a id="core-write-2"></a>**core-write 2. Replace the thread-local `BTreeSet<u32>` collector with a reused `Vec<u32>` + `sort_unstable` + `dedup`.**
`refs.rs:56-57, 115-124, 290-320`. A node allocation per distinct FormID per record plus a `RefCell` borrow per insert. Change: push, then sort and dedup at the end of `collect_references`; `mem::take` one `Vec` per thread so the allocation is reused. Category: allocation. Workload: refs, save (`UpdateRefs`), conflicts. Gain: medium. Parity: none; same sorted unique list.

<a id="core-write-3"></a>**core-write 3. Global mutexes on build and pin.** See [core-load 6](#core-load-6).

<a id="core-write-4"></a>**core-write 4. `char_at` in the regex VM validates the entire remaining subject on every step (quadratic matching).**
`pcre.rs:2287-2296`: `std::str::from_utf8(&self.subject[pos..self.end]).ok()?` then `chars().next()`, so each consumed character scans to `end`; `exec` additionally restarts the VM at every start position. Change: decode one code point with the existing `decode(bytes)` (`pcre.rs:2570`) and no validation (the subject came from a `&str`); same for `is_continuation`/`back_char`. Category: algorithmic. Workload: scripts (`Split`, `ReplaceAll`, filters with regex). Gain: large on any subject over a few KB. Parity: none.

<a id="core-write-5"></a>**core-write 5. Regex: allocate the VM once per search and add a first-byte prefilter.**
`pcre.rs:2174-2203` (`exec`), `:2243-2255` (`Vm::new` allocates `saves`), `:2322` (`run` allocates `stack` per call, including nested `Assert` runs), `perl_regex.rs:762-765` (`char_index` counts chars from 0 on every match), `:311-315, 339-343`. Change: one `Vm` per `exec` with `saves.fill(UNSET)` and a reused `stack`; a compile-time set of possible first bytes or a literal prefix, skipped with `memchr`/`memchr2`/`memchr3` (the crate has SSE2/AVX2 dispatch and a scalar fallback); an incremental `(byte, char)` cursor in `perl_regex.rs`. Category: allocation, SIMD. Workload: scripts, filter. Gain: medium. Parity: the prefilter must only skip positions that cannot match (empty-matchable patterns, lookbehinds, `\G`, `not_empty` disable it); equivalence test over the probe corpus with the prefilter forced off.

<a id="core-write-6"></a>**core-write 6. Localization lookup builds a path string and clones the text per read.**
`localization.rs:681-705` (`get_value`), `:630-645` (`localization_file_name_by_type/by_element`), `:302-307` (`find` clones a `String`), `:179` (`HashMap<u32, usize>` with SipHash), `:364-386` (eager decode of every string). Each read runs the decider, `format!("Strings\\{}_{}{}", ...)`, `path_file_name`, a binary search by UTF-16 fold compare, then a clone. Change: cache `[Option<usize>; 3]` table indices per plugin `FileImpl` keyed by the existing `generation()`; `FxHashMap<u32, usize>` or a sorted `Vec<(u32, u32)>` with binary search; keep raw byte ranges and decode lazily on `find` (with the io 3 fast path); return `Arc<str>` or `&str` under the read guard where callers only format. Category: allocation, algorithmic. Workload: dump, conflicts, save (game master cache names), GUI. Gain: medium for SSE, FO4 and SF. Parity: `find` keeps first-entry-wins (`IndexOfObject`); invalidate on `generation()` change.

<a id="core-write-7"></a>**core-write 7. Save: the INCC interior-cell count initialises every CELL record serially.**
`write.rs:1058-1078`: `records().iter().filter(sig == CELL).filter(|r| { r.get_element_native_value("DATA")...; trim_initialized_records(64, None); interior })` builds the full element tree of every CELL on the save thread to read one flag bit. Change: for an unmodified CELL with valid storage read the `DATA` sub-record's first byte from the raw bytes (walk the sub-record headers; fall back to the element read for compressed or modified records); or count on the pool (`par_iter().filter().count()` is order-independent). Category: algorithmic, concurrency. Workload: save, clean. Gain: medium on large masters and big patches. Parity: the raw read must equal `flags & 1` of the definition; keep the element path as fallback and a debug assertion comparing both on the corpus.

<a id="core-write-8"></a>**core-write 8. Save: `elements()` clones the whole child `Vec<Arc>` at every container, several times per group.**
`write.rs:276-290` (`container_write_to_stream`), `:485-491, 498` (group fast path clones up to three times), `:315-324` (`container_counted_record_count` over the whole file for HEDR), `mod.rs:774-776`. Change: `ContainerBase::with_elements(|&[ElementRef]|)` under the read guard, or iterate by index with `element_at` as `build_ref` does (`refs.rs:165-171` has the same helper). Category: allocation. Workload: save, refs, reachable, check. Gain: small-medium; the per-element constant of the whole save walk. Parity: none.

<a id="core-write-9"></a>**core-write 9. Reference cache file: per-string `Vec<u16>` and per-record clones on encode and decode.**
`refcache.rs:183-189` (`write_string` collects `Vec<u16>`), `:256-266` (`Reader::string` collects then `from_utf16_lossy`), `:396-412` (`save` clones `references` and `data`), `refs.rs:894` (`set_references_from_cache(references.clone())`). Change: write `encode_utf16()` directly after reserving the length; decode with `char::decode_utf16` into a pre-sized `String`; have `save` borrow; pre-size `out` from the record count. Category: allocation, io. Workload: refs (first run and every cached run). Gain: small-medium. Parity: byte-identical stream (the round-trip tests at `refcache.rs:470-536` pin the layout).

<a id="core-write-10"></a>**core-write 10. `crc32_app` reads and hashes the whole executable serially before the first build.**
`refcache.rs:65-73`: `current_exe().and_then(fs::read).map(crc32)` in a `OnceLock` evaluated on the first `cache_file_name` call. Change: start it on a spawned thread at game setup so it overlaps the load, or hash through a memory map. Workload: startup, refs. Gain: small (tens of ms, once). Parity: none.

<a id="core-write-11"></a>**core-write 11. `references_injected` re-resolves every reference of every record.**
`injected.rs:85-109, 29-46`; caller `crates/xedit-session/src/clean.rs:800-808`. Per record: `file.masters()` clone, `self.references()` clone, `injected_source` → `record_by_form_id` per FormID, the same resolution the index build just did and dropped (`Collected.targets`). Change: run `records_cleanup_injected`'s filter on the pool preserving order and reuse the per-file memo of core-write 1. Workload: clean. Gain: small-medium. Parity: per-record results are independent; collect in file order.

<a id="core-write-12"></a>**core-write 12. `rebuild_onam` grows the ONAM array one element at a time through the edit machinery.**
`write.rs:1171-1300` (`assign_add()` at `:1242`, `set_native_value` at `:1256`, `is_winning_override()` per record at `:1215`); `sub_record.rs:2353-2385` (`array_assign_add`: `do_init`, `check_count`, `check_terminator` each time). Change: collect the FormIDs first (same filter, same order), add inside one `begin_update`/`end_update` with a single `check_count`, or build the plain `u32` array from a prepared source element; keep `master_update_fix_persistence` side effects in loop order. Workload: save (ESM-flagged files, `always_save_onam` games). Gain: small-medium. Parity: ONAM order and the `Setting Persistent:` message order; keep the element-by-element path as the oracle in a parity test.

<a id="core-write-13"></a>**core-write 13. Load-order startup: directory scan reads each header twice and sorts with uppercase copies.**
`crates/xedit-loadorder/src/load_order.rs:580-590, 196-220, 120-131`. `sort_by_key(|n| n.to_ascii_uppercase())` computes a `String` per comparison; `read_module_header` does two `read_exact`s per plugin; mod-group CRC checks `fs::read` a whole unloaded plugin. Change: `sort_by_cached_key`; one read of `header_size + data_size` after `metadata`; a memory map or chunked `Crc::update` for the unloaded-module CRC. Workload: startup. Gain: small. Parity: none.

<a id="core-write-14"></a>**core-write 14. Reachable build: path-string lookups per record in the special cases.**
`reachable.rs:108-115` (`get_element_by_path("DATA")` + `get_edit_value()` for every reached CELL), `:142-147` (`get_sort_key` string per record), `:161-166` (`children()` clones). Change: native value and bit test; iterate with `element_at`. Keep the edit value where the definition's `ToStr` matters (Oblivion CLAS/EYES). Workload: clean (filters with reachable info). Gain: small. Parity: check native-vs-edit equivalence for the CELL `DATA` flag byte per game.

### xedit-analysis and the analysis commands

How the conflict scan works today: `conflict_statuses` clones every file's record vector into one 23M-entry `Vec<Arc>` (`conflict.rs:1278`); for each record it takes `master_or_self` (RwLock + Weak upgrade) and `master.overrides()` (RwLock + a new Vec of upgraded Weaks) just to test emptiness (`:1298-1320`); groups run in 1024-chunks with `par_iter().with_max_len(1)` through `classify_group`, which builds a fresh `ConflictContext`, pins the chain, runs `conflict_level_for_main_record`, resets and trims (`:1324-1370`). Per record, `node_datas_for_main_record` makes one `ViewNodeData`/`ViewContainer` per chain member; `init_children` decides rows (member count, merged sort keys via `BTreeSet<Key>` + per-container `HashMap<String, usize>`, or `TDiff`), and the leaf row goes to `conflict_level_for_node_datas`, which compares cells by `display_sort_key(true)` strings with a `HashSet<String>` of unique values and two type sets per row. No byte-level shortcut is taken. Results are keyed by element identity and the serial groups run after the parallel ones, so output does not depend on scheduling.

<a id="analysis-1"></a>**analysis 1. Filter and clean run the conflict scan one record at a time on the calling thread for FO3, FNV, TES4, TES5 and SSE.** (Top-ten #2.)
`filter.rs:1559-1562, 1675-1676`; `crates/xedit-session/src/save.rs:84-102` (`set_can_sort_info(true)` for those games), `:131`. `statuses_as_walked = wants_conflict && can_sort_info() && sort_info()`; pass 1 then calls `conflict_statuses_of(slice::from_ref(&record), ..)` per record, each a `pool.install` of a one-element `par_iter` with a HashMap allocation and a trim, and no parallelism. `FilterOptions::for_cleaning()` and `for_conflicts()` take this path by default. The comment says the ask order matters for INFO responses (PNAM/INOM/INOA fills sort a topic). Change: precompute in batch (the existing `!statuses_as_walked` path) for every chain that contains no INFO or DIAL record, or whose top group is not DIAL; keep the per-record path in walk order for those chains only. Alternatively pre-walk the tree collecting records in walk order and classify in parallel windows that contain no DIAL/INFO record. Category: concurrency. Workload: clean, filter, conflicts via `filter.apply`. Gain: large. Parity: topic-sort side effects must occur at the same point relative to the walk; guard with the FNV/FO3 filter and clean fixtures (`filter-topic-bind.pas`, BrokenSteel); keep the exact per-record path when `fill_pnam()` is on.

<a id="analysis-2"></a>**analysis 2. No cheap identical-to-master shortcut before the element walk.** (Top-ten #3.)
`conflict.rs:386-445` (`conflict_level_for_main_record`), `:1561-1602`, `:1613-1632`. A two-record chain goes to `conflict_level_for_child_node_datas` unless `is_compare_to_same`, which is false without a compare-to load. Change: when all chain members are unmodified, have equal signature, flags, FormID, data size and form version, the override file's master mapping for the record is the identity (its masters start with the master file's masters followed by the master file, so file-local FormID bytes mean the same record), neither record is a partial form, and no mod group, `translation_mode` or injection is involved: compare payloads with an AVX2 (`_mm256_cmpeq_epi8` + `movemask`) / SSE2 / scalar kernel under `xedit_io::simd::Level` and emit `caNoConflict` with `ctMaster`/`ctIdenticalToMaster` without building elements; any mismatch falls through. Category: SIMD, algorithmic. Workload: conflicts, clean (ITM detection), filter. Gain: large. Parity: equal bytes do not always give equal classification (a `cpIgnore`-only difference gives `ctIgnored`, not `ctIdenticalToMaster`; header version, master index, `IS_PARTIAL_FORM`); ship behind an env toggle with a dual-run verification mode that classifies both ways over the corpus and diffs, and keep the conditions narrow until the diff is empty.

<a id="analysis-3"></a>**analysis 3. Per-record `overrides()` Vec allocation and lock across all 23M records.**
`conflict.rs:1278, 1307-1308, 393, 657, 446-460`; `mod.rs:3259-3265`. Change: `has_overrides()`/`override_count()` reading under the lock without collecting; collect the chain once in `node_datas_for_main_record` and reuse it for the `fix` loop; iterate files' records under a borrowed read guard instead of cloning 23M Arcs; replace the `seen` `HashSet<usize>` by marking the master or `FxHashSet`. Category: allocation. Workload: conflicts, filter, clean. Gain: medium; this prologue is serial and on the critical path before any parallel work. Parity: none.

<a id="analysis-4"></a>**analysis 4. Sort keys computed up to three times per element, and per-row allocations.**
`crates/xedit-core/src/implementation/sortable.rs:156-180` (`view_elements`), `conflict.rs:118-120, 843-849, 909, 1836-1850, 720, 1784, 1832-1833`. `view_elements` computes `get_sort_key(true)` per entry to sort; `init_children` recomputes `display_sort_key(element, false)` and formats `"{sort_key}<{dup:04X}>"` into a `HashMap<String, usize>` per container plus a `BTreeSet<Key>`; `conflict_level_for_node_datas` computes `display_sort_key(element, true)` again per cell; each row allocates `children`, `unique_values: HashSet<String>`, `element_types`, `def_types`. Change: memoise both keys per element within one `ConflictContext` (or a `OnceCell` on `ViewContainer`), sort + dedup borrowed `&str` instead of the string set, bitmasks for the two type sets, a scratch `Vec<ViewNodeData>` reused across rows. Category: allocation, algorithmic. Workload: conflicts, clean, filter. Gain: medium-large; sort key construction is the inner loop of the scan. Parity: none if cached by element identity inside one group's classification, which pins its records.

<a id="analysis-5"></a>**analysis 5. Check for Errors flushes its batch at every topic's response group.**
`check.rs:286-297, 200-212`: `if group.group_type() == 7 { self.flush(); }` so in dialogue-heavy plugins a batch is one DIAL plus a few INFOs and the 512 batching never fills. Change: build and sort the topic inline (as `show_in_navigation` does in `crates/xedit-session/src/conflicts.rs:490-500`) and keep batching, or flush only the pending DIAL. Category: concurrency. Workload: check. Gain: medium on FO3/FNV/TES5 masters. Parity: the sort happens on the walking thread before enumeration; the DIAL's captured build messages still land before its INFOs' output; verify with the DIAL-heavy check fixtures and `fill_pnam` on and off.

<a id="analysis-6"></a>**analysis 6. Check for Errors builds every record twice even when the first build logged nothing.**
`check.rs:136-163`: `get_check()` builds, `record.reset()`, then the walk rebuilds, to replay upstream's duplicated "Errors were found in" messages. Change: skip the reset when the first `capture_progress` returned no messages. Category: algorithmic. Workload: check. Gain: medium; halves builds in the common case. Parity: init fixes are idempotent and already applied by the first build; confirm with the check corpus and keep an env switch to force the double build.

<a id="analysis-7"></a>**analysis 7. Batch barrier every 1024 groups and the global mutexes on every record build.**
`conflict.rs:1324-1370, 1384`; `mod.rs:2060-2083, 2090-2124, 3462-3468`. `pool.install` + collect per 1024-group chunk serialises at each chunk tail. Change: raise `GROUPS_PER_BATCH` adaptively (the `init_cycles` re-run is rare) or `collect_into_vec` over all groups with per-chunk cycle counters; per-thread `INITIALIZED_RECORDS` and pins as in [core-load 6](#core-load-6). Category: concurrency. Workload: conflicts, check, clean. Gain: medium at 16+ threads. Parity: none; keep the cycle re-run.

<a id="analysis-8"></a>**analysis 8. `ViewContainer::element_by_sort_order` builds a SipHash `HashMap<i32, usize>` per container.**
`conflict.rs:172-183, 1644-1652`. Sort orders are small non-negative ints bounded by the member count. Change: `Vec<Option<u32>>` indexed by sort order with an `FxHashMap` fallback for out-of-range orders; same first-index-wins rule. Category: allocation. Workload: conflicts, clean, filter. Gain: small-medium. Parity: none.

<a id="analysis-9"></a>**analysis 9. NavTree build enumerates each container's children two or three times.**
`filter.rs:147-155, 162, 237, 261, 268-270`. `NavTree::new` calls `elements_of(&element)` to test emptiness, `init_children` again, and per child `elements_of(..).is_empty()`, each collecting full child `Vec<ElementRef>`s. For a 1M-record group that is three Arc-clone passes, and the tree precedes every clean pass. Change: emptiness via `get_element_count() > 0`; pass the collected `elements` down. Category: allocation. Workload: clean, filter. Gain: medium on full load orders. Parity: none; same node order.

<a id="analysis-10"></a>**analysis 10. Quick auto clean recomputes the whole tree and conflict scan for passes 2 and 3.**
`crates/xedit-session/src/clean.rs:524-528`; `cleaning.rs:456-476`. Change: keep the `ConflictResults` of pass 1 and reclassify only the FormID chains whose records were removed, made partial or undeleted (plus the serial GMST/DFOB/NAVI/TES4 set), then rebuild the tree. Category: algorithmic. Workload: clean. Gain: medium-large when a second pass runs (any plugin with ITMs or UDRs). Parity: holds only because a record's status depends solely on its chain for non-GMST/DFOB/NAVI/TES4 records; validate against the three-pass fixtures; keep full recompute behind a flag for the harness.

<a id="analysis-11"></a>**analysis 11. Regex compiled and filter text upper-cased per record.**
`filter.rs:1339-1346, 1357, 1376-1384`. `text_matches` upper-cases both sides per record; `check_value_regex` builds `PerlRegEx::new()`, `set_regex`, `set_options` per record. Change: pre-uppercase the filter texts in `FilterChecks::new`; compile each regex once per filter into a `RefCell<PerlRegEx>` and only `set_subject` per record; `memchr::memmem::Finder` for the plain match. Category: allocation. Workload: filter. Gain: medium when editor-ID, name or value filters are used. Parity: none; same engine and options.

<a id="analysis-12"></a>**analysis 12. `conflicts.list` allocates two `String`s per record for counters.**
`crates/xedit-session/src/conflicts.rs:264, 281-292`: `file.records()` (another full clone of the record vector) and `*summary.by_conflict_all.entry(status.all.enum_name().to_owned()).or_default() += 1` plus the same for `conflict_this`: ~46M String allocations and BTreeMap lookups in a serial tail. Change: count into `[usize; N]` arrays by discriminant and convert once per file; iterate under the read lock. Category: allocation. Workload: conflicts. Gain: small-medium. Parity: identical BTreeMap output.

<a id="analysis-13"></a>**analysis 13. Mod group filter upper-cases the file name per record per chain.**
`crates/xedit-session/src/modgroups.rs:241-256, 293`; `crates/xedit-loadorder/src/mod_groups.rs:801-813`. Change: key `modules` by file pointer or load order; precompute the per-file module index once in `activate`. Gain: small. Parity: none.

<a id="analysis-14"></a>**analysis 14. Merged patch rebuilds the same master lists per override.**
`merged_patch.rs:422-447, 234-266`. For every list name × every override, `master_records_from_master_files_and_self(record)` and `build_list(current_master...)` rebuild the current master's list; `StringList::add_object` on a sorted list is `Vec::insert` (O(n²) for large FLST/LVLI). Change: memoise `build_list` per (record id, list name); compute `current_masters` once per override; build sorted lists by push + the ported quick sort + dedup only after an equivalence test, since the unstable `TStringList.QuickSort` order decides duplicate counters. Workload: merged (not a gate). Gain: small-medium. Parity: keep the ported sort for the final order.

### xedit-session, xedit-cli, xtask

Lifecycle as implemented: `main` spawns a 1 GiB-stack thread (`crates/xedit-cli/src/main.rs:4057-4068`); `run()` builds `Registry::standard()` (115 commands, `schema_for!` for request and response of each, `crates/xedit-session/src/lib.rs:196-197`) before loading; `Session::load_with` (`commands.rs:134-290`) sets up one game's definitions, loads the plugins and masters, opens the resource containers (`load_session_resources`) and the hardcoded file; the handler runs and the response is printed with `println!` (`main.rs:4333-4346`). The parallel dump (`dump.rs:939-1229`) batches 512 records or 4 MiB per batch, at most four batches in flight, each delivered whole, written in file order through a 1 MiB `BufWriter`.

<a id="session-1"></a>**session 1. Add a release profile.** (Top-ten #1; section 8.)

<a id="session-2"></a>**session 2. Named-pipe serve writes one syscall per JSON fragment.**
`main.rs:3880-3883`, `rpc.rs:170-171`: `rpc::serve_lines(BufReader::new(&file), &file, ..)` passes an unbuffered `&File` as the writer; `writeln!(output, "{reply}")` drives `Value`'s `Display` through the fmt adapter, so every key, comma and string is its own `WriteFile`. Change: `BufWriter::with_capacity(1 << 20, &file)` and `serde_json::to_writer` + newline + flush. Category: io. Workload: serve/mcp. Gain: large for any response above a few KiB over the pipe. Parity: same bytes.

<a id="session-3"></a>**session 3. stdout JSON goes through the 1 KiB `LineWriter`.**
`main.rs:4333, 4345`; stdio serve and mcp use `stdout().lock()` unbuffered (`main.rs:3876, 3895`, `rpc.rs:170`). `println!("{}", json!({ "ok": true, "result": result }))` formats a possibly multi-MB line one KiB at a time. Change: a 1 MiB `BufWriter` around the locked stdout and `serde_json::to_writer`. Category: io. Workload: serve/mcp, every `--json` one-shot with a big result. Gain: medium. Parity: compact JSON is byte-identical to `Display`.

<a id="session-4"></a>**session 4. `records.find` builds a full summary of every record before filtering; signature compare allocates per record.**
`commands.rs:745-750, 780-800, 714-722`. `records_of` filters with `record.get_signature().to_string() == signature`; `records_find` maps every record through `summary_of` (five `String`s, `get_name()` may build the record) and only then filters and takes `limit`. Change: parse the signature once and compare by value; filter on editor ID and name first; summarise matches only; stop after `limit` or count matches without summaries. Category: allocation, algorithmic. Workload: serve/mcp, GUI search. Gain: medium on masters with hundreds of thousands of records. Parity: same order and `total`; keep case-insensitive semantics.

<a id="session-5"></a>**session 5. Registry builds 230 JSON Schemas on every process start.**
`lib.rs:181-203` (`schema_for!` at `196-197`), `main.rs:3799`. Only `schema`, `rpc.discover` and `tools/list` read them. Change: `fn() -> Schema` thunks or `OnceLock<Schema>` per command, resolved in `commands()`/`catalogue()`. Category: startup, simplification. Workload: startup, dev loop (every parity case spawns the port). Gain: small, measure first. Parity: none.

<a id="session-6"></a>**session 6. Parallel dump: fixed four batches in flight and whole-batch delivery.**
`dump.rs:1010-1016, 1137-1150, 1168-1180`. `BATCHES_IN_FLIGHT = 4` and `BATCH_RECORDS = 512` regardless of `threads()`; a batch arrives as one `Vec<Vec<u8>>` via `collect()`, so the writer cannot emit the head of a batch while a heavy tail record (NAVI, large CELL/WRLD) renders, and with 16-32 workers the available work shrinks to four batch tails. Change: scale in-flight batches with the pool (`max(4, threads / 2)`) bounded by bytes; deliver per record through `Arc<[OnceLock<Vec<u8>>]>` or an `mpsc` of `(index, buffer)` so `write_first` streams in index order; pre-size the per-record buffer from `data_size`. Category: concurrency. Workload: dump. Gain: medium on high core counts. Parity: order is still by batch and index; the `init_cycles` fallback stays.

<a id="session-7"></a>**session 7. Resource containers are opened for commands that never read a resource.**
`commands.rs:262-270`, `resources.rs:427-438, 305-311`. Every non-xDump load calls `load_session_resources` → `load_gui_resources(.., load_bsas())`, parsing the header and file table of every archive of the inis and of every module before `records.list`, `masters.*`, `formids.*` run. Change: record the container list in the same order and open each archive lazily on first `resource_exists`/`resource_open` through a `OnceLock` per container. Category: io, algorithmic. Workload: startup (FO4/SF with dozens of BA2s), first serve/mcp command. Gain: medium for FO4-class data folders. Parity: container order and precedence must be identical. Consider also opening the archives in parallel with a deterministic container order (`container_handler.rs:122-144` is serial under a `RwLock`).

<a id="session-8"></a>**session 8. MCP `tools/call` serialises each result twice and ships it twice.**
`mcp.rs:134-142`: `"text": result.to_string()` then `reply["structuredContent"] = result`; the reply is serialised again with the text copy JSON-escaped. Change: keep `structuredContent`; make the text a compact rendering under a size cap or a one-line summary. Category: allocation, io. Workload: serve/mcp. Gain: small-medium per big call. Parity: `structuredContent` unchanged.

<a id="session-9"></a>**session 9. Dump line rendering allocates per non-ASCII string and per size field.**
`dump.rs:1248-1255, 1455, 1145`. `write_text` calls `Encoding::Mbcs(1252).get_bytes(text)` (fresh `Vec<u8>`) for any line with a non-ASCII char; `get_data_size().to_string()` under `-size`; each record job starts from `Vec::new()`. Change: encode 1252 straight into `out` through a stack chunk; `itoa` for the size; a per-worker buffer pool. Category: allocation. Workload: dump. Gain: small. Parity: same bytes.

<a id="session-10"></a>**session 10. Harness job count is a fixed 3 even when the oracle is cached.**
`crates/xtask/src/parity.rs:1484, 687-690, 1745-1760`. The memory budget already reserves each process's expected peak from the previous `.peak` file. Change: default `jobs` to `available_parallelism() / 2` capped by the budget; keep `--jobs`. Category: concurrency. Workload: dev loop. Gain: medium for full-corpus reruns. Parity: none.

Not worth doing in the session layer: teardown (the tree is owned by the static `FILES_MAP`, so dropping `Session.files` only decrements Arcs and statics are not dropped at exit); building references on load (nothing in `wb_file` consults `set_build_refs`).

### xedit-script and the script host

Representation as implemented: `Value` (`crates/xedit-script/src/values.rs:719-749`) is a 19-variant enum of about 32 bytes with `Str(Vec<u8>)` and `Rc<RefCell<dyn ScriptObject>>` objects; `Clone` deep-copies strings. Variables live in `VarList { entries: Vec<VarEntry> }` (`adapter.rs:552-566`), one per frame plus the adapter's unit globals, each entry carrying three `String`s; lookup is a linear case-insensitive `iter().position` from the newest entry (`adapter.rs:607-613`) and `add_var` does `entries.insert(0, ..)`; no slot resolution at parse time. A call builds an `Args` (`adapter.rs:162-176`) in `build_call_args` (`eval.rs:1521-1539`), then `Interpreter::get_value` (`eval.rs:1092-1146`) tries frame locals, `Adapter::get_value` (`adapter.rs:1332-1416`: unit globals and units linearly, then binary `get_class`/`get_const`/`get_fun`), the host hook, and script routines by a linear reverse scan. About 600 host functions, 1127 VCL members and 782 constants are re-registered into a fresh `Interpreter` on every `script.run` (`crates/xedit-session/src/script.rs:2733-2770`).

<a id="script-1"></a>**script 1. Runtime re-lexes the source on every call, assignment and statement to recover `CurPos`/`PosBeg`.** (Top-ten #10.)
`eval.rs:1556-1558` (`internal_get_value`), `:1973` (`exec_assign`), `:1949-1958` + `:2583-2594` (`exec_block` → `hook_statement_separators`), `:2598-2617` (`token_after`). `following_token_end(span.end)` and `following_token_start(span.end)` each build a `Tokenizer`, lex one token and return `token.text: Vec<u8>`; `exec_block` calls `hook_statement_separators` for every statement whether or not `on_statement` is set. `SetEditValue(ElementByPath(e, 'EDID'), s);` performs five tokenizer runs. Change: record the following-token positions and the separator positions in the AST at parse time with the same tokenizer; make `token_after` return positions without materialising text. Category: algorithmic. Workload: script run. Gain: large. Parity: positions must equal what `token_after` computes today, so compute them once at parse with the same function.

<a id="script-2"></a>**script 2. Every variable read deep-copies the value; string variables copy on every read, write and concat.** (Top-ten #10.)
`adapter.rs:628-646` (`VarList::get_value` → `jv_var_copy`), `values.rs:1068-1070, 1078-1113` (`*dest = source.clone()`), `:1570-1578` (concat), `eval.rs:1412` (string literal cloned per evaluation), `values.rs:1354-1356` (`to_str` clones). `s := s + x` copies `s` three times; Delphi strings are refcounted copy-on-write. Change: `Value::Str` as `Rc<[u8]>`/`Rc<Vec<u8>>` with `Rc::make_mut`, temporaries moved into `set_value`, literals behind an `Rc` in the AST, a borrowing `as_str`. Category: allocation. Workload: script run. Gain: large for string-heavy scripts. Parity: value semantics (no aliasing visible) via `make_mut`; `set_element` (`eval.rs:1747`) must go through it.

<a id="script-3"></a>**script 3. Per-call host dispatch allocates `Args` buffers, the identifier string and a copy of the parameter-type list.**
`adapter.rs:1578` (`args.identifier = String::from_utf8_lossy(..).into_owned()`), `:1267` (`args.types = param_types.to_vec()`), `eval.rs:1521-1539`, `adapter.rs:208-213`. Change: `Rc<[ParamType]>` shared into `Args`, identifier as `Rc<str>` from the registration, `var_names` as `Option<Rc<str>>` or AST indices, a pooled `Args` per interpreter, `SmallVec<[Value; 4]>` for `values`. Category: allocation. Workload: script run. Gain: medium (4-6 allocations per host call). Parity: none.

<a id="script-4"></a>**script 4. Frame variables are pushed with `Vec::insert(0, ..)` and three `String` allocations each, then found by linear scan.**
`adapter.rs:577-605` (`add_var`), `:607-613` (`find_var`), `eval.rs:952-1000` (`enter_function`), `:1035-1063` (`add_frame_vars`). Change: resolve locals to slot indices at body-parse time (`RoutineBody` knows params and locals; `Ident` nodes get `Option<slot>`), keep `VarList` for unit globals and the by-name fallback; at minimum push to the back with `rev()` scan and `Rc<str>` identifiers. Category: algorithmic. Workload: script run. Gain: large for scripts with helper functions per record (`Process` itself is one call per record). Parity: redeclaration error order, newest-wins shadowing, the `Result`-named parameter case, case-insensitive match.

<a id="script-5"></a>**script 5. Script-function calls clone `cur_unit` and `param_types` per invocation; `Result`/`var` read-back clones again.**
`eval.rs:1124, 1136, 1002-1032, 871-889`. Change: `cur_unit` as `Rc<str>` compared by pointer first; borrow `param_types`; pop the frame first and move `Result`/`var` values out of it. Workload: script run. Gain: medium. Parity: none.

<a id="script-6"></a>**script 6. Adapter registration re-sorts every list per VCL member and rescans all registrations per RTL member (quadratic startup per `script.run`).**
`crates/xedit-session/src/script_vcl/mod.rs:198-200, 95-97`, `adapter.rs:1221-1232` (`find_class` → `ensure_sorted`), `:803` (`add_get` invalidates the sort), `script_rtl/classes.rs:531-540` (`has_member` → `adapter.registrations()`), `adapter.rs:981-1060` (builds ~3000 owned `RegistrationKey`s per call, 58 callers). 1127 iterations each sort seven lists of up to ~2500 entries and then invalidate the flag; all of it runs twice when the fresh-program factory (`script.rs:2760`) is used, once per `script.run`/`script.resume`. Change: a `HashMap<lowercase name, Rc<ClassDef>>` beside `class_list`, or pre-resolve the VCL class `Rc`s before the member loop; a direct non-allocating `has_get(class, name)`; longer term build the registration set once per session and clone the adapter per run. Category: algorithmic. Workload: script run startup, vcl, the 156-case parity run. Gain: medium. Parity: the final stable sort order is identical when sorted once at the end.

<a id="script-7"></a>**script 7. Each returned element allocates a wrapper and walks the container chain to find its main record.**
`script.rs:1417-1422, 1008-1021, 1030-1043, 1051-1073`. Change: cache the owning main record on the core element or answer `main_record()` in O(1) from `ElementImpl`; drop the `RefCell` for the immutable wrapper. Category: allocation. Workload: script run. Gain: medium. Parity: the reset semantics in `Drop` (`cntElementRefs`, `stamp_reset`) are observable in later `GetEditValue` output and stay unchanged.

<a id="script-8"></a>**script 8. Host string arguments are converted twice.**
`script_units/mod.rs:152-158` (`string_at`), `script_rtl/mod.rs:621-630` (`arg_str`), callers `container.rs:244` (`ElementByPath`), `element.rs:557` (`SetEditValue`). Change: `str_at(args, i) -> Cow<'_, str>` borrowing the bytes. Workload: script run. Gain: medium on the two most called host functions. Parity: none.

<a id="script-9"></a>**script 9. `TStringList` `Find`/`IndexOf`/`CompareStrings` build a `String` per compared line.**
`script_strings.rs:152-176, 266-271, 721-723, 736-738, 780-782`. Change: compare bytes directly with an `ansi_compare_text` over `&[u8]` producing the same `Ordering` as the current `&str` version. Workload: script run. Gain: medium for list-heavy scripts. Parity: verify on the sorted-list probes.

<a id="script-10"></a>**script 10. `for` loops write the control variable through the full by-name `set_value` chain and box an iterator per loop.**
`eval.rs:2198-2235`. Change: resolve the control variable once before the loop; two plain loops instead of `Box<dyn Iterator>`. Category: dispatch. Gain: small-medium. Parity: the first-iteration `ieUnknownIdentifier` and typed coercion must still apply; a unit-level loop variable still goes through the adapter.

<a id="script-11"></a>**script 11. `script.check` is sequential and re-parses shared units once per script.**
`script.rs:223-242`, `crates/xedit-script/src/check.rs:59-120`. 150 scripts that `uses mteFunctions` parse that unit 150 times, single-threaded. Change: `check_file` per script on the rayon pool with results in request order; a per-call `Mutex<HashMap<path, Result<Module, Error>>>` cache. Category: concurrency. Workload: script check (the 150-file parity gate). Gain: medium. Parity: output in request order; the cycle guard stays per script.

<a id="script-12"></a>**script 12. `StrUtils` helpers index by `char` with `Vec<char>` collects.**
`script_misc.rs:291-350` (`left_str`, `right_str`, `mid_str`, `stuff_string`), `:492-520` (`string_replace` lower-cases the whole text and pattern per call). Change: operate on `&[u8]` like `copy_str`; byte-wise ASCII case-folded search. Gain: small. Parity: confirm with a non-ASCII probe before switching (Delphi indexes UTF-16 units; the port indexes chars today).

<a id="script-13"></a>**script 13. Unit-level variable lookup is a linear scan over every script global on every frame miss.**
`adapter.rs:1343, 1346, 607-613`. Every host call first scans all script globals and source units. Change: a case-folded `HashMap<Box<[u8]>, usize>` maintained in `add_var`/`delete_var`. Category: dispatch. Gain: small-medium. Parity: last-added-wins across units and the empty-unit wildcard match reproduced by the index.

<a id="script-14"></a>**script 14. Statement hook and separator lexing run even when nothing observes them.**
`eval.rs:1952-1955, 2572-2577`, `script.rs:2772-2780` (the hook only reads `globals::force_terminate()`). Change: skip `hook_statement_separators` when `on_statement` is `None`; poll `force_terminate` with an atomic read per statement. Category: simplification. Gain: small after script 1. Parity: `IE_USER_BREAK` still raised before the next statement.

<a id="script-15"></a>**script 15. `Adapter::get_value` falls back to `String::from_utf8_lossy` for the OnGet chain and the host hook.**
`adapter.rs:1402-1410`, `eval.rs:1118-1121`. Change: pass the `Cow` without `into_owned`; iterate an `Rc<[OnGetHandler]>` snapshot. Gain: small. Parity: none.

### xedit-assets

NIF model as read: `Tree` owns `nodes: Vec<Node>`; a `Node` (`data_format.rs:673-684`) is roughly 96 bytes plus one heap `Vec<u8>` per value. Load is fully eager and recursive (`data_format_nif.rs:399-486`, `data_format.rs:2187-2286`); every leaf copies its bytes, disabled leaves still allocate and run `set_edit_value(default)`, every union member is created. A BSTriShape vertex (`data_format_nif/defs.rs:333-398`) materialises about 27 nodes and 20 allocations, so a 100k-vertex FO4 mesh is roughly 2.7M nodes and 250 MB resident. Save re-serialises every node (`data_format.rs:2315-2465`); an unchanged file is rebuilt leaf by leaf.

<a id="assets-1"></a>**assets 1. Per-leaf heap allocation and eager materialisation of vertex arrays.** (Top-ten #9.)
`data_format.rs:673-684, 782-803, 2094-2128, 2207-2264, 2266-2286`. Change: inline small-value storage in `Node` (`[u8; 16]` + len, spilling to `Vec` above 16 bytes; all NIF scalars, half/float vectors and colours fit); for arrays whose member definition is fixed-size and event-free (vertices, normals, UVs, triangles, bone weights and indices), keep the bytes as one contiguous span and create child nodes lazily on first `item()`/`element_by_name`, with `serialize`/`data_size` emitting the span directly when nothing was materialised. Category: allocation. Workload: nif, sniff, lodgen. Gain: large. Parity: bit-exact; guard the lazy path to member defs with no `on_after_load`/`on_get_enabled`/`on_decide` and a fixed `default_data_size`.

<a id="assets-2"></a>**assets 2. Geometry accessors read every component through path strings, `Variant` and `FloatToStr`/`StrToFloat`.**
`data_format_nif/geometry.rs:28-67, 116-123, 569-595, 785-795`; `data_format.rs:1301-1361`. Per component: `format!("[{index}]")`, a merge-member lookup, the Delphi 15-digit formatter and a parse. Change: typed bulk readers (`Tree::value_f32x3(el)` from the bytes, halves through `xedit_core::half_float::half_to_float_slice`, the existing F16C kernel) and mirrored writers. Category: allocation, SIMD. Workload: nif, sniff (optimize, tangents, bounds, transforms), lodgen (UV range scan). Gain: large for the geometry operations. Parity: bit-exact for `as_text = false`; for `as_text = true` 15 significant digits round-trip any f32 and any half exactly, confirmed by an exhaustive test over all 65536 halves and the special f32 patterns before replacing it.

<a id="assets-3"></a>**assets 3. LODGen atlas mipmaps: each level Lanczos-resampled from the full atlas, serially, with per-pixel format dispatch.**
`lod/atlas.rs:396-400`, `imaging/mod.rs:475-510`, `imaging/formats.rs:2005-2089, 2091-2172`. Cost per level is ~6·N² pixel fetches regardless of level, so a 4096² atlas pays ~70 full-image passes, times three atlases. Change: parallelise `stretch_resample` over destination rows (each row has its own buffer and disjoint output), hoist the format dispatch by converting each needed source row to `ColorFP` once, and vectorise the four-channel accumulate as one AVX `_mm256_mul_pd` + `_mm256_add_pd` + `_mm256_cvtpd_ps` per contributor, no FMA. Category: concurrency, SIMD. Workload: lodgen, dds. Gain: large; the heaviest xEdit-side CPU phase of an FO4/SSE LODGen run. Parity: bit-exact: rows independent, per-channel operation order unchanged, mul-then-add in f64 then round to f32 is the same IEEE sequence lane-wise; do not reorder contributors.

<a id="assets-4"></a>**assets 4. LODGen texture and mesh preparation is fully serial.**
`lod/atlas.rs:201-361` (one `NifFile::load_from_data` per LOD mesh), `:586-700` (load, Lanczos resize, normal and specular load per texture), `:447-565` (`write_atlas` ×3); `crates/xedit-session/src/lodgen.rs:589-599`. Change: map each mesh and texture to a result struct (textures found, image triple, warning lines) on the pool, then fold in input order on the calling thread with the `message()` lines replayed in order. Worldspaces cannot run in parallel as written (shared `LODGen.txt`/`LODGenAtlasMap.txt` in `scripts_path`, `lod/generate.rs:1217, 1791`). Category: concurrency. Workload: lodgen. Gain: medium-large. Parity: bit-exact when folded in input order; `resource_exists`/`open_resource` are already used from the parallel dump.

<a id="assets-5"></a>**assets 5. DXT1/3/5, ATI1/2 encode and decode are scalar per block.**
`imaging/formats.rs:1471-1488, 1496-1511` (120 pair distances per block), `:1539-1575, 1613-1760, 1256-1470`. Change: encode blocks in parallel by rows of blocks; an AVX2 kernel for `get_endpoints` (16 pixels as i16 lanes, the 16×16 distance matrix with `_mm256_madd_epi16`, then the first `(i, j)` in lexicographic order with the maximum, found by scanning the stored matrix in the original order) and `get_color_mask`; SSE2 and scalar remain. Category: SIMD, concurrency. Workload: lodgen, dds, archive pack (texture conversion). Gain: medium. Parity: bit-exact integer math; the tie-break rules (`>` keeps the earliest pair, `<` the earliest colour) must be reproduced by scanning in order rather than a horizontal max/min.

<a id="assets-6"></a>**assets 6. `block_referenced_by` is O(blocks × refs) and is called inside per-block loops.**
`data_format_nif.rs:1073-1092`; callers `sniff/proc_check_for_errors.rs:1176, 2137`, `sniff/proc_universal_fixer.rs:1023`, `sniff/proc_merge_properties.rs:114`, `data_format_nif/geometry.rs:828, 852`. A 5k-block NIF is ~25M `links_to` resolutions. Change: a reverse index `Vec<Vec<El>>` built once per pass in block order (same per-block result order), cached with a structural generation counter bumped by `put`, `container_delete`, `container_move`, `remap`, `nif_delete/nif_move` and `def_set_native` for `DefKind::NiRef`. Category: algorithmic. Workload: sniff. Gain: medium-large on big meshes. Parity: bit-exact with the same iteration order and invalidation on every reference write.

<a id="assets-7"></a>**assets 7. `name()`/`index()` allocate and scan linearly; array children cost O(n) per name.**
`data_format.rs:1011-1026, 1071-1082, 2754-2758, 2832-2840, 928-931`, `data_format_nif.rs:539-556`. Change: `Cow<'static, str>` from `name()`; pass the index down in container iterations; take `names()` once in `nif_unserialize_from_json`; optionally `index_in_parent: u32` on `Node`. Category: allocation, algorithmic. Workload: nif (JSON import/export, text dump). Gain: medium for `assets.from_json`/`to_text`. Parity: identical strings.

<a id="assets-8"></a>**assets 8. Find unwelded vertices and the duplicate-vertex check are O(n²) over vertices.**
`sniff/proc_unwelded_vertices.rs:45-74`; `sniff/proc_check_for_errors.rs:2349-2360`. Change: duplicate check by hashing the `v_size`-byte slices into a first-seen map (identical counting). Unwelded: a uniform grid with cell size `(1 / inv_distance) × 1.1` (margin for the fast-inverse-sqrt error), candidate pairs from the 27 neighbouring cells tested with the same predicate, `(i, j)` pairs sorted lexicographically and emitted in that order; NaN vertices in one extra bucket. Category: algorithmic. Workload: sniff. Gain: large on dense meshes. Parity: the duplicate check is bit-exact; unwelded is bit-exact only if the grid margin provably covers the `fast_inv_sqrt` error (one Newton step, below 0.2%) and `skip_same` is preserved; verify against the Sniff corpus before switching the default.

<a id="assets-9"></a>**assets 9. `prepare_image_alpha`, `swap_channels`, `convert_pixels` and `point_transform` go through per-pixel format dispatch.**
`lod/atlas.rs:150-168` (column-major walk of a row-major buffer), `imaging/canvases.rs:16-28`, `imaging/formats.rs:1095-1225`, `imaging/mod.rs:125-150`. Change: specialise A8R8G8B8 to a byte loop over `bits[3::4]` with AVX2 `_mm256_cmpgt_epi8` on biased bytes + `blendv`; `rgb_to_bgrx`-style shuffles for the 4→3/3→4 conversions and channel swaps under the existing `simd::Level`; row-major iteration. Category: SIMD, simplification. Workload: lodgen, dds. Gain: small-medium. Parity: bit-exact (byte moves and compares).

<a id="assets-10"></a>**assets 10. Sniff commit thread serialises output file writes; archive unpacking returns owned copies.**
`sniff/main_form.rs:566-589`, `sniff/processor.rs:858-867` (`fs::write` on the commit thread), `:465-473` + `crates/xedit-io/src/archive/mod.rs:992-996`. Change: workers write their output (to the final path or a temp name) and hand only status and message lines to the commit thread; pass uncompressed archive entries as slices of the map ([io 7](#io-7)). Category: io, concurrency. Workload: sniff. Gain: small-medium for write-heavy operations. Parity: messages and report order unchanged; keep `dry_run` and `sink` on the commit path.

<a id="assets-11"></a>**assets 11. Unchanged files are re-serialised leaf by leaf on save.**
`data_format.rs:2315-2373, 2375-2442`, `data_format_nif.rs:507-563`. Change: with the span storage of assets 1, `serialize` of an unmaterialised array is one `extend_from_slice`; a per-block `dirty` bit so `nif_data_size` skips clean blocks and `serialize` copies the block's original byte range; the header stays on the slow path (`update_header` always rebuilds it). Category: io, allocation. Workload: nif, sniff. Gain: medium after assets 1. Parity: bit-exact only if clean blocks serialise to their input bytes; validate per block in debug builds over the NIF corpus.

<a id="assets-12"></a>**assets 12. `Vector3`/`Transform` math is scalar f64; min/max and transforms are 4-wide candidates.**
`nif_math.rs:135-262, 306-333, 900-915, 960-977, 1030-1087`. Change: only after assets 1 and 2, `[f64; 4]` storage with `_mm256_min_pd`/`_mm256_max_pd` (current value as the second operand so a NaN or -0.0 in `v` keeps the old value exactly as the `<`/`>` tests do) and per-component `mul_pd`/`add_pd` in the scalar order. Category: SIMD. Gain: small. Parity: min/max and element-wise ops are bit-exact lane-wise; `length()`/`normalize()` and the tangent sums must keep the scalar evaluation order (no horizontal adds, no FMA); `arc_tan2`/`sin`/`cos` emulations stay scalar; no DAZ/FTZ. `crates/xedit-io/src/fpu.rs` guards only the text conversions against a changed MXCSR rounding mode; SIMD kernels run under the same MXCSR as scalar code.

<a id="assets-13"></a>**assets 13. Mesh optimiser float scores: leave scalar.**
`mesh_optimize.rs:408-411, 436-444, 551-700`. Faithful `Single` arithmetic via f64 round trips. Listed so a blanket SIMD pass does not touch it: any reassociation changes scores and therefore triangle order.

### xedit-gui (phase-7-gui branch)

Frame loop as implemented: the UI thread owns `TfrmMain` and `bridge: Option<CommandBridge>`; load runs on an `xedit-load` thread and jobs (`refs.build`, `filter.apply`, `files.clean`, lodgen) `take()` the whole bridge onto an `xedit-job` thread (`job_runner.rs:143-180`), during which the tree keeps painting from `Arc<dyn Element>`s and reads names through the core's per-element locks concurrently with the worker; every other command runs inline on the UI thread. Per frame: drain channels, `nav_panel` (`nav.prepare`, `TreeView::show`), status bar, selection compare, `show_nav_selection` (runs `records.compare` inline on change), `view_area`, menu actions, `load_visible_conflicts` (`conflicts.list`, ≤200 rows), dialogs.

<a id="gui-1"></a>**gui 1. Navigation selection rebuilt twice per frame from the core, then deep-compared.**
`main_form.rs:1311, 7114-7115`, `nav_tree.rs:640-700`. `self.nav.selection(&self.nav_state)` runs in `nav_panel` and again in `draw`; `selected_node` allocates six strings per selected row (`to_string`, `get_display_name`) and reads counts and header flags; then `selection != self.selection` compares every string. Change: cache `Selection` behind a generation counter bumped on `TreeState.selected/focused/anchor` change and on `NavSource.dirty`/`refresh`; store node ids and resolve names in the menu handlers. Category: per-frame. Gain: large with a Shift-range of thousands of rows, medium otherwise. Parity: `GetSortedSelection(True)` order and the selection-changed → view refresh timing.

<a id="gui-2"></a>**gui 2. `pending_conflicts` walks every expanded row every frame, allocating two `String`s per record.**
`nav_tree.rs:517-543, 796-800, 500-515`, `main_form.rs:7130`. The loop is over the whole flattened `visible` list, not the painted range; `record_key` builds `(file.get_name(), form_id.to_string(false))` and the map is `HashMap<(String, String), _>`; `node_conflict` repeats the allocation per painted row. Change: have `TreeView::show` return the painted row range in `TreeOutput` and scan only that; store the status on `NavNode` or key by `(file index, u32)`. Category: per-frame, algorithmic. Gain: large once a 100k-row group is expanded (O(rows) + 2·rows allocations per frame forever). Parity: none; upstream classifies painted nodes lazily too; keep the 200-per-pass batching top to bottom.

<a id="gui-3"></a>**gui 3. View grid paints and interacts with every column of every visible row; per-cell popup checks and env reads.**
`widgets/grid.rs:393-493, 433, 480-482`, `menu_model.rs:96-99, 112`, `view_grid.rs:671-673`. No horizontal culling; per cell `ui.interact`, `context_menu` (a `ctx.data` lookup + `Popup::menu`), `env::var_os("XEDIT_GUI_DEBUG_GRID")`, and two string clones. Change: cull columns to the viewport with a horizontal scrollbar; one context menu per grid anchored at the secondary-clicked cell; the debug flag in a `OnceLock`; `edit_text` on demand when an edit starts. Category: per-frame. Gain: medium-large for records with 100-250 overrides. Parity: column order, focused-column label, `pmuViewHeader` `PopupIndex`, `vstViewBeforeCellPaint` fills.

<a id="gui-4"></a>**gui 4. Long commands still run inline on the UI thread; the view filter re-compares on every keystroke.**
`main_form.rs:6677` (`script.run`), `:3142-3155` (only `files.clean` goes to a job), `:2068-2107` + `:988-1003` (view filter → `reload_view` → `records.compare` per keystroke), `panes.rs:341-343` (`refs.get`), `main_form.rs:1447-1534`. Change: route `Registry::mutates == Some(true)` or a known-slow allowlist through `JobRunner` (Cancel then works as upstream `pnlCancel`); debounce the view filter or run `records.compare` on the job thread. Category: concurrency. Gain: large for responsiveness. Parity: upstream runs these on the UI thread with `DoProcessMessages`; message order in `mmoMessages` and wait-form text must match; a debounce is a visible, acceptable difference.

<a id="gui-5"></a>**gui 5. `NavSource::refresh` after every mutating command leaks nodes and drops every conflict colour.**
`nav_tree.rs:325-332, 348-439`, `main_form.rs:3115-3129`. `refresh` clears children and `loaded` but keeps the stale `NavNode`s in `nodes`, so `nodes` grows by the expanded tree size per edit; clearing `conflicts` re-queries every visible record 200 per frame. Change: rebuild in place matching children to existing ids by element id (compact orphans, remap `TreeState`); invalidate conflict status only for the records the response names or per touched file. Category: memory, algorithmic. Gain: medium, grows with session length. Parity: `NavUpdate(True)` keeps expansion and selection; upstream resets `ConflictAll := caUnknown` only on affected nodes.

<a id="gui-6"></a>**gui 6. Editor ID / FormID search: linear scan of all nodes per hit and a full re-flatten per `reveal`.**
`main_form.rs:1489-1534`, `nav_tree.rs:703-741`. Change: a `HashMap<(file index, u32), node id>` filled in `load_children`; pick the hit first, `reveal` once. Gain: medium on a tree with millions of materialised nodes. Parity: the "walks initialised nodes only" quirk is preserved because the index holds built nodes only.

<a id="gui-7"></a>**gui 7. Compare response copied twice into the view model.**
`main_form.rs:1067-1095`, `view_grid.rs:244-271, 323-350`, `command.rs:65-73`. The request is serialised to `Value`, the response deserialised, then `build_level_of` clones `name`, `member_name` and eight `Option<String>` per cell. Change: `from_compare(response: CompareResponse, ..)` by value; longer term a typed in-process entry point beside `Registry::call` so the GUI skips the JSON round trip (the design rule "one operation layer" is kept: same typed request and response structs, no JSON in between). Category: allocation. Gain: medium on big records. Parity: none.

<a id="gui-8"></a>**gui 8. Messages memo re-joins the whole log and re-lays it out every frame.**
`panes.rs:914-932, 883-892, 1001-1014`. `lines.join("\r\n")` every frame, `TextEdit::multiline` over the entire log, `selected_text` O(n), `status_text()` clones the last line. Change: cache the joined text and append in `add`/`push`; render lines with `show_rows` plus a light selection model; at minimum cache the join. Category: per-frame, allocation. Gain: medium with the Messages tab open after a big load or script. Parity: `TMemo` selection, Ctrl+double-click FormID jump, autoscroll, `SaveSelected`.

<a id="gui-9"></a>**gui 9. Referenced By loads every reference row inline, unbounded.**
`panes.rs:322-343, 375-385`, `main_form.rs:665-680, 1589-1591`. `RefsGetRequest { offset: 0, limit: None }` on the UI thread; every `RecordRef` becomes owned Strings; `sort_items` runs `compare_text` per comparison on every column click. Change: `refs.get` through `JobRunner` with paging, precomputed lowercase sort keys, a per-record cache for back/forward. Category: concurrency, allocation. Gain: medium for keywords and texture sets with 100k+ referrers. Parity: tab caption counts, sort column cycling, Ctrl = descending.

<a id="gui-10"></a>**gui 10. Selection stored as `Vec<usize>` with linear membership and an O(n²) dedup.**
`widgets/virtual_tree.rs:167-169, 195-205`, `selection.rs:141-151`. Change: sorted ranges or a bitset keyed by row; `HashSet<(form_id, file)>` for `active_records`. Gain: small-medium with large range selections. Parity: sorted selection order, Ctrl-toggle semantics.

<a id="gui-11"></a>**gui 11. Per-row allocations in the tree paint path and core reads on the UI thread.**
`nav_tree.rs:1096-1136`, `widgets/virtual_tree.rs:330-345, 365-366, 422-426`. Three fresh `String`s per painted row per frame (`get_editor_id`/`get_display_name` take the record locks and may trigger `quick_init` on the UI thread), `TreeRow::default()` per row, `visual.clone()`, `columns.clone()`, `set_column_widths(.. collect())` per frame. Change: cache the three label strings on `NavNode` once built and invalidate in `refresh`, which also lets the galley cache hit on stable `&str`; take `&Visuals` by reference. Gain: small-medium. Parity: labels must follow edits; EditorID availability text during init.

<a id="gui-12"></a>**gui 12. Job progress lines are surfaced only when the job ends.**
`job_runner.rs:196-214`, `main_form.rs:5795-5812`, contrast `:763-770`. Change: drain `wait.take_messages()` every frame in `poll_job`. Category: simplification. Gain: small (removes an end-of-job hitch, matches upstream's live `AddMessage`). Parity: message order relative to the "finished in" line.

<a id="gui-13"></a>**gui 13. Per-frame environment reads and diagnostic formatting.**
`main_form.rs:6926, 6931-6941`, `widgets/grid.rs:359`, `menu_model.rs:112`, `lib.rs:42`. `env::var("XEDIT_GUI_SCALE")` every frame; a geometry `format!` every 60 frames before discovering `XEDIT_GUI_LOG` is unset. Change: `OnceLock`s in `lib.rs`. Gain: small, trivial. Parity: none.

<a id="gui-14"></a>**gui 14. `NavNode` footprint and full-materialisation paths.**
`nav_tree.rs:117-133, 308-323, 765-784`. About 88 B per node plus a heap block per container; `expand_all` and `records_below` materialise entire subtrees. Change: `u32` ids, derive `container` on demand, children as a contiguous `Range<u32>`. Category: memory. Gain: medium only for Expand All on Starfield-size data. Parity: hidden-group node order (`ivsHidden` after its record) and `vstNavInitChildren` numbering.

## 6. SIMD plan

Rules, from the phase and from `simd.rs` as it stands: a kernel is an alternative route to what its scalar definition computes; the scalar version is the definition; the equivalence test covers every input shape; `XEDIT_SIMD=off` forces the scalar route; runtime detection picks the level. Extend `Level` with `Avx512` (detect `avx512f` + `avx512bw`; `avx512vbmi` where a kernel needs it) and keep `level.min(detect())` lowering so a test can ask for any level.

Where AVX-512 pays and where it does not: most of these loops are memory-bound or short, and on those AVX2 already reaches bandwidth. AVX-512 is worth a separate kernel only where the per-byte work is real (the DXT distance matrix, the mipmap accumulate, half conversion with `vcvtph2ps` on 16 lanes, case-folded UTF-16 hashing) or where its mask registers simplify tails (`memcmp`, ASCII check). Many client CPUs have no AVX-512 (Intel 12th-14th generation disables it; AMD Zen 4 and 5 have it), so the AVX2 path is the one most users will run and must be the one tuned first.

Candidate kernels, with the bit-exactness class:

| Kernel | Caller | Levels | Bit-exact | Finding |
| --- | --- | --- | --- | --- |
| Byte equality of two record payloads with early exit | identical-to-master fast path | SSE2 / AVX2 / AVX-512BW | yes | [analysis 2](#analysis-2) |
| `is_ascii` over a byte string | encoding fast path | SWAR (std) / AVX2 / AVX-512BW | yes | [io 3](#io-3) |
| Zero-terminator scan | string values, string tables, archive names | `memchr` crate (SSE2/AVX2) | yes | [io 6](#io-6), [core-load 3](#core-load-3) |
| Case-folded UTF-16 xxHash64 feed | `lookup_hash_text` | AVX2 widen + compare/add | yes | [io 4](#io-4) |
| Regex first-byte prefilter | `pcre.rs` | `memchr`/`memchr2`/`memchr3` | yes | [core-write 5](#core-write-5) |
| Branchless or Eytzinger FormID search | `fl_form_id_keys` | SSE2 / AVX2 compare of 8 or 16 keys | yes | [core-load 1](#core-load-1) |
| Half ↔ float over vertex arrays | geometry readers | F16C (exists) / AVX-512F | yes | [assets 2](#assets-2) |
| DXT endpoint and mask selection | atlas encode | AVX2 `madd_epi16` / AVX-512BW | yes, with in-order tie-break | [assets 5](#assets-5) |
| Four-channel f64 accumulate in the resampler | mipmaps | AVX (`mul_pd` + `add_pd`, no FMA) / AVX-512F | yes, lane-wise same order | [assets 3](#assets-3) |
| Alpha threshold, channel swap, 3↔4 byte conversion | atlas, DDS | SSSE3 / AVX2 shuffles (extend `rgb_to_bgrx`) | yes | [assets 9](#assets-9) |
| `[f64; 4]` min/max and element-wise transforms | `nif_math` | SSE2 / AVX | yes, with operand order rule | [assets 12](#assets-12) |

Not bit-exact and therefore out: anything that reassociates a sum (`length`, `normalize`, tangent sums), FMA anywhere, horizontal reductions of floats, DAZ/FTZ, the mesh optimiser scores, the Delphi float formatting. Not useful: SIMD CRC32 and inflate (libdeflate already does it), xxHash64 (sequential by design), UTF-8 validation (std already vectorised).

Testing shape for each kernel: the `every_level_gives_the_scalar_bytes` pattern of `simd.rs` over every length from 0 to a few multiples of the widest vector, plus random data; the ignored `throughput` test for a quick number; and the full parity harness once with `XEDIT_SIMD=off` and once with it on, which the phase gate already demands.

## 7. Concurrency plan

What keeps parallel output identical today and must stay: work is split into units whose results are merged in a fixed order on the calling thread; cross-thread builds are detected through `init_cycles` and redone serially; the order-dependent groups (GMST/DFOB/NAVI/TES4) and INFO/DIAL topic sorting are classified serially; nothing reads `threads()` to decide an output.

The remaining serial stages, in the order they appear in a session:

1. Archive table parsing at startup, serial under a `RwLock` ([session 7](#session-7)): open in parallel, insert in container order.
2. The `Register` replay and `sort_records` after the parallel scan ([core-load 2](#core-load-2)).
3. The `crc32_app` hash before the first reference build ([core-write 10](#core-write-10)).
4. The conflict scan prologue over 23M records ([analysis 3](#analysis-3)) and the filter's per-record classification on five games ([analysis 1](#analysis-1)).
5. Check for Errors' per-topic flushes ([analysis 5](#analysis-5)).
6. The INCC count, ONAM rebuild and injected scan in save and clean ([core-write 7](#core-write-7), [core-write 12](#core-write-12), [core-write 11](#core-write-11)).
7. LODGen mesh scan, texture load and resize, atlas resample and encode ([assets 3](#assets-3) to [assets 5](#assets-5)).
8. `script.check` over many files ([script 11](#script-11)).
9. Contention that caps the scaling of what is already parallel: the global `INITIALIZED_RECORDS`/`PINNED_RECORDS` mutexes ([core-load 6](#core-load-6)), the per-plugin `Arc<FileBytes>` counter ([core-load 5](#core-load-5)), the 1024-group barrier ([analysis 7](#analysis-7)), the four-batch dump window ([session 6](#session-6)), `find_record_def`'s two reader locks ([core-load 12](#core-load-12)).

GUI thread model: move every command that can take more than a frame onto the job runner ([gui 4](#gui-4), [gui 9](#gui-9)) so the UI keeps the tree painting as it already does during load, refs and clean.

## 8. Build profile and benchmarks

**Profile.** The root `Cargo.toml` has `[profile.dev]` and `[profile.test]` only. Add:

```toml
[profile.release]
lto = "fat"
codegen-units = 1
# panic stays "unwind": sniff/main_form.rs:545 turns a worker panic into
# DfError with catch_unwind, main.rs:4067 uses resume_unwind, and rayon
# propagates worker panics by unwinding.

[profile.profiling]
inherits = "release"
debug = 1
strip = false

[profile.harness]
inherits = "release"
lto = "thin"
codegen-units = 16
```

and point `cargo xtask parity`'s `build_port` (`crates/xtask/src/parity.rs:1558-1565`) at `harness` so the dev loop does not pay fat LTO, while published numbers and releases use `release`. Do not set `target-cpu`: binaries ship to users and the output must not depend on the CPU; runtime dispatch in `simd.rs` is the route. PGO through `cargo pgo` with the SSE dump and refs corpus as training is worth one experiment after LTO (open question 5 in the session report); BOLT is Linux-only.

**Benchmarks.** Only `crates/xedit-session/benches/dump.rs` exists (criterion, SSE `Update.esm` load and dump, gated on `XEDIT_SSE_DATA`); CI runs `cargo test --workspace`. Nothing pins the thread count or SIMD level, and both come from environment variables read at first use. The gate needs, per workload:

| Gate workload | Exists | Add |
| --- | --- | --- |
| Load | SSE `Update.esm` | FO4 `Fallout4.esm` + a DLC with BA2 string tables; a 10-plugin load order; cold and warm cache; wall time and peak RSS; `--threads 1` and default |
| Reference build | no | `refs.build` over `Skyrim.esm` + `Update.esm` with the cache disabled, and a second case loading the cache; records/s; 1 and N threads |
| Conflict scan | no | `conflicts.list` over `Skyrim.esm` + `Update.esm` + one DLC (real overrides); records/s; fixed `--threads` |
| Clean | no | quick clean `dry_run` of a DLC esm on an SSE and an FO4 load order (the two filter paths); the finding count must equal the parity fixture |
| Archive pack | no | pack a fixed asset folder to BA2 GNRL, BA2 DX10 and compressed SSE BSA at `threads = 1` and `0`; MB/s; plus extraction of the result |

Shape: an `xtask bench` that times the release `xedit` binary per workload, repeats three times, takes the median, records CPU model, thread count, SIMD level, profile and commit, runs each case under `XEDIT_SIMD=off` and on, and writes JSON under `docs/history/` for the "numbers published in the repository" gate; the Delphi build runs on the same corpus with the same runner for the comparison column. Per-record script timings (`Add Prefix Plus Name To EDID.pas` on the corpus) and a NIF corpus load/save pass belong in the same runner even though they are not gate workloads.

## 9. Leave alone

- Any float arithmetic whose result reaches an output: `mesh_optimize.rs` scores, `nif_math` sums and normalisation, the Delphi `FloatToStr` emulation, the LODGen contributor sums (parallelise across rows, never within a sum).
- zlib implementation choice and the 8 MiB libdeflate/zlib split (bytes would change); LZ4 HC level (the cache files already accept both).
- Upstream caching rules that are observable (`mr_name` kept across a FormID change, official-file-only name caches, the `Drop` reset semantics of script element wrappers).
- The `init_cycles` serial re-run and the serial classification of GMST/DFOB/NAVI/TES4 and INFO/DIAL chains.
- `panic = "unwind"`.
- The definitions' eager build at game setup, until a measurement says it matters.

## 10. Measure before acting

The reviewers' open questions, merged. Each decides the rank of one or more findings.

1. **Load split** for `Fallout4.esm` and `Skyrim.esm`: parallel scan, serial `Register` replay, `sort_records`, master loading, archive table parsing, cold versus warm page cache. Ranks [core-load 2](#core-load-2), [core-load 8](#core-load-8), [io 6](#io-6), [session 7](#session-7).
2. **Reference build split** per `collect_one`: element construction versus `resolve_reference` versus the global-lock bookkeeping, sampled at 1, 4 and 16 threads. Ranks [core-write 1](#core-write-1), [core-load 1](#core-load-1), [core-load 6](#core-load-6), [core-load 5](#core-load-5).
3. **Conflict scan**: share of chains that are byte-identical and satisfy the identity master-index condition; share of time in sort-key construction versus element build versus bookkeeping; `filter.apply --preset cleaning` on an FNV or TES5 load order with and without the as-walked path. Ranks [analysis 1](#analysis-1), [analysis 2](#analysis-2), [analysis 4](#analysis-4).
4. **Encoding**: ASCII hit rate of `get_string` per game and the cost of two `MultiByteToWideChar` calls under a profiler. Sizes [io 3](#io-3).
5. **Archives**: `lz4_flex::FrameDecoder` versus `LZ4F_decompress` on a full SSE texture BSA; per-call `Compressor::new` versus thread-local on a texture pack (page faults and wall time). Sizes [io 1](#io-1), [io 5](#io-5).
6. **Save**: ratio of `prepare_save` (INCC, ONAM, counted records) to `container_write_to_stream` on a large master. Sizes [core-write 7](#core-write-7), [core-write 8](#core-write-8), [core-write 12](#core-write-12).
7. **Scripts**: a sampled profile of `Add Prefix Plus Name To EDID.pas` to split re-lexing, `VarList` scans and `ScriptElement::new`; `script.run` startup with an empty `Process`. Ranks [script 1](#script-1) to [script 7](#script-7).
8. **Assets**: load + save share versus operation share on the Sniff corpus; the LODGen phase split (UV scan, texture load/resize, atlas resample, DXT encode, external LODGen.exe); the exhaustive half/f32 text round-trip check; vertex-count distribution of the corpus. Ranks [assets 1](#assets-1) to [assets 5](#assets-5), [assets 8](#assets-8).
9. **Dump**: per-worker idle at batch tails on 8, 16 and 32 threads. Sizes [session 6](#session-6).
10. **GUI**: frame time with a 10k-100k row range selection; `visible.len()` and the `pending_conflicts` share after expanding a large group; `records.compare` wall time on the worst records; egui frame cost with 250 columns; `nodes.len()` growth after N edits. Ranks [gui 1](#gui-1) to [gui 5](#gui-5).
11. **Build**: fat LTO + `codegen-units = 1`, then PGO; if the combined gain on the five gate workloads is under about 5%, keep LTO only.

## 11. Suggested order of work

No step depends on an earlier one unless stated. Each step ends with the full parity harness on the touched modes, run once with `XEDIT_SIMD=off` and once with it on, and at `--threads 1` and the default.

1. **Baseline.** Release profile; the `xtask bench` runner and the five gate benchmarks; record the Delphi numbers and the port's numbers at `main` with and without the new profile. Everything after this is measured against it.
2. **Cheap, safe, broad.** Encoding fast path ([io 3](#io-3)); non-cloning element access (`with_elements`, `element_at`, `name_matches`; [core-load 4](#core-load-4), [core-load 7](#core-load-7), [core-write 8](#core-write-8), [analysis 9](#analysis-9)); `read_built` fast path ([core-load 9](#core-load-9)); buffered JSON writers ([session 2](#session-2), [session 3](#session-3)); regex `char_at` ([core-write 4](#core-write-4)); the LZ4 routes ([io 1](#io-1), [io 2](#io-2)); thread-local compressor states ([io 5](#io-5)).
3. **Reference build.** FormID memo and key array ([core-write 1](#core-write-1), [core-load 1](#core-load-1)); `Vec` collector ([core-write 2](#core-write-2)); per-thread init bookkeeping ([core-load 6](#core-load-6)); cache file allocations ([core-write 9](#core-write-9)).
4. **Conflict scan and clean.** Batch classification on the five games ([analysis 1](#analysis-1)); prologue allocations ([analysis 3](#analysis-3)); sort-key memo and row scratch ([analysis 4](#analysis-4)); then the identical-to-master fast path behind its dual-run verification ([analysis 2](#analysis-2)); clean pass reuse ([analysis 10](#analysis-10)); INCC and ONAM on save ([core-write 7](#core-write-7), [core-write 12](#core-write-12)).
5. **Load.** Prefetch ([core-load 8](#core-load-8)); two-phase registration ([core-load 2](#core-load-2)); `find_record_def` freeze ([core-load 12](#core-load-12)); lazy resource containers ([session 7](#session-7)); archive table work ([io 4](#io-4), [io 6](#io-6)); `MainRecordImpl` cold fields ([core-load 10](#core-load-10)).
6. **Archive pack.** Remaining packer items ([io 10](#io-10), [io 11](#io-11)); DXT kernels and row-parallel encode ([assets 5](#assets-5)) for the texture route.
7. **Scripts.** Parse-time positions ([script 1](#script-1)); copy-on-write strings ([script 2](#script-2)); slot resolution ([script 4](#script-4)); the `Args` and host-argument items ([script 3](#script-3), [script 8](#script-8)); registration once per session ([script 6](#script-6)); parallel `script.check` ([script 11](#script-11)).
8. **Assets and LODGen.** Inline values and lazy vertex arrays ([assets 1](#assets-1)); typed vertex readers with the F16C kernel ([assets 2](#assets-2)); row-parallel resampler and dispatch hoist ([assets 3](#assets-3)); parallel texture and mesh preparation ([assets 4](#assets-4)); the reverse reference index ([assets 6](#assets-6)); the O(n²) vertex checks ([assets 8](#assets-8)).
9. **GUI.** Selection cache ([gui 1](#gui-1)); painted-range conflict scan ([gui 2](#gui-2)); column culling and one context menu ([gui 3](#gui-3)); commands on the job runner ([gui 4](#gui-4)); in-place refresh ([gui 5](#gui-5)); the smaller items.
10. **Publish.** The gate table in `README.md` and `docs/PLAN.md` with the numbers from step 1's runner, per workload, against the Delphi build.
