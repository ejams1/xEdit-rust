// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! The registry and the session a long-lived front end (`serve`, `mcp`)
//! runs its commands against.

use serde_json::Value;
use xedit_session::{BatchCommand, CommandError, Registry, Session};

/// Loads the session of the global options: the game and plugins, and the
/// edit gate.
pub fn open_session(game: Option<&str>, load: &[String], edit: bool) -> Result<Session, CommandError> {
    let mut session = match game {
        Some(game) => Session::load(game, load).map_err(|message| CommandError::new("load_failed", message))?,
        None if load.is_empty() => Session::default(),
        None => return Err(CommandError::new("invalid_params", "--load needs --game")),
    };
    if game.is_some() {
        // The cache path follows the data folder of the loaded plugins.
        xedit_session::refs::init_cache_path();
    }
    session.allow_edit(edit);
    Ok(session)
}

enum State {
    /// Not loaded yet: the first command loads it.
    Pending {
        game: Option<String>,
        load: Vec<String>,
    },
    Open(Session),
    /// The load failed; every command reports the failure.
    Failed(CommandError),
}

/// The commands and the one session they run against. The edit gate is set
/// when the session opens and no command changes it, so a client of a
/// daemon cannot lift the gate its operator set.
pub struct Engine {
    registry: Registry,
    state: State,
    edit: bool,
}

impl Engine {
    /// Loads the session now, so a daemon fails at startup when the plugins
    /// do not load.
    pub fn open(game: Option<&str>, load: &[String], edit: bool) -> Result<Self, CommandError> {
        Ok(Self::with_session(open_session(game, load, edit)?))
    }

    /// Loads the session at the first command. An MCP client times out on a
    /// handshake that waits for a big load order, so `mcp` answers
    /// `initialize` and `tools/list` first.
    pub fn lazy(game: Option<String>, load: Vec<String>, edit: bool) -> Self {
        Self {
            registry: Registry::standard(),
            state: State::Pending { game, load },
            edit,
        }
    }

    pub fn with_session(session: Session) -> Self {
        let edit = session.edit_allowed();
        Self {
            registry: Registry::standard(),
            state: State::Open(session),
            edit,
        }
    }

    pub fn registry(&self) -> &Registry {
        &self.registry
    }

    /// Whether mutating commands may run for real.
    pub fn edit_allowed(&self) -> bool {
        self.edit
    }

    /// Runs one command.
    pub fn call(&mut self, name: &str, params: Value) -> Result<Value, CommandError> {
        let session = session_of(&mut self.state, self.edit)?;
        self.registry.call(session, name, params)
    }

    /// Runs several commands in order against the session (`Registry::run_batch`).
    pub fn batch(&mut self, commands: Vec<BatchCommand>, keep_going: bool) -> Result<Value, CommandError> {
        let session = session_of(&mut self.state, self.edit)?;
        Ok(self.registry.run_batch(session, commands, keep_going))
    }
}

/// The open session, loading it first when it is still pending.
fn session_of(state: &mut State, edit: bool) -> Result<&mut Session, CommandError> {
    if let State::Pending { game, load } = state {
        *state = match open_session(game.as_deref(), load, edit) {
            Ok(session) => State::Open(session),
            Err(error) => State::Failed(error),
        };
    }
    match state {
        State::Open(session) => Ok(session),
        State::Failed(error) => Err(error.clone()),
        State::Pending { .. } => unreachable!("opened above"),
    }
}
