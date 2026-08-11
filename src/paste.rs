use anyhow::{bail, Context, Result};
use std::process::{Command, Stdio};
use std::thread;
use std::time::Duration;

/// Inject Ctrl+V into the focused client. Tries wtype, then ydotool.
/// Prefer calling this *after* the history popup has closed and focus has returned.
pub fn inject_paste() -> Result<()> {
    let mut errors = Vec::new();

    if which("wtype") {
        match run_wtype() {
            Ok(()) => return Ok(()),
            Err(e) => errors.push(format!("wtype: {e:#}")),
        }
    }

    if which("ydotool") {
        match run_ydotool() {
            Ok(()) => return Ok(()),
            Err(e) => errors.push(format!("ydotool: {e:#}")),
        }
    }

    if errors.is_empty() {
        bail!("neither wtype nor ydotool found — install one to paste on select");
    }
    bail!("{}", errors.join("; "));
}

/// Copy is done; schedule paste after focus can leave the popup.
pub fn inject_paste_later(delay: Duration) {
    thread::spawn(move || {
        thread::sleep(delay);
        if let Err(e) = inject_paste() {
            // Clipboard is already set — user can Ctrl+V manually.
            eprintln!("clipd: paste inject (clipboard already set): {e:#}");
        }
    });
}

fn run_wtype() -> Result<()> {
    let status = Command::new("wtype")
        .args(["-M", "ctrl", "-P", "v", "-m", "ctrl"])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .context("spawn")?;
    if status.success() {
        Ok(())
    } else {
        bail!("exit {status}");
    }
}

fn run_ydotool() -> Result<()> {
    // KEY_LEFTCTRL=29, KEY_V=47
    let status = Command::new("ydotool")
        .args(["key", "29:1", "47:1", "47:0", "29:0"])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .context("spawn")?;
    if status.success() {
        Ok(())
    } else {
        bail!("exit {status}");
    }
}

fn which(bin: &str) -> bool {
    Command::new("which")
        .arg(bin)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}
