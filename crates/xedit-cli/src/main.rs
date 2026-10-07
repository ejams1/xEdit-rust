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
    /// Save files.
    Saves {
        #[command(subcommand)]
        action: SavesAction,
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
    /// Write the element tree of a plugin as xDump prints it.
    Dump {
        /// Game of the plugin, as for --game.
        #[arg(long)]
        game: String,
        /// Path of the plugin.
        file: String,
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
        Action::Save {
            file,
            output,
            dry_run,
            no_backup,
        } => (
            "files.save".to_owned(),
            json!({ "file": file, "output": output, "dry_run": dry_run, "backup": !no_backup }),
        ),
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
        .stack_size(1 << 30)
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

fn main() -> ExitCode {
    let cli = Cli::parse();
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
    if let Action::Dump { game, file } = cli.action {
        return run_dump(move |out| {
            let mode = xedit_session::dump::setup_game(&game)?;
            xedit_session::dump::dump_file(&file, mode, out)
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
    let outcome = run(cli.game, cli.load, cli.edit, cli.action);
    match (&outcome, cli.json) {
        (Ok(result), true) => println!("{}", json!({ "ok": true, "result": result })),
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
