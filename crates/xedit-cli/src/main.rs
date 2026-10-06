// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use std::process::ExitCode;

use clap::{Parser, Subcommand};
use serde_json::{Value, json};
use xedit_session::{CommandError, Registry, Session};

/// xEdit command-line interface.
#[derive(Parser)]
#[command(name = "xedit", version)]
struct Cli {
    /// Print one JSON envelope: {"ok":true,"result":...} or {"ok":false,"error":{"code","message"}}.
    #[arg(long, global = true)]
    json: bool,

    #[command(subcommand)]
    action: Action,
}

#[derive(Subcommand)]
enum Action {
    /// Print every command with its request and response JSON Schema.
    Schema,
    /// Write the element tree of a plugin as xDump prints it.
    Dump {
        /// Game of the plugin: fo4, sse or tes5.
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

fn run(action: Action) -> Result<Value, CommandError> {
    let registry = Registry::standard();
    match action {
        Action::Dump { .. } => unreachable!("handled in main"),
        Action::Schema => Ok(registry.catalogue()),
        Action::Call { name, params } => {
            let params = serde_json::from_str(&params)
                .map_err(|e| CommandError::new("invalid_params", format!("--params is not valid JSON: {e}")))?;
            registry.call(&mut Session::default(), &name, params)
        }
    }
}

fn main() -> ExitCode {
    let cli = Cli::parse();
    if let Action::Dump { game, file } = &cli.action {
        let result = xedit_session::dump::setup_game(game).and_then(|mode| {
            let stdout = std::io::stdout();
            let mut out = std::io::BufWriter::with_capacity(1 << 20, stdout.lock());
            xedit_session::dump::dump_file(file, mode, &mut out)
        });
        if let Err(error) = result {
            eprintln!("error: {error}");
            return ExitCode::FAILURE;
        }
        return ExitCode::SUCCESS;
    }
    let outcome = run(cli.action);
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
