use crate::config::Config;
use crate::plasma;
use anyhow::Result;
use std::sync::atomic::{AtomicBool, AtomicUsize};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

/// Shared daemon state (watcher, IPC, tray).
pub struct DaemonState {
    pub watching: AtomicBool,
    pub max_items: AtomicUsize,
    pub shortcut: Mutex<String>,
    pause_until: Mutex<Option<Instant>>,
}

impl DaemonState {
    pub fn new(cfg: &Config) -> Arc<Self> {
        Arc::new(Self {
            watching: AtomicBool::new(false),
            max_items: AtomicUsize::new(cfg.max_items),
            shortcut: Mutex::new(cfg.shortcut.clone()),
            pause_until: Mutex::new(None),
        })
    }

    pub fn set_shortcut(&self, shortcut: &str) -> Result<()> {
        let pause_until = self.pause_until.lock().unwrap();
        plasma::set_shortcut_enabled(pause_until.is_none(), shortcut)?;
        *self.shortcut.lock().unwrap() = shortcut.to_string();
        Ok(())
    }

    pub fn pause_for(&self, duration: Duration) -> Result<()> {
        let mut pause_until = self.pause_until.lock().unwrap();
        let shortcut = self.shortcut.lock().unwrap().clone();
        plasma::set_shortcut_enabled(false, &shortcut)?;
        *pause_until = Some(Instant::now() + duration);
        Ok(())
    }

    pub fn resume(&self) -> Result<()> {
        let mut pause_until = self.pause_until.lock().unwrap();
        let shortcut = self.shortcut.lock().unwrap().clone();
        plasma::set_shortcut_enabled(true, &shortcut)?;
        *pause_until = None;
        Ok(())
    }

    /// Resume an expired pause while holding the state lock through the Plasma update.
    pub fn resume_if_expired(&self, now: Instant) -> Result<bool> {
        let mut pause_until = self.pause_until.lock().unwrap();
        if !pause_expired(*pause_until, now) {
            return Ok(false);
        }
        let shortcut = self.shortcut.lock().unwrap().clone();
        plasma::set_shortcut_enabled(true, &shortcut)?;
        *pause_until = None;
        Ok(true)
    }

    pub fn pause_remaining_secs(&self) -> Option<u64> {
        self.pause_until.lock().unwrap().map(|until| {
            let remaining = until.saturating_duration_since(Instant::now());
            remaining.as_secs() + u64::from(remaining.subsec_nanos() != 0)
        })
    }

    pub fn is_paused(&self) -> bool {
        self.pause_until.lock().unwrap().is_some()
    }
}

fn pause_expired(pause_until: Option<Instant>, now: Instant) -> bool {
    matches!(pause_until, Some(until) if until <= now)
}

#[cfg(test)]
mod tests {
    use super::pause_expired;
    use std::time::{Duration, Instant};

    #[test]
    fn pause_expires_only_at_its_deadline() {
        let now = Instant::now();
        assert!(!pause_expired(None, now));
        assert!(!pause_expired(Some(now + Duration::from_secs(1)), now));
        assert!(pause_expired(Some(now), now));
        assert!(pause_expired(Some(now - Duration::from_secs(1)), now));
    }
}
