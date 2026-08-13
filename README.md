# clipd

Clipboard history for **KDE Plasma on Wayland**.

Plasma owns the hotkey (default **Meta+Shift+V**). `clipd` does not register global shortcuts itself.

## Install

**Needs:** Rust (`cargo`), `wl-clipboard`, and `wtype` (or `ydotool`).

```bash
git clone https://github.com/havedill/clipd.git
cd clipd
cargo install --path . --force
~/.cargo/bin/clipd install
```

`cargo install` writes `~/.cargo/bin/clipd`. `clipd install` copies that into `~/.local/bin/clipd` (usually first on `PATH`) and wires the systemd unit + Plasma shortcut to it. Always run install via `~/.cargo/bin/clipd install` after a cargo build so a stale `~/.local/bin/clipd` cannot shadow the new binary.

Copy something, then press **Meta+Shift+V** (Super+Shift+V).

If the shortcut does nothing: **System Settings → Keyboard → Shortcuts** → search **clipd** → set your key → Apply (or log out once).

## Use

| Action | How |
|--------|-----|
| Open history | Meta+Shift+V / `clipd show` |
| Settings | Popup → **Settings** |
| Pause hotkey | Tray icon or Settings |
| Status | `clipd status` |

Pausing temporarily releases only the Plasma global shortcut so another app can use it.
Clipboard capture and history remain active, and restarting the daemon resumes the shortcut.

## Config

`~/.config/clipd/config.toml` — history limit, documented shortcut, window size.

History is stored encrypted under `~/.local/share/clipd/` (key in KWallet / Secret Service).

## License

MIT — see [LICENSE](LICENSE).
