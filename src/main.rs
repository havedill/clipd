//! clipd — Plasma-owned hotkey clipboard history for Wayland.
//! Never registers global shortcuts; Plasma runs `clipd show`.

mod clipboard;
mod config;
mod crypto;
mod daemon;
mod install;
mod ipc;
mod paste;
mod plasma;
mod state;
mod store;
mod tray;
mod ui;

use anyhow::{Context, Result};
use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(name = "clipd", about = "Clipboard history daemon + popup (Plasma binds the hotkey)")]
struct Cli {
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// Watch clipboard, encrypt + store history
    Daemon,
    /// Open history popup (Plasma shortcut should run this)
    Show,
    /// Print daemon status
    Status,
    /// Install desktop files, autostart unit, Plasma Ctrl+D; disable CopyQ conflict
    Install {
        #[arg(long)]
        disable_copyq: bool,
    },
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    match cli.cmd {
        Cmd::Daemon => daemon::run(),
        Cmd::Show => ui::run_show(),
        Cmd::Status => {
            let s = ipc::status().context("daemon not reachable — is `clipd daemon` running?")?;
            println!("{s}");
            Ok(())
        }
        Cmd::Install { disable_copyq } => install::run(disable_copyq),
    }
}
