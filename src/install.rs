use crate::config::Config;
use crate::plasma;
use anyhow::{Context, Result};
use std::fs;
use std::process::Command;

/// Install desktop entries, systemd user unit, and Plasma global shortcut.
pub fn run() -> Result<()> {
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

    println!(
        "done. Shortcut: {} → clipd show\n\
         If the key does nothing: System Settings → Keyboard → Shortcuts → clipd History → bind your key → Apply\n\
         (or log out once so Plasma reloads shortcuts).",
        cfg.shortcut
    );
    Ok(())
}
