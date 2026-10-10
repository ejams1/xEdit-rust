// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

mod engine;
mod mcp;
mod pipe;
mod rpc;
mod serve;

use std::io::BufReader;
use std::process::ExitCode;

use clap::{Parser, Subcommand};
use serde_json::{Value, json};
use xedit_session::{CommandError, Registry, parse_batch};

/// The dump allocates and frees millions of small strings and element
/// nodes; mimalloc serves them faster than the Windows heap.
#[global_allocator]
static GLOBAL: mimalloc::MiMalloc = mimalloc::MiMalloc;

/// xEdit command-line interface.
#[derive(Parser)]
#[command(name = "xedit", version)]
struct Cli {
    /// Print one JSON envelope: {"ok":true,"result":...} or {"ok":false,"error":{"code","message"}}.
    #[arg(long, global = true)]
    json: bool,

    /// Game of the plugins to load, as the xDump switch: tes3, tes4, fo3, fnv, tes5, enderal, fo4, sse, tes5vr, enderalse, fo4vr, fo76 or sf1.
    #[arg(long, global = true)]
    game: Option<String>,

    /// Plugin to load, in load order; repeat for several. Masters load with it.
    #[arg(long, global = true)]
    load: Vec<String>,

    /// Allow the commands that change plugin data or files to do so. Without it they only run with --dry-run.
    #[arg(long, global = true)]
    edit: bool,

    /// Give the topic responses without PNAM the PNAM of the response they follow when they are built (xEdit's -FillPNAM; the quick clean mode turns it on).
    #[arg(long, global = true)]
    fill_pnam: bool,

    /// Threads that load the plugins and build the records of a dump; 1 runs everything on one thread. Default: RAYON_NUM_THREADS, else one per CPU. The output is the same for every count.
    #[arg(long, global = true)]
    threads: Option<usize>,

    /// Folder of the reference cache files (xEdit's -C:). Default: "<AppName>Edit Cache" in the data folder, as xEdit.
    #[arg(long, global = true)]
    cache_path: Option<String>,

    /// Neither read nor write reference cache files (xEdit's -DontCache).
    #[arg(long, global = true)]
    dont_cache: bool,

    /// Do not read reference cache files (xEdit's -DontCacheLoad).
    #[arg(long, global = true)]
    dont_cache_load: bool,

    /// Do not write reference cache files (xEdit's -DontCacheSave).
    #[arg(long, global = true)]
    dont_cache_save: bool,

    /// The program's own mod group file. Default: <AppName>Edit.modgroups next to xedit.exe, as xEdit's next to its program.
    #[arg(long, global = true)]
    modgroups_file: Option<String>,

    /// The xEdit settings file that keeps the saved mod group selection. Default: xEdit's: <AppName>Edit.ini next to the program when it exists, else Plugins.<app>viewsettings next to the game's Plugins.txt in the local application data.
    #[arg(long, global = true)]
    settings: Option<String>,
    /// Language of the string tables of localized plugins (xEdit's -l:), such as English or En; the game's default when omitted.
    #[arg(long, global = true)]
    language: Option<String>,

    /// Load in xEdit's translate mode (-translate): only the translatable elements are compared and edited, and the commands that change the structure of a plugin are refused.
    #[arg(long, global = true)]
    translate: bool,

    #[command(subcommand)]
    action: Action,
}

#[derive(Subcommand)]
enum Action {
    /// Print every command with its request and response JSON Schema.
    Schema,
    /// The game and the loaded plugins.
    Session {
        #[command(subcommand)]
        action: SessionAction,
    },
    /// The loaded files.
    Files {
        #[command(subcommand)]
        action: FilesAction,
    },
    /// Records of the loaded plugins.
    Records {
        #[command(subcommand)]
        action: RecordsAction,
    },
    /// Elements of a record.
    Elements {
        #[command(subcommand)]
        action: ElementsAction,
    },
    /// Masters of a plugin. Each command rewrites the FormIDs of the plugin to follow its masters; save to keep the change.
    Masters {
        #[command(subcommand)]
        action: MastersAction,
    },
    /// Patches built from the loaded plugins.
    Patch {
        #[command(subcommand)]
        action: PatchAction,
    },
    /// Mod groups: the .modgroups files that say which records of a load order hide others, the saved selection, and their CRC32s.
    Modgroups {
        #[command(subcommand)]
        action: ModgroupsAction,
    },
    /// String tables of localized plugins (the localization editor), the language, and localizing or delocalizing a plugin.
    Localization {
        #[command(subcommand)]
        action: LocalizationAction,
    },
    /// The reference index: the records that refer to a record, the reference cache, and the reachable information the "not reachable" filter option reads.
    Refs {
        #[command(subcommand)]
        action: RefsAction,
    },
    /// The navigation tree filter and the presets of its options dialog (filter.apply, filter.remove, filter.presets, filter.preset.save|delete).
    Filter {
        #[command(subcommand)]
        action: FilterAction,
    },
    /// FormIDs of records.
    Formids {
        #[command(subcommand)]
        action: FormidsAction,
    },
    /// BSA and BA2 archives: list, extract and pack, the modes of BSArch. They need no --game or --load.
    Archive {
        #[command(subcommand)]
        action: ArchiveAction,
    },
    /// Save files.
    Saves {
        #[command(subcommand)]
        action: SavesAction,
    },
    /// Clean a plugin (files.clean): remove the records identical to their master (--itm), undelete and disable the deleted references (--udr), or run xEdit's quick auto clean mode (--quick: both, saved, repeated while a pass changes the plugin). Without --edit only --dry-run, which counts.
    Clean {
        /// Plugin name; the only loaded plugin when omitted.
        #[arg(long)]
        file: Option<String>,
        /// Remove the records that are identical to their master and the groups left empty.
        #[arg(long)]
        itm: bool,
        /// Undelete and disable the deleted references.
        #[arg(long)]
        udr: bool,
        /// The quick auto clean mode of -quickautoclean; loads the plugins as that mode does (full record definitions, PNAM fill) and saves the plugin.
        #[arg(long)]
        quick: bool,
        /// Count what would be cleaned, change nothing.
        #[arg(long)]
        dry_run: bool,
        /// Where --quick saves the plugin; the loaded path when omitted.
        #[arg(long)]
        output: Option<String>,
        /// Do not move an existing file at the output path to the backup folder.
        #[arg(long)]
        no_backup: bool,
    },
    /// The tool modes of xEdit (xeInit.pas): list them, or run one over the loaded plugins (setesm, clearesm, masterupdate, masterrestore, onamupdate, sortandcleanmasters, generateseq, checkforerrors, checkforitm, checkfordr, export).
    Tool {
        #[command(subcommand)]
        action: ToolAction,
    },
    /// Check for errors (files.check, "Check for Errors"): every element of the files or records is checked, and each record with errors is printed with its errors as xEdit's message log shows them. Loads the plugins as the -CheckForErrors mode does (no internal edits of the load). With --json the records and errors as JSON.
    Check {
        /// Loaded file to check; repeat for several. The plugins given with --load when neither a file nor a record is named.
        #[arg(long)]
        file: Vec<String>,
        /// Load order FormID of a record to check; repeat for several.
        #[arg(long)]
        record: Vec<String>,
        /// The loaded file whose version of the records is checked; the last loaded plugin when omitted.
        #[arg(long)]
        record_file: Option<String>,
        /// Check the last file of the load order instead, as xEdit's -CheckForErrors mode does (the hardcoded records when only the game master is loaded).
        #[arg(long)]
        last: bool,
    },
    /// Write a loaded plugin to disk as xEdit saves it (files.save).
    Save {
        /// Plugin name; the only loaded plugin when omitted.
        #[arg(long)]
        file: Option<String>,
        /// Path to write to; the loaded path when omitted.
        #[arg(long)]
        output: Option<String>,
        /// Build the file and report its size and CRC32, but write nothing.
        #[arg(long)]
        dry_run: bool,
        /// Do not move an existing file at the output path to the backup folder.
        #[arg(long)]
        no_backup: bool,
    },
    /// List the conflict status of the records of the loaded plugins and of each file (conflicts.list, ConflictLevelForMainRecord). Lists the records that are not the only record of their FormID unless --include-single.
    Conflicts {
        /// List only the records of this loaded file; repeat for several. Every record is still compared with all loaded files.
        #[arg(long)]
        file: Vec<String>,
        /// List only the records with this signature, such as NPC_; repeat for several.
        #[arg(long)]
        signature: Vec<String>,
        /// List only the records whose conflict is at least this ConflictAll: caOnlyOne, caNoConflict, caConflictBenign, caOverride, caConflict or caConflictCritical.
        #[arg(long)]
        min_conflict_all: Option<String>,
        /// List only the records whose own status is this ConflictThis, such as ctConflictLoses; repeat for several.
        #[arg(long)]
        conflict_this: Vec<String>,
        /// Also list the records that are the only record of their FormID.
        #[arg(long)]
        include_single: bool,
        /// Compare only the master and the leaf overrides ("Only show Master and Leafs").
        #[arg(long)]
        master_and_leafs: bool,
        /// Classify a FormID with one override as an override without comparing, as -quickshowconflicts does.
        #[arg(long)]
        quick_show_conflicts: bool,
        /// Activate the valid mod group of this name, so the records it hides are left out (ctHiddenByModGroup); repeat for several.
        #[arg(long = "modgroups")]
        modgroups: Vec<String>,
        /// Activate every valid mod group, as xEdit's -autoload does.
        #[arg(long)]
        all_modgroups: bool,
        /// Activate the valid mod groups of the selection saved in xEdit's settings file.
        #[arg(long)]
        saved_modgroups: bool,
        /// Listed records to skip.
        #[arg(long, default_value_t = 0)]
        offset: usize,
        /// Records to list at most.
        #[arg(long)]
        limit: Option<usize>,
    },
    /// Compare the records of a FormID side by side, row by row with their conflict status, as the view tab does (records.compare).
    Compare {
        /// Load order FormID of the record as hexadecimal digits.
        form_id: String,
        /// Plugin the record is seen from; the last loaded plugin when omitted.
        #[arg(long)]
        file: Option<String>,
        /// Compare only the master and the leaf overrides ("Only show Master and Leafs").
        #[arg(long)]
        master_and_leafs: bool,
        /// Hide the rows without a conflict ("Hide no conflict and empty rows").
        #[arg(long)]
        hide_no_conflict: bool,
        /// Also list the rows the view hides (ignored members, members no record has).
        #[arg(long)]
        include_hidden: bool,
        /// Activate the valid mod group of this name, so the records it hides are left out (ctHiddenByModGroup); repeat for several.
        #[arg(long = "modgroups")]
        modgroups: Vec<String>,
        /// Activate every valid mod group, as xEdit's -autoload does.
        #[arg(long)]
        all_modgroups: bool,
        /// Activate the valid mod groups of the selection saved in xEdit's settings file.
        #[arg(long)]
        saved_modgroups: bool,
        /// The view filter of the view tab (edViewFilterName): keep the rows whose name contains this text.
        #[arg(long)]
        view_filter_name: Option<String>,
        /// edViewFilterValue: keep the rows with this text in one of their cells.
        #[arg(long)]
        view_filter_value: Option<String>,
        /// A "cobViewFilter" set to Or: a row matches when the name or a value matches, instead of both.
        #[arg(long)]
        view_filter_or: bool,
        /// cbViewFilterKeepChildren: also keep the rows below a matching one.
        #[arg(long)]
        keep_children: bool,
        /// cbViewFilterKeepSiblings: also keep the rows beside a matching one.
        #[arg(long)]
        keep_siblings: bool,
        /// cbViewFilterKeepParentsSiblings: also keep the rows beside the parent of a matching one.
        #[arg(long)]
        keep_parents_siblings: bool,
    },
    /// Write the element tree of a plugin as xDump prints it.
    Dump {
        /// Game of the plugin, as for --game.
        #[arg(long)]
        game: String,
        /// Path of the plugin.
        file: String,
        /// Write the errors of the elements instead, as xDump -check does ("Check for Errors" of xDump).
        #[arg(long)]
        check: bool,
    },
    /// NIF and KF meshes, BGSM and BGEM materials, LOD, FUZ and DDS files, loose or in an archive.
    Assets {
        #[command(subcommand)]
        action: AssetsAction,
    },
    /// The operations of Sniff on the NIF, KF and material files of a folder or archive.
    Sniff {
        #[command(subcommand)]
        action: SniffAction,
    },
    /// Generate the LOD of worldspaces as xEdit's LODGen mode does (lodgen.generate), with the LODGen form's options. Load every plugin of the load order with --load. Needs --edit unless --dry-run.
    Lodgen {
        /// The editor ID of a worldspace to generate LOD for; repeat for more. Without it the form's default.
        #[arg(long = "worldspace", value_name = "EDITORID")]
        worldspaces: Vec<String>,
        /// An option of the LODGen form, NAME=VALUE (AtlasWidth=4096, BuildAtlas=0, ...); repeat for more.
        #[arg(long = "set", value_name = "NAME=VALUE")]
        set: Vec<String>,
        /// The settings file of the options (<APP>LODGen.ini); read, and written back unless a dry run.
        #[arg(long)]
        settings: Option<String>,
        /// The folder for the LOD files (-O:); the data folder by default.
        #[arg(long)]
        output: Option<String>,
        /// The folder with LODGenx64.exe, Texconvx64.exe and the atlas maps (-S:).
        #[arg(long)]
        scripts: Option<String>,
        /// The temporary folder (-T:).
        #[arg(long)]
        temp: Option<String>,
        /// The data folder (-D:); the folder of the plugins by default.
        #[arg(long)]
        data: Option<String>,
        /// The game ini whose archive lists load (-I:).
        #[arg(long)]
        game_ini: Option<String>,
        /// The RandSeed of the tree rotations; from the clock by default.
        #[arg(long)]
        seed: Option<u32>,
        /// Split the trees LOD atlas of the worldspaces into billboards (the form's hidden Split Trees LOD button) instead of generating.
        #[arg(long)]
        split_trees: bool,
        /// List the worldspaces and options, but generate nothing.
        #[arg(long)]
        dry_run: bool,
    },
    /// Run a command by name.
    Call {
        /// Command name as listed by `xedit schema`, for example system.version.
        name: String,
        /// Command parameters as a JSON object.
        #[arg(long, default_value = "{}")]
        params: String,
    },
    /// Run several commands in one session: a JSON array of {"command": name, "params": {...}} from a file or stdin (-).
    Batch {
        /// Path of the JSON file, or - for stdin.
        file: String,
        /// Go on after a failed command instead of stopping at it.
        #[arg(long)]
        keep_going: bool,
    },
    /// Keep the session loaded and answer JSON-RPC requests, one per line, on stdio or a named pipe. Every command is a method; see rpc.discover.
    Serve {
        /// Listen on the Windows named pipe \\.\pipe\NAME instead of stdio.
        #[arg(long)]
        pipe: Option<String>,
    },
    /// Serve the commands as Model Context Protocol tools on stdio. The plugins load when the first tool is called.
    Mcp,
}

#[derive(Subcommand)]
enum ToolAction {
    /// List the tool modes of xEdit with their switches and what each does (tool.modes).
    Modes,
    /// Run a tool mode over the loaded plugins (tool.run). Needs --edit unless --dry-run. The modes that save (setesm, masterupdate, onamupdate, and the check modes' -quick forms) write every plugin they changed, each to its own path.
    Run {
        /// The tool mode: setesm, clearesm, masterupdate, masterrestore, onamupdate, sortandcleanmasters, generateseq, checkforerrors, checkforitm, checkfordr, export, edit, view, translate.
        mode: String,
        /// The modules the mode works on (sortandcleanmasters, generateseq); every loaded plugin when omitted.
        files: Vec<String>,
        /// Report what the mode would do, but change nothing and write nothing.
        #[arg(long)]
        dry_run: bool,
        /// Where the mode saves the plugin (one plugin); the loaded paths when omitted.
        #[arg(long)]
        output: Option<String>,
        /// Do not move an existing file at the output path to the backup folder.
        #[arg(long)]
        no_backup: bool,
        /// The format of the export mode: RAW (the default) or UESPWIKI.
        #[arg(long)]
        format: Option<String>,
        /// Where the generateseq mode writes the .seq files; <data path>\Seq when omitted.
        #[arg(long)]
        seq_path: Option<String>,
enum SniffAction {
    /// List the operations with their settings and defaults (sniff.list).
    List,
    /// Run an operation on a folder or archive (sniff.run), as Sniff's -OP: does. Needs --edit unless --dry-run.
        /// The title of the operation, any case, such as "Update bounds".
        operation: String,
        /// The folder or the archive (BSA, BA2) with the files.
        input: String,
        /// The folder to write the changed files to; not needed by operations that only report.
        /// A settings ini in Sniff's form (the section is the title without spaces).
        settings: Option<String>,
        /// A setting of the operation's section, NAME=VALUE; repeat for more.
        #[arg(long = "set", value_name = "NAME=VALUE")]
        set: Vec<String>,
        /// Only the files whose path holds this text.
        path_contains: Option<String>,
        /// Leave the subfolders of an input folder.
        no_subdir: bool,
        /// Report a file that fails and go on.
        skip_on_errors: bool,
        /// Write the unchanged files too.
        copy_all: bool,
        /// Threads; 0 for the CPU count less one.
        threads: Option<i32>,
        /// Also write the messages to this file, as Sniff's -LOG: does.
        log: Option<String>,
        /// Process the files and report, but write nothing.
    },
}
#[derive(Subcommand)]
enum AssetsAction {
    /// Print a file as text (ToText) or JSON (ToJSON, as Sniff writes it).
    Dump {
        /// The file, or the archive that holds it.
        file: String,
        /// The path of the file inside the archive FILE.
        archive_path: Option<String>,
        /// The format of the file: nif, bgsm, bgem, lod, dlodsettings, lst, btt, fuz or dds; by the extension when omitted.
        kind: Option<String>,
        /// text or json.
        /// Decimals of the float values, 6 to 16.
        decimals: Option<usize>,
        /// Rotations as Euler angles in degrees instead of an angle and an axis.
        euler: bool,
    },
    /// List the blocks of a NIF file.
    Blocks {
        /// The file, or the archive that holds it.
        file: String,
        /// The path of the file inside the archive FILE.
        archive_path: Option<String>,
        /// The format of the file; by the extension when omitted.
        kind: Option<String>,
    },
    /// List the NIF block types.
    Types,
    /// Load a file and write it back as xEdit saves it.
    Save {
        /// The file, or the archive that holds it.
        file: String,
        /// Path to write to.
        output: String,
        /// The path of the file inside the archive FILE.
        archive_path: Option<String>,
        /// The format of the file; by the extension when omitted.
        kind: Option<String>,
        /// Build the file and report it, but write nothing.
    },
    /// Set a value of a file and write it.
    Set {
        /// The file, or the archive that holds it.
        file: String,
        /// Path of the element, with \ between the names, below BLOCK for a NIF.
        path: String,
        /// The new value as the dump prints it.
        value: String,
        /// Path to write to.
        output: String,
        /// For a NIF: header, footer, a block index or a block path.
        block: Option<String>,
        /// The path of the file inside the archive FILE.
        archive_path: Option<String>,
        /// The format of the file; by the extension when omitted.
        kind: Option<String>,
        /// Report the value before and after, but write nothing.
    },
    /// Build a file from its JSON form and write it.
    FromJson {
        /// The JSON file.
        file: String,
        /// Path to write to.
        output: String,
        /// The format to build; by the extension before .json when omitted.
        kind: Option<String>,
        /// Build the file and report it, but write nothing.

    },
}

#[derive(Subcommand)]
enum SavesAction {
    /// Write the element tree of a save or co-save as `xDump -saves` prints it.
    Dump {
        /// Game of the save, as for --game.
        #[arg(long)]
        game: String,
        /// Data folder of the game, where the plugins the save lists are.
        #[arg(long)]
        data: String,
        /// Path of the save or co-save.
        file: String,
    },
}

#[derive(Subcommand)]
enum SessionAction {
    /// Report the game, the data folder and the loaded plugins.
    Info,
}

#[derive(Subcommand)]
enum FilesAction {
    /// List the loaded files with their masters and record counts.
    List,
    /// Make a new empty plugin in the data folder with the game master as its master (files.new, AddNewFile); it exists in memory until saved, so use it in batch or serve. Needs --edit unless --dry-run.
    New {
        /// File name; the extension is replaced by .esp, or .esl with --light, as xEdit does.
        file: String,
        /// A light module (ESL flag).
        #[arg(long)]
        light: bool,
        /// A medium module (Starfield).
        #[arg(long)]
        medium: bool,
        /// Check the name, make nothing.
        #[arg(long)]
        dry_run: bool,
    },
    /// Read or set the module flags of a plugin header (files.flags). Needs --edit unless --dry-run; --dry-run alone reads them.
    Flags {
        /// Plugin name; the only loaded plugin when omitted.
        #[arg(long)]
        file: Option<String>,
        /// The ESM flag.
        #[arg(long)]
        esm: Option<bool>,
        /// The light (ESL) flag, where the game has it.
        #[arg(long)]
        light: Option<bool>,
        /// The medium flag (Starfield); clears light and update.
        #[arg(long)]
        medium: Option<bool>,
        /// The update (overlay) flag (Starfield); clears light and medium.
        #[arg(long, alias = "overlay")]
        update: Option<bool>,
        /// The blueprint flag (Starfield).
        #[arg(long)]
        blueprint: Option<bool>,
        /// The localized flag.
        #[arg(long)]
        localized: Option<bool>,
        /// Report the flags the change would give, but change nothing.
        #[arg(long)]
        dry_run: bool,
    },
}

#[derive(Subcommand)]
enum LocalizationAction {
    /// The loaded string tables, as the localization editor lists them (localization.files). Loads the tables of the localized plugins first.
    Files {
        /// Only the tables of this plugin.
        #[arg(long)]
        file: Option<String>,
        /// List only the tables loaded so far.
        #[arg(long)]
        no_load: bool,
    },
    /// The strings of a table with their IDs (localization.strings).
    Strings {
        /// File name of the table, such as Dawnguard_English.STRINGS.
        table: String,
        /// Strings to skip.
        #[arg(long, default_value_t = 0)]
        offset: usize,
        /// Strings to return at most (default 100).
        #[arg(long)]
        limit: Option<usize>,
    },
    /// A string by table and ID, or the string a localized element holds (localization.get).
    Get {
        /// File name of the table; with --id.
        #[arg(long)]
        table: Option<String>,
        /// String ID as hexadecimal digits; with --table.
        #[arg(long)]
        id: Option<String>,
        /// Load order FormID of a record; with --path.
        #[arg(long)]
        form_id: Option<String>,
        /// Path of the localized string element, such as FULL.
        #[arg(long)]
        path: Option<String>,
        /// Plugin whose version of the record is meant.
        #[arg(long)]
        file: Option<String>,
    },
    /// Change the text of a string (localization.set, the localization editor's Save). Needs --edit unless --dry-run; save the plugin to write its tables.
    Set {
        /// The new text.
        text: String,
        /// File name of the table; with --id.
        #[arg(long)]
        table: Option<String>,
        /// String ID as hexadecimal digits; with --table.
        #[arg(long)]
        id: Option<String>,
        /// Load order FormID of a record; with --path.
        #[arg(long)]
        form_id: Option<String>,
        /// Path of the localized string element, such as FULL.
        #[arg(long)]
        path: Option<String>,
        /// Plugin whose version of the record is meant.
        #[arg(long)]
        file: Option<String>,
        /// Store the text as the editor's memo gives it: CR LF line breaks and one at the end.
        #[arg(long)]
        editor_text: bool,
        /// Report the change, make none.
        #[arg(long)]
        dry_run: bool,
    },
    /// Write a table as text, an [ID] line and the text per string (localization.export). Needs --edit unless --dry-run.
    Export {
        /// File name of the table.
        table: String,
        /// Path of the text file; <table>.txt when omitted.
        #[arg(long)]
        output: Option<String>,
        /// Report the size, write nothing.
        #[arg(long)]
        dry_run: bool,
    },
    /// The language of the session and the languages and tables the data folder and its archives hold (localization.languages).
    Languages,
    /// Switch the language of the string tables (localization.language). Refused while tables have unsaved changes, unless --discard.
    Language {
        /// The language, as localization languages lists it.
        language: String,
        /// Drop the unsaved changes of the string tables.
        #[arg(long)]
        discard: bool,
    },
    /// Localize a plugin: its strings move into new string tables and the plugin holds their IDs (localization.localize, "Localize plugin"). Needs --edit unless --dry-run; save the plugin to write it and its tables.
    Localize {
        /// Plugin name; the only loaded plugin when omitted.
        #[arg(long)]
        file: Option<String>,
        /// A table to translate from (repeat); found texts take the string at the same position of the --translate-to tables.
        #[arg(long)]
        translate_from: Vec<String>,
        /// A table to translate to (repeat), as many as --translate-from.
        #[arg(long)]
        translate_to: Vec<String>,
        /// Count the localizable strings, change nothing.
        #[arg(long)]
        dry_run: bool,
    },
    /// Delocalize a plugin: the texts of its strings go into the plugin and the localized flag is cleared (localization.delocalize, "Delocalize plugin"). Needs --edit unless --dry-run.
    Delocalize {
        /// Plugin name; the only loaded plugin when omitted.
        #[arg(long)]
        file: Option<String>,
        /// Count the localizable strings, change nothing.
        #[arg(long)]
        dry_run: bool,
    },
}

#[derive(Subcommand)]
enum RefsAction {
    /// The records that refer to a record (ReferencedBy of its master) and the records it refers to (refs.get). Builds the references of the loaded files first, or loads them from the cache.
    Get {
        /// Load order FormID of the record as hexadecimal digits.
        form_id: String,
        /// Plugin whose version of the record to read; the last loaded plugin when omitted.
        #[arg(long)]
        file: Option<String>,
        /// Entries of the referenced-by list to skip.
        #[arg(long, default_value_t = 0)]
        offset: usize,
        /// Entries of the referenced-by list to return at most.
        #[arg(long)]
        limit: Option<usize>,
        /// edReferencedByFilterName: keep the entries whose name contains this text.
        #[arg(long)]
        filter_name: Option<String>,
        /// edReferencedByFilterSignature: keep the entries with this signature.
        #[arg(long)]
        filter_signature: Option<String>,
        /// edReferencedByFilterFileName: keep the entries of a file whose name contains this text.
        #[arg(long)]
        filter_file: Option<String>,
        /// A "cobReferencedByFilter" set to Or: the name or the signature may match instead of both.
        #[arg(long)]
        filter_or: bool,
    },
    /// Write the referenced-by lists of every loaded file as text (the parity check's format, see refs.pas of the harness).
    Dump,
    /// Build the references of the loaded files, or load them from the reference cache, and save the cache (refs.build, BuildOrLoadRef).
    Build {
        /// Only this loaded file (Build Reference Info); every loaded file when omitted.
        #[arg(long)]
        file: Option<String>,
        /// Only load the references from the cache; build none.
        #[arg(long)]
        only_load: bool,
enum ArchiveAction {
    /// Read the header and the file table of an archive (archive.list): format, version, flags, warnings and with --files the files.
    List {
        /// Path of the archive.
        archive: String,
        /// List the files.
        #[arg(long)]
        files: bool,
        /// Only the files below this folder of the archive.
        #[arg(long)]
        folder: Option<String>,
        /// Files to skip.
        #[arg(long, default_value_t = 0)]
        offset: usize,
        /// Files to return at most.
        #[arg(long, default_value_t = 1000)]
        limit: usize,
    },
    /// Unpack an archive into a folder (archive.extract). Needs --edit unless --dry-run.
    Extract {
        /// Path of the archive.
        archive: String,
        /// Folder that exists to unpack into; the folder of the archive when omitted.
        output: Option<String>,
        /// Threads that decompress and write; 0 uses every CPU. The files are the same for every count.
        #[arg(long, default_value_t = 0)]
        threads: usize,
        /// Report what would be written, but write nothing.
        #[arg(long)]
        dry_run: bool,
    },
    /// Pack folders, files and archives into an archive (archive.pack), byte for byte as BSArch does. Needs --edit unless --dry-run.
    Pack {
        /// Path of the archive to write.
        archive: String,
        /// Folders, files and archives to pack; later ones win on files with the same name.
        #[arg(required = true)]
        sources: Vec<String>,
        /// Archive format: tes3, tes4, fo3, fnv, tes5, sse, fo4, fo4dds, sf1 or sf1dds.
        #[arg(long)]
        format: String,
        /// Compress the files: zlib, lz4, lz4f, or without a value the default of the format.
        #[arg(short = 'z', long, num_args = 0..=1, default_missing_value = "default")]
        compress: Option<String>,
        /// Split into archives of this many GB (at most 8), 0 for none. Default: BSA formats 2 GB, BA2 formats none.
        #[arg(long, allow_hyphen_values = true)]
        split: Option<i64>,
        /// Pack only the files whose name matches one of these masks (* and ?); repeat for several.
        #[arg(long = "filter")]
        filters: Vec<String>,
        /// Do not let identical files share their data.
        #[arg(long)]
        no_share: bool,
        /// Threads that read and compress; 0 uses every CPU. The archives are the same for every count.
        #[arg(long, default_value_t = 0)]
        threads: usize,
        /// Override the archive flags of a BSA with this hexadecimal value.
        #[arg(long)]
        archive_flags: Option<String>,
        /// Override the file flags of a BSA with this hexadecimal value.
        #[arg(long)]
        file_flags: Option<String>,
        /// Add the sources and report the files that would be packed, but write nothing.
        #[arg(long)]
        dry_run: bool,
    },
    /// Build the reachable information of the loaded files (refs.build_reachable, mniNavBuildReachableClick): the references first when they are missing, then ResetReachable and BuildReachable for every file. Sets the state the --by-not-reachable-status option of `filter apply` reads.
    BuildReachable {
        /// Skip the reference build of the files that have none.
        #[arg(long)]
        no_build_refs: bool,
    },
}

#[derive(Subcommand)]
#[allow(clippy::large_enum_variant)]
enum FilterAction {
    /// Apply a filter to the navigation tree of the loaded files (filter.apply, mniNavFilterApplyClick with the options of TfrmFilterOptions) and report the nodes it left: the two passes, the nodes left and the records taken out of each file. Without any option flag the options are all off but "conflict status inherited by parent" (the dialog's defaults), unless --preset names one.
    Apply {
        /// Plugin of the tree; repeat for several, in load order. Every loaded plugin when omitted (the "* Selected" menu items).
        #[arg(long)]
        file: Vec<String>,
        /// The name of a `[Filter <name>]` preset of xEdit's settings file to read the options from (FilterLoadPreset); any option flag below replaces the whole preset.
        #[arg(long)]
        preset: Option<String>,
        /// Keep the records whose ConflictAll is one of these: caUnknown, caOnlyOne, caNoConflict, caConflictBenign, caOverride, caConflict or caConflictCritical. Comma separated or repeated.
        #[arg(long, value_delimiter = ',')]
        conflict_all: Vec<String>,
        /// Keep the records whose own status (ConflictThis) is one of these: ctUnknown, ctIgnored, ctNotDefined, ctIdenticalToMaster, ctOnlyOne, ctHiddenByModGroup, ctMaster, ctConflictBenign, ctOverride, ctIdenticalToMasterWinsConflict, ctConflictWins or ctConflictLoses.
        #[arg(long, value_delimiter = ',')]
        conflict_this: Vec<String>,
        /// Keep the records that are (or with false are not) injected.
        #[arg(long, num_args = 0..=1, default_missing_value = "true")]
        by_inject_status: Option<bool>,
        /// Keep the records that are (or are not) not reachable; needs `xedit refs build-reachable` first.
        #[arg(long, num_args = 0..=1, default_missing_value = "true")]
        by_not_reachable_status: Option<bool>,
        /// Keep the records whose references (or, with false, whose records themselves) are injected.
        #[arg(long, num_args = 0..=1, default_missing_value = "true")]
        by_references_injected_status: Option<bool>,
        /// Keep the records whose editor ID contains this text.
        #[arg(long)]
        editor_id: Option<String>,
        /// Keep the records with this text in the value of any of their elements.
        #[arg(long)]
        element_value: Option<String>,
        /// Keep the records whose display name contains this text.
        #[arg(long)]
        name: Option<String>,
        /// Keep the records whose base record's editor ID contains this text; eight or nine characters are the FormID of the base record.
        #[arg(long)]
        base_editor_id: Option<String>,
        /// Keep the records whose base record's display name contains this text.
        #[arg(long)]
        base_name: Option<String>,
        /// Keep the references (ACHR, ACRE) whose scale (XSCL) is missing or 1.
        #[arg(long)]
        scaled_actors: bool,
        /// Keep the records with these signatures, such as WEAP,NPC_.
        #[arg(long, value_delimiter = ',')]
        signature: Vec<String>,
        /// Keep the records whose base record has one of these signatures.
        #[arg(long, value_delimiter = ',')]
        base_signature: Vec<String>,
        /// Keep the records that are (or with --persistent=false are not) persistent.
        #[arg(long)]
        by_persistent: bool,
        /// The value of --by-persistent: keep the persistent records (the default), or the temporary ones with --persistent=false.
        #[arg(long, num_args = 0..=1, default_missing_value = "true")]
        persistent: Option<bool>,
        /// Also keep only the records whose persistent reference is unnecessary (IsUnnecessaryPersistent).
        #[arg(long)]
        unnecessary_persistent: bool,
        /// With --unnecessary-persistent: only the references whose master is temporary, and with --is-master the records that are masters.
        #[arg(long)]
        master_is_temporary: bool,
        #[arg(long)]
        is_master: bool,
        /// Also keep only the records whose position changed from the one of their master (IsPositionChanged).
        #[arg(long)]
        persistent_pos_changed: bool,
        /// Keep the deleted records.
        #[arg(long)]
        deleted: bool,
        /// Keep the records that are (or are not) visible when distant.
        #[arg(long, num_args = 0..=1, default_missing_value = "true")]
        by_vwd: Option<bool>,
        /// Keep the records whose base record has (or has not) a visible-when-distant mesh (the _far.nif in a loaded archive).
        #[arg(long, num_args = 0..=1, default_missing_value = "true")]
        by_has_vwd_mesh: Option<bool>,
        /// Keep the records that are (or are not) part of a precombined mesh of their cell (Fallout 4 and 76).
        #[arg(long, num_args = 0..=1, default_missing_value = "true")]
        by_has_precombined_mesh: Option<bool>,
        /// Read the texts of --editor-id, --name, --base-editor-id, --base-name and --element-value as regular expressions (case insensitive, multi-line).
        #[arg(long)]
        regex_comparison: bool,
        /// Take the block and sub-block groups out of the tree, their records take their place.
        #[arg(long)]
        flatten_blocks: bool,
        /// Take the cell children groups out of the tree.
        #[arg(long)]
        flatten_cell_childs: bool,
        /// Move the persistent references of a worldspace into the cell they stand in.
        #[arg(long)]
        assign_pers_wrld_child: bool,
        /// Give every node with children the highest conflict status of its children ("conflict status inherited by parent").
        #[arg(long, num_args = 0..=1, default_missing_value = "true")]
        inherit_conflict_by_parent: Option<bool>,
        /// A record without overrides leaves the tree without a comparison (the xeVeryQuickShowConflicts mode of Filter Conflicts).
        #[arg(long)]
        conflict_only: bool,
        /// A record that is the only version of its FormID leaves the tree ("Filter for only one").
        #[arg(long)]
        only_one: bool,
        /// Leave the game master out of the tree (FilterNoGameMaster).
        #[arg(long)]
        no_game_master: bool,
        /// Activate the valid mod group of this name, so the records it hides are left out; repeat for several.
        #[arg(long = "modgroups")]
        modgroups: Vec<String>,
        /// Activate every valid mod group.
        #[arg(long)]
        all_modgroups: bool,
        /// Activate the valid mod groups of the selection saved in xEdit's settings file.
        #[arg(long)]
        saved_modgroups: bool,
        /// List the records the tree keeps, in the order of the tree, instead of the counts alone.
        #[arg(long)]
        list_records: bool,
    },
    /// Remove the filter from the navigation tree (filter.remove, mniNavFilterRemoveClick): the tree holds the loaded files again.
    Remove,
    /// List the filter presets of xEdit's settings file and the last one used (filter.presets, FilterListPresets).
    Presets,
    /// Write the options as a filter preset of xEdit's settings file (filter.preset.save, FilterSavePreset).
    Save {
        /// The name of the preset; empty for the defaults.
        #[arg(long, default_value = "")]
        name: String,
        /// The options to write as JSON, as `filter apply` takes them.
        #[arg(long)]
        options: Option<String>,
        /// Report the preset and whether it changes the stored one, write nothing.
        #[arg(long)]
        dry_run: bool,
    },
    /// Remove a filter preset from xEdit's settings file (filter.preset.delete, btnFilterDelClick).
    Delete {
        /// The name of the preset; empty for the defaults.
        #[arg(long, default_value = "")]
        name: String,
        /// Report whether the preset is there, write nothing.
        #[arg(long)]
        dry_run: bool,
    },
}

#[derive(Subcommand)]
enum ModgroupsAction {
    /// List the valid mod groups by name with their items and whether the saved selection has them (modgroups.list).
    List {
        /// Also list the invalid mod groups and those of modules that are not loaded.
        #[arg(long)]
        all: bool,
    },
    /// Show one mod group with the state of each item (modgroups.show).
    Show {
        /// Name of the mod group.
        name: String,
        /// The .modgroups file, when several have a group of the name.
        #[arg(long)]
        file: Option<String>,
    },
    /// Choose the active mod groups and save the selection in xEdit's settings file (modgroups.select). Needs --edit unless --dry-run.
    Select {
        /// Name of a valid mod group to select; give several for several.
        names: Vec<String>,
        /// Keep the saved selection too.
        #[arg(long)]
        saved: bool,
        /// Select every valid mod group.
        #[arg(long)]
        all: bool,
        /// Report the selection, but write nothing.
        #[arg(long)]
        dry_run: bool,
    },
    /// Create a mod group in the .modgroups file of one of its modules (modgroups.create). Needs --edit unless --dry-run.
    Create {
        /// Name of the new mod group.
        name: String,
        /// Loaded module of the group, in order (the first a target only, the last a source only); repeat for each.
        #[arg(long = "module")]
        modules: Vec<String>,
        /// Item of the group as a line of a .modgroups file ([flags]file[:crc32,...]), in place of --module; repeat for each.
        #[arg(long = "item")]
        items: Vec<String>,
        /// Give each module's item its current CRC32.
        #[arg(long)]
        include_crcs: bool,
        /// Add the current CRC32 to items whose CRC32s lack it.
        #[arg(long)]
        add_current_crcs: bool,
        /// Module of the group whose .modgroups file gets it; the first loaded one when omitted.
        #[arg(long)]
        file: Option<String>,
        /// Report the file that would be written, but write nothing.
        #[arg(long)]
        dry_run: bool,
    },
    /// Rename a mod group or replace its items (modgroups.edit). Needs --edit unless --dry-run.
    Edit {
        /// Name of the mod group.
        name: String,
        /// The .modgroups file, when several have a group of the name.
        #[arg(long)]
        file: Option<String>,
        /// New name of the group.
        #[arg(long)]
        new_name: Option<String>,
        /// New item as a line of a .modgroups file; repeat for each. The items stay when none is given.
        #[arg(long = "item")]
        items: Vec<String>,
        /// Add the current CRC32 to items whose CRC32s lack it.
        #[arg(long)]
        add_current_crcs: bool,
        /// Report the file that would be written, but write nothing.
        #[arg(long)]
        dry_run: bool,
    },
    /// Delete mod groups (modgroups.delete). Needs --edit unless --dry-run.
    Delete {
        /// Name of a mod group to delete; give several for several.
        names: Vec<String>,
        /// Only the groups of this .modgroups file.
        #[arg(long)]
        file: Option<String>,
        /// Report the files that would be written, but write nothing.
        #[arg(long)]
        dry_run: bool,
    },
    /// Add the current CRC32s of the modules to the items of mod groups (modgroups.update_crcs). Needs --edit unless --dry-run.
    UpdateCrcs {
        /// Do not add CRC32s to items that have none.
        #[arg(long)]
        no_add: bool,
        /// Do not add the current CRC32 to items whose CRC32s lack it.
        #[arg(long)]
        no_update: bool,
        /// Module whose items decide which groups need the update; repeat for several. Every module that lacks a CRC32 when omitted.
        #[arg(long = "module")]
        modules: Vec<String>,
        /// Mod group to update; repeat for several. Every group that needs it when omitted.
        #[arg(long = "modgroup")]
        mod_groups: Vec<String>,
        /// Report what would change, but write nothing.
        #[arg(long)]
        dry_run: bool,
    },
}

#[derive(Subcommand)]
enum FormidsAction {
    /// Change the FormID of a record and update the records that refer to it (formids.change). Needs --edit unless --dry-run.
    Change {
        /// Load order FormID of the record as hexadecimal digits.
        form_id: String,
        /// The new load order FormID; the next free FormID of the file (or of --target-file) when omitted.
        new_form_id: Option<String>,
        /// Plugin whose version of the record changes; the last loaded plugin when omitted.
        #[arg(long)]
        file: Option<String>,
        /// Take the next free FormID of this file: the record's file or one of its masters.
        #[arg(long)]
        target_file: Option<String>,
        /// Change the later overrides of the record too.
        #[arg(long)]
        overrides: bool,
        /// Report the change and the referencing records, but change nothing.
        #[arg(long)]
        dry_run: bool,
    },
    /// Renumber the new records of a plugin (formids.renumber). Needs --edit unless --dry-run.
    Renumber {
        /// Plugin name; the only loaded plugin when omitted.
        #[arg(long)]
        file: Option<String>,
        /// The first object ID in hexadecimal (six digits, three for a light target); the next object ID when omitted.
        #[arg(long)]
        start: Option<String>,
        /// Compact the FormIDs into the ESL range from 000800.
        #[arg(long)]
        compact: bool,
        /// Give the records FormIDs of this master instead.
        #[arg(long)]
        inject_into: Option<String>,
        /// With --inject-into: keep the object IDs the master has free.
        #[arg(long)]
        preserve_object_ids: bool,
        /// With --preserve-object-ids: stop when an object ID can not be kept.
        #[arg(long)]
        all_or_nothing: bool,
        /// Report the plan, but change nothing.
        #[arg(long)]
        dry_run: bool,
    },
}

#[derive(Subcommand)]
enum RecordsAction {
    /// List the records of a plugin.
    List {
        /// Plugin name; the only loaded plugin when omitted.
        #[arg(long)]
        file: Option<String>,
        /// Keep only records with this signature, such as NPC_.
        #[arg(long)]
        signature: Option<String>,
        /// Records to skip.
        #[arg(long, default_value_t = 0)]
        offset: usize,
        /// Records to return at most.
        #[arg(long, default_value_t = 100)]
        limit: usize,
    },
    /// Find records whose editor ID or name contains a text.
    Find {
        /// Plugin name; the only loaded plugin when omitted.
        #[arg(long)]
        file: Option<String>,
        /// Keep only records with this signature.
        #[arg(long)]
        signature: Option<String>,
        /// Text the editor ID contains, compared without case.
        #[arg(long)]
        editor_id: Option<String>,
        /// Text the name contains, compared without case.
        #[arg(long)]
        name: Option<String>,
        /// Records to return at most.
        #[arg(long, default_value_t = 100)]
        limit: usize,
    },
    /// Read a record with all its elements.
    Get {
        /// Load order FormID as hexadecimal digits.
        form_id: String,
        /// Plugin the record is seen from; the last loaded plugin when omitted.
        #[arg(long)]
        file: Option<String>,
        /// Levels of child elements to include; all when omitted.
        #[arg(long)]
        depth: Option<usize>,
    },
    /// Copy a record into a plugin as an override or a new record (records.copy, xEdit's CopyInto). Needs --edit unless --dry-run.
    Copy {
        /// Load order FormID of the record to copy as hexadecimal digits.
        form_id: String,
        /// Plugin to copy into.
        #[arg(long)]
        to: String,
        /// Plugin the record is seen from (that version is copied); the last loaded plugin when omitted.
        #[arg(long)]
        from: Option<String>,
        /// Copy as a new record with a new FormID instead of an override.
        #[arg(long)]
        as_new: bool,
        /// Copy the records of the child group too (the references of a cell, the responses of a topic).
        #[arg(long)]
        deep: bool,
        /// Text put before the editor ID of the copy.
        #[arg(long, default_value = "")]
        prefix: String,
        /// Text put after the editor ID of the copy.
        #[arg(long, default_value = "")]
        suffix: String,
        /// Text removed from the start of the editor ID of the copy.
        #[arg(long, default_value = "")]
        prefix_remove: String,
        /// Text removed from the end of the editor ID of the copy.
        #[arg(long, default_value = "")]
        suffix_remove: String,
        /// Report the masters the copy needs and whether the target has the record, but copy nothing.
        #[arg(long)]
        dry_run: bool,
    },
    /// Remove a record and its child group from a plugin (records.delete, xEdit's Remove). Needs --edit unless --dry-run.
    Delete {
        /// Load order FormID as hexadecimal digits.
        form_id: String,
        /// Plugin whose version of the record is removed; the only loaded plugin when omitted.
        #[arg(long)]
        file: Option<String>,
        /// Report the record, but remove nothing.
        #[arg(long)]
        dry_run: bool,
    },
    /// Copy the records that refer to injected records of a plugin which is not their master into that plugin and remove those references from the originals (records.cleanup_injected, xEdit's "Cleanup injected records"). Needs --edit unless --dry-run.
    CleanupInjected {
        /// Load order FormIDs of the records; every record of the plugin that refers to such injected records when none is given.
        form_ids: Vec<String>,
        /// Plugin whose records are cleaned up; the only loaded plugin when omitted.
        #[arg(long)]
        file: Option<String>,
        /// Report the records and the masters, but change nothing.
        #[arg(long)]
        dry_run: bool,
    },
}

#[derive(Subcommand)]
enum ElementsAction {
    /// Read an element of a record by its path, such as DATA\\Health.
    Get {
        /// Load order FormID of the record as hexadecimal digits.
        form_id: String,
        /// Path of the element inside the record, with \\ between the names.
        path: String,
        /// Plugin the record is seen from; the last loaded plugin when omitted.
        #[arg(long)]
        file: Option<String>,
        /// Levels of child elements to include; all when omitted.
        #[arg(long)]
        depth: Option<usize>,
    },
    /// Set the value of an element by its path (elements.set). Needs --edit unless --dry-run.
    Set {
        /// Load order FormID of the record as hexadecimal digits.
        form_id: String,
        /// Path of the element inside the record, with \\ between the names. A missing last element is added.
        path: String,
        /// The edit value, as xEdit shows it in its editor. Omit with --default.
        value: Option<String>,
        /// Plugin the record is seen from; the last loaded plugin when omitted.
        #[arg(long)]
        file: Option<String>,
        /// Set the value as a native value: a JSON number or boolean.
        #[arg(long)]
        native: bool,
        /// Set the element to the default of its definition.
        #[arg(long)]
        default: bool,
        /// Report the element and what would change, but change nothing.
        #[arg(long)]
        dry_run: bool,
    },
    /// Add a member, an array entry or a child record (elements.add, xEdit's Add). Needs --edit unless --dry-run.
    Add {
        /// Load order FormID of the record as hexadecimal digits.
        form_id: String,
        /// What to add: a member name or signature (EDID, FULL), a child record signature of a cell, topic, worldspace or quest (REFR, INFO, CELL[3,-2], CELL[P]), or an array position.
        name: String,
        /// Path of the container inside the record to add to; the record itself when omitted.
        #[arg(long)]
        path: Option<String>,
        /// Plugin the record is seen from; the last loaded plugin when omitted.
        #[arg(long)]
        file: Option<String>,
        /// Report the container, but add nothing.
        #[arg(long)]
        dry_run: bool,
    },
    /// Remove an element of a record by its path (elements.remove, xEdit's Remove). Needs --edit unless --dry-run.
    Remove {
        /// Load order FormID of the record as hexadecimal digits.
        form_id: String,
        /// Path of the element inside the record, with \\ between the names.
        path: String,
        /// Plugin the record is seen from; the last loaded plugin when omitted.
        #[arg(long)]
        file: Option<String>,
        /// Report the element, but remove nothing.
        #[arg(long)]
        dry_run: bool,
    },
}

#[derive(Subcommand)]
enum PatchAction {
    /// Create a merged patch (patch.merged, xEdit's "Create Merged Patch"): a new .esp in the data folder whose overrides merge the leveled lists, container items, factions, keywords and the other lists that several loaded plugins change, with every loaded file it needs as a master; then save it. Needs --edit unless --dry-run, which lists what would be merged.
    Merged {
        /// File name of the patch; the extension is replaced by .esp, as xEdit does.
        file: String,
        /// List the records and lists the patch would merge; make no plugin.
        #[arg(long)]
        dry_run: bool,
        /// Keep the patch in memory only (for batch and serve, where files.save follows).
        #[arg(long)]
        no_save: bool,
        /// Where to save the patch; the data folder when omitted.
        #[arg(long)]
        output: Option<String>,
        /// Do not move an existing file at the output path to the backup folder.
        #[arg(long)]
        no_backup: bool,
    },
}

#[derive(Subcommand)]
enum MastersAction {
    /// Add loaded plugins as masters (masters.add, AddMastersIfMissing), then sort the masters by load order. Needs --edit unless --dry-run.
    Add {
        /// File names of loaded plugins to add, such as Dawnguard.esm.
        #[arg(required = true)]
        masters: Vec<String>,
        /// Plugin to change; the only loaded plugin when omitted.
        #[arg(long)]
        file: Option<String>,
        /// Keep the masters in the order they are added instead of sorting them by load order.
        #[arg(long)]
        no_sort: bool,
        /// Report the master list the command would leave, but change nothing.
        #[arg(long)]
        dry_run: bool,
    },
    /// Remove the masters no FormID of the plugin points to (masters.clean, CleanMasters). Needs --edit unless --dry-run.
    Clean {
        /// Plugin to change; the only loaded plugin when omitted.
        #[arg(long)]
        file: Option<String>,
        /// Report the master list the command would leave, but change nothing.
        #[arg(long)]
        dry_run: bool,
    },
    /// Sort the masters by load order (masters.sort, SortMasters). Needs --edit unless --dry-run.
    Sort {
        /// Plugin to change; the only loaded plugin when omitted.
        #[arg(long)]
        file: Option<String>,
        /// Report the master list the command would leave, but change nothing.
        #[arg(long)]
        dry_run: bool,
    },
}

/// The command name and parameters of a subcommand.
/// `xedit filter ...`: the request of `filter.*`. The option flags of
/// `filter apply` build the whole options object (the dialog's checkboxes);
/// without one the request names the preset instead, and without either the
/// options are the dialog's defaults.
#[allow(clippy::too_many_arguments)]
fn filter_command(action: FilterAction) -> Result<(String, Value), CommandError> {
    match action {
        FilterAction::Presets => Ok(("filter.presets".to_owned(), json!({}))),
        FilterAction::Remove => Ok(("filter.remove".to_owned(), json!({}))),
        FilterAction::Save { name, options, dry_run } => {
            let options: Option<Value> = match options {
                Some(text) => Some(
                    serde_json::from_str(&text)
                        .map_err(|error| CommandError::new("invalid_params", error.to_string()))?,
                ),
                None => None,
            };
            Ok((
                "filter.preset.save".to_owned(),
                json!({ "name": name, "options": options, "dry_run": dry_run }),
            ))
        }
        FilterAction::Delete { name, dry_run } => Ok((
            "filter.preset.delete".to_owned(),
            json!({ "name": name, "dry_run": dry_run }),
        )),
        FilterAction::Apply {
            file,
            preset,
            conflict_all,
            conflict_this,
            by_inject_status,
            by_not_reachable_status,
            by_references_injected_status,
            editor_id,
            element_value,
            name,
            base_editor_id,
            base_name,
            scaled_actors,
            signature,
            base_signature,
            by_persistent,
            persistent,
            unnecessary_persistent,
            master_is_temporary,
            is_master,
            persistent_pos_changed,
            deleted,
            by_vwd,
            by_has_vwd_mesh,
            by_has_precombined_mesh,
            regex_comparison,
            flatten_blocks,
            flatten_cell_childs,
            assign_pers_wrld_child,
            inherit_conflict_by_parent,
            conflict_only,
            only_one,
            no_game_master,
            modgroups,
            all_modgroups,
            saved_modgroups,
            list_records,
        } => {
            let any_option = !conflict_all.is_empty()
                || !conflict_this.is_empty()
                || by_inject_status.is_some()
                || by_not_reachable_status.is_some()
                || by_references_injected_status.is_some()
                || editor_id.is_some()
                || element_value.is_some()
                || name.is_some()
                || base_editor_id.is_some()
                || base_name.is_some()
                || scaled_actors
                || !signature.is_empty()
                || !base_signature.is_empty()
                || by_persistent
                || persistent.is_some()
                || unnecessary_persistent
                || master_is_temporary
                || is_master
                || persistent_pos_changed
                || deleted
                || by_vwd.is_some()
                || by_has_vwd_mesh.is_some()
                || by_has_precombined_mesh.is_some()
                || regex_comparison
                || flatten_blocks
                || flatten_cell_childs
                || assign_pers_wrld_child
                || inherit_conflict_by_parent.is_some();
            let options = any_option.then(|| {
                json!({
                    "conflict_all": (!conflict_all.is_empty()).then(|| conflict_all.clone()),
                    "conflict_this": (!conflict_this.is_empty()).then(|| conflict_this.clone()),
                    "by_inject_status": by_inject_status,
                    "by_not_reachable_status": by_not_reachable_status,
                    "by_references_injected_status": by_references_injected_status,
                    "by_editor_id": editor_id,
                    "by_element_value": element_value,
                    "by_name": name,
                    "by_base_editor_id": base_editor_id,
                    "by_base_name": base_name,
                    "scaled_actors": scaled_actors,
                    "by_signature": (!signature.is_empty()).then(|| signature.join(",")),
                    "by_base_signature": (!base_signature.is_empty()).then(|| base_signature.join(",")),
                    "by_persistent": by_persistent || persistent.is_some(),
                    "persistent": persistent.unwrap_or(false),
                    "unnecessary_persistent": unnecessary_persistent,
                    "master_is_temporary": master_is_temporary,
                    "is_master": is_master,
                    "persistent_pos_changed": persistent_pos_changed,
                    "deleted": deleted,
                    "by_vwd": by_vwd,
                    "by_has_vwd_mesh": by_has_vwd_mesh,
                    "by_has_precombined_mesh": by_has_precombined_mesh,
                    "regex_comparison": regex_comparison,
                    "flatten_blocks": flatten_blocks,
                    "flatten_cell_childs": flatten_cell_childs,
                    "assign_pers_wrld_child": assign_pers_wrld_child,
                    "inherit_conflict_by_parent": inherit_conflict_by_parent.unwrap_or(true),
                })
            });
            Ok((
                "filter.apply".to_owned(),
                json!({
                    "files": file,
                    "options": options,
                    "preset": preset,
                    "mod_groups": modgroups,
                    "all_mod_groups": all_modgroups,
                    "saved_mod_groups": saved_modgroups,
                    "list_records": list_records,
                    // The flags the dialog has no checkbox for.
                    "conflict_only": conflict_only,
                    "only_one": only_one,
                    "no_game_master": no_game_master,
                }),
            ))
        }
    }
}

fn command_of(action: Action) -> Result<(String, Value), CommandError> {
    Ok(match action {
        Action::Dump { .. }
        | Action::Saves { .. }
        | Action::Schema
        | Action::Batch { .. }
        | Action::Serve { .. }
        | Action::Mcp => {
            unreachable!("handled before")
        }
        Action::Session {
            action: SessionAction::Info,
        } => ("session.info".to_owned(), json!({})),
        Action::Files {
            action: FilesAction::List,
        } => ("files.list".to_owned(), json!({})),
        Action::Files {
            action:
                FilesAction::New {
                    file,
                    light,
                    medium,
                    dry_run,
                },
        } => (
            "files.new".to_owned(),
            json!({ "file": file, "light": light, "medium": medium, "dry_run": dry_run }),
        ),
        Action::Files {
            action:
                FilesAction::Flags {
                    file,
                    esm,
                    light,
                    medium,
                    update,
                    blueprint,
                    localized,
                    dry_run,
                },
        } => (
            "files.flags".to_owned(),
            json!({
                "file": file, "esm": esm, "light": light, "medium": medium, "update": update,
                "blueprint": blueprint, "localized": localized, "dry_run": dry_run
            }),
        ),
        Action::Localization { action } => match action {
            LocalizationAction::Files { file, no_load } => (
                "localization.files".to_owned(),
                json!({ "file": file, "load": !no_load }),
            ),
            LocalizationAction::Strings { table, offset, limit } => (
                "localization.strings".to_owned(),
                json!({ "table": table, "offset": offset, "limit": limit }),
            ),
            LocalizationAction::Get {
                table,
                id,
                form_id,
                path,
                file,
            } => (
                "localization.get".to_owned(),
                json!({ "table": table, "id": id, "form_id": form_id, "path": path, "file": file }),
            ),
            LocalizationAction::Set {
                text,
                table,
                id,
                form_id,
                path,
                file,
                editor_text,
                dry_run,
            } => (
                "localization.set".to_owned(),
                json!({
                    "text": text, "table": table, "id": id, "form_id": form_id, "path": path, "file": file,
                    "editor_text": editor_text, "dry_run": dry_run
                }),
            ),
            LocalizationAction::Export { table, output, dry_run } => (
                "localization.export".to_owned(),
                json!({ "table": table, "output": output, "dry_run": dry_run }),
            ),
            LocalizationAction::Languages => ("localization.languages".to_owned(), json!({})),
            LocalizationAction::Language { language, discard } => (
                "localization.language".to_owned(),
                json!({ "language": language, "discard": discard }),
            ),
            LocalizationAction::Localize {
                file,
                translate_from,
                translate_to,
                dry_run,
            } => (
                "localization.localize".to_owned(),
                json!({
                    "file": file, "translate_from": translate_from, "translate_to": translate_to, "dry_run": dry_run
                }),
            ),
            LocalizationAction::Delocalize { file, dry_run } => (
                "localization.delocalize".to_owned(),
                json!({ "file": file, "dry_run": dry_run }),
            ),
        },
        Action::Refs { action } => match action {
            RefsAction::Get {
                form_id,
                file,
                offset,
                limit,
                filter_name,
                filter_signature,
                filter_file,
                filter_or,
            } => (
                "refs.get".to_owned(),
                json!({
                    "form_id": form_id, "file": file, "offset": offset, "limit": limit,
                    "filter_name": filter_name, "filter_signature": filter_signature,
                    "filter_file": filter_file, "filter_or": filter_or
                }),
            ),
            RefsAction::Dump => unreachable!("handled before"),
            RefsAction::Build { file, only_load } => {
                ("refs.build".to_owned(), json!({ "file": file, "only_load": only_load }))
            }
            RefsAction::BuildReachable { no_build_refs } => (
                "refs.build_reachable".to_owned(),
                json!({ "build_refs": !no_build_refs }),
        Action::Archive { action } => match action {
            ArchiveAction::List {
                archive,
                files,
                folder,
                offset,
                limit,
            } => (
                "archive.list".to_owned(),
                json!({ "archive": archive, "files": files, "folder": folder, "offset": offset, "limit": limit }),
            ),
            ArchiveAction::Extract {
                archive,
                output,
                threads,
                dry_run,
            } => (
                "archive.extract".to_owned(),
                json!({ "archive": archive, "output": output, "threads": threads, "dry_run": dry_run }),
            ),
            ArchiveAction::Pack {
                archive,
                sources,
                format,
                compress,
                split,
                filters,
                no_share,
                threads,
                archive_flags,
                file_flags,
                dry_run,
            } => (
                "archive.pack".to_owned(),
                json!({
                    "archive": archive, "sources": sources, "format": format, "compress": compress,
                    "split": split, "filters": filters, "share": !no_share, "threads": threads,
                    "archive_flags": archive_flags, "file_flags": file_flags, "dry_run": dry_run
                }),
            ),
        },
        Action::Filter { action } => filter_command(action)?,
        Action::Modgroups { action } => match action {
            ModgroupsAction::List { all } => ("modgroups.list".to_owned(), json!({ "all": all })),
            ModgroupsAction::Show { name, file } => {
                ("modgroups.show".to_owned(), json!({ "name": name, "file": file }))
            }
            ModgroupsAction::Select {
                names,
                saved,
                all,
                dry_run,
            } => (
                "modgroups.select".to_owned(),
                json!({ "names": names, "saved": saved, "all": all, "dry_run": dry_run }),
            ),
            ModgroupsAction::Create {
                name,
                modules,
                items,
                include_crcs,
                add_current_crcs,
                file,
                dry_run,
            } => (
                "modgroups.create".to_owned(),
                json!({
                    "name": name, "modules": modules, "items": items, "include_crcs": include_crcs,
                    "add_current_crcs": add_current_crcs, "file": file, "dry_run": dry_run
                }),
            ),
            ModgroupsAction::Edit {
                name,
                file,
                new_name,
                items,
                add_current_crcs,
                dry_run,
            } => (
                "modgroups.edit".to_owned(),
                json!({
                    "name": name, "file": file, "new_name": new_name,
                    "items": if items.is_empty() { None } else { Some(items) },
                    "add_current_crcs": add_current_crcs, "dry_run": dry_run
                }),
            ),
            ModgroupsAction::Delete { names, file, dry_run } => (
                "modgroups.delete".to_owned(),
                json!({ "names": names, "file": file, "dry_run": dry_run }),
            ),
            ModgroupsAction::UpdateCrcs {
                no_add,
                no_update,
                modules,
                mod_groups,
                dry_run,
            } => (
                "modgroups.update_crcs".to_owned(),
                json!({
                    "add": !no_add, "update": !no_update, "modules": modules,
                    "mod_groups": mod_groups, "dry_run": dry_run
                }),
            ),
        },
        Action::Formids { action } => match action {
            FormidsAction::Change {
                form_id,
                new_form_id,
                file,
                target_file,
                overrides,
                dry_run,
            } => (
                "formids.change".to_owned(),
                json!({
                    "form_id": form_id, "new_form_id": new_form_id, "file": file,
                    "target_file": target_file, "overrides": overrides, "dry_run": dry_run
                }),
            ),
            FormidsAction::Renumber {
                file,
                start,
                compact,
                inject_into,
                preserve_object_ids,
                all_or_nothing,
                dry_run,
            } => (
                "formids.renumber".to_owned(),
                json!({
                    "file": file, "start": start, "compact": compact, "inject_into": inject_into,
                    "preserve_object_ids": preserve_object_ids, "all_or_nothing": all_or_nothing, "dry_run": dry_run
                }),
            ),
        },
        Action::Records { action } => match action {
            RecordsAction::List {
                file,
                signature,
                offset,
                limit,
            } => (
                "records.list".to_owned(),
                json!({ "file": file, "signature": signature, "offset": offset, "limit": limit }),
            ),
            RecordsAction::Find {
                file,
                signature,
                editor_id,
                name,
                limit,
            } => (
                "records.find".to_owned(),
                json!({ "file": file, "signature": signature, "editor_id": editor_id, "name": name, "limit": limit }),
            ),
            RecordsAction::Get { form_id, file, depth } => (
                "records.get".to_owned(),
                json!({ "form_id": form_id, "file": file, "depth": depth }),
            ),
            RecordsAction::Copy {
                form_id,
                to,
                from,
                as_new,
                deep,
                prefix,
                suffix,
                prefix_remove,
                suffix_remove,
                dry_run,
            } => (
                "records.copy".to_owned(),
                json!({ "form_id": form_id, "to": to, "from": from, "as_new": as_new, "deep": deep,
                        "prefix": prefix, "suffix": suffix, "prefix_remove": prefix_remove,
                        "suffix_remove": suffix_remove, "dry_run": dry_run }),
            ),
            RecordsAction::Delete { form_id, file, dry_run } => (
                "records.delete".to_owned(),
                json!({ "form_id": form_id, "file": file, "dry_run": dry_run }),
            ),
            RecordsAction::CleanupInjected {
                form_ids,
                file,
                dry_run,
            } => (
                "records.cleanup_injected".to_owned(),
                json!({ "form_ids": form_ids, "file": file, "dry_run": dry_run }),
            ),
        },
        Action::Elements { action } => match action {
            ElementsAction::Get {
                form_id,
                path,
                file,
                depth,
            } => (
                "elements.get".to_owned(),
                json!({ "form_id": form_id, "path": path, "file": file, "depth": depth }),
            ),
            ElementsAction::Set {
                form_id,
                path,
                value,
                file,
                native,
                default,
                dry_run,
            } => {
                let value = match (default, value) {
                    (true, _) | (false, None) => Value::Null,
                    (false, Some(text)) if native => serde_json::from_str(&text).map_err(|e| {
                        CommandError::new(
                            "invalid_params",
                            format!("--native needs a JSON number or boolean: {e}"),
                        )
                    })?,
                    (false, Some(text)) => Value::String(text),
                };
                (
                    "elements.set".to_owned(),
                    json!({ "form_id": form_id, "path": path, "file": file, "value": value, "dry_run": dry_run }),
                )
            }
            ElementsAction::Add {
                form_id,
                name,
                path,
                file,
                dry_run,
            } => (
                "elements.add".to_owned(),
                json!({ "form_id": form_id, "name": name, "path": path, "file": file, "dry_run": dry_run }),
            ),
            ElementsAction::Remove {
                form_id,
                path,
                file,
                dry_run,
            } => (
                "elements.remove".to_owned(),
                json!({ "form_id": form_id, "path": path, "file": file, "dry_run": dry_run }),
            ),
        },
        Action::Patch { action } => match action {
            PatchAction::Merged {
                file,
                dry_run,
                no_save,
                output,
                no_backup,
            } => (
                "patch.merged".to_owned(),
                json!({
                    "file": file, "dry_run": dry_run, "save": !no_save, "output": output, "backup": !no_backup
                }),
            ),
        },
        Action::Tool { action } => match action {
            ToolAction::Modes => ("tool.modes".to_owned(), json!({})),
            ToolAction::Run {
                mode,
                files,
                dry_run,
                output,
                no_backup,
                format,
                seq_path,
            } => (
                "tool.run".to_owned(),
                json!({
                    "mode": mode, "files": files, "dry_run": dry_run, "output": output,
                    "backup": !no_backup, "format": format, "seq_path": seq_path
                }),
            ),
        },
        Action::Masters { action } => match action {
            MastersAction::Add {
                masters,
                file,
                no_sort,
                dry_run,
            } => (
                "masters.add".to_owned(),
                json!({ "masters": masters, "file": file, "sort": !no_sort, "dry_run": dry_run }),
            ),
            MastersAction::Clean { file, dry_run } => {
                ("masters.clean".to_owned(), json!({ "file": file, "dry_run": dry_run }))
            }
            MastersAction::Sort { file, dry_run } => {
                ("masters.sort".to_owned(), json!({ "file": file, "dry_run": dry_run }))
            }
        },
        Action::Clean {
            file,
            itm,
            udr,
            quick,
            dry_run,
            output,
            no_backup,
        } => (
            "files.clean".to_owned(),
            json!({
                "file": file, "itm": itm, "udr": udr, "quick": quick, "dry_run": dry_run,
                "output": output, "backup": !no_backup
            }),
        ),
        Action::Check {
            file,
            record,
            record_file,
            last,
        } => (
            "files.check".to_owned(),
            json!({ "files": file, "records": record, "record_file": record_file, "last": last }),
        ),
        Action::Save {
            file,
            output,
            dry_run,
            no_backup,
        } => (
            "files.save".to_owned(),
            json!({ "file": file, "output": output, "dry_run": dry_run, "backup": !no_backup }),
        ),
        Action::Conflicts {
            file,
            signature,
            min_conflict_all,
            conflict_this,
            include_single,
            master_and_leafs,
            quick_show_conflicts,
            modgroups,
            all_modgroups,
            saved_modgroups,
            offset,
            limit,
        } => (
            "conflicts.list".to_owned(),
            json!({
                "files": file, "signatures": signature, "min_conflict_all": min_conflict_all,
                "conflict_this": conflict_this, "include_single": include_single,
                "master_and_leafs": master_and_leafs, "quick_show_conflicts": quick_show_conflicts,
                "mod_groups": modgroups, "all_mod_groups": all_modgroups, "saved_mod_groups": saved_modgroups,
                "offset": offset, "limit": limit
            }),
        ),
        Action::Compare {
            form_id,
            file,
            master_and_leafs,
            hide_no_conflict,
            include_hidden,
            modgroups,
            all_modgroups,
            saved_modgroups,
            view_filter_name,
            view_filter_value,
            view_filter_or,
            keep_children,
            keep_siblings,
            keep_parents_siblings,
        } => (
            "records.compare".to_owned(),
            json!({
                "form_id": form_id, "file": file, "master_and_leafs": master_and_leafs,
                "hide_no_conflict": hide_no_conflict, "include_hidden": include_hidden,
                "mod_groups": modgroups, "all_mod_groups": all_modgroups, "saved_mod_groups": saved_modgroups,
                "view_filter_name": view_filter_name, "view_filter_value": view_filter_value,
                "view_filter_or": view_filter_or, "keep_children": keep_children,
                "keep_siblings": keep_siblings, "keep_parents_siblings": keep_parents_siblings
            }),
        ),
        Action::Assets { action } => match action {
            AssetsAction::Dump {
                file,
                archive_path,
                kind,
                format,
                decimals,
                euler,
            } => (
                "assets.dump".to_owned(),
                json!({ "file": file, "archive_path": archive_path, "kind": kind, "format": format, "decimals": decimals, "euler": euler }),
            ),
            AssetsAction::Blocks {
                file,
                archive_path,
                kind,
            } => (
                "assets.blocks".to_owned(),
                json!({ "file": file, "archive_path": archive_path, "kind": kind }),
            ),
            AssetsAction::Types => ("assets.types".to_owned(), json!({})),
            AssetsAction::Save {
                file,
                output,
                archive_path,
                kind,
                dry_run,
            } => (
                "assets.save".to_owned(),
                json!({ "file": file, "output": output, "archive_path": archive_path, "kind": kind, "dry_run": dry_run }),
            ),
            AssetsAction::Set {
                file,
                path,
                value,
                output,
                block,
                archive_path,
                kind,
                dry_run,
            } => (
                "assets.set".to_owned(),
                json!({
                    "file": file,
                    "edits": [{ "block": block, "path": path, "value": value }],
                    "output": output,
                    "archive_path": archive_path,
                    "kind": kind,
                    "dry_run": dry_run,
                }),
            ),
            AssetsAction::FromJson {
                file,
                output,
                kind,
                dry_run,
            } => (
                "assets.from-json".to_owned(),
                json!({ "file": file, "output": output, "kind": kind, "dry_run": dry_run }),
            ),
        },
        Action::Sniff { action } => match action {
            SniffAction::List => ("sniff.list".to_owned(), json!({})),
            SniffAction::Run {
                operation,
                input,
                output,
                settings,
                set,
                path_contains,
                no_subdir,
                skip_on_errors,
                copy_all,
                threads,
                log,
                dry_run,
            } => {
                let mut options = serde_json::Map::new();
                for pair in set {
                    let (name, value) = pair.split_once('=').ok_or_else(|| {
                        CommandError::new("invalid_params", format!("--set {pair}: expected NAME=VALUE"))
                    })?;
                    options.insert(name.to_owned(), json!(value));
                }
                (
                    "sniff.run".to_owned(),
                    json!({
                        "operation": operation, "input": input, "output": output, "settings": settings,
                        "options": options, "path_contains": path_contains, "subdir": !no_subdir,
                        "skip_on_errors": skip_on_errors, "copy_all": copy_all, "threads": threads,
                        "log": log, "dry_run": dry_run
                    }),
                )
            }
        },
        Action::Lodgen {
            worldspaces,
            set,
            settings,
            output,
            scripts,
            temp,
            data,
            game_ini,
            seed,
            split_trees,
            dry_run,
        } => {
            let mut options = serde_json::Map::new();
            for pair in set {
                let (name, value) = pair
                    .split_once('=')
                    .ok_or_else(|| CommandError::new("invalid_params", format!("--set {pair}: expected NAME=VALUE")))?;
                options.insert(name.to_owned(), json!(value));
            }
            (
                "lodgen.generate".to_owned(),
                json!({
                    "worldspaces": worldspaces, "options": options, "settings": settings, "output": output,
                    "scripts": scripts, "temp": temp, "data": data, "game_ini": game_ini, "seed": seed, "split_trees": split_trees,
                    "dry_run": dry_run
                }),
            )
        }
        Action::Call { name, params } => {
            let params = serde_json::from_str(&params)
                .map_err(|e| CommandError::new("invalid_params", format!("--params is not valid JSON: {e}")))?;
            (name, params)
        }
    })
}

fn run(game: Option<String>, load: Vec<String>, edit: bool, action: Action) -> Result<Value, CommandError> {
    let registry = Registry::standard();
    if let Action::Schema = action {
        return Ok(registry.catalogue());
    }
    let batch = match &action {
        Action::Batch { file, keep_going } => {
            let text = if file == "-" {
                let mut text = String::new();
                std::io::Read::read_to_string(&mut std::io::stdin(), &mut text)
                    .map_err(|e| CommandError::new("io", e.to_string()))?;
                text
            } else {
                std::fs::read_to_string(file).map_err(|e| CommandError::new("io", format!("{file}: {e}")))?
            };
            Some((parse_batch(&text)?, *keep_going))
        }
        _ => None,
    };
    let mut session = engine::open_session(game.as_deref(), &load, edit)?;
    if let Some((commands, keep_going)) = batch {
        // Every command runs in the one session, so an edit is visible to
        // the commands after it and a save at the end writes it.
        return Ok(registry.run_batch(&mut session, commands, keep_going));
    }
    let (name, params) = command_of(action)?;
    registry.call(&mut session, &name, params)
}

/// Runs a dump on a thread with a large stack, because the element tree
/// resolves deeply through the definitions. The progress messages go to
/// stderr like the log of xDump.
fn run_dump(dump: impl FnOnce(&mut dyn std::io::Write) -> Result<(), String> + Send + 'static) -> ExitCode {
    xedit_session::dump::log_progress_to_stderr();
    let worker = std::thread::Builder::new()
        .stack_size(xedit_core::threads::STACK_SIZE)
        .spawn(move || {
            let stdout = std::io::stdout();
            let mut out = std::io::BufWriter::with_capacity(1 << 20, stdout.lock());
            dump(&mut out)
        })
        .expect("the dump thread");
    let result = worker
        .join()
        .unwrap_or_else(|_| Err("the dump thread panicked".to_owned()));
    if let Err(error) = result {
        eprintln!("error: {error}");
        return ExitCode::FAILURE;
    }
    ExitCode::SUCCESS
}

/// `xedit serve`: the session stays loaded, the commands are JSON-RPC methods.
fn run_serve(
    game: Option<String>,
    load: Vec<String>,
    edit: bool,
    pipe: Option<String>,
) -> Result<(), Box<dyn std::error::Error>> {
    let engine = engine::Engine::open(game.as_deref(), &load, edit)?;
    let mut handler = serve::ServeHandler::new(engine);
    match pipe {
        None => rpc::serve_lines(
            BufReader::new(std::io::stdin().lock()),
            std::io::stdout().lock(),
            &mut handler,
        )?,
        Some(name) => pipe::listen(&name, |file| {
            rpc::serve_lines(BufReader::new(&file), &file, &mut handler)?;
            Ok(!handler.shutdown_requested())
        })?,
    }
    Ok(())
}

/// `xedit mcp`: the registry as MCP tools on stdio.
fn run_mcp(game: Option<String>, load: Vec<String>, edit: bool) -> Result<(), Box<dyn std::error::Error>> {
    if game.is_none() && !load.is_empty() {
        return Err(CommandError::new("invalid_params", "--load needs --game").into());
    }
    let mut handler = mcp::McpHandler::new(engine::Engine::lazy(game, load, edit));
    rpc::serve_lines(
        BufReader::new(std::io::stdin().lock()),
        std::io::stdout().lock(),
        &mut handler,
    )?;
    Ok(())
}

/// The command line xEdit's own switches were given, or a failure to read
/// it. `xeInit.pas` reads the game, the tool mode and the settings from the
/// switches and the executable name; a command line that holds none of them
/// is the CLI's own syntax and goes to clap.
fn legacy_command_line() -> Result<Option<xedit_session::tool_modes::LegacyRun>, String> {
    let params: Vec<String> = std::env::args().skip(1).collect();
    let exe_name = std::env::current_exe()
        .ok()
        .and_then(|path| path.file_name().map(|name| name.to_string_lossy().into_owned()))
        .unwrap_or_else(|| "xedit.exe".to_owned());
    xedit_session::tool_modes::parse_legacy(&params, &exe_name)
}

/// Runs a tool mode of a legacy command line (`-quickautoclean -autoexit
/// -autoload <plugin>`, the switches a mod manager passes): the plugins load
/// as the mode loads them, the mode runs, and the message log of xEdit goes
/// to stdout. The exit code is the count of the check modes, at most 127, as
/// upstream exits.
fn run_legacy(run: xedit_session::tool_modes::LegacyRun) -> ExitCode {
    xedit_session::dump::log_progress_to_stderr();
    if let Some(path) = &run.data_path {
        xedit_core::interface::globals::set_data_path(path);
    }
    for switch in &run.ignored_switches {
        eprintln!("warning: {switch} is accepted and not acted on by this build");
    }
    let Some(game) = run.game_tag.as_deref() else {
        eprintln!(
            "error: invalid_params: the game is not known; pass -SSE, -FO4 and the like, or name the program after its game"
        );
        return ExitCode::FAILURE;
    };
    xedit_session::tool_modes::set_on_load(run.switches);
    // A tool mode is an edit mode: `wbEditAllowed` is on and the mode's
    // settings decide what the load does.
    let mut session = match engine::open_session(Some(game), &run.plugins, true) {
        Ok(session) => session,
        Err(error) => {
            eprintln!("error: {error}");
            return ExitCode::FAILURE;
        }
    };
    let registry = Registry::standard();
    let params = json!({
        "mode": run.mode,
        "files": run.modules,
        "dry_run": false,
        "output": run.output,
        "backup": true,
        "format": run.format,
    });
    let result = registry.call(&mut session, "tool.run", params);
    let result = match result {
        Ok(result) => result,
        Err(error) => {
            eprintln!("error: {error}");
            return ExitCode::FAILURE;
        }
    };
    let mut log = String::new();
    for line in result["messages"].as_array().into_iter().flatten() {
        let line = line.as_str().unwrap_or_default();
        println!("{line}");
        log.push_str(line);
        log.push_str("\r\n");
    }
    // `-R:<file>`: xEdit's message log as a file.
    if let Some(path) = &run.log_file
        && let Err(error) = std::fs::write(path, &log)
    {
        eprintln!("warning: -R:{path}: {error}");
    }
    let code = result["exit_code"].as_u64().unwrap_or(0) as u8;
    ExitCode::from(code)
}

fn main() -> ExitCode {
    // `wbCommandLine` first: the switches of xEdit itself (`-quickautoclean`,
    // `-setesm`, `-D:`, `-SSE`, ...) are not the CLI's syntax and are read as
    // xEdit reads them.
    match legacy_command_line() {
        Ok(Some(run)) => return run_legacy(run),
        Ok(None) => {}
        Err(error) => {
            eprintln!("error: {error}");
            return ExitCode::FAILURE;
        }
    }
    let cli = Cli::parse();
    xedit_core::threads::set_threads(cli.threads.unwrap_or(0));
    xedit_session::refs::set_cache_options(
        cli.cache_path.as_deref(),
        cli.dont_cache,
        cli.dont_cache_load,
        cli.dont_cache_save,
    );
    // `-FillPNAM`, and the settings `-quickautoclean` gives the load.
    xedit_session::commands::set_fill_pnam_on_load(cli.fill_pnam);
    xedit_session::commands::set_language_on_load(cli.language.as_deref());
    xedit_session::commands::set_translate_on_load(cli.translate);
    xedit_session::commands::set_quick_clean_on_load(matches!(cli.action, Action::Clean { quick: true, .. }));
    // The settings `-CheckForErrors` gives the load.
    let check = matches!(cli.action, Action::Check { .. });
    xedit_session::commands::set_check_on_load(check);
    // `xeInit.pas`: the tool mode decides what the load does, so it is set
    // before the plugins load.
    if let Action::Tool {
        action: ToolAction::Run { mode, .. },
    } = &cli.action
        && let Some(tool_mode) = xedit_session::tool_modes::mode_of_name(mode)
    {
        xedit_session::tool_modes::set_on_load(xedit_session::tool_modes::LoadSwitches {
            tool_mode,
            ..Default::default()
        });
    }
    xedit_session::modgroups::set_file_options(cli.modgroups_file.as_deref(), cli.settings.as_deref());
    if matches!(cli.action, Action::Serve { .. } | Action::Mcp) {
        // stdout carries the protocol; the progress of a load goes to stderr.
        xedit_session::dump::log_progress_to_stderr();
        let outcome = match cli.action {
            Action::Serve { pipe } => run_serve(cli.game, cli.load, cli.edit, pipe),
            _ => run_mcp(cli.game, cli.load, cli.edit),
        };
        return match outcome {
            Ok(()) => ExitCode::SUCCESS,
            Err(error) => {
                eprintln!("error: {error}");
                ExitCode::FAILURE
            }
        };
    }
    if let Action::Dump { game, file, check } = cli.action {
        return run_dump(move |out| {
            let mode = xedit_session::dump::setup_game(&game)?;
            if check {
                xedit_session::dump::check_file(&file, mode, out)
            } else {
                xedit_session::dump::dump_file(&file, mode, out)
            }
        });
    }
    if let Action::Saves {
        action: SavesAction::Dump { game, data, file },
    } = cli.action
    {
        return run_dump(move |out| {
            let mode = xedit_session::dump::setup_saves(&game, &file)?;
            xedit_session::dump::dump_save(&file, &data, mode, out)
        });
    }
    // The progress messages of a load and a save go to stderr like the log
    // of xEdit; the result goes to stdout.
    xedit_session::dump::log_progress_to_stderr();
    if let Action::Refs {
        action: RefsAction::Dump,
    } = cli.action
    {
        let (game, load, edit) = (cli.game, cli.load, cli.edit);
        return run_dump(move |out| {
            let mut session = engine::open_session(game.as_deref(), &load, edit).map_err(|error| error.to_string())?;
            xedit_session::refs::write_index(&mut session, out).map_err(|error| error.to_string())
        });
    }
    let outcome = run(cli.game, cli.load, cli.edit, cli.action);
    match (&outcome, cli.json) {
        (Ok(result), true) => println!("{}", json!({ "ok": true, "result": result })),
        // `xedit check` prints the message log of "Check for Errors".
        (Ok(result), false) if check => {
            for line in result["messages"].as_array().into_iter().flatten() {
                println!("{}", line.as_str().unwrap_or_default());
            }
        }
        (Ok(result), false) => println!("{result:#}"),
        (Err(error), true) => println!("{}", json!({ "ok": false, "error": error })),
        (Err(error), false) => eprintln!("error: {error}"),
    }
    if outcome.is_ok() {
        ExitCode::SUCCESS
    } else {
        ExitCode::FAILURE
    }
}
