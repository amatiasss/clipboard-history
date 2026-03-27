# clipboard-history

Clipboard history manager for the [COSMIC Desktop](https://github.com/pop-os/cosmic-epoch) (Pop!_OS). Consists of two components:

- **clipboard-daemon** — background process that monitors the Wayland clipboard and persists history to disk
- **clipboard-applet** — COSMIC panel applet with search, keyboard navigation, and private mode

## Features

- Clipboard history persisted at `~/.local/share/clipboard-history/history.json`
- Search/filter with real-time filtering
- Keyboard navigation (↑↓ to navigate, Enter to copy, Backspace to delete chars, Ctrl+K to focus search)
- Private mode (suspends capture while active)
- Delete individual entries or all filtered entries at once
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
git clone https://github.com/<your-user>/clipboard-history
cd clipboard-history
cargo build --release

# Install binaries
sudo install -m755 target/release/clipboard-applet /usr/bin/clipboard-applet
sudo install -m755 target/release/clipboard-daemon /usr/bin/clipboard-daemon

# Install systemd user service for the daemon
cp clipboard-daemon.service ~/.config/systemd/user/
systemctl --user enable --now clipboard-daemon

# Install desktop entry for the applet
cp com.github.clipboard-history.desktop ~/.local/share/applications/
```

Then add the applet to your COSMIC panel via **Settings → Desktop → Panel → Add applet**.

## Development

### Project structure

```
clipboard-history/
├── crates/
│   ├── clipboard-applet/       # COSMIC panel applet (libcosmic)
│   │   ├── src/main.rs         # All applet logic (single file)
│   │   └── i18n/               # Fluent translation files
│   │       ├── en/clipboard_applet.ftl
│   │       └── pt-BR/clipboard_applet.ftl
│   └── clipboard-daemon/       # Wayland clipboard monitor
│       └── src/main.rs
├── Cargo.toml                  # Workspace
├── clipboard-daemon.service    # systemd user service
└── com.github.clipboard-history.desktop
```

### Key technical decisions

- **No IPC between daemon and applet** — both read/write the same JSON file. The applet reloads history on every popup open.
- **Keyboard input via subscription** — the COSMIC panel injects `CTRL | LOGO` modifiers into all events. Key characters are captured via `Key::Character(c)` (not `text`), ignoring modifiers entirely.
- **xdg-popup does not receive keyboard focus** from the Wayland compositor until clicked. Keyboard input is captured on the panel's layer surface via `listen_with` subscription, which works regardless of popup focus.
- **Scroll sync** — uses `scrollable::snap_to` with a relative offset calculated from `selected_index / (count - 1)`.
- **i18n** — uses `i18n-embed` + `fluent`. Loader is a `once_cell::sync::Lazy<FluentLanguageLoader>`. The `fl!` macro wraps `i18n_embed_fl::fl!` with the global loader.
- **Private mode** — implemented as a sentinel file `.private` in the data dir. The daemon checks for it on every clipboard event; the applet toggles it.

### Build & deploy (development cycle)

```bash
cargo build --release -p clipboard-applet \
  && install -m755 target/release/clipboard-applet ~/.local/bin/clipboard-applet \
  && sudo install -m755 target/release/clipboard-applet /usr/bin/clipboard-applet \
  && kill -9 $(pgrep -f clipboard-applet) 2>/dev/null
```

The COSMIC panel restarts the applet automatically after the process is killed.

### Adding a new language

Create `crates/clipboard-applet/i18n/<lang-code>/clipboard_applet.ftl` with the same keys as `i18n/en/clipboard_applet.ftl`.

### Known limitations

- xdg-popup does not receive keyboard focus from the Wayland compositor without a mouse click. Typing works immediately only after clicking inside the popup.
- No IPC: if the daemon is not running, no new entries are captured (existing history is still accessible).
- `wl-copy` child process is detached with `mem::forget` to avoid blocking the UI thread.
