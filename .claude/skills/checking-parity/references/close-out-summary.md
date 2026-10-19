## Close-out results (2026-10-18)

The parity backlog's close-out (issue #41, pull request #77) ran every mode at the final tip, one step per mode with the oracle cache shared, and `sniff` one step per game (the crash-carrying step runner of rule 16). The numbers per mode are the README Status table and the report of each run is written to `target/parity/<mode>.json`, as the sections below describe. The full logs of the close-out are under the batch's scratch folder, not committed.

What the close-out learned about the oracle itself:

- The released `Sniff.exe` corrupts its heap under threads (`0xc0000374`) and hangs on two Fallout 4 Creation Club texture archives (`cctosfo4001-neosky`, `cctosfo4002_neonflats`, `find-textures-copy`) even on one thread. The harness retries a crash twice and a hang once on one thread, then records the archive as `oracle-failed` and carries on; it never ends the run.
- The `Find textures` report prints `XBOX` from an uninitialised local record on every line of a BA2 DDS archive; the port prints nothing. The saved bytes are equal, so `sniff` counts these as log differences, not failures.
- `clean` of Fallout 76's `NW.esm` loses the oracle log's counts on every run (`oracle-log-lost`); the saved bytes are compared and equal.
- `check-dump` of `Starfield.esm` needs more than the 23.7 GiB per-process cap in a batch run (a rerun alone with `--max-memory 28` also met the machine's limit), and `tool-modes`'s `setesm` on `BlueprintShips-SFBGS050.esm` exceeds the 20-minute oracle limit and still exceeds 60 minutes alone (the other four modes on that file and the other eight `setesm` files are equal).
- The script run phase has four cases whose oracle timed out at 60 or 120 minutes under load; they are rerun owed with `--jobs 1 --oracle-timeout 180`.
- BSArchPro's autopacking mode starts no work when its window is on a desktop nothing displays: `bsarchpro` runs it on the user's desktop, shown. The case list named asset sources as `<work>\<name>` where the fixtures are under `<work>\src`, and the XBox refusal is shown on the `TFormTaskProgress` form (`Packing...`), not a `#32770`: all six cases are equal now (five archives byte for byte, one refusal).
