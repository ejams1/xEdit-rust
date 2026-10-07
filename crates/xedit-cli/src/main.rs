// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use std::process::ExitCode;

use clap::{Parser, Subcommand};
use serde_json::{Value, json};
use xedit_session::{CommandError, Registry, Session};

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

/// The command name and parameters of a subcommand.
fn command_of(action: Action) -> Result<(String, Value), CommandError> {
    Ok(match action {
        Action::Dump { .. } | Action::Saves { .. } | Action::Schema | Action::Batch { .. } => {
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

/// One command of a batch: `{"command": name, "params": {...}}`.
struct BatchCommand {
    command: String,
    params: Value,
}

/// The commands of a batch, from its JSON text.
fn parse_batch(text: &str) -> Result<Vec<BatchCommand>, CommandError> {
    let invalid = |message: String| CommandError::new("invalid_params", message);
    let value: Value = serde_json::from_str(text).map_err(|e| invalid(format!("the batch is not valid JSON: {e}")))?;
    let Value::Array(items) = value else {
        return Err(invalid("the batch must be a JSON array of commands".to_owned()));
    };
    items
        .into_iter()
        .enumerate()
        .map(|(index, item)| {
            let command = item["command"]
                .as_str()
                .ok_or_else(|| invalid(format!("command {index} has no \"command\" name")))?
                .to_owned();
            let params = match &item["params"] {
                Value::Null => json!({}),
                params @ Value::Object(_) => params.clone(),
                _ => return Err(invalid(format!("command {index}: \"params\" must be an object"))),
            };
            Ok(BatchCommand { command, params })
        })
        .collect()
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
    let mut session = match game {
        Some(game) => Session::load(&game, &load).map_err(|message| CommandError::new("load_failed", message))?,
        None if load.is_empty() => Session::default(),
        None => return Err(CommandError::new("invalid_params", "--load needs --game")),
    };
    session.allow_edit(edit);
    if let Some((commands, keep_going)) = batch {
        // Every command runs in the one session, so an edit is visible to
        // the commands after it and a save at the end writes it.
        let mut results = Vec::with_capacity(commands.len());
        for command in commands {
            match registry.call(&mut session, &command.command, command.params) {
                Ok(result) => results.push(json!({ "ok": true, "command": command.command, "result": result })),
                Err(error) => {
                    results.push(json!({ "ok": false, "command": command.command, "error": error }));
                    if !keep_going {
                        break;
                    }
                }
            }
        }
        return Ok(Value::Array(results));
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

fn main() -> ExitCode {
    let cli = Cli::parse();
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
