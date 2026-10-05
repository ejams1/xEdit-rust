# xEdit-rust
Rebuild of the xEdit project in Rust. Additionally features CLI control of all xEdit features and native support for AI agents to control it.

The build plan is in [docs/PLAN.md](docs/PLAN.md).

## License

xEdit-rust is a port of [xEdit](https://github.com/TES5Edit/TES5Edit) by ElminsterAU and the xEdit contributors. xEdit is licensed under the Mozilla Public License 2.0, and ported files are Modifications that must stay under it, so this whole repository is licensed under [MPL-2.0](LICENSE). See [NOTICE](NOTICE) for attribution and third-party terms.

Rules for contributions:

- Every source file starts with the MPL-2.0 notice shown in [NOTICE](NOTICE).
- A file ported from upstream names its upstream origin below that notice.
- Every crate sets `license.workspace = true`.
- Dependencies must pass `cargo deny check licenses` against [deny.toml](deny.toml).
