# Using the xedit CLI

`xedit` is the command-line interface of the Rust port. Build it with `cargo build --release -p xedit-cli`; the binary is `target/release/xedit.exe`. Every subcommand except `dump`, `saves dump`, `refs dump`, `batch`, `serve` and `mcp` is a command of the session registry, so `xedit schema` prints the full list with the JSON Schema of each request and response, and `xedit call <name> --params '<json>'` runs any of them by name. The schema is the authority on parameters; this skill says how to use them safely.
