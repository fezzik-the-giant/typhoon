// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2025 Ryan Cohan

use crossterm::event::{self, Event, KeyCode, KeyEvent};
use std::time::Duration;
use tokio::sync::mpsc;

use crate::api::{ApiRequest, ApiResponse};
use crate::app::{App, ArtistDetailFocus, SearchPane, Tab, View};
use crate::mpris::MprisCmd;
use crate::player::{PlayerCmd, PlayerEvent};

pub fn run_app(
    terminal: &mut ratatui::Terminal<ratatui::backend::CrosstermBackend<std::io::Stdout>>,
    app: &mut App,
    mut api_rx: mpsc::UnboundedReceiver<ApiResponse>,
    mut player_rx: mpsc::UnboundedReceiver<PlayerEvent>,
    mut mpris_rx: mpsc::UnboundedReceiver<MprisCmd>,
) -> anyhow::Result<()> {
    loop {
        terminal.draw(|f| crate::ui::draw(f, app))?;

        // Drain API responses
        while let Ok(resp) = api_rx.try_recv() {
            app.handle_api_response(resp);
        }

        // Drain player events
        while let Ok(evt) = player_rx.try_recv() {
            app.handle_player_event(evt);
        }

        // Drain MPRIS control commands
        while let Ok(cmd) = mpris_rx.try_recv() {
            match cmd {
                MprisCmd::Next => app.next_track(),
                MprisCmd::Previous => app.prev_track(),
                MprisCmd::PlayPause | MprisCmd::Play | MprisCmd::Pause => app.toggle_pause(),
                MprisCmd::Stop => { let _ = app.player_tx.send(PlayerCmd::Stop); }
            }
        }

        // Check for more data to load
        check_load_more(app);

        app.tick();

        if app.should_quit {
            break;
        }

        // Poll for key events with a short timeout to keep animations smooth
        if event::poll(Duration::from_millis(16))? {
            if let Event::Key(key) = event::read()? {
                handle_key(app, key);
            }
        }
    }
    Ok(())
}

fn kitty_delete_album_art() {
    if std::env::var("KITTY_WINDOW_ID").is_ok() {
        use std::io::Write;
        print!("\x1b_Ga=d,d=i,i=2\x1b\\");
        let _ = std::io::stdout().flush();
    }
}

fn kitty_delete_artist_art() {
    if std::env::var("KITTY_WINDOW_ID").is_ok() {
        use std::io::Write;
        print!("\x1b_Ga=d,d=i,i=3\x1b\\");
        let _ = std::io::stdout().flush();
    }
}

fn leaving_album(app: &App) -> bool {
    matches!(app.view_stack.last(), Some(View::AlbumDetail(_)))
}

fn leaving_artist(app: &App) -> bool {
    matches!(app.view_stack.last(), Some(View::ArtistDetail(_)))
}

fn handle_key(app: &mut App, key: KeyEvent) {
    if app.queue_focused {
        handle_queue_input(app, key);
        return;
    }

    if app.command.active {
        handle_command_input(app, key);
        return;
    }

    // Search overlay captures all keys while active, regardless of current tab.
    if app.search.active {
        handle_search_input(app, key);
        return;
    }

    // Global bindings
    match key.code {
        KeyCode::Char('q') | KeyCode::Char('Q') => {
            app.should_quit = true;
        }
        KeyCode::Char('/') => {
            app.command.active = true;
            app.command.input.clear();
            app.command.selected = 0;
        }
        KeyCode::Tab => {
            if leaving_album(app) { kitty_delete_album_art(); }
            if leaving_artist(app) { kitty_delete_artist_art(); }
            app.next_tab();
        }
        KeyCode::Char(' ') => app.toggle_pause(),
        KeyCode::Char('n') => app.next_track(),
        KeyCode::Char('p') => app.prev_track(),
        KeyCode::Esc => {
            if leaving_album(app) { kitty_delete_album_art(); }
            if leaving_artist(app) { kitty_delete_artist_art(); }
            app.go_back();
        }
        _ => handle_navigation(app, key),
    }
}

fn handle_command_input(app: &mut App, key: KeyEvent) {
    match key.code {
        KeyCode::Esc => {
            app.command.active = false;
        }
        KeyCode::Enter => {
            let matches = app.command.matches();
            let cmd = matches.get(app.command.selected)
                .or_else(|| matches.first())
                .copied();
            if let Some(cmd) = cmd {
                execute_command(app, cmd);
            } else {
                app.command.active = false;
            }
        }
        KeyCode::Tab => {
            // Accept ghost-text completion.
            if let Some(&first) = app.command.matches().first() {
                app.command.input = first.to_string();
                app.command.selected = 0;
            }
        }
        KeyCode::Up => {
            if app.command.selected > 0 {
                app.command.selected -= 1;
            }
        }
        KeyCode::Down => {
            let len = app.command.matches().len();
            if app.command.selected + 1 < len {
                app.command.selected += 1;
            }
        }
        KeyCode::Backspace => {
            app.command.input.pop();
            app.command.selected = 0;
        }
        KeyCode::Char(c) => {
            app.command.input.push(c);
            app.command.selected = 0;
        }
        _ => {}
    }
}

fn execute_command(app: &mut App, cmd: &str) {
    app.command.active = false;
    app.command.input.clear();
    let cleanup = |app: &App| {
        if leaving_album(app) { kitty_delete_album_art(); }
        if leaving_artist(app) { kitty_delete_artist_art(); }
    };
    match cmd {
        "favorites" => {
            cleanup(app);
            app.set_tab(Tab::Favorites);
        }
        "artists" => {
            cleanup(app);
            app.set_tab(Tab::Artists);
        }
        "playlists" => {
            cleanup(app);
            app.set_tab(Tab::Playlists);
        }
        "search" => {
            cleanup(app);
            app.set_tab(Tab::Search);
        }
        _ => {}
    }
}

fn handle_navigation(app: &mut App, key: KeyEvent) {
    // First pass: mutate the view's own list state (navigation within a detail view).
    // Collect any "play" action as data so we can call app methods after the borrow ends.
    enum Action {
        None,
        PlayTracks(Vec<crate::api::models::Track>, usize),
        OpenAlbum,
        AddToQueue(crate::api::models::Track),
        FavoriteTrack(crate::api::models::Track),
        FollowArtist(crate::api::models::Artist),
        UnfavoriteTrack(crate::api::models::Track),
        UnfollowArtist(crate::api::models::Artist),
        TrackRadio(crate::api::models::Track),
        ArtistRadio(crate::api::models::Artist),
    }

    let action: Action = if let Some(view) = app.view_stack.last_mut() {
        match view {
            View::ArtistDetail(detail) => {
                match key.code {
                    KeyCode::Up | KeyCode::Char('k') => {
                        match detail.focus {
                            ArtistDetailFocus::Tracks => detail.tracks.prev(),
                            ArtistDetailFocus::Albums => detail.albums.prev(),
                            ArtistDetailFocus::Bio => {
                                detail.bio_scroll = detail.bio_scroll.saturating_sub(1);
                            }
                        }
                        return;
                    }
                    KeyCode::Down | KeyCode::Char('j') => {
                        match detail.focus {
                            ArtistDetailFocus::Tracks => detail.tracks.next(),
                            ArtistDetailFocus::Albums => detail.albums.next(),
                            ArtistDetailFocus::Bio => {
                                detail.bio_scroll = detail.bio_scroll.saturating_add(1);
                            }
                        }
                        return;
                    }
                    KeyCode::Left | KeyCode::Char('h') => {
                        detail.focus = match detail.focus {
                            ArtistDetailFocus::Albums => ArtistDetailFocus::Tracks,
                            ArtistDetailFocus::Tracks => ArtistDetailFocus::Bio,
                            ArtistDetailFocus::Bio    => ArtistDetailFocus::Bio,
                        };
                        return;
                    }
                    KeyCode::Right | KeyCode::Char('l') => {
                        detail.focus = match detail.focus {
                            ArtistDetailFocus::Bio    => ArtistDetailFocus::Tracks,
                            ArtistDetailFocus::Tracks => ArtistDetailFocus::Albums,
                            ArtistDetailFocus::Albums => ArtistDetailFocus::Albums,
                        };
                        return;
                    }
                    KeyCode::Enter => {
                        if detail.focus == ArtistDetailFocus::Tracks {
                            let idx = detail.tracks.selected;
                            let tracks = detail.tracks.items.clone();
                            Action::PlayTracks(tracks, idx)
                        } else if detail.focus == ArtistDetailFocus::Albums {
                            Action::OpenAlbum
                        } else {
                            return;
                        }
                    }
                    KeyCode::Char('a') if detail.focus == ArtistDetailFocus::Tracks => {
                        match detail.tracks.items.get(detail.tracks.selected).cloned() {
                            Some(t) => Action::AddToQueue(t),
                            None => return,
                        }
                    }
                    KeyCode::Char('f') if detail.focus == ArtistDetailFocus::Tracks => {
                        match detail.tracks.items.get(detail.tracks.selected).cloned() {
                            Some(t) => Action::FavoriteTrack(t),
                            None => return,
                        }
                    }
                    KeyCode::Char('f') => Action::FollowArtist(detail.artist.clone()),
                    KeyCode::Char('d') if detail.focus == ArtistDetailFocus::Tracks => {
                        match detail.tracks.items.get(detail.tracks.selected).cloned() {
                            Some(t) => Action::UnfavoriteTrack(t),
                            None => return,
                        }
                    }
                    KeyCode::Char('d') => Action::UnfollowArtist(detail.artist.clone()),
                    KeyCode::Char('r') if detail.focus == ArtistDetailFocus::Tracks => {
                        match detail.tracks.items.get(detail.tracks.selected).cloned() {
                            Some(t) => Action::TrackRadio(t),
                            None => return,
                        }
                    }
                    KeyCode::Char('r') => Action::ArtistRadio(detail.artist.clone()),
                    _ => return,
                }
            }
            View::PlaylistDetail(detail) => {
                match key.code {
                    KeyCode::Up => { detail.tracks.prev(); return; }
                    KeyCode::Down => { detail.tracks.next(); return; }
                    KeyCode::Enter => {
                        let idx = detail.tracks.selected;
                        let tracks = detail.tracks.items.clone();
                        Action::PlayTracks(tracks, idx)
                    }
                    KeyCode::Char('a') => {
                        match detail.tracks.items.get(detail.tracks.selected).cloned() {
                            Some(t) => Action::AddToQueue(t),
                            None => return,
                        }
                    }
                    KeyCode::Char('f') => {
                        match detail.tracks.items.get(detail.tracks.selected).cloned() {
                            Some(t) => Action::FavoriteTrack(t),
                            None => return,
                        }
                    }
                    KeyCode::Char('d') => {
                        match detail.tracks.items.get(detail.tracks.selected).cloned() {
                            Some(t) => Action::UnfavoriteTrack(t),
                            None => return,
                        }
                    }
                    KeyCode::Char('r') => {
                        match detail.tracks.items.get(detail.tracks.selected).cloned() {
                            Some(t) => Action::TrackRadio(t),
                            None => return,
                        }
                    }
                    _ => return,
                }
            }
            View::AlbumDetail(detail) => {
                match key.code {
                    KeyCode::Up => { detail.tracks.prev(); return; }
                    KeyCode::Down => { detail.tracks.next(); return; }
                    KeyCode::Enter => {
                        let idx = detail.tracks.selected;
                        let tracks = detail.tracks.items.clone();
                        Action::PlayTracks(tracks, idx)
                    }
                    KeyCode::Char('a') => {
                        match detail.tracks.items.get(detail.tracks.selected).cloned() {
                            Some(t) => Action::AddToQueue(t),
                            None => return,
                        }
                    }
                    KeyCode::Char('f') => {
                        match detail.tracks.items.get(detail.tracks.selected).cloned() {
                            Some(t) => Action::FavoriteTrack(t),
                            None => return,
                        }
                    }
                    KeyCode::Char('d') => {
                        match detail.tracks.items.get(detail.tracks.selected).cloned() {
                            Some(t) => Action::UnfavoriteTrack(t),
                            None => return,
                        }
                    }
                    KeyCode::Char('r') => {
                        match detail.tracks.items.get(detail.tracks.selected).cloned() {
                            Some(t) => Action::TrackRadio(t),
                            None => return,
                        }
                    }
                    _ => return,
                }
            }
        }
    } else {
        Action::None
    };

    // Apply any collected action (borrow of view_stack has ended)
    match action {
        Action::PlayTracks(tracks, idx) => { app.play_tracks(tracks, idx); return; }
        Action::OpenAlbum => { kitty_delete_album_art(); kitty_delete_artist_art(); app.open_selected_album(); return; }
        Action::AddToQueue(track) => { app.add_to_queue(track); return; }
        Action::FavoriteTrack(track) => { app.favorite_track(&track); return; }
        Action::FollowArtist(artist) => { app.follow_artist(&artist); return; }
        Action::UnfavoriteTrack(track) => { app.unfavorite_track(&track); return; }
        Action::UnfollowArtist(artist) => { app.unfollow_artist(&artist); return; }
        Action::TrackRadio(track) => { app.start_track_radio(&track); return; }
        Action::ArtistRadio(artist) => { app.start_artist_radio(&artist); return; }
        Action::None => {}
    }

    // Top-level tab navigation (no active detail view)
    match key.code {
        KeyCode::Up | KeyCode::Char('k') => match app.current_tab {
            Tab::Artists => app.artists.prev(),
            Tab::Playlists => app.playlists.prev(),
            Tab::Favorites => app.favorites.prev(),
            Tab::Search => app.search.pane_prev(),
        },
        KeyCode::Down | KeyCode::Char('j') => match app.current_tab {
            Tab::Artists => app.artists.next(),
            Tab::Playlists => app.playlists.next(),
            Tab::Favorites => app.favorites.next(),
            Tab::Search => app.search.pane_next(),
        },
        KeyCode::Left | KeyCode::Char('h') if app.current_tab == Tab::Search => {
            app.search.prev_pane();
        }
        KeyCode::Right | KeyCode::Char('l') if app.current_tab == Tab::Search => {
            app.search.next_pane();
        }
        KeyCode::Right | KeyCode::Char('l') if app.current_tab != Tab::Search => {
            app.focus_queue();
        }
        KeyCode::Enter => match app.current_tab {
            Tab::Artists => app.open_selected_artist(),
            Tab::Playlists => app.open_selected_playlist(),
            Tab::Favorites => {
                let idx = app.favorites.selected;
                let tracks = app.favorites.items.clone();
                if !tracks.is_empty() {
                    app.play_tracks(tracks, idx);
                }
            }
            Tab::Search => {
                match app.search.pane {
                    SearchPane::Tracks => {
                        let idx = app.search.track_sel;
                        if let Some(track) = app.search.tracks.get(idx).cloned() {
                            app.play_track(track);
                        }
                    }
                    SearchPane::Artists => {
                        let idx = app.search.artist_sel;
                        if let Some(artist) = app.search.artists.get(idx).cloned() {
                            app.open_artist(artist);
                        }
                    }
                    SearchPane::Playlists => {
                        let idx = app.search.playlist_sel;
                        if let Some(playlist) = app.search.playlists.get(idx).cloned() {
                            app.open_playlist(playlist);
                        }
                    }
                }
            }
        },
        KeyCode::Char('a') => match app.current_tab {
            Tab::Favorites => {
                if let Some(track) = app.favorites.selected_item().cloned() {
                    app.add_to_queue(track);
                }
            }
            Tab::Search if app.search.pane == SearchPane::Tracks => {
                if let Some(track) = app.search.tracks.get(app.search.track_sel).cloned() {
                    app.add_to_queue(track);
                }
            }
            _ => {}
        },
        KeyCode::Char('f') => match app.current_tab {
            Tab::Artists => {
                if let Some(artist) = app.artists.selected_item().cloned() {
                    app.follow_artist(&artist);
                }
            }
            Tab::Favorites => {
                if let Some(track) = app.favorites.selected_item().cloned() {
                    app.favorite_track(&track);
                }
            }
            Tab::Search if app.search.pane == SearchPane::Tracks => {
                if let Some(track) = app.search.tracks.get(app.search.track_sel).cloned() {
                    app.favorite_track(&track);
                }
            }
            Tab::Search if app.search.pane == SearchPane::Artists => {
                if let Some(artist) = app.search.artists.get(app.search.artist_sel).cloned() {
                    app.follow_artist(&artist);
                }
            }
            _ => {}
        },
        KeyCode::Char('d') => match app.current_tab {
            Tab::Artists => {
                if let Some(artist) = app.artists.selected_item().cloned() {
                    app.unfollow_artist(&artist);
                }
            }
            Tab::Favorites => {
                if let Some(track) = app.favorites.selected_item().cloned() {
                    app.unfavorite_track(&track);
                }
            }
            Tab::Search if app.search.pane == SearchPane::Tracks => {
                if let Some(track) = app.search.tracks.get(app.search.track_sel).cloned() {
                    app.unfavorite_track(&track);
                }
            }
            Tab::Search if app.search.pane == SearchPane::Artists => {
                if let Some(artist) = app.search.artists.get(app.search.artist_sel).cloned() {
                    app.unfollow_artist(&artist);
                }
            }
            _ => {}
        },
        KeyCode::Char('r') => match app.current_tab {
            Tab::Artists => {
                if let Some(artist) = app.artists.selected_item().cloned() {
                    app.start_artist_radio(&artist);
                }
            }
            Tab::Favorites => {
                if let Some(track) = app.favorites.selected_item().cloned() {
                    app.start_track_radio(&track);
                }
            }
            Tab::Search if app.search.pane == SearchPane::Tracks => {
                if let Some(track) = app.search.tracks.get(app.search.track_sel).cloned() {
                    app.start_track_radio(&track);
                }
            }
            Tab::Search if app.search.pane == SearchPane::Artists => {
                if let Some(artist) = app.search.artists.get(app.search.artist_sel).cloned() {
                    app.start_artist_radio(&artist);
                }
            }
            _ => {}
        },
        _ => {}
    }
}

fn handle_search_input(app: &mut App, key: KeyEvent) {
    match key.code {
        KeyCode::Tab => {
            app.search.active = false;
            app.next_tab();
        }
        KeyCode::Esc => {
            // Close overlay, stay on current view.
            app.search.active = false;
        }
        KeyCode::Enter => {
            let query = app.search.query.clone();
            app.search.active = false;
            if !query.is_empty() {
                // Now we're committing to showing search results — navigate away.
                if leaving_album(app) { kitty_delete_album_art(); }
                app.view_stack.clear();
                app.current_tab = Tab::Search;
                app.search.loading = true;
                let _ = app.api_tx.send(ApiRequest::Search { query });
            }
        }
        KeyCode::Backspace => {
            app.search.query.pop();
        }
        KeyCode::Char(c) => {
            app.search.query.push(c);
        }
        _ => {}
    }
}

fn handle_queue_input(app: &mut App, key: KeyEvent) {
    match key.code {
        KeyCode::Esc | KeyCode::Left | KeyCode::Char('h') => {
            app.unfocus_queue();
        }
        KeyCode::Up | KeyCode::Char('k') => {
            if app.queue_cursor > 0 {
                app.queue_cursor -= 1;
            }
        }
        KeyCode::Down | KeyCode::Char('j') => {
            let len = app.now_playing.queue.len();
            if len > 0 && app.queue_cursor + 1 < len {
                app.queue_cursor += 1;
            }
        }
        KeyCode::Enter => {
            let cursor = app.queue_cursor;
            app.play_from_queue(cursor);
        }
        KeyCode::Char('d') | KeyCode::Delete => {
            let cursor = app.queue_cursor;
            app.remove_from_queue(cursor);
        }
        KeyCode::Char('f') => {
            if let Some(track) = app.now_playing.queue.get(app.queue_cursor).cloned() {
                app.toggle_favorite_track(&track);
            }
        }
        KeyCode::Char(' ') => app.toggle_pause(),
        KeyCode::Char('q') | KeyCode::Char('Q') => {
            app.should_quit = true;
        }
        _ => {}
    }
}

fn check_load_more(app: &mut App) {
    match app.current_tab {
        Tab::Artists if app.view_stack.is_empty() => {
            if app.artists.should_load_more() {
                app.load_artists();
            }
        }
        Tab::Playlists if app.view_stack.is_empty() => {
            if app.playlists.should_load_more() {
                app.load_playlists();
            }
        }
        Tab::Favorites if app.view_stack.is_empty() => {
            if app.favorites.should_load_more() {
                app.load_favorites();
            }
        }
        _ => {}
    }
}
