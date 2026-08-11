use crate::config::Config;
use crate::plasma;
use anyhow::{Context, Result};
use std::fs;
use std::process::Command;

/// Install desktop entries + systemd user unit; optionally disable CopyQ conflict.
pub fn run(disable_copyq: bool) -> Result<()> {
    let exe = std::env::current_exe().context("current_exe")?;
    let exe = exe.canonicalize().unwrap_or(exe).display().to_string();
    let cfg = Config::load().unwrap_or_default();

    plasma::install_show_launcher(&exe, &cfg.shortcut)?;

    let autostart_dir = dirs::config_dir()
        .context("config dir")?
        .join("autostart");
    fs::create_dir_all(&autostart_dir)?;
    let auto = autostart_dir.join("clipd.desktop");
    fs::write(
        &auto,
        format!(
            "[Desktop Entry]\n\
             Type=Application\n\
             Name=clipd\n\
             Comment=Clipboard history daemon\n\
             Exec={exe} daemon\n\
             Icon=edit-paste\n\
             Terminal=false\n\
             X-GNOME-Autostart-enabled=true\n\
             X-KDE-autostart-phase=2\n"
        ),
    )?;
    println!("wrote {}", auto.display());

    let unit_dir = dirs::config_dir()
        .context("config dir")?
        .join("systemd/user");
    fs::create_dir_all(&unit_dir)?;
    let unit = unit_dir.join("clipd.service");
    fs::write(
        &unit,
        format!(
            "[Unit]\n\
             Description=clipd clipboard history daemon\n\
             After=graphical-session.target\n\
             PartOf=graphical-session.target\n\
             \n\
             [Service]\n\
             ExecStart={exe} daemon\n\
             Restart=on-failure\n\
             RestartSec=2\n\
             \n\
             [Install]\n\
             WantedBy=graphical-session.target\n"
        ),
    )?;
    println!("wrote {}", unit.display());
    let _ = Command::new("systemctl")
        .args(["--user", "daemon-reload"])
        .status();
    let _ = Command::new("systemctl")
        .args(["--user", "enable", "--now", "clipd.service"])
        .status();

    // Clear CopyQ Plasma service shortcut so it cannot steal the key.
    clear_copyq_plasma_shortcut()?;

    if disable_copyq {
        disable_copyq_bits()?;
    } else {
        println!("hint: re-run with --disable-copyq to turn off CopyQ autostart/hotkey conflict");
    }

    println!(
        "done. Global shortcut via ~/.local/share/kglobalaccel/clipd-show.desktop ({})\n\
         If apps still steal the key: System Settings → Keyboard → Shortcuts → clipd History → re-bind, then Apply.\n\
         Or log out/in once so Plasma reloads kglobalaccel.",
        cfg.shortcut
    );
    Ok(())
}

fn clear_copyq_plasma_shortcut() -> Result<()> {
    let path = dirs::config_dir()
        .context("config dir")?
        .join("kglobalshortcutsrc");
    if !path.exists() {
        return Ok(());
    }
    let mut text = fs::read_to_string(&path)?;
    let section = "[services][copyq-tray-menu.desktop]";
    let line = "_launch=none,none,CopyQ Tray Menu";
    if let Some(start) = text.find(section) {
        let after = &text[start + section.len()..];
        let end = after
            .find("\n[")
            .map(|i| start + section.len() + i)
            .unwrap_or(text.len());
        text.replace_range(start..end, &format!("{section}\n{line}\n"));
        fs::write(&path, text)?;
    }
    Ok(())
}

fn disable_copyq_bits() -> Result<()> {
    let autostart = dirs::config_dir()
        .context("config dir")?
        .join("autostart");

    for name in ["com.github.hluk.copyq.desktop", "copyq-resume.desktop"] {
        let p = autostart.join(name);
        if !p.exists() {
            continue;
        }
        if p.symlink_metadata()
            .map(|m| m.file_type().is_symlink())
            .unwrap_or(false)
        {
            fs::remove_file(&p)?;
            fs::write(
                &p,
                "[Desktop Entry]\nType=Application\nName=CopyQ (disabled by clipd)\nHidden=true\n",
            )?;
            println!("replaced symlink + disabled {}", p.display());
            continue;
        }
        let mut t = fs::read_to_string(&p)?;
        if !t.contains("Hidden=true") {
            if !t.ends_with('\n') {
                t.push('\n');
            }
            t.push_str("Hidden=true\n");
            fs::write(&p, t)?;
            println!("disabled autostart {}", p.display());
        }
    }

    let _ = Command::new("systemctl")
        .args(["--user", "disable", "--now", "copyq-resume.service"])
        .status();
    let _ = Command::new("copyq").args(["exit"]).status();

    let copyq_cmds = dirs::config_dir()
        .context("config dir")?
        .join("copyq/copyq-commands.ini");
    if copyq_cmds.exists() {
        match fs::read_to_string(&copyq_cmds) {
            Ok(mut t) => {
                t = t.replace("IsGlobalShortcut=true", "IsGlobalShortcut=false");
                if let Err(e) = fs::write(&copyq_cmds, t) {
                    eprintln!("warn: could not update {}: {e}", copyq_cmds.display());
                } else {
                    println!("set IsGlobalShortcut=false in {}", copyq_cmds.display());
                }
            }
            Err(e) => eprintln!("warn: could not read {}: {e}", copyq_cmds.display()),
        }
    }

    println!("CopyQ conflict bits disabled (autostart hidden, resume unit disabled).");
    Ok(())
}
