use crate::clipboard;
use crate::config::Config;
use crate::ipc::{ListMsg, OkMsg, Request, StatusMsg};
use crate::paste;
use crate::state::DaemonState;
use crate::store::{Kind, Store};
use crate::tray;
use anyhow::{bail, Context, Result};
use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::Path;
use std::sync::atomic::Ordering;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

pub fn run() -> Result<()> {
    let cfg = Config::load()?;
    let state = DaemonState::new(&cfg);
    let store = Arc::new(Mutex::new(Store::open(&cfg)?));
    let config = Arc::new(Mutex::new(cfg));
    let suppress = Arc::new(Mutex::new(Instant::now()));

    let sock_path = Config::socket_path();
    let listener = bind_listener(&sock_path)?;
    println!("clipd: listening on {}", sock_path.display());

    if let Err(e) = state.resume() {
        eprintln!("clipd: startup shortcut sync failed: {e:#}");
    }

    tray::spawn(Arc::clone(&state));

    // Auto-resume when the pause timer expires. The state method keeps the
    // deadline until Plasma has actually accepted the re-enable.
    {
        let state = Arc::clone(&state);
        thread::spawn(move || loop {
            thread::sleep(Duration::from_secs(1));
            if let Err(e) = state.resume_if_expired(Instant::now()) {
                eprintln!("clipd: auto-resume failed; retrying: {e:#}");
            }
        });
    }

    // Watcher thread — retry if wl-paste isn't installed yet.
    {
        let store = Arc::clone(&store);
        let suppress = Arc::clone(&suppress);
        let state = Arc::clone(&state);
        thread::spawn(move || loop {
            state.watching.store(true, Ordering::SeqCst);
            let result = clipboard::watch_loop(|| {
                if Instant::now() < *suppress.lock().unwrap() {
                    return Ok(());
                }
                let Some((mime, data)) = clipboard::read_clipboard()? else {
                    return Ok(());
                };
                let kind = if mime.starts_with("image/") {
                    Kind::Image
                } else {
                    Kind::Text
                };
                if data.len() > 32 * 1024 * 1024 {
                    eprintln!("clipd: skip oversized clipboard ({} bytes)", data.len());
                    return Ok(());
                }
                let mut s = store.lock().unwrap();
                match s.insert(kind, &mime, &data)? {
                    Some(id) => eprintln!("clipd: stored id={id} kind={:?} {}B", kind, data.len()),
                    None => {}
                }
                Ok(())
            });
            state.watching.store(false, Ordering::SeqCst);
            eprintln!("clipd: watcher stopped: {:#}; retry in 2s", result.unwrap_err());
            thread::sleep(Duration::from_secs(2));
        });
    }

    for conn in listener.incoming() {
        match conn {
            Ok(stream) => {
                let store = Arc::clone(&store);
                let config = Arc::clone(&config);
                let suppress = Arc::clone(&suppress);
                let state = Arc::clone(&state);
                thread::spawn(move || {
                    if let Err(e) = handle_client(stream, store, config, suppress, state) {
                        eprintln!("clipd: ipc error: {e:#}");
                    }
                });
            }
            Err(e) => eprintln!("clipd: accept: {e}"),
        }
    }
    Ok(())
}

fn bind_listener(path: &Path) -> Result<UnixListener> {
    match UnixListener::bind(path) {
        Ok(listener) => Ok(listener),
        Err(e) if e.kind() == std::io::ErrorKind::AddrInUse => {
            if UnixStream::connect(path).is_ok() {
                bail!("clipd daemon is already running");
            }
            std::fs::remove_file(path)
                .with_context(|| format!("remove stale socket {}", path.display()))?;
            UnixListener::bind(path).with_context(|| format!("bind {}", path.display()))
        }
        Err(e) => Err(e).with_context(|| format!("bind {}", path.display())),
    }
}

fn handle_client(
    stream: UnixStream,
    store: Arc<Mutex<Store>>,
    config: Arc<Mutex<Config>>,
    suppress: Arc<Mutex<Instant>>,
    state: Arc<DaemonState>,
) -> Result<()> {
    let mut reader = BufReader::new(&stream);
    let mut line = String::new();
    reader.read_line(&mut line)?;
    let req: Request = serde_json::from_str(line.trim()).context("parse request")?;
    let mut writer = &stream;

    match req {
        Request::Status => {
            let items = store.lock().unwrap().count()?;
            let msg = StatusMsg {
                ok: true,
                items,
                watching: state.watching.load(Ordering::SeqCst),
                pause_remaining_secs: state.pause_remaining_secs(),
                max_items: state.max_items.load(Ordering::SeqCst),
                shortcut: state.shortcut.lock().unwrap().clone(),
            };
            writeln!(writer, "{}", serde_json::to_string(&msg)?)?;
        }
        Request::List { limit } => {
            let items = store.lock().unwrap().list(limit)?;
            let msg = ListMsg { ok: true, items };
            writeln!(writer, "{}", serde_json::to_string(&msg)?)?;
        }
        Request::Select { id } => {
            let result = (|| -> Result<()> {
                let (_kind, mime, payload) = {
                    let s = store.lock().unwrap();
                    let p = s.get_payload(id)?;
                    s.touch(id)?;
                    p
                };
                *suppress.lock().unwrap() = Instant::now() + Duration::from_millis(600);
                clipboard::copy_bytes(&mime, &payload)?;
                // Paste after popup closes / focus returns (wtype fails while we still own focus).
                paste::inject_paste_later(Duration::from_millis(180));
                Ok(())
            })();
            write_ok(&mut writer, result)?;
        }
        Request::Pause { minutes } => {
            let result = if minutes == 0 {
                state.resume()
            } else {
                state.pause_for(Duration::from_secs(u64::from(minutes) * 60))
            };
            write_ok(&mut writer, result)?;
        }
        Request::SetConfig {
            max_items,
            shortcut,
        } => {
            let result = (|| -> Result<()> {
                let mut cfg = config.lock().unwrap();
                let mut next = cfg.clone();
                let mut shortcut_requested = false;
                if let Some(n) = max_items {
                    next.max_items = n.max(1);
                }
                if let Some(s) = shortcut {
                    let s = s.trim().to_string();
                    if !s.is_empty() {
                        next.shortcut = s;
                        shortcut_requested = true;
                    }
                }

                let old_shortcut = cfg.shortcut.clone();
                let shortcut_changed = next.shortcut != old_shortcut;
                if shortcut_requested {
                    if let Err(e) = state.set_shortcut(&next.shortcut) {
                        if shortcut_changed {
                            if let Err(rollback) = state.set_shortcut(&old_shortcut) {
                                eprintln!("clipd: shortcut rollback failed: {rollback:#}");
                            }
                        }
                        return Err(e);
                    }
                }
                if let Err(e) = next.save() {
                    if shortcut_changed {
                        if let Err(rollback) = state.set_shortcut(&old_shortcut) {
                            eprintln!("clipd: shortcut rollback failed: {rollback:#}");
                        }
                    }
                    return Err(e);
                }

                if next.max_items != cfg.max_items {
                    state.max_items.store(next.max_items, Ordering::SeqCst);
                    store.lock().unwrap().set_max_items(next.max_items);
                }
                *cfg = next;
                Ok(())
            })();
            write_ok(&mut writer, result)?;
        }
        Request::SetWindowSize { width, height } => {
            let result = (|| -> Result<()> {
                let mut cfg = config.lock().unwrap();
                let mut next = cfg.clone();
                next.set_window_size(width, height);
                next.save()?;
                *cfg = next;
                Ok(())
            })();
            write_ok(&mut writer, result)?;
        }
    }
    Ok(())
}

fn write_ok(writer: &mut impl Write, result: Result<()>) -> Result<()> {
    let msg = match result {
        Ok(()) => OkMsg {
            ok: true,
            error: None,
        },
        Err(e) => OkMsg {
            ok: false,
            error: Some(format!("{e:#}")),
        },
    };
    writeln!(writer, "{}", serde_json::to_string(&msg)?)?;
    Ok(())
}
