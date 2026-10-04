# Releasing Cellar

Releases are built by GitHub Actions
([`.github/workflows/release.yml`](../.github/workflows/release.yml)) on
GitHub's Windows, macOS and Linux machines. You don't need a Mac to build
the macOS version. Pushing a tag like `v0.2.0` builds everything and creates
a **draft** release with these files:

| File | Contents |
|---|---|
| `cellar-vX.Y.Z-windows-x64.exe` | The program, ready to run |
| `cellar-vX.Y.Z-windows-x64.zip` | `cellar.exe` + README + LICENSE |
| `cellar-vX.Y.Z-macos-universal.dmg` | `Cellar.app` for Apple Silicon and Intel, drag-to-Applications |
| `cellar-vX.Y.Z-linux-x64.tar.gz` | `cellar` binary + README + LICENSE + icon |
| `SHA256SUMS.txt` | Checksums of all files above |

Nothing is published until you click **Publish release** on GitHub.

## 1. Test the builds (optional, recommended after workflow changes)

On GitHub: **Actions** → **Release** → **Run workflow** (branch `main`).
This builds all platforms without creating a release. When the run is done,
download the files from the **Artifacts** section on the run's page and
try them, especially the `.dmg` on a Mac.

Private repositories pay for this in Actions minutes (macOS minutes count
10×); public repositories build for free.

## 2. Prepare the version

1. Set `version` in `Cargo.toml` (for example `0.2.0`).
2. Run `cargo build` so `Cargo.lock` picks up the new version. The release
   builds use `--locked` and fail if `Cargo.lock` is out of date.
3. Add a `## Cellar 0.2.0 — YYYY-MM-DD` section to `CHANGELOG.md`.
4. Check that everything passes, then commit and push:

```bash
cargo clippy --all-targets -- -D warnings
```
```bash
cargo test
```
```bash
git commit -am "Release 0.2.0"
```
```bash
git push
```

Wait for the regular **CI** run on `main` to pass.

## 3. Tag the release

The tag must start with `v` and should match the version in `Cargo.toml`:

```bash
git tag -a v0.2.0 -m "Cellar 0.2.0"
```
```bash
git push origin v0.2.0
```

This starts the **Release** workflow (about 15 minutes). If a build fails,
fix it on `main`, then move the tag to the fixed commit and push it again:

```bash
git tag -fa v0.2.0 -m "Cellar 0.2.0"
```
```bash
git push -f origin v0.2.0
```

Delete the old draft release on GitHub first, otherwise a second draft
appears.

## 4. Publish

1. On GitHub, open **Releases** and the new draft.
2. Replace the generated notes with the version's section from
   `CHANGELOG.md`, and add the install notes below.
3. Click **Publish release**.

### Install notes for the release text

The builds are not code-signed, so Windows and macOS show a warning the
first time. Paste this into the release text:

> **Windows:** if SmartScreen says "Windows protected your PC", click
> *More info* → *Run anyway*.
>
> **macOS:** open the `.dmg` and drag Cellar to Applications. The first
> time, right-click Cellar → *Open* (on macOS 15 and later: try to open it,
> then System Settings → Privacy & Security → *Open Anyway*). If macOS says
> the app is damaged, run
> `xattr -dr com.apple.quarantine /Applications/Cellar.app` in Terminal.
>
> **Linux:** unpack the archive and run `./cellar`.
>
> **Uninstall:** `cellar --uninstall` lists what would be removed (the
> program and Cellar's settings; never your workbooks), and
> `cellar --uninstall --yes` removes it. On macOS run
> `/Applications/Cellar.app/Contents/MacOS/cellar --uninstall --yes` if
> `cellar` isn't on your `PATH`.

Removing these warnings requires paid code signing: an Apple Developer
account for signing and notarizing on macOS, and a code-signing certificate
on Windows (SignPath.io offers free signing for open-source projects).

## Notes

- **Windows command line.** `cellar.exe` is a GUI program, so double-clicking
  it opens no console window. The batch commands (`--convert`,
  `--export-md`, `--export-charts`, `--uninstall`) attach to the console they were started
  from. `cmd` and PowerShell don't wait for GUI programs, so their output
  can appear after the next prompt and `%ERRORLEVEL%` / `$LASTEXITCODE` may
  not be set. To wait for the result, pipe it:
  `cellar --convert in.xlsx | Out-Host` (PowerShell) or
  `start /wait cellar --convert in.xlsx` (cmd). Batch files wait anyway.
- **Icons.** The Windows icon (`assets/icon.ico`, embedded by `build.rs`)
  and the macOS icon (made from `assets/icon-1024.png` during the release
  build) come from `assets/logo.svg`. After changing the logo, run
  `cargo run --example render_icons` and commit the regenerated files.
- **macOS minimum version** is 11 (Big Sur), set in the workflow's
  `Info.plist`. The bundle identifier is `io.github.haderlka.cellar`; if
  it changes, change `BUNDLE_ID` in `src/infrastructure/app_dirs.rs` too,
  which `--uninstall` uses to find macOS's per-app files.
- **macOS file types.** The `Info.plist` declares `.cellar` files
  (`io.github.haderlka.cellar.workbook`, a kind of JSON) as Cellar's own,
  so double-clicking one in Finder opens Cellar. Excel and CSV files list
  Cellar under *Open With*. macOS picks this up when `Cellar.app` is copied
  to Applications. Finder delivers the files as an Apple Event, which
  `src/gui/open_files.rs` handles; the app can only be tested this way as a
  bundle, not with `cargo run`.
- **Linux** builds on Ubuntu 22.04 so the binary runs on systems with
  glibc 2.35 or newer.
