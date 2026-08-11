# clipd

Clipboard history for **KDE Plasma (Wayland)**.

Plasma owns the hotkey (default **Ctrl+D**). `clipd` never registers global shortcuts itself.

## Install (Arch / CachyOS)

```bash
sudo pacman -S --needed rust wl-clipboard wtype
git clone https://github.com/havedill/clipd.git
cd clipd
cargo install --path . --force
clipd install
```

That’s it. Then copy something and press **Ctrl+D**.

### Optional

```bash
# If CopyQ is fighting the same hotkey:
clipd install --disable-copyq
```

If Ctrl+D still goes to the browser: **System Settings → Keyboard → Shortcuts** → search **clipd** → set Ctrl+D → Apply (or log out once).

## Everyday use

| Action | How |
|--------|-----|
| History popup | Ctrl+D (or `clipd show`) |
| Settings / max items | Popup → **Settings** |
| Pause hotkey | Tray icon, or Settings |
| Status | `clipd status` |

## Config

`~/.config/clipd/config.toml` — `max_items`, `shortcut` (documented; Plasma binds it), window size.

History is encrypted on disk (`~/.local/share/clipd/history.db`); the key lives in KWallet / Secret Service.

## License

MIT — see [LICENSE](LICENSE).
