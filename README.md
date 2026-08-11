# clipd

Minimal clipboard history for **KDE Plasma on Wayland**.

Plasma owns the global hotkey (default Ctrl+D → `clipd show`). This binary never registers global shortcuts itself — that pattern is what breaks for many clipboard managers after Plasma / `kglobalacceld` updates.

## Features

- Text and image history
- Encrypted-at-rest SQLite (AES-256-GCM; key in KWallet / Secret Service)
- Configurable `max_items`
- Autostart via systemd user unit + desktop file
- Tray menu: show history, pause/resume the Plasma shortcut
- Popup: search, preview, settings, close on unfocus, place near cursor (via KWin)

## Build

```bash
cargo build --release
cargo install --path . --force
```

Requires a recent Rust toolchain (`edition = "2021"`).

## Runtime dependencies

| Tool | Role |
|------|------|
| `wl-clipboard` | Watch / set clipboard (`wl-paste`, `wl-copy`) |
| `wtype` or `ydotool` | Inject Ctrl+V on select (`wtype` preferred) |
| KWallet / Secret Service | Store the encryption key |
| Plasma / KWin | Global shortcut + cursor placement |

```bash
# Arch / CachyOS example
sudo pacman -S --needed wl-clipboard wtype
```

## Setup

```bash
clipd install                 # desktop files, systemd user unit, Plasma shortcut
# optional, if migrating away from CopyQ:
clipd install --disable-copyq

systemctl --user status clipd
clipd status
clipd show
```

If the hotkey does not fire while a browser has focus, open **System Settings → Keyboard → Shortcuts**, find **clipd History** (or re-run `clipd install`), bind your key, Apply — or log out/in once so Plasma reloads global accel.

## Paths

| What | Where |
|------|--------|
| Config | `~/.config/clipd/config.toml` |
| Encrypted DB | `~/.local/share/clipd/history.db` |
| Socket | `$XDG_RUNTIME_DIR/clipd.sock` |
| Shortcut desktop | `~/.local/share/kglobalaccel/clipd-show.desktop` |

## Security notes

- History payloads on disk are ciphertext. Without the Secret Service key, the DB is not readable as plaintext.
- The daemon and UI talk over a Unix socket in `XDG_RUNTIME_DIR` (same-user only).
- Pausing the shortcut unbinds the Plasma hotkey temporarily so apps can use that key chord.

## License

MIT — see [LICENSE](LICENSE).
