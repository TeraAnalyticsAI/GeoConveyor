# GeoConveyor

A Tera Analytics product: a native batch-processing GUI for geospatial data, powered by [terrio](https://github.com/opengeos/terrio). One tab per command (info, convert, translate, warp, mosaic, rasterize, polygonize, dem, calc, proximity, sieve, grid, filter, crs, formats), every option of the installed terrio, folder in → folder out.

Written in Rust with egui. One binary, no Python, no Qt.

## Background

GUIs over command-line geospatial tools have a long history. [OGR2GUI](https://github.com/zer0infinity/OGR2GUI) is one of the originals: a front end for GDAL/OGR's `ogr2ogr` that lets you pick inputs, outputs, projections, SQL queries and spatial filters without hand-writing a command line. It is a Windows x64 fork of Inventis's `ogr2gui`, released under GPL-3.0.

GeoConveyor carries the same idea forward:

| | OGR2GUI | GeoConveyor |
|---|---|---|
| Wraps | `ogr2ogr` (vector conversion) | `terrio` (vector and raster: 15 commands) |
| Options | Fixed set built into the UI | Built from `terrio <command> --help` at start-up, so new flags appear automatically |
| Workflow | One conversion at a time | Whole folders, N files in parallel |
| Platform | Windows | Linux x86-64 prebuilt; builds from source elsewhere |
| Toolkit | C++ | Rust + egui |

Both share the same principle: the GUI never reimplements the tool, it only builds the command for you. Credit to David Tran and the original Inventis authors for showing how useful that is.

## Run

Prebuilt (Linux x86-64): `./geoconveyor`. It needs `terrio` installed (`curl -LsSf https://terrio.opengeos.org/install.sh | sh` on Linux/macOS, `pip install terrio`, or anywhere on PATH; browse to it otherwise).

Build from source:

```
# Rust toolchain, if you do not have one:  curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
cargo build --release          # binary: target/release/geoconveyor
cargo test                     # engine tests; need terrio on PATH, skipped otherwise
```

No `-dev` packages are needed on Linux (X11/Wayland/GL are loaded at run time). File dialogs use the desktop portal (`xdg-desktop-portal`, present on standard Ubuntu/GNOME/KDE desktops).

## How it works

- Each tab's form is built at start-up from `terrio <command> --help`, so every flag your terrio has is there, and flags added by a newer terrio appear after Reload.
- Files run N at a time (Parallel files), one terrio process each. Output is written under a `.partial-` name and renamed on success, so a failed or cancelled file leaves nothing behind.
- Copy command / Copy all commands give the exact command lines for scripts.
- Settings (folders, every tab's options) are saved to `~/.config/geoconveyor/settings.json` (`%APPDATA%\geoconveyor\` on Windows).

## Layout

```
src/help.rs     parse clap --help, find terrio, formats
src/specs.rs    per-command facts (patterns, suffix, each/together)
src/model.rs    option fields, forms, per-tab state, saved settings
src/jobs.rs     scan folder, build the terrio command per file, quoting
src/runner.rs   worker pool, temp-then-rename, cancel
src/app.rs      the egui UI (drawing only)
tests/          end-to-end tests with small sample data in tests/data
```

## Acknowledgements

- [terrio](https://github.com/opengeos/terrio), the engine behind every tab.
- [OGR2GUI](https://github.com/zer0infinity/OGR2GUI) (David Tran, fork of Inventis's ogr2gui), the earlier GUI for `ogr2ogr` that inspired this approach.
- [GDAL/OGR](https://gdal.org), the library both tools ultimately build on.
