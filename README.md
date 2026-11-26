# SolidCraft Luer Fitting Plugin (`solidcraft-luer-fitting`)

A `#![no_std]` WebAssembly Component (`craft:plugin@0.2.0`) plugin for SolidCraft that generates **ISO 80369-7 (Sub-standard 7)** 6% Luer taper fittings directly onto planar faces, construction planes, or standalone.

## Features

- **Point** (`edge` / `sketch_point`): Pick a circular edge (uses its center) or sketch point to locate the fitting. Optional — defaults to the center of the selected plane/face.
- **Plane** (`face` / `plane`): Pick a planar face on an existing body (automatically joins protruding fittings or cuts internal variants into that body) or a construction plane (defaults to `XY`).
- **Type** (6 ISO 80369-7 variants):
  1. `Male Slip` — protruding 6% taper cone
  2. `Male Lock` — protruding 6% taper cone + collar with internal double-start right-hand trapezoidal thread
  3. `Male Lock (internal)` — flush recessed locking cavity + cone carved into an existing solid
  4. `Female Slip` — protruding boss with internal 6% taper socket
  5. `Female Slip (internal)` — recessed 6% taper socket cut into an existing solid
  6. `Female Lock` — protruding 6% taper hub with external double-start right-hand trapezoidal thread
- **Hole diameter** (Male types only, default `2.25 mm`): Adjustable central fluid bore diameter, automatically validated against cone tip wall thickness.
- **Clearance** (default `0.00 mm`, range `-0.50 mm` to `+0.50 mm`): Diametral clearance applied to the 6% taper cone and thread engagement (`+` looser, `-` tighter for 3D printing tolerances).
- **Flip direction** (default `false`): Inverts orientation along the plane normal.

## Build & Install (Windows GNU Toolchain)

```powershell
$env:RUSTUP_TOOLCHAIN = "stable-x86_64-pc-windows-gnu"

Set-Location "C:\Users\garym\Documents\solidcraft-luer-fitting\plugin"
cargo build --release --target wasm32-unknown-unknown

Set-Location "C:\Users\garym\Documents\solidcraft-luer-fitting\packager"
cargo run --release

$dst = Join-Path $env:LOCALAPPDATA "SolidCraft\plugins\solidcraft-luer-fitting"
New-Item -ItemType Directory -Force -Path $dst | Out-Null
Copy-Item "C:\Users\garym\Documents\solidcraft-luer-fitting\dist\solidcraft-luer-fitting\*" $dst -Force
```

## Verify with `solidcraft-cli`

```powershell
& "C:\Users\garym\Documents\SolidCraft-r48-trial\target\ci\solidcraft-cli.exe" plugin check "$env:LOCALAPPDATA\SolidCraft\plugins\solidcraft-luer-fitting"
```

## Tests

`tests\` holds 18 geometry cases for `solidcraft-cli plugin test`. Each of the 6 fitting types runs three ways, and every case checks the body count and the total volume:

- alone in an empty design (`<type>.json`);
- placed free in a design that already has two boxes, `tests\two_boxes.sc` (`existing_<type>.json`): the boxes must stay untouched;
- on the top face of the second box (`face_<type>.json`): only that box is joined or cut.

`plugin test` wants the built plugin and the `tests\` folder side by side, so copy both into a scratch folder:

```powershell
$t = Join-Path $env:TEMP "solidcraft-luer-fitting"
Remove-Item $t -Recurse -Force -ErrorAction SilentlyContinue
New-Item -ItemType Directory -Force -Path $t | Out-Null
Copy-Item "C:\Users\garym\Documents\solidcraft-luer-fitting\dist\solidcraft-luer-fitting\*" $t -Force
Copy-Item "C:\Users\garym\Documents\solidcraft-luer-fitting\tests" $t -Recurse -Force
& "C:\Users\garym\Documents\SolidCraft-r48-trial\target\ci\solidcraft-cli.exe" plugin test $t --json
```

## Changes

- **0.1.1**: a fitting built in a design that already had another body cut its thread and pocket into the wrong body (the first one in the design) and left the new fitting plain. The plugin now uses the body the revolve itself reports. Added the `tests\` cases above.
- **0.1.0**: first release.
