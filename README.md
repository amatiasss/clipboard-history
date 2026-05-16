# clipboard-history

Clipboard history manager for the [COSMIC Desktop](https://github.com/pop-os/cosmic-epoch) (Pop!_OS). Consists of three components:

- **clipboard-daemon** — background process that monitors the Wayland clipboard and persists history to disk
- **clipboard-applet** — COSMIC panel applet with search, keyboard navigation, and private mode
- **clipboard-launcher** — standalone floating launcher window (keyboard-driven, similar to app launchers)

## Features

- Clipboard history persisted at `~/.local/share/clipboard-history/history.json`
- Search/filter with real-time filtering
- Keyboard navigation (↑↓ or Ctrl+J/K to navigate, Enter to copy, Backspace to delete chars)
- Quick copy with Ctrl+1–9 (positions 1–9 in the filtered list)
- Delete individual entries (`d` key in launcher, Ctrl+K in applet, or delete button)
- Private mode (suspends capture while active) — applet only
- Emoji shortcode expansion (`:smile:` → 😄)
- i18n support (English and Brazilian Portuguese included)

## Requirements

- COSMIC Desktop (Pop!_OS 24.04 or later)
- `wl-clipboard` (`wl-paste`, `wl-copy`)
- Rust toolchain (for building from source)

## Installation

```bash
# Install wl-clipboard if not present
sudo apt install wl-clipboard

# Clone and build
git clone https://github.com/amatiasss/clipboard-history
cd clipboard-history
cargo build --release

# Install binaries
sudo install -m755 target/release/clipboard-applet /usr/bin/clipboard-applet
sudo install -m755 target/release/clipboard-daemon /usr/bin/clipboard-daemon
sudo install -m755 target/release/clipboard-launcher /usr/bin/clipboard-launcher

# Install systemd user service for the daemon
cp clipboard-daemon.service ~/.config/systemd/user/
systemctl --user enable --now clipboard-daemon

# Install desktop entries
cp com.github.clipboard-history.desktop ~/.local/share/applications/
cp crates/clipboard-launcher/com.github.clipboard-history.Launcher.desktop ~/.local/share/applications/
```

Then add the applet to your COSMIC panel via **Settings → Desktop → Panel → Add applet**.

To use the launcher, bind `clipboard-launcher` to a keyboard shortcut in your compositor settings.

## Development

### Project structure

```
clipboard-history/
├── crates/
│   ├── clipboard-applet/       # COSMIC panel applet (libcosmic)
│   │   ├── src/main.rs
│   │   └── i18n/
│   │       ├── en/clipboard_applet.ftl
│   │       └── pt-BR/clipboard_applet.ftl
│   ├── clipboard-launcher/     # Standalone floating launcher (libcosmic, winit)
│   │   ├── src/main.rs
│   │   ├── i18n/
│   │   │   ├── en/clipboard_launcher.ftl
│   │   │   └── pt-BR/clipboard_launcher.ftl
│   │   └── com.github.clipboard-history.Launcher.desktop
│   └── clipboard-daemon/       # Wayland clipboard monitor
│       └── src/main.rs
├── Cargo.toml                  # Workspace
├── clipboard-daemon.service    # systemd user service
└── com.github.clipboard-history.desktop
```

### Key technical decisions

- **No IPC between daemon and applet/launcher** — all three components read/write the same JSON file. The applet and launcher reload history on every open.
- **Launcher uses a layer surface** — rendered as a floating centered window via `SctkLayerSurfaceSettings` with `KeyboardInteractivity::Exclusive`. It closes on `Escape` or when it loses focus (`LayerEvent::Unfocused`).
- **Keyboard input via subscription** — the COSMIC panel injects `CTRL | LOGO` modifiers into all events. Key characters are captured via `Key::Character(c)` (not `text`), ignoring modifiers entirely.
- **xdg-popup does not receive keyboard focus** from the Wayland compositor until clicked. Keyboard input is captured on the panel's layer surface via `listen_with` subscription, which works regardless of popup focus.
- **Scroll sync** — uses `scrollable::snap_to` with a relative offset calculated from `selected_index / (count - 1)`.
- **i18n** — uses `i18n-embed` + `fluent`. Loader is a `once_cell::sync::Lazy<FluentLanguageLoader>`. The `fl!` macro wraps `i18n_embed_fl::fl!` with the global loader.
- **Private mode** — implemented as a sentinel file `.private` in the data dir. The daemon checks for it on every clipboard event; the applet toggles it.
- **History file locking** — `fs2` file locks are used on every read/write to prevent corruption when daemon and applet/launcher access the file concurrently.

### Launcher keyboard shortcuts

| Key | Action |
|-----|--------|
| Type | Filter entries |
| ↑ / ↓ or Ctrl+K / Ctrl+J | Navigate list |
| Enter | Copy selected entry |
| Ctrl+1–9 | Quick copy entry at position 1–9 |
| d | Delete selected entry |
| Backspace | Delete last search character |
| Escape | Close launcher |

### Build & deploy (development cycle)

```bash
# Applet
cargo build --release -p clipboard-applet \
  && sudo install -m755 target/release/clipboard-applet /usr/bin/clipboard-applet \
  && kill -9 $(pgrep -f clipboard-applet) 2>/dev/null

# Launcher
cargo build --release -p clipboard-launcher \
  && sudo install -m755 target/release/clipboard-launcher /usr/bin/clipboard-launcher
```

The COSMIC panel restarts the applet automatically after the process is killed.

### Adding a new language

Create the translation file for each crate that needs it:

- `crates/clipboard-applet/i18n/<lang-code>/clipboard_applet.ftl`
- `crates/clipboard-launcher/i18n/<lang-code>/clipboard_launcher.ftl`

Use the existing `en/` files as reference for the required keys.

### Known limitations

- xdg-popup does not receive keyboard focus from the Wayland compositor without a mouse click. Typing works immediately only after clicking inside the popup (applet only; the launcher has exclusive keyboard focus by design).
- No IPC: if the daemon is not running, no new entries are captured (existing history is still accessible).
- `wl-copy` child process is detached with `mem::forget` to avoid blocking the UI thread.
