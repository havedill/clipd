use crate::config::Config;
use crate::store::ItemMeta;
use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};
use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::UnixStream;
use std::time::Duration;

#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "cmd", rename_all = "snake_case")]
pub enum Request {
    Status,
    List { limit: usize },
    Select { id: i64 },
    /// minutes=0 resumes.
    Pause { minutes: u32 },
    SetConfig {
        #[serde(default)]
        max_items: Option<usize>,
        #[serde(default)]
        shortcut: Option<String>,
    },
    SetWindowSize { width: f32, height: f32 },
}

#[derive(Debug, Serialize, Deserialize)]
pub struct StatusMsg {
    pub ok: bool,
    pub items: usize,
    pub watching: bool,
    #[serde(default)]
    pub pause_remaining_secs: Option<u64>,
    #[serde(default)]
    pub max_items: usize,
    #[serde(default)]
    pub shortcut: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ListMsg {
    pub ok: bool,
    pub items: Vec<ItemMeta>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct OkMsg {
    pub ok: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

pub fn connect() -> Result<UnixStream> {
    let path = Config::socket_path();
    let stream =
        UnixStream::connect(&path).with_context(|| format!("connect {}", path.display()))?;
    stream.set_read_timeout(Some(Duration::from_secs(5)))?;
    stream.set_write_timeout(Some(Duration::from_secs(5)))?;
    Ok(stream)
}

pub fn roundtrip<T: for<'de> Deserialize<'de>>(req: &Request) -> Result<T> {
    let mut stream = connect()?;
    let line = serde_json::to_string(req)? + "\n";
    stream.write_all(line.as_bytes())?;
    let mut reader = BufReader::new(stream);
    let mut resp = String::new();
    reader.read_line(&mut resp)?;
    if resp.is_empty() {
        bail!("empty response from daemon");
    }
    Ok(serde_json::from_str(resp.trim())?)
}

pub fn status() -> Result<String> {
    let s: StatusMsg = roundtrip(&Request::Status)?;
    let pause = match s.pause_remaining_secs {
        Some(secs) => format!(" paused={}s", secs),
        None => String::new(),
    };
    Ok(format!(
        "ok={} items={} watching={} max_items={} shortcut={}{pause}",
        s.ok, s.items, s.watching, s.max_items, s.shortcut
    ))
}

pub fn status_msg() -> Result<StatusMsg> {
    roundtrip(&Request::Status)
}

pub fn list(limit: usize) -> Result<Vec<ItemMeta>> {
    let s: ListMsg = roundtrip(&Request::List { limit })?;
    if !s.ok {
        bail!("list failed");
    }
    Ok(s.items)
}

pub fn select(id: i64) -> Result<()> {
    let s: OkMsg = roundtrip(&Request::Select { id })?;
    if !s.ok {
        bail!("{}", s.error.unwrap_or_else(|| "select failed".into()));
    }
    Ok(())
}

pub fn pause(minutes: u32) -> Result<()> {
    let s: OkMsg = roundtrip(&Request::Pause { minutes })?;
    if !s.ok {
        bail!("{}", s.error.unwrap_or_else(|| "pause failed".into()));
    }
    Ok(())
}

pub fn set_config(max_items: Option<usize>, shortcut: Option<String>) -> Result<()> {
    let s: OkMsg = roundtrip(&Request::SetConfig {
        max_items,
        shortcut,
    })?;
    if !s.ok {
        bail!("{}", s.error.unwrap_or_else(|| "set_config failed".into()));
    }
    Ok(())
}

pub fn set_window_size(width: f32, height: f32) -> Result<()> {
    let s: OkMsg = roundtrip(&Request::SetWindowSize { width, height })?;
    if !s.ok {
        bail!(
            "{}",
            s.error.unwrap_or_else(|| "set_window_size failed".into())
        );
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{Request, StatusMsg};

    #[test]
    fn pause_request_keeps_wire_compatibility() {
        let request: Request =
            serde_json::from_str(r#"{"cmd":"pause","minutes":15}"#).unwrap();
        assert!(matches!(request, Request::Pause { minutes: 15 }));
    }

    #[test]
    fn old_status_without_pause_field_still_decodes() {
        let status: StatusMsg = serde_json::from_str(
            r#"{"ok":true,"items":2,"watching":true,"max_items":200,"shortcut":"Meta+V"}"#,
        )
        .unwrap();
        assert_eq!(status.pause_remaining_secs, None);
    }
}
