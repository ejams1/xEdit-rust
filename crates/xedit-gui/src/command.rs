// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! The in-process command bridge: the GUI's handle onto the `xedit-session`
//! registry, the same registry `xedit serve` and `xedit mcp` build their
//! surfaces from.
//!
//! Every GUI action goes through [`CommandBridge::call`] or its typed form
//! [`CommandBridge::request`] (design rule 1: one operation layer). The
//! request and response types are the ones `crates/xedit-session` declares,
//! so a GUI call carries exactly the JSON a CLI subcommand or an MCP tool
//! call carries; no command is added for the GUI.
//!
//! The initial load is the same startup step the CLI performs
//! (`xedit_cli::engine::open_session`): `Session::load`, which sets the game
//! up and loads the plugins. `session.load` is the command that reloads an
//! open session. Both leave the process-wide file list holding the session's
//! files, so the GUI keeps exactly one bridge (and therefore one session)
//! alive.

use serde::Serialize;
use serde::de::DeserializeOwned;
use serde_json::Value;
use xedit_session::{CommandError, Registry, Session};

/// The registry and the session it runs against.
pub struct CommandBridge {
    registry: Registry,
    session: Session,
}

impl CommandBridge {
    /// A bridge over an already-loaded session.
    pub fn new(session: Session) -> Self {
        Self {
            registry: Registry::standard(),
            session,
        }
    }

    /// The session, for the reads that are not commands (the game mode and
    /// the window title).
    pub fn session(&self) -> &Session {
        &self.session
    }

    pub fn session_mut(&mut self) -> &mut Session {
        &mut self.session
    }

    /// Runs one command with JSON parameters.
    pub fn call(&mut self, name: &str, params: Value) -> Result<Value, CommandError> {
        self.registry.call(&mut self.session, name, params)
    }

    /// Runs one command with a typed request and response: the same structs
    /// the CLI and the daemon use.
    ///
    /// ```ignore
    /// let info: xedit_session::SessionInfo =
    ///     bridge.request("session.info", &xedit_session::NoParams {})?;
    /// ```
    pub fn request<Req, Res>(&mut self, name: &str, request: &Req) -> Result<Res, CommandError>
    where
        Req: Serialize,
        Res: DeserializeOwned,
    {
        let params = serde_json::to_value(request).map_err(|error| CommandError::new("internal", error.to_string()))?;
        let value = self.call(name, params)?;
        serde_json::from_value(value).map_err(|error| CommandError::new("internal", error.to_string()))
    }

    /// Whether the command changes plugin data or files (`Registry::mutates`).
    pub fn mutates(&self, name: &str) -> Option<bool> {
        self.registry.mutates(name)
    }
}

/// Loads a session for the GUI: the startup load of the CLI
/// (`engine::open_session`), moved off the UI thread by the caller.
///
/// The progress messages of the load go to the callback installed with
/// `xedit_core::interface::misc::set_progress_callback`; the reference
/// cache path follows the data folder once the plugins are loaded.
pub fn load_session(game: &str, plugins: &[String], edit: bool) -> Result<Session, CommandError> {
    let mut session = Session::load(game, plugins).map_err(|message| {
        // The wait form's Cancel stopped the load (`GeneralProgress`'s
        // `Abort`), which is not a failure of the load itself.
        if message.starts_with("cancelled") {
            CommandError::new("cancelled", "the operation was cancelled")
        } else {
            CommandError::new("load_failed", message)
        }
    })?;
    xedit_session::refs::init_cache_path();
    session.allow_edit(edit);
    Ok(session)
}

#[cfg(test)]
mod tests {
    use super::*;
    use xedit_session::NoParams;
    use xedit_session::commands::SessionInfo;

    #[test]
    fn a_typed_request_uses_the_session_types() {
        let mut bridge = CommandBridge::new(Session::default());
        let error = match bridge.request::<NoParams, SessionInfo>("session.info", &NoParams {}) {
            Err(error) => error,
            Ok(_) => panic!("session.info answered without a session"),
        };
        assert_eq!(error.code, "no_session");
    }

    #[test]
    fn an_unknown_command_reports_the_registry_error() {
        let mut bridge = CommandBridge::new(Session::default());
        let error = bridge.call("no.such.command", serde_json::json!({})).unwrap_err();
        assert_eq!(error.code, "unknown_command");
    }

    #[test]
    fn the_registry_is_the_standard_one() {
        let mut bridge = CommandBridge::new(Session::default());
        let version: xedit_session::Version = bridge.request("system.version", &NoParams {}).unwrap();
        assert_eq!(version.version, env!("CARGO_PKG_VERSION"));
    }

    #[test]
    fn the_mutates_flag_matches_the_registry() {
        let bridge = CommandBridge::new(Session::default());
        assert_eq!(bridge.mutates("files.save"), Some(true));
        assert_eq!(bridge.mutates("files.list"), Some(false));
        assert_eq!(bridge.mutates("no.such.command"), None);
    }

    #[test]
    fn the_session_load_request_is_the_cli_one() {
        // `session.load` needs an open session; the request type proves the
        // fields the GUI will send are the CLI's.
        let request = xedit_session::commands::SessionLoadRequest {
            plugins: vec!["a.esp".to_owned()],
            compare_to: None,
            mode: None,
            force: false,
            dry_run: true,
        };
        let text = serde_json::to_string(&request).unwrap();
        assert!(text.contains("\"plugins\":[\"a.esp\"]"));
    }
}
