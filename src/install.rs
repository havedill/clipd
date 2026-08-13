use crate::config::Config;
use crate::plasma;
use anyhow::{bail, Context, Result};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::SystemTime;

/// Install desktop entries, systemd user unit, and Plasma global shortcut.
pub fn run() -> Result<()> {
    let running = std::env::current_exe().context("current_exe")?;
    let running = running.canonicalize().unwrap_or(running);
    ensure_not_stale_shadow(&running)?;

    // Stop the running daemon first — Linux can't overwrite an executing binary
    // ("Text file busy") when we refresh ~/.local/bin/clipd.
    let _ = Command::new("systemctl")
        .args(["--user", "stop", "clipd.service"])
        .status();

    // Canonical runtime path: ~/.local/bin/clipd (often earlier on PATH than ~/.cargo/bin).
    let dest = local_bin_clipd()?;
    if let Some(parent) = dest.parent() {
        fs::create_dir_all(parent)?;
    }
    if running != dest {
        fs::copy(&running, &dest)
            .with_context(|| format!("copy {} → {}", running.display(), dest.display()))?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mut perms = fs::metadata(&dest)?.permissions();
            perms.set_mode(0o755);
            fs::set_permissions(&dest, perms)?;
        }
        println!("installed binary → {}", dest.display());
    } else if let Some(cargo) = cargo_bin_clipd() {
        // Running from ~/.local/bin already; refresh it from cargo when newer.
        let cargo = cargo.canonicalize().unwrap_or(cargo);
        if let (Some(d), Some(c)) = (mtime(&dest), mtime(&cargo)) {
            if c > d {
                fs::copy(&cargo, &dest)
                    .with_context(|| format!("copy {} → {}", cargo.display(), dest.display()))?;
                println!("refreshed binary ← {}", cargo.display());
            }
        }
    }
    let exe = dest.display().to_string();
    let mut cfg = Config::load().unwrap_or_default();

    // Never abort install before the daemon is restarted — a rejected shortcut
    // must not leave clipd stopped.
    let shortcut = plasma::install_show_launcher_with_fallback(&exe, &mut cfg)?;

    // systemd is the single startup owner. Older installs also created this
    // entry, which could start a second daemon and steal the IPC socket.
    let old_autostart = dirs::config_dir()
        .context("config dir")?
        .join("autostart/clipd.desktop");
    if old_autostart.exists() {
        fs::remove_file(&old_autostart)?;
        println!("removed {}", old_autostart.display());
    }

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
    let status = Command::new("systemctl")
        .args(["--user", "daemon-reload"])
        .status()
        .context("systemctl --user daemon-reload")?;
    if !status.success() {
        bail!("systemctl --user daemon-reload failed with {status}");
    }
    let status = Command::new("systemctl")
        .args(["--user", "enable", "--now", "clipd.service"])
        .status()
        .context("systemctl --user enable clipd.service")?;
    if !status.success() {
        bail!("systemctl --user enable --now clipd.service failed with {status}");
    }

    println!(
        "done. Shortcut: {} → clipd show\n\
         If the key does nothing: System Settings → Keyboard → Shortcuts → clipd History → bind your key → Apply\n\
         (or log out once so Plasma reloads shortcuts).",
        shortcut
    );
    Ok(())
}

fn local_bin_clipd() -> Result<PathBuf> {
    let home = dirs::home_dir().context("home dir")?;
    Ok(home.join(".local/bin/clipd"))
}

fn cargo_bin_clipd() -> Option<PathBuf> {
    let home = dirs::home_dir()?;
    let p = home.join(".cargo/bin/clipd");
    p.exists().then_some(p)
}

fn mtime(path: &Path) -> Option<SystemTime> {
    fs::metadata(path).ok()?.modified().ok()
}

/// `cargo install` writes ~/.cargo/bin; an older ~/.local/bin/clipd often shadows it on PATH.
fn ensure_not_stale_shadow(running: &Path) -> Result<()> {
    let Some(cargo) = cargo_bin_clipd() else {
        return Ok(());
    };
    let cargo = cargo.canonicalize().unwrap_or(cargo);
    if cargo == running {
        return Ok(());
    }
    let run_m = mtime(running);
    let cargo_m = mtime(&cargo);
    if let (Some(r), Some(c)) = (run_m, cargo_m) {
        if c > r {
            bail!(
                "refusing install from stale binary.\n\
                 Ran:    {}\n\
                 Newer:  {}\n\
                 Fix:    {} install",
                running.display(),
                cargo.display(),
                cargo.display()
            );
        }
    }
    Ok(())
}
