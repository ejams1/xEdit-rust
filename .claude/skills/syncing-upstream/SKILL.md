---
name: syncing-upstream
description: Use when upstream xEdit (TES5Edit/TES5Edit on GitHub) has published a new release and its changes have to be brought into the Rust port, or when checking whether the port is behind upstream.
---

# Syncing with upstream

The port follows upstream release by release. `upstream-map.toml` records the tag and commit the port matches (`[upstream]`) and the state of every upstream unit. Only a release with published binaries can be synced, because the binaries are the oracle that verifies the port (`checking-parity`); commits after the newest release are not merged.

## Procedure

1. Fetch upstream in the checkout and list the release tags: `git fetch --tags origin`, then `git tag --sort=-creatordate | head`. Release tags are named `xedit-<version>`. Stop when the newest tag with binaries on the GitHub releases page is the tag in `upstream-map.toml`: the port is current.
2. List the changes between the recorded tag and the new one: `git log --stat <old>..<new> -- Core xEdit xDump BSArch Sniff Tools`. Classify each changed unit through its entry in `upstream-map.toml`:
   - Definitions (`Core/wbDefinitions*.pas`): go to step 3.
   - Core, tools and GUI units with a `rust` path: port the diff by hand with `porting-pascal-unit`.
   - Units still `pending` without a `rust` path: nothing to port; the diff is picked up when the unit is ported.
3. Definition units: check out the new tag and regenerate the changed units with `cargo xtask port-defs emit-unit` (`porting-definitions`). Port the callbacks whose Pascal body changed: `git diff <old>..<new> -- Core/wbDefinitions<Game>.pas` shows them, and a callback that is new upstream appears as a new stub.
4. When the change adds an operation (a GUI action, a switch, a script function), add the session command and CLI subcommand, and update the coverage ledger.
5. Run `cargo xtask sync <upstream checkout> <commit of the new tag>`. It records the commit, adds new units and new ledger entries as `pending`, and prints the units and entries that upstream removed. Set `tag` in `[upstream]` by hand: sync only writes the commit.
6. Unpack the release binaries of the new tag, point `XEDIT_ORACLE_DIR` at them and run `cargo xtask parity dump` and `cargo xtask parity saves` for every game. The cache is keyed by the oracle tag, so the new oracle dumps every file again.
7. Run `cargo fmt`, `cargo clippy --workspace --all-targets`, `cargo test --workspace` and `cargo xtask check`, then open one pull request per upstream release.

## Notes

- `cargo xtask sync` rewrites `upstream-map.toml` and `coverage/ledger.toml` with LF line endings. When it adds nothing, restore both files with `git checkout --` rather than commit a line-ending change.
- An upstream change to a unit that is not ported yet is not lost: the unit stays `pending` and is ported from the new tag.
- Upstream fixes that change dump output make the cached oracle dumps of the old tag useless for the new tag; never compare the port at the new tag against the old cache.
