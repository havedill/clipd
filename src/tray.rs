//! StatusNotifierItem tray (Plasma). Show / pause / quit.

use crate::state::DaemonState;
use std::sync::Arc;
use std::time::Duration;

pub fn spawn(state: Arc<DaemonState>) {
    std::thread::Builder::new()
        .name("clipd-tray".into())
        .spawn(move || {
            let service = ksni::TrayService::new(ClipdTray { state });
            let handle = service.handle();
            std::thread::spawn(move || loop {
                std::thread::sleep(Duration::from_secs(1));
                handle.update(|_| {});
            });
            let _ = service.run();
        })
        .expect("spawn tray");
}

struct ClipdTray {
    state: Arc<DaemonState>,
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
        let shortcut = self.state.shortcut.lock().unwrap().clone();
        let description = match self.state.pause_remaining_secs() {
            Some(secs) => format!(
                "Shortcut paused: {shortcut} ({m}m {s:02}s remaining)",
                m = secs / 60,
                s = secs % 60
            ),
            None => format!("History shortcut: {shortcut}"),
        };
        ksni::ToolTip {
            title: "clipd".into(),
            description,
            ..Default::default()
        }
    }

    fn menu(&self) -> Vec<ksni::MenuItem<Self>> {
        use ksni::menu::*;
        let mut items = vec![
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
        ];
        for minutes in [5_u64, 15, 30, 60] {
            items.push(
                StandardItem {
                    label: format!("Pause shortcut for {minutes}m"),
                    activate: Box::new(move |tray: &mut ClipdTray| {
                        if let Err(e) = tray.state.pause_for(Duration::from_secs(minutes * 60)) {
                            eprintln!("clipd: pause shortcut: {e:#}");
                        }
                    }),
                    ..Default::default()
                }
                .into(),
            );
        }
        if self.state.is_paused() {
            items.push(
                StandardItem {
                    label: "Resume shortcut".into(),
                    activate: Box::new(|tray: &mut ClipdTray| {
                        if let Err(e) = tray.state.resume() {
                            eprintln!("clipd: resume shortcut: {e:#}");
                        }
                    }),
                    ..Default::default()
                }
                .into(),
            );
        }
        items.extend([
            MenuItem::Separator,
            StandardItem {
                label: "Quit daemon".into(),
                activate: Box::new(|_| std::process::exit(0)),
                ..Default::default()
            }
            .into(),
        ]);
        items
    }
}
