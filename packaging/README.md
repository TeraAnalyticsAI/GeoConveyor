# Packaging GeoConveyor 0.3.1

| Target | How | Output |
|---|---|---|
| Linux binary | `cargo build --release` | `target/release/geoconveyor` |
| Linux installer | ship `packaging/linux/install.sh` with the binary, `.desktop` file and icons | `./install.sh` (per-user, no root) |
| Flatpak | `packaging/linux/build-flatpak.sh` (needs flatpak + flatpak-builder) | `GeoConveyor-0.3.1.flatpak` |
| Windows installer | on Windows: `cargo build --release`, then Inno Setup 6: `iscc /DAppVersion=0.3.1 packaging\windows\geoconveyor.iss` | `dist\GeoConveyor-0.3.1-setup.exe` |

`.github/workflows/release.yml` does all of it on GitHub (Linux + Flatpak on ubuntu-24.04,
Windows installer + portable zip on windows-latest) when you push a `v*` tag or run it by hand.

## terrio

GeoConveyor drives the `terrio` command-line tool ; it is not bundled. Install it on Linux/macOS with `curl -LsSf https://terrio.opengeos.org/install.sh | sh` (terrio's own installer), or with `pip install terrio` (also the route on Windows).

* **Linux / Windows:** found on PATH (and `~/.local/bin`, `%APPDATA%\Python\Scripts`), or browse to it in Settings.
* **Flatpak:** the sandbox can't see host programs, so GeoConveyor runs the host's terrio with
  `flatpak-spawn --host` (the `--talk-name=org.freedesktop.Flatpak` permission). Folders are shared
  through `--filesystem=home`, `/mnt`, `/media`, `/run/media`. Because of that permission it can't go on Flathub as is.

## Things to set before publishing

* App ID `xyz.teraanalytics.GeoConveyor` matches your teraanalytics.xyz domain (reverse-DNS: xyz.teraanalytics); if you ever change it, update the manifest,
  desktop file, metainfo and `build-flatpak.sh` together.
* `project_license` in the metainfo is `LicenseRef-proprietary`; set your real license.
* The Windows `AppId` GUID in `geoconveyor.iss` identifies the app to the uninstaller; keep it fixed across versions.
* Windows binaries are unsigned, so SmartScreen will warn until you sign them.

## Release smoke test

Run this on a real Linux and Windows machine before publishing:

1. Install from the artifact (or unzip the portable build) and launch GeoConveyor.
2. Pick an input folder and an output folder; confirm terrio is found in Settings.
3. Run one Convert batch and one Mosaic/Warp batch; check the log, status column and output files.
4. Run the same job again with Skip, then with Overwrite, and with Cancel during a run.
5. Change the language and theme, close and reopen the app, and confirm the settings stuck.
6. Use `install.sh --uninstall` (Linux) or the Windows uninstaller and check the files are gone.
