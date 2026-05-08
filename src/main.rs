// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2025 Ryan Cohan

#![allow(dead_code, unused_variables, unused_imports)]
use std::io;
use anyhow::Result;
use crossterm::{
    event::{DisableMouseCapture, EnableMouseCapture},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{Terminal, backend::CrosstermBackend};
use tokio::sync::mpsc;

mod api;
mod app;
mod events;
mod player;
mod ui;

use api::ApiWorker;
use app::App;
use player::PlayerWorker;

fn setup_panic_hook() {
    let original = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let _ = disable_raw_mode();
        let _ = execute!(
            std::io::stderr(),
            LeaveAlternateScreen,
            crossterm::cursor::Show,
        );
        original(info);
    }));
}

fn main() -> Result<()> {
    setup_panic_hook();

    // Load config and ensure authentication (blocking, before TUI)
    let mut config = api::auth::load_config()?;
    api::auth::ensure_auth(&mut config)?;

    // Channels: TUI → ApiWorker and TUI → PlayerWorker
    let (api_req_tx, api_req_rx) = mpsc::unbounded_channel();
    let (api_resp_tx, api_resp_rx) = mpsc::unbounded_channel();
    let (player_cmd_tx, player_cmd_rx) = mpsc::unbounded_channel();
    let (player_evt_tx, player_evt_rx) = mpsc::unbounded_channel();

    // Spawn async workers on a dedicated Tokio thread.
    // We keep the handle so we can join it on exit and let PlayerWorker kill mpv cleanly.
    let worker_config = config.clone();
    let worker_thread = std::thread::spawn(move || {
        let rt = tokio::runtime::Runtime::new().expect("tokio runtime");
        rt.block_on(async move {
            let api_worker = ApiWorker::new(worker_config, api_req_rx, api_resp_tx);
            let player_worker = PlayerWorker::new(player_cmd_rx, player_evt_tx);
            tokio::join!(api_worker.run(), player_worker.run());
        });
    });

    // Set up terminal
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen, EnableMouseCapture)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    // Build app state and run
    let mut app = App::new(config, api_req_tx, player_cmd_tx);
    let result = events::run_app(&mut terminal, &mut app, api_resp_rx, player_evt_rx);

    // Restore terminal unconditionally
    disable_raw_mode()?;
    execute!(
        terminal.backend_mut(),
        LeaveAlternateScreen,
        DisableMouseCapture,
    )?;
    terminal.show_cursor()?;

    // Dropping app closes the command channels, which causes both workers to exit
    // their loops. Joining ensures PlayerWorker reaches child.kill() before we return.
    drop(app);
    let _ = worker_thread.join();

    result
}
