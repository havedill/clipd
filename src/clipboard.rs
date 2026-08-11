use anyhow::{bail, Context, Result};
use std::io::{BufRead, BufReader};
use std::process::{Command, Stdio};
use std::time::Duration;

/// Spawn `wl-paste --watch` and call `on_change` whenever the clipboard changes.
pub fn watch_loop(mut on_change: impl FnMut() -> Result<()>) -> Result<()> {
    let mut child = Command::new("wl-paste")
        .args(["--watch", "echo", "x"])
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .context("spawn wl-paste --watch (is wl-clipboard installed?)")?;

    let out = child.stdout.take().context("wl-paste stdout")?;
    let mut lines = BufReader::new(out).lines();
    while let Some(line) = lines.next() {
        let _ = line.context("wl-paste watch line")?;
        // Debounce; consecutive-identical dedupe in the store covers the rest.
        std::thread::sleep(Duration::from_millis(50));
        if let Err(e) = on_change() {
            eprintln!("clipd: ingest error: {e:#}");
        }
    }
    bail!("wl-paste --watch exited");
}

pub fn read_clipboard() -> Result<Option<(String, Vec<u8>)>> {
    if let Some(data) = try_type("text")? {
        if !data.is_empty() {
            return Ok(Some(("text/plain".into(), data)));
        }
    }
    for mime in ["image/png", "image/jpeg", "image/bmp", "image/webp"] {
        if let Some(data) = try_type(mime)? {
            if !data.is_empty() {
                return Ok(Some((mime.into(), data)));
            }
        }
    }
    Ok(None)
}

fn try_type(mime: &str) -> Result<Option<Vec<u8>>> {
    let mut cmd = Command::new("wl-paste");
    cmd.arg("--type").arg(mime);
    // --no-newline is text-only; it would truncate binary image data.
    if mime == "text" || mime.starts_with("text/") {
        cmd.arg("--no-newline");
    }
    let output = cmd
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .output()
        .context("wl-paste")?;
    if !output.status.success() {
        return Ok(None);
    }
    Ok(Some(output.stdout))
}

pub fn copy_bytes(mime: &str, data: &[u8]) -> Result<()> {
    let mut child = Command::new("wl-copy")
        .args(["--type", mime])
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .context("spawn wl-copy")?;
    if let Some(mut stdin) = child.stdin.take() {
        use std::io::Write;
        stdin.write_all(data).context("write to wl-copy")?;
    }
    let status = child.wait().context("wait wl-copy")?;
    if !status.success() {
        bail!("wl-copy failed: {status}");
    }
    Ok(())
}
