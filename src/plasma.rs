use anyhow::{Context, Result};
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
pub fn install_show_launcher(exe: &str, shortcut: &str) -> Result<()> {
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

    set_shortcut_enabled(true, shortcut)?;
    let _ = Command::new("kbuildsycoca6").args(["--noincremental"]).status();
    Ok(())
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

/// Enable or clear the Plasma global shortcut (used for pause).
pub fn set_shortcut_enabled(enabled: bool, shortcut: &str) -> Result<()> {
    let launch = if enabled {
        format!("{shortcut},none,clipd History")
    } else {
        "none,none,clipd History".into()
    };

    // Update X-KDE-Shortcuts in both desktop files.
    let xs = if enabled { shortcut } else { "" };
    for path in [applications_desktop()?, kglobalaccel_desktop()?] {
        if !path.exists() {
            continue;
        }
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
    if let Ok(path) = shortcutsrc() {
        if path.exists() {
            let mut text = fs::read_to_string(&path)?;
            // Drop any historically mangled section.
            if let Some(bad) = text.find("[services\\x5d\\x5bclipd-show.desktop]") {
                let after = &text[bad + 1..];
                let end = after
                    .find("\n[")
                    .map(|i| bad + 1 + i)
                    .unwrap_or(text.len());
                text.replace_range(bad..end, "");
            }
            let section = "[services][clipd-show.desktop]";
            let line = format!("_launch={launch}");
            if let Some(start) = text.find(section) {
                let after = &text[start + section.len()..];
                let end = after
                    .find("\n[")
                    .map(|i| start + section.len() + i)
                    .unwrap_or(text.len());
                let replacement = format!("{section}\n{line}\n");
                text.replace_range(start..end, &replacement);
            } else {
                text.push_str(&format!("\n{section}\n{line}\n"));
            }
            fs::write(&path, text)?;
        }
    }

    let _ = Command::new("kbuildsycoca6").args(["--noincremental"]).status();
    let _ = Command::new("qdbus6")
        .args([
            "org.kde.kglobalaccel",
            "/kglobalaccel",
            "org.kde.KGlobalAccel.blockGlobalShortcuts",
            "false",
        ])
        .status();
    Ok(())
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
