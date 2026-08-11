use crate::config::Config;
use crate::plasma;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

/// Shared daemon state (watcher, IPC, tray).
pub struct DaemonState {
    pub watching: AtomicBool,
    pub max_items: AtomicUsize,
    pub pause_until: Mutex<Option<Instant>>,
    pub shortcut: Mutex<String>,
    /// Bumped so the tray refreshes labels.
    pub tray_tick: AtomicUsize,
}

impl DaemonState {
    pub fn new(cfg: &Config) -> Arc<Self> {
        Arc::new(Self {
            watching: AtomicBool::new(false),
            max_items: AtomicUsize::new(cfg.max_items),
            pause_until: Mutex::new(None),
            shortcut: Mutex::new(cfg.shortcut.clone()),
            tray_tick: AtomicUsize::new(0),
        })
    }

    pub fn is_paused(&self) -> bool {
        match *self.pause_until.lock().unwrap() {
            Some(until) => until > Instant::now(),
            None => false,
        }
    }

    pub fn pause_remaining_secs(&self) -> Option<u64> {
        match *self.pause_until.lock().unwrap() {
            Some(until) if until > Instant::now() => {
                Some(until.saturating_duration_since(Instant::now()).as_secs())
            }
            _ => None,
        }
    }

    pub fn pause_for(&self, dur: Duration) {
        let until = Instant::now() + dur;
        *self.pause_until.lock().unwrap() = Some(until);
        let shortcut = self.shortcut.lock().unwrap().clone();
        if let Err(e) = plasma::set_shortcut_enabled(false, &shortcut) {
            eprintln!("clipd: pause unbind failed: {e:#}");
        }
        self.tray_tick.fetch_add(1, Ordering::SeqCst);
        eprintln!("clipd: shortcut paused for {}s", dur.as_secs());
    }

    pub fn resume(&self) {
        *self.pause_until.lock().unwrap() = None;
        let shortcut = self.shortcut.lock().unwrap().clone();
        if let Err(e) = plasma::set_shortcut_enabled(true, &shortcut) {
            eprintln!("clipd: resume bind failed: {e:#}");
        }
        self.tray_tick.fetch_add(1, Ordering::SeqCst);
        eprintln!("clipd: shortcut resumed");
    }

    pub fn set_shortcut(&self, shortcut: &str) {
        *self.shortcut.lock().unwrap() = shortcut.to_string();
        if !self.is_paused() {
            let _ = plasma::set_shortcut_enabled(true, shortcut);
        }
    }
}
