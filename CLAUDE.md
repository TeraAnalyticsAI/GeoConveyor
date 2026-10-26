# GeoConveyor (Tera Analytics) — project notes for Claude Code

Native Rust/egui batch GUI for the `terrio` geospatial CLI (https://github.com/opengeos/terrio).
Version 0.3.1. One tab per terrio command; folder in -> folder out; one terrio process per file.
Publisher: Tera Analytics (teraanalytics.xyz). Flatpak id: `xyz.teraanalytics.GeoConveyor`.

## Commands
    cargo build --release          # target/release/geoconveyor
    cargo test                     # lib tests + tests/engine.rs (needs real `terrio` on PATH; verified with 0.3.2, 0.4.1 and 0.5.0)
    python3 tools/make_packs.py lang   # REGENERATE lang/{en,es,fr,de}.json after adding/changing any t("...") string

## Layout
- `src/help.rs`   parses `terrio <cmd> --help` into options; finds/launches terrio (Flatpak: via `flatpak-spawn --host`)
- `src/specs.rs`  per-command specs; `InputType` + `relevant()` decide which Convert options show for Image/Vector/Point cloud
- `src/model.rs`  forms, tab/session settings, Settings load/save (+ migration from old `terrio-gui` config dir)
- `src/jobs.rs`   file scan, output naming, job argv; `src/runner.rs` worker pool, temp-name-then-rename outputs
- `src/i18n.rs`   English text is the key; packs in `lang/*.json`; external packs in `<config>/geoconveyor/lang/`
- `src/style.rs`  look & feel: light/dark palettes, pill buttons/tabs, toggle, check, status pill, command box, progress
- `src/app.rs`    all UI: sessions (tabs), caret menu, Settings/Help/About, forms
- `packaging/`    icon (SVG/PNG/ICO), Flatpak manifest, Linux install.sh, Windows Inno Setup script; CI in `.github/workflows/release.yml`

## Gotchas
- egui/eframe 0.37 API: `App::ui(&mut self, ui, frame)`, `Panel::top().show(ui, ..)`. Default fonts lack ▾▲▼; arrows are painted.
- Never wrap a full-width widget row in a bare `with_layout(right_to_left)` inside a vertical ui: it takes all remaining
  height and pushes content out of view. Wrap it in `ui.horizontal(..)` (this bit the Search options row once).
- Only the GUI's own strings are translated; terrio's option descriptions stay English. pt/it/zh/ja are listed as "coming soon".
- Windows code and the Flatpak/Windows installers have NEVER been built (written in a sandbox without those toolchains);
  expect small fixes on the first CI run. The Windows exe has no embedded icon yet (window icon is set at runtime).
- Windows: `.cargo/config.toml` links the C runtime statically (`+crt-static`) so the exe runs without the VC++ Redistributable
  (VCRUNTIME140.dll). Don't remove it; CI fails the Windows job if the exe imports that DLL again.
- Windows renders with wgpu (D3D12, WARP software fallback), Linux with glow. Plain OpenGL fails on Windows VMs/RDP
  ("egui_glow requires opengl 2.0+"). `GEOCONVEYOR_RENDERER=glow` forces OpenGL on Windows.
- terrio's own Windows builds need VCRUNTIME140.dll (we can't fix that here). The app sets SetErrorMode so a failed terrio
  launch shows no system popup; `help::LaunchProblem` turns the exit code into an in-app message, and the installer offers
  the VC++ Redistributable download when it's missing.
- UI was checked under Xvfb with software GL; typing and native file dialogs were not exercised.
