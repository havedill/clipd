# clipd

Clipboard history for **KDE Plasma on Wayland**.

Plasma owns the hotkey (default **Ctrl+D**). `clipd` does not register global shortcuts itself.

## Install

**Needs:** Rust (`cargo`), `wl-clipboard`, and `wtype` (or `ydotool`).

```bash
git clone https://github.com/havedill/clipd.git
cd clipd
cargo install --path . --force
clipd install
```

Copy something, then press **Ctrl+D**.

If the shortcut does nothing: **System Settings → Keyboard → Shortcuts** → search **clipd** → set your key → Apply (or log out once).

## Use

| Action | How |
|--------|-----|
| Open history | Ctrl+D / `clipd show` |
| Settings | Popup → **Settings** |
| Pause hotkey | Tray icon or Settings |
| Status | `clipd status` |

## Config

`~/.config/clipd/config.toml` — history limit, documented shortcut, window size.

History is stored encrypted under `~/.local/share/clipd/` (key in KWallet / Secret Service).

## License

MIT — see [LICENSE](LICENSE).
