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
}

/// The command name and parameters of a subcommand.
fn command_of(action: Action) -> Result<(String, Value), CommandError> {
    Ok(match action {
        Action::Dump { .. } | Action::Schema => unreachable!("handled before"),
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
        Action::Elements {
            action:
                ElementsAction::Get {
                    form_id,
                    path,
                    file,
                    depth,
                },
        } => (
            "elements.get".to_owned(),
            json!({ "form_id": form_id, "path": path, "file": file, "depth": depth }),
        ),
        Action::Call { name, params } => {
            let params = serde_json::from_str(&params)
                .map_err(|e| CommandError::new("invalid_params", format!("--params is not valid JSON: {e}")))?;
            (name, params)
        }
    })
}

fn run(game: Option<String>, load: Vec<String>, action: Action) -> Result<Value, CommandError> {
    let registry = Registry::standard();
    if let Action::Schema = action {
        return Ok(registry.catalogue());
    }
    let (name, params) = command_of(action)?;
    let mut session = match game {
        Some(game) => Session::load(&game, &load).map_err(|message| CommandError::new("load_failed", message))?,
        None if load.is_empty() => Session::default(),
        None => return Err(CommandError::new("invalid_params", "--load needs --game")),
    };
    registry.call(&mut session, &name, params)
}

fn main() -> ExitCode {
    let cli = Cli::parse();
    if let Action::Dump { game, file } = cli.action {
        // The progress messages go to stderr like the log of xDump.
        xedit_session::dump::log_progress_to_stderr();
        // The element tree resolves deeply through the definitions, so the
        // dump runs on a thread with a large stack.
        let worker = std::thread::Builder::new()
            .stack_size(1 << 30)
            .spawn(move || {
                xedit_session::dump::setup_game(&game).and_then(|mode| {
                    let stdout = std::io::stdout();
                    let mut out = std::io::BufWriter::with_capacity(1 << 20, stdout.lock());
                    xedit_session::dump::dump_file(&file, mode, &mut out)
                })
            })
            .expect("the dump thread");
        let result = worker
            .join()
            .unwrap_or_else(|_| Err("the dump thread panicked".to_owned()));
        if let Err(error) = result {
            eprintln!("error: {error}");
            return ExitCode::FAILURE;
        }
        return ExitCode::SUCCESS;
    }
    let outcome = run(cli.game, cli.load, cli.action);
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
