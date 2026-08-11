//! StatusNotifierItem tray (Plasma). Pause / show / quit.

use crate::state::DaemonState;
use std::sync::Arc;
use std::time::{Duration, Instant};

pub fn spawn(state: Arc<DaemonState>) {
    std::thread::Builder::new()
        .name("clipd-tray".into())
        .spawn(move || {
            let service = ksni::TrayService::new(ClipdTray { state });
            let _ = service.run();
        })
        .expect("spawn tray");
}

struct ClipdTray {
    state: Arc<DaemonState>,
}

impl ClipdTray {
    fn pause_label(&self) -> String {
        match *self.state.pause_until.lock().unwrap() {
            Some(until) if until > Instant::now() => {
                let secs = until.saturating_duration_since(Instant::now()).as_secs();
                format!("Paused ({m}m {s:02}s left)", m = secs / 60, s = secs % 60)
            }
            _ => "Shortcut active".into(),
        }
    }
}

impl ksni::Tray for ClipdTray {
    fn id(&self) -> String {
        "clipd".into()
    }

    fn title(&self) -> String {
        "clipd".into()
    }

    fn icon_name(&self) -> String {
        if self.state.is_paused() {
            "media-playback-pause".into()
        } else {
            "edit-paste".into()
        }
    }

    fn tool_tip(&self) -> ksni::ToolTip {
        ksni::ToolTip {
            title: "clipd".into(),
            description: self.pause_label(),
            ..Default::default()
        }
    }

    fn menu(&self) -> Vec<ksni::MenuItem<Self>> {
        use ksni::menu::*;
        vec![
            StandardItem {
                label: "Show history".into(),
                activate: Box::new(|_| {
                    let exe = std::env::current_exe().unwrap_or_default();
                    let _ = std::process::Command::new(exe).arg("show").spawn();
                }),
                ..Default::default()
            }
            .into(),
            MenuItem::Separator,
            StandardItem {
                label: self.pause_label(),
                enabled: false,
                ..Default::default()
            }
            .into(),
            StandardItem {
                label: "Pause shortcut 5 min".into(),
                activate: Box::new(|t: &mut ClipdTray| {
                    t.state.pause_for(Duration::from_secs(5 * 60))
                }),
                ..Default::default()
            }
            .into(),
            StandardItem {
                label: "Pause shortcut 15 min".into(),
                activate: Box::new(|t: &mut ClipdTray| {
                    t.state.pause_for(Duration::from_secs(15 * 60))
                }),
                ..Default::default()
            }
            .into(),
            StandardItem {
                label: "Pause shortcut 30 min".into(),
                activate: Box::new(|t: &mut ClipdTray| {
                    t.state.pause_for(Duration::from_secs(30 * 60))
                }),
                ..Default::default()
            }
            .into(),
            StandardItem {
                label: "Pause shortcut 60 min".into(),
                activate: Box::new(|t: &mut ClipdTray| {
                    t.state.pause_for(Duration::from_secs(60 * 60))
                }),
                ..Default::default()
            }
            .into(),
            StandardItem {
                label: "Resume shortcut".into(),
                activate: Box::new(|t: &mut ClipdTray| t.state.resume()),
                ..Default::default()
            }
            .into(),
            MenuItem::Separator,
            StandardItem {
                label: "Quit daemon".into(),
                activate: Box::new(|_| std::process::exit(0)),
                ..Default::default()
            }
            .into(),
        ]
    }
}
