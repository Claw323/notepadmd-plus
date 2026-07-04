# NotepadMD+

A lightweight Markdown notepad for Windows. Open, edit, preview, and save
`.md` / `.markdown` / `.txt` files. Feels like classic Notepad, with a
polished rendered reading mode.

![mode] Plain Text · Pretty · Split — switch instantly with the toolbar or
`Ctrl+1` / `Ctrl+2` / `Ctrl+3`.

## Stack

- **Rust + [egui](https://github.com/emilk/egui) (eframe)** — immediate-mode
  native GUI, statically linked.
- **egui_commonmark** (pulldown-cmark + syntect) — Markdown rendering with
  syntax-highlighted code blocks, tables, task lists, blockquotes, etc.
- **rfd** — native Windows file dialogs.
- Hand-rolled, fast line-based Markdown syntax highlighting in the editor.

### Why this stack (and not Tauri/Electron)

The hard requirements were: a **single self-contained executable**, **no
runtime or installable framework after delivery**, and **fully offline**.

- Electron ships a ~100 MB Chromium per app. Out.
- Tauri is small, but on Windows it renders through **WebView2** — an
  external Microsoft runtime. It's preinstalled on Windows 11 and most
  Windows 10 machines, but it is still a separate framework dependency and
  can require an installer/bootstrapper on older systems.
- Rust + egui compiles to **one statically linked `.exe` (~9 MB)** with zero
  external dependencies. It starts fast, works offline forever, and can be
  copied around like classic `notepad.exe`.

Tradeoff: menus and dialogs are drawn by the app (native-adjacent rather
than Win32-native), and the preview is a quality egui renderer rather than a
browser engine. File Open/Save dialogs are real native Windows dialogs, and
the app uses Segoe UI / Consolas system fonts when available.

## Features

- **File**: New, Open, Save, Save As, Revert, recent-files list, drag &
  drop to open, "Open with" file association support (pass a path as the
  first argument), unsaved-changes prompts before close/open/reload.
- **Modes**: Plain Text (editable, syntax-highlighted), Pretty (rendered
  reading view), Split (both side by side).
- **Editing**: Markdown syntax highlighting, optional line numbers (correct
  even under word wrap), word-wrap toggle, undo/redo, cut/copy/paste,
  find + replace (with match-case, wrap-around, F3/Shift+F3), Tab inserts a
  real tab, external file-change detection with reload prompt.
- **Pretty mode**: headings, lists, task lists, blockquotes, fenced code
  with syntax highlighting, tables, links (open in the default browser),
  images, horizontal rules, centered reading column with comfortable
  typography. Light and dark theme. Text is mouse-selectable — including
  across code blocks — with Ctrl+C and a right-click menu (Copy / Copy All /
  Edit in Plain Text). The cross-block selection relies on a small vendored
  patch to `egui_commonmark_backend` (see `vendor/`, wired via
  `[patch.crates-io]`).
- **App**: resizable window that remembers its size/position, remembers last
  mode, light/dark/system theme, small Preferences dialog, About dialog,
  status bar with path / modified state / Ln,Col / mode.
- UTF-8 throughout. `\r\n` line endings are preserved on save. Files that
  are not valid UTF-8 get a friendly prompt offering a lossy open.
- Saves are atomic (temp file + rename); a failed save never touches your
  buffer or the original file, and shows a friendly error.

## Keyboard shortcuts

| | |
|---|---|
| Ctrl+N / Ctrl+O / Ctrl+S / Ctrl+Shift+S | New / Open / Save / Save As |
| F5 | Revert to saved |
| Ctrl+F / Ctrl+H / F3 / Shift+F3 | Find / Replace / Next / Previous |
| Ctrl+1 / Ctrl+2 / Ctrl+3 | Plain Text / Pretty / Split |
| Alt+Z | Toggle word wrap |
| Ctrl+Plus / Ctrl+Minus / Ctrl+0 | Zoom |

## Building

### On Windows (native)

Install Rust from <https://rustup.rs>, then:

```
build-windows.cmd
```

or manually: `cargo build --release` → `target\release\notepadmd_plus.exe`.

### Cross-compiling from macOS / Linux

```
rustup target add x86_64-pc-windows-gnu
brew install mingw-w64        # or apt install mingw-w64
./build-windows.sh
```

### Dev mode

```
cargo run          # debug build with console window
cargo test         # unit tests (find/replace, highlighter, cursor math)
```

## Release output

The final executable appears at:

```
dist/NotepadMD+.exe
```

(a renamed copy of `target/x86_64-pc-windows-gnu/release/notepadmd_plus.exe`
— on native Windows builds, `target\release\notepadmd_plus.exe`).

It is a **true single file**: icon and version metadata embedded, no console
window in release builds, no installer, no runtime. Copy it anywhere and run
it. Settings are stored per-user under `%APPDATA%` (via eframe storage).

## Packaging notes / known constraints

- **Single-file: achieved.** No compromise needed; there is no sidecar, no
  DLLs to ship, no framework to install.
- The binary is unsigned, so Windows SmartScreen may show "Windows protected
  your PC" on first run of a downloaded copy (Run anyway → More info). Code
  signing is the fix if you distribute widely.
- Cross-built with the `x86_64-pc-windows-gnu` toolchain; a native MSVC
  build (`cargo build --release` on Windows) produces an equivalent exe.
- Very large files (tens of MB) will type more slowly in Plain Text mode —
  the editor re-lays-out the document as you edit. Everyday documents,
  including large READMEs and long notes, are smooth.
- The embedded fallback font doesn't cover CJK glyphs; on Windows the app
  loads Segoe UI/Consolas at startup, which covers Latin/Cyrillic/Greek.
  Files containing CJK text load and save correctly either way.

## License

MIT — see [LICENSE](LICENSE). `vendor/egui_commonmark_backend` is a lightly
patched copy of [egui_commonmark](https://github.com/lampsitter/egui_commonmark)'s
backend crate, used under its MIT/Apache-2.0 dual license (license files
included in the vendor directory).
