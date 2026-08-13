use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Config {
    /// Max history items (oldest evicted).
    pub max_items: usize,
    /// Documented only — Plasma binds this. clipd never registers it.
    pub shortcut: String,
    pub autostart: bool,
    /// Last popup inner size (points).
    pub window_width: f32,
    pub window_height: f32,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            max_items: 200,
            shortcut: "Meta+Shift+V".into(),
            autostart: true,
            window_width: 560.0,
            window_height: 480.0,
        }
    }
}

impl Config {
    pub fn path() -> PathBuf {
        dirs::config_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join("clipd/config.toml")
    }

    pub fn clamped_window_size(&self) -> [f32; 2] {
        [
            self.window_width.clamp(320.0, 4000.0),
            self.window_height.clamp(240.0, 3000.0),
        ]
    }

    pub fn set_window_size(&mut self, w: f32, h: f32) {
        self.window_width = w.clamp(320.0, 4000.0);
        self.window_height = h.clamp(240.0, 3000.0);
    }

    pub fn load() -> Result<Self> {
        let path = Self::path();
        if !path.exists() {
            let cfg = Self::default();
            cfg.save()?;
            return Ok(cfg);
        }
        let text = fs::read_to_string(&path)
            .with_context(|| format!("read {}", path.display()))?;
        let cfg: Config = toml::from_str(&text).context("parse config.toml")?;
        Ok(cfg)
    }

    pub fn save(&self) -> Result<()> {
        let path = Self::path();
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        let text = toml::to_string_pretty(self)?;
        // Atomic replace so a concurrent reader never sees a partial file
        // (UI window-size saves vs daemon SetConfig).
        let tmp = path.with_extension("toml.tmp");
        fs::write(&tmp, text)?;
        fs::rename(&tmp, &path)?;
        Ok(())
    }

    pub fn data_dir() -> PathBuf {
        dirs::data_local_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join("clipd")
    }

    pub fn db_path() -> PathBuf {
        Self::data_dir().join("history.db")
    }

    pub fn socket_path() -> PathBuf {
        std::env::var_os("XDG_RUNTIME_DIR")
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from("/tmp"))
            .join("clipd.sock")
    }
}
