use anyhow::{bail, Context, Result};
use std::fs;
use std::path::PathBuf;
use std::process::Command;

const DESKTOP_NAME: &str = "clipd-show.desktop";

fn applications_desktop() -> Result<PathBuf> {
    Ok(dirs::data_local_dir()
        .context("data local dir")?
        .join("applications")
        .join(DESKTOP_NAME))
}

fn kglobalaccel_desktop() -> Result<PathBuf> {
    Ok(dirs::data_local_dir()
        .context("data local dir")?
        .join("kglobalaccel")
        .join(DESKTOP_NAME))
}

fn shortcutsrc() -> Result<PathBuf> {
    Ok(dirs::config_dir()
        .context("config dir")?
        .join("kglobalshortcutsrc"))
}

/// Write/update desktop entries Plasma uses for a *real* global grab.
pub fn write_show_launcher(exe: &str, shortcut: &str) -> Result<()> {
    parse_qt_shortcut(shortcut)?;
    let body = desktop_body(exe, shortcut);
    let apps = applications_desktop()?;
    if let Some(p) = apps.parent() {
        fs::create_dir_all(p)?;
    }
    fs::write(&apps, &body)?;
    println!("wrote {}", apps.display());

    // Plasma 6: shortcuts in ~/.local/share/kglobalaccel/ are real global accels
    // (they beat focused apps). A plain [services] launcher entry often loses.
    let kg = kglobalaccel_desktop()?;
    if let Some(p) = kg.parent() {
        fs::create_dir_all(p)?;
    }
    fs::write(&kg, &body)?;
    println!("wrote {}", kg.display());
    Ok(())
}

/// Write launcher desktop files and bind the shortcut in Plasma.
pub fn install_show_launcher(exe: &str, shortcut: &str) -> Result<()> {
    write_show_launcher(exe, shortcut)?;
    set_shortcut_enabled(true, shortcut)
}

/// Bind `shortcut`, or fall back to `Config::default().shortcut` when Plasma rejects it.
pub fn install_show_launcher_with_fallback(
    exe: &str,
    cfg: &mut crate::config::Config,
) -> Result<String> {
    if let Err(e) = install_show_launcher(exe, &cfg.shortcut) {
        let configured = cfg.shortcut.clone();
        let fallback = crate::config::Config::default().shortcut;
        eprintln!("clipd: shortcut `{configured}` failed: {e:#}");
        if configured == fallback {
            eprintln!("clipd: continuing without a global shortcut (use tray or `clipd show`)");
            return Ok(configured);
        }
        eprintln!("clipd: falling back to `{fallback}`");
        cfg.shortcut = fallback.clone();
        cfg.save()?;
        if let Err(e) = install_show_launcher(exe, &fallback) {
            eprintln!("clipd: fallback shortcut failed: {e:#}");
            eprintln!("clipd: continuing without a global shortcut (use tray or `clipd show`)");
        }
        return Ok(fallback);
    }
    Ok(cfg.shortcut.clone())
}

fn desktop_body(exe: &str, shortcut: &str) -> String {
    format!(
        "[Desktop Entry]\n\
         Type=Application\n\
         Name=clipd History\n\
         Comment=Show clipboard history (Plasma binds the hotkey)\n\
         Exec={exe} show\n\
         Icon=edit-paste\n\
         Terminal=false\n\
         OnlyShowIn=KDE;\n\
         StartupNotify=false\n\
         X-KDE-Shortcuts={shortcut}\n"
    )
}

/// Update the Plasma global shortcut binding (desktop files + kglobalshortcutsrc + live accel).
pub fn set_shortcut_enabled(enabled: bool, shortcut: &str) -> Result<()> {
    let keys = if enabled {
        parse_qt_shortcut(shortcut)?
    } else {
        Vec::new()
    };
    let launch = if enabled {
        format!("{shortcut},none,clipd History")
    } else {
        "none,none,clipd History".into()
    };

    // Update X-KDE-Shortcuts in both desktop files.
    let xs = if enabled { shortcut } else { "" };
    let launchers = [applications_desktop()?, kglobalaccel_desktop()?];
    if let Some(path) = launchers.iter().find(|path| !path.exists()) {
        bail!(
            "missing Plasma launcher {}; run `clipd install`",
            path.display()
        );
    }
    for path in launchers {
        let mut text = fs::read_to_string(&path)?;
        if let Some((head, rest)) = text.split_once("X-KDE-Shortcuts=") {
            let after = rest.find('\n').map(|i| &rest[i..]).unwrap_or("");
            text = format!("{head}X-KDE-Shortcuts={xs}{after}");
        } else {
            if !text.ends_with('\n') {
                text.push('\n');
            }
            text.push_str(&format!("X-KDE-Shortcuts={xs}\n"));
        }
        fs::write(&path, text)?;
    }

    // Patch kglobalshortcutsrc directly (kwriteconfig6 mangles nested [services][…] groups).
    let path = shortcutsrc()?;
    let text = if path.exists() {
        fs::read_to_string(&path)?
    } else {
        String::new()
    };
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(&path, patch_shortcutsrc(&text, &launch))?;

    let status = Command::new("kbuildsycoca6")
        .args(["--noincremental"])
        .status()
        .context("run kbuildsycoca6")?;
    if !status.success() {
        bail!("kbuildsycoca6 failed with {status}");
    }

    // Update the *live* binding — file edits alone leave the old chord active.
    set_live_shortcut_keys(&keys, shortcut)?;
    Ok(())
}

/// Qt::Key_* + modifier bits used by KGlobalAccel (letter/digit chords only).
fn parse_qt_shortcut(shortcut: &str) -> Result<Vec<i32>> {
    let mut mods = 0i32;
    let mut key = None;
    for part in shortcut.split('+') {
        let p = part.trim();
        if p.is_empty() {
            bail!("invalid shortcut `{shortcut}`");
        }
        match p.to_ascii_lowercase().as_str() {
            "meta" | "super" | "win" | "mod4" => mods |= 0x1000_0000,
            "ctrl" | "control" => mods |= 0x0400_0000,
            "alt" => mods |= 0x0800_0000,
            "shift" => mods |= 0x0200_0000,
            one if one.len() == 1 => {
                if key.is_some() {
                    bail!("shortcut must contain exactly one key");
                }
                let c = one.chars().next().unwrap().to_ascii_uppercase();
                if !c.is_ascii_alphanumeric() {
                    bail!("shortcut key must be an ASCII letter or digit");
                }
                key = Some(c as i32);
            }
            _ => bail!(
                "unsupported shortcut part `{p}`; use modifiers plus one ASCII letter or digit"
            ),
        }
    }
    if mods == 0 {
        bail!("shortcut must contain at least one modifier");
    }
    let key = key.context("shortcut must contain exactly one key")?;
    Ok(vec![mods | key])
}

fn patch_shortcutsrc(text: &str, launch: &str) -> String {
    let mut lines: Vec<String> = text.lines().map(str::to_string).collect();
    remove_section(
        &mut lines,
        "[services\\x5d\\x5bclipd-show.desktop]",
    );
    let line = format!("_launch={launch}");
    upsert_section_key(&mut lines, "[services][clipd-show.desktop]", &line);
    upsert_section_key(&mut lines, "[clipd-show.desktop]", &line);
    let mut text = lines.join("\n");
    text.push('\n');
    text
}

fn remove_section(lines: &mut Vec<String>, section: &str) {
    let Some(start) = lines.iter().position(|line| line.trim() == section) else {
        return;
    };
    let end = lines[start + 1..]
        .iter()
        .position(|line| line.trim_start().starts_with('['))
        .map(|offset| start + 1 + offset)
        .unwrap_or(lines.len());
    lines.drain(start..end);
}

fn upsert_section_key(lines: &mut Vec<String>, section: &str, value: &str) {
    if let Some(start) = lines.iter().position(|line| line.trim() == section) {
        let end = lines[start + 1..]
            .iter()
            .position(|line| line.trim_start().starts_with('['))
            .map(|offset| start + 1 + offset)
            .unwrap_or(lines.len());
        if let Some(key) = lines[start + 1..end]
            .iter()
            .position(|line| line.trim_start().starts_with("_launch="))
            .map(|offset| start + 1 + offset)
        {
            lines[key] = value.to_string();
        } else {
            lines.insert(start + 1, value.to_string());
        }
        return;
    }
    if lines.last().is_some_and(|line| !line.is_empty()) {
        lines.push(String::new());
    }
    lines.push(section.to_string());
    lines.push(value.to_string());
}

fn set_live_shortcut_keys(keys: &[i32], shortcut: &str) -> Result<()> {
    // QKeySequence over D-Bus is up to 4 ints per chord.
    let keys_arg = if keys.is_empty() {
        "@a(ai) []".to_string()
    } else {
        let inner = keys
            .iter()
            .map(|k| format!("([{k}, 0, 0, 0],)"))
            .collect::<Vec<_>>()
            .join(", ");
        format!("@a(ai) [{inner}]")
    };
    let action = "['clipd-show.desktop', '_launch', 'clipd History', 'clipd History']";
    if let Some(key) = keys.first() {
        if let Some(owner) = shortcut_conflict(*key)? {
            bail!("shortcut `{shortcut}` is already used by {owner}");
        }
    }
    // setShortcutKeys checks conflicts and returns the keys Plasma accepted.
    // SetPresent | NoAutoloading means this is an explicit user change.
    let output = Command::new("gdbus")
        .args([
            "call",
            "--session",
            "--dest",
            "org.kde.kglobalaccel",
            "--object-path",
            "/kglobalaccel",
            "--method",
            "org.kde.KGlobalAccel.setShortcutKeys",
            action,
            &keys_arg,
            "6",
        ])
        .output()
        .context("gdbus setShortcutKeys")?;
    if !output.status.success() {
        anyhow::bail!(
            "gdbus setShortcutKeys failed: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        );
    }
    if !keys.is_empty() && String::from_utf8_lossy(&output.stdout).contains("@a(ai) []") {
        bail!("shortcut `{shortcut}` is already used by another Plasma action");
    }

    // Notify the desktop-file action owner after Plasma accepts the binding.
    let output = Command::new("gdbus")
        .args([
            "call",
            "--session",
            "--dest",
            "org.kde.kglobalaccel",
            "--object-path",
            "/kglobalaccel",
            "--method",
            "org.kde.KGlobalAccel.setForeignShortcutKeys",
            action,
            &keys_arg,
        ])
        .output()
        .context("gdbus setForeignShortcutKeys")?;
    if !output.status.success() {
        anyhow::bail!(
            "gdbus failed: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        );
    }
    Ok(())
}

fn shortcut_conflict(key: i32) -> Result<Option<String>> {
    let output = Command::new("gdbus")
        .args([
            "call",
            "--session",
            "--dest",
            "org.kde.kglobalaccel",
            "--object-path",
            "/kglobalaccel",
            "--method",
            "org.kde.KGlobalAccel.getGlobalShortcutsByKey",
            &key.to_string(),
        ])
        .output()
        .context("gdbus getGlobalShortcutsByKey")?;
    if !output.status.success() {
        bail!(
            "gdbus getGlobalShortcutsByKey failed: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        );
    }
    Ok(parse_shortcut_conflict(&String::from_utf8_lossy(
        &output.stdout,
    )))
}

fn parse_shortcut_conflict(output: &str) -> Option<String> {
    let fields = quoted_fields(output);
    for action in fields.chunks_exact(6) {
        if action[2] != DESKTOP_NAME {
            return Some(format!("{} — {}", action[3], action[1]));
        }
    }
    None
}

fn quoted_fields(text: &str) -> Vec<String> {
    let mut fields = Vec::new();
    let mut chars = text.chars();
    while let Some(c) = chars.next() {
        if c != '\'' {
            continue;
        }
        let mut field = String::new();
        while let Some(c) = chars.next() {
            match c {
                '\\' => {
                    if let Some(escaped) = chars.next() {
                        field.push(escaped);
                    }
                }
                '\'' => break,
                _ => field.push(c),
            }
        }
        fields.push(field);
    }
    fields
}

#[cfg(test)]
mod tests {
    use super::{parse_qt_shortcut, parse_shortcut_conflict, patch_shortcutsrc};

    #[test]
    fn parses_supported_shortcuts() {
        assert_eq!(
            parse_qt_shortcut("Meta+V").unwrap(),
            vec![0x1000_0000 | i32::from(b'V')]
        );
        assert_eq!(
            parse_qt_shortcut("shift+ctrl+7").unwrap(),
            vec![0x0200_0000 | 0x0400_0000 | i32::from(b'7')]
        );
    }

    #[test]
    fn rejects_ambiguous_or_unsupported_shortcuts() {
        assert!(parse_qt_shortcut("V").is_err());
        assert!(parse_qt_shortcut("Meta+A+B").is_err());
        assert!(parse_qt_shortcut("Meta+F12").is_err());
        assert!(parse_qt_shortcut("Meta+/").is_err());
    }

    #[test]
    fn patches_both_plasma_sections_without_corrupting_others() {
        let input = "[General]\nfoo=bar\n[services][clipd-show.desktop]\nold=value\n_launch=old\n[Other]\nkeep=yes\n";
        let output = patch_shortcutsrc(input, "Meta+V,none,clipd History");

        assert!(output.contains("[General]\nfoo=bar"));
        assert!(output.contains("[Other]\nkeep=yes"));
        assert!(output.contains(
            "[services][clipd-show.desktop]\nold=value\n_launch=Meta+V,none,clipd History"
        ));
        assert!(output.contains("[clipd-show.desktop]\n_launch=Meta+V,none,clipd History"));
        assert_eq!(output.matches("_launch=Meta+V,none,clipd History").count(), 2);
    }

    #[test]
    fn detects_other_shortcut_owner_but_ignores_clipd() {
        let conflict = "([('Show Desktop', 'Peek at Desktop', 'kwin', 'KWin', 'default', 'Default Context', [268435524], [268435524]), ('_launch', 'clipd History', 'clipd-show.desktop', 'clipd History', 'default', 'Default Context', [268435524], [67108932])],)";
        assert_eq!(
            parse_shortcut_conflict(conflict).as_deref(),
            Some("KWin — Peek at Desktop")
        );

        let ours = "([('_launch', 'clipd History', 'clipd-show.desktop', 'clipd History', 'default', 'Default Context', [268435542], [268435542])],)";
        assert_eq!(parse_shortcut_conflict(ours), None);
    }
}

/// Move the mapped `clipd` window to the pointer. Wayland clients cannot set
/// absolute position themselves; KWin scripting can.
///
/// When `size` is set (`[width, height]` in points), also force the frame size —
/// Wayland often ignores the client's initial `with_inner_size`.
pub fn move_show_to_cursor(size: Option<[f32; 2]>) -> Result<()> {
    let dir = dirs::cache_dir()
        .unwrap_or_else(|| PathBuf::from("/tmp"))
        .join("clipd");
    fs::create_dir_all(&dir)?;
    let script = dir.join("place-near-cursor.js");

    let (force_w, force_h, force_size) = match size {
        Some([w, h]) if w >= 320.0 && h >= 240.0 => {
            (w.round() as i32, h.round() as i32, "true")
        }
        _ => (0, 0, "false"),
    };

    fs::write(
        &script,
        format!(
            r#"(function () {{
    var FORCE_SIZE = {force_size};
    var WANT_W = {force_w};
    var WANT_H = {force_h};
    function place(w) {{
        var cls = (w.resourceClass || "") + "";
        var cap = (w.caption || "") + "";
        var nam = (w.resourceName || "") + "";
        if (cls !== "clipd" && cap !== "clipd" && nam !== "clipd") return false;

        var pos = workspace.cursorPos;
        var g = w.frameGeometry;
        // WANT_* is egui content size; add a little for title-bar chrome when forcing.
        var winW = FORCE_SIZE ? (WANT_W + 0) : g.width;
        var winH = FORCE_SIZE ? (WANT_H + 36) : g.height;
        var area;
        try {{
            area = workspace.clientArea(KWin.MaximizeArea, w);
        }} catch (e) {{
            area = {{ x: 0, y: 0, width: 3840, height: 2160 }};
        }}

        var margin = 12;
        var x = Math.round(pos.x + margin);
        var y = Math.round(pos.y + margin);

        if (y + winH > area.y + area.height) {{
            y = Math.round(pos.y - margin - winH);
        }}
        if (x + winW > area.x + area.width) {{
            x = Math.round(pos.x - margin - winW);
        }}

        var maxX = area.x + area.width - winW;
        var maxY = area.y + area.height - winH;
        if (maxX < area.x) maxX = area.x;
        if (maxY < area.y) maxY = area.y;
        x = Math.max(area.x, Math.min(x, maxX));
        y = Math.max(area.y, Math.min(y, maxY));

        try {{
            w.frameGeometry = Qt.rect(x, y, winW, winH);
        }} catch (e) {{
            w.frameGeometry = {{ x: x, y: y, width: winW, height: winH }};
        }}
        return true;
    }}
    var list = workspace.stackingOrder;
    for (var i = 0; i < list.length; i++) {{
        if (place(list[i])) return;
    }}
}})();
"#
        ),
    )?;

    let path = script.display().to_string();
    let _ = Command::new("qdbus6")
        .args([
            "org.kde.KWin",
            "/Scripting",
            "org.kde.kwin.Scripting.unloadScript",
            "clipd-pos",
        ])
        .status();

    let out = Command::new("qdbus6")
        .args([
            "org.kde.KWin",
            "/Scripting",
            "org.kde.kwin.Scripting.loadScript",
            &path,
            "clipd-pos",
        ])
        .output()
        .context("kwin loadScript")?;
    let id = String::from_utf8_lossy(&out.stdout).trim().to_string();
    if id.is_empty() {
        anyhow::bail!(
            "kwin loadScript failed: {}",
            String::from_utf8_lossy(&out.stderr)
        );
    }
    let script_path = format!("/Scripting/Script{id}");
    let _ = Command::new("qdbus6")
        .args([
            "org.kde.KWin",
            &script_path,
            "org.kde.kwin.Script.run",
        ])
        .status();
    Ok(())
}
