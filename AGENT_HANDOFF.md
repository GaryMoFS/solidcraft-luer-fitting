# Agent Handoff Prompt — `solidcraft-luer-fitting`

You are working on `solidcraft-luer-fitting`, a `#![no_std]` Rust WebAssembly Component (`craft:plugin@0.2.0`) plugin for SolidCraft that generates ISO 80369-7 (Sub-standard 7) 6% Luer taper fittings.

## Paths & Repository
- **GitHub:** https://github.com/GaryMoFS/solidcraft-luer-fitting (branch `main`)
- **Local repo:** `C:\Users\garym\Documents\solidcraft-luer-fitting`
- **Installed plugin:** `%LOCALAPPDATA%\SolidCraft\plugins\solidcraft-luer-fitting\`
- **SolidCraft host & CLI:** `C:\Users\garym\Documents\SolidCraft-r49-trial` (or latest `SolidCraft-rXX-trial`; `solidcraft-cli.exe` in `target\ci\`)

## Architecture
- `plugin.json` — Manifest (`id: "solidcraft-luer-fitting"`, capabilities `["commands@0.2", "dialog@0.1"]`, entry `luer-fitting`). Fields: `point` (`edge`, `sketch_point`), `plane` (`face`, `plane`), `fitting_type` (6 variants), `hole_diameter` (auto-hidden on female types via `x-solidcraft-visible-when`), `clearance` (`-0.50` to `+0.50 mm` diametral), `flip_direction`.
- `wit/plugin.wit` — Must stay byte-identical to `<SolidCraft>\crates\craft-plugin\wit\plugin.wit` (world `command-dialog-plugin`).
- `plugin/src/lib.rs` — `#![no_std]` WASM entrypoint (`EntryGuest` + `DialogGuest`), `#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic)]`, command runner & unique doc-name helper.
- `plugin/src/placement.rs` — Resolves planar `face`/`plane` + circular `edge`/`sketch_point` (projecting the point onto the plane) into a `Placement` frame and optional `target_body`.
- `plugin/src/luer.rs` — ISO 80369-7 geometry builders for all 6 types (`male_slip`, `male_lock`, `male_lock_internal`, `female_slip`, `female_slip_internal`, `female_lock`):
  - Uses `sketch.create` + `sketch.line` (closed `"points"` polyline) + `sketch.finish` + `solid.revolve` (`axis: "y"` on radial-axial plane `x_dir = frame.x_dir`, `y_dir = frame.z_dir`).
  - Uses `solid.coil` with custom closed trapezoidal sketch profiles on opposite radial planes (`+x_dir` and `-x_dir`) for exact analytical double-start right-hand threads (`pitch = 5.0 mm`, `clockwise = false`). Note: when passing `sketch` to `solid.coil`, never pass `diameter`, `section_size`, `section`, `section_position`, `start_angle`, or `end_cut`.
  - Protruding fittings join `target_body` (or create `new_body` if standalone); `(internal)` fittings cut into `target_body` (or create a standalone host cylinder first if no face was picked).
- `packager/src/main.rs` — Encodes `plugin/target/wasm32-unknown-unknown/release/solidcraft_luer_fitting_plugin.wasm` into `dist/solidcraft-luer-fitting/solidcraft-luer-fitting.wasm` + `dist/solidcraft-luer-fitting.zip`.

## Build, Install & Verify (Windows GNU Toolchain)
MSVC `link.exe` is not installed on this machine; always use the GNU toolchain:

```powershell
$env:RUSTUP_TOOLCHAIN = "stable-x86_64-pc-windows-gnu"

Set-Location "C:\Users\garym\Documents\solidcraft-luer-fitting\plugin"
cargo build --release --target wasm32-unknown-unknown

Set-Location "C:\Users\garym\Documents\solidcraft-luer-fitting\packager"
cargo run --release

$dst = Join-Path $env:LOCALAPPDATA "SolidCraft\plugins\solidcraft-luer-fitting"
New-Item -ItemType Directory -Force -Path $dst | Out-Null
Copy-Item "C:\Users\garym\Documents\solidcraft-luer-fitting\dist\solidcraft-luer-fitting\*" $dst -Force

# Validate manifest, limits, and WIT compatibility:
& "C:\Users\garym\Documents\SolidCraft-r48-trial\target\ci\solidcraft-cli.exe" plugin check $dst
```

To run end-to-end geometry tests across all 6 fitting types, copy `dist\solidcraft-luer-fitting\*` to a temp folder, add UTF-8 (no BOM) `tests\*.json` files (`{"entry":"luer-fitting","params":{...}}`), and run `solidcraft-cli.exe plugin test <temp-dir> --json`. Note that changing the installed `.wasm` hash requires re-approving grants in SolidCraft.

## Requested Change
<describe what you want to add or change here>
