// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2025 Ryan Cohan

use ratatui::{
    Frame,
    layout::{Alignment, Constraint, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, List, ListItem, Paragraph, Wrap},
};

use crate::app::{App, ArtPayload, ArtistDetailFocus, SearchPane, StatusLevel, Tab, View};
use crate::api::models::{Album, Artist, Playlist, Track};

const ACCENT: Color = Color::Cyan;
const DIM: Color = Color::DarkGray;
const HIGHLIGHT_BG: Color = Color::Rgb(40, 40, 55);
const SELECT_BG: Color = Color::Rgb(30, 100, 200);
const SIDEBAR_W: u16 = 20;
const QUEUE_W: u16 = 26;

pub fn draw(f: &mut Frame, app: &App) {
    let area = f.area();

    let rows = Layout::vertical([
        Constraint::Min(0),    // sidebar + content + queue
        Constraint::Length(8), // now-playing bar
        Constraint::Length(1), // keybinds bar
    ])
    .split(area);

    let cols = Layout::horizontal([
        Constraint::Length(SIDEBAR_W),
        Constraint::Min(0),
        Constraint::Length(QUEUE_W),
    ])
    .split(rows[0]);

    render_sidebar(f, app, cols[0]);
    render_content(f, app, cols[1]);
    render_queue(f, app, cols[2]);
    render_now_playing(f, app, rows[1]);
    render_keybinds(f, app, rows[2]);

    if app.command.active {
        render_command_overlay(f, app, area);
    }
}

// ── Sidebar ───────────────────────────────────────────────────────────────────

fn render_sidebar(f: &mut Frame, app: &App, area: Rect) {
    let block = Block::default()
        .borders(Borders::RIGHT)
        .border_style(Style::default().fg(Color::Rgb(40, 40, 40)));
    let inner = block.inner(area);
    f.render_widget(block, area);

    if inner.width == 0 || inner.height < 4 {
        return;
    }

    let art_h = (inner.width / 2).min(inner.height.saturating_sub(5));

    let layout = Layout::vertical([
        Constraint::Length(art_h),
        Constraint::Length(1),
        Constraint::Min(0),
    ])
    .split(inner);

    render_sidebar_art(f, app, layout[0]);

    let div = "─".repeat(inner.width as usize);
    f.render_widget(
        Paragraph::new(div).style(Style::default().fg(Color::Rgb(40, 40, 40))),
        layout[1],
    );

    render_sidebar_nav(f, app, layout[2]);
}

fn render_sidebar_art(f: &mut Frame, app: &App, area: Rect) {
    let np = &app.now_playing;
    let w = area.width;
    let h = area.height;
    if w == 0 || h == 0 {
        return;
    }

    if let Some(bytes) = &np.art_bytes {
        let mut cache = np.art_cache.borrow_mut();
        let stale = cache.as_ref().map(|(cw, ch, _)| *cw != w || *ch != h).unwrap_or(true);
        if stale {
            let payload = if is_kitty() {
                ArtPayload::KittySeq(kitty_image_seq(bytes, w, h, 1))
            } else {
                ArtPayload::HalfBlocks(image_to_half_blocks(bytes, w as u32, h as u32))
            };
            *cache = Some((w, h, payload));
        }
        match cache.as_ref() {
            Some((_, _, ArtPayload::HalfBlocks(lines))) => {
                f.render_widget(Paragraph::new(lines.clone()), area);
            }
            Some((_, _, ArtPayload::KittySeq(seq))) if !seq.is_empty() => {
                let buf = f.buffer_mut();
                for y in area.y..area.y + area.height {
                    for x in area.x..area.x + area.width {
                        if let Some(cell) = buf.cell_mut((x, y)) {
                            cell.reset();
                        }
                    }
                }
                if let Some(cell) = buf.cell_mut((area.x, area.y)) {
                    cell.set_symbol(seq);
                }
            }
            _ => {}
        }
    } else if np.art_loading {
        let spinner = spinner_char(app.tick);
        f.render_widget(
            Paragraph::new(spinner.to_string())
                .style(Style::default().fg(DIM))
                .alignment(Alignment::Center),
            area,
        );
    } else {
        let ch = np.track.as_ref()
            .and_then(|t| t.title.chars().next())
            .map(|c| c.to_uppercase().to_string())
            .unwrap_or_else(|| "♪".to_string());
        let style = if np.track.is_some() {
            Style::default().fg(Color::Black).bg(ACCENT).add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(DIM)
        };
        f.render_widget(
            Paragraph::new(ch).style(style).alignment(Alignment::Center),
            area,
        );
    }
}

fn render_sidebar_nav(f: &mut Frame, app: &App, area: Rect) {
    if app.search.active {
        let cursor = if (app.tick / 30) % 2 == 0 { "█" } else { " " };
        f.render_widget(
            Paragraph::new(Line::from(vec![
                Span::styled(" / ", Style::default().fg(ACCENT).add_modifier(Modifier::BOLD)),
                Span::styled(
                    format!("{}{}", app.search.query, cursor),
                    Style::default().fg(Color::White),
                ),
            ])),
            area,
        );
        return;
    }

    for (i, tab) in Tab::ALL.iter().enumerate() {
        let y = area.y + i as u16;
        if y >= area.y + area.height {
            break;
        }
        let selected = app.current_tab == *tab;
        let style = if selected {
            Style::default().bg(SELECT_BG).fg(Color::White).add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(DIM)
        };
        f.render_widget(
            Paragraph::new(format!(" {}", tab.title())).style(style),
            Rect::new(area.x, y, area.width, 1),
        );
    }
}

// ── Queue panel ───────────────────────────────────────────────────────────────

fn render_queue(f: &mut Frame, app: &App, area: Rect) {
    let focused = app.queue_focused;
    let border_style = if focused {
        Style::default().fg(ACCENT)
    } else {
        Style::default().fg(Color::Rgb(40, 40, 40))
    };
    let block = Block::default()
        .title(Span::styled(" Queue ", Style::default().fg(ACCENT).add_modifier(Modifier::BOLD)))
        .borders(Borders::LEFT)
        .border_style(border_style);
    let inner = block.inner(area);
    f.render_widget(block, area);

    let queue = &app.now_playing.queue;
    if queue.is_empty() {
        f.render_widget(
            Paragraph::new("no queue")
                .style(Style::default().fg(DIM))
                .alignment(Alignment::Center),
            inner,
        );
        return;
    }

    let current = app.now_playing.queue_index;
    let cursor = app.queue_cursor;
    let anchor = if focused { cursor } else { current };
    let item_h = 2usize;
    let visible = (inner.height as usize).saturating_div(item_h).max(1);
    let offset = if anchor + 1 > visible { anchor + 1 - visible } else { 0 };

    let mut y = inner.y;
    for (i, track) in queue.iter().enumerate().skip(offset) {
        if y + 1 >= inner.y + inner.height {
            break;
        }
        let is_cur = i == current;
        let is_cursor = focused && i == cursor;
        let bg = if is_cur { SELECT_BG } else if is_cursor { HIGHLIGHT_BG } else { Color::Reset };
        let artist_style = Style::default().bg(bg).fg(if is_cur { Color::Rgb(180, 200, 255) } else { DIM });
        let title_style = Style::default().bg(bg).fg(Color::White).add_modifier(Modifier::BOLD);

        let indicator = if is_cur { "♪ " } else if is_cursor { "▶ " } else { "  " };
        f.render_widget(
            Paragraph::new(format!("{indicator}{}", track.artist_name())).style(artist_style),
            Rect::new(inner.x, y, inner.width, 1),
        );
        f.render_widget(
            Paragraph::new(format!("  {}", track.title)).style(title_style),
            Rect::new(inner.x, y + 1, inner.width, 1),
        );
        y += item_h as u16;
    }
}

// ── Command overlay ───────────────────────────────────────────────────────────

fn render_command_overlay(f: &mut Frame, app: &App, area: Rect) {
    let matches = app.command.matches();

    let box_w: u16 = 34;
    // border(2) + input(1) + divider(1) + items (at least 1)
    let box_h: u16 = 4 + matches.len().max(1) as u16;

    let x = area.x + area.width.saturating_sub(box_w) / 2;
    let y = area.y + area.height.saturating_sub(box_h) / 2;
    let overlay = Rect::new(
        x.min(area.right().saturating_sub(box_w)),
        y.min(area.bottom().saturating_sub(box_h)),
        box_w.min(area.width),
        box_h.min(area.height),
    );

    f.render_widget(Clear, overlay);
    let block = Block::default()
        .title(Span::styled(" command ", Style::default().fg(ACCENT).add_modifier(Modifier::BOLD)))
        .borders(Borders::ALL)
        .border_style(Style::default().fg(ACCENT));
    let inner = block.inner(overlay);
    f.render_widget(block, overlay);

    if inner.height == 0 {
        return;
    }

    // Input line: "/ <typed><ghost>█"
    let q_lower = app.command.input.to_lowercase();
    let ghost = matches.first().map(|m| &m[q_lower.len()..]).unwrap_or("");
    let cursor = if (app.tick / 30) % 2 == 0 { "█" } else { " " };
    f.render_widget(
        Paragraph::new(Line::from(vec![
            Span::styled("/ ", Style::default().fg(ACCENT).add_modifier(Modifier::BOLD)),
            Span::styled(app.command.input.clone(), Style::default().fg(Color::White)),
            Span::styled(ghost, Style::default().fg(DIM)),
            Span::styled(cursor, Style::default().fg(Color::White)),
        ])),
        Rect::new(inner.x, inner.y, inner.width, 1),
    );

    if inner.height < 2 {
        return;
    }

    // Thin divider between input and list
    f.render_widget(
        Paragraph::new("─".repeat(inner.width as usize)).style(Style::default().fg(DIM)),
        Rect::new(inner.x, inner.y + 1, inner.width, 1),
    );

    // Command rows
    if matches.is_empty() {
        f.render_widget(
            Paragraph::new(" no match").style(Style::default().fg(DIM)),
            Rect::new(inner.x, inner.y + 2, inner.width, 1),
        );
    } else {
        for (i, cmd) in matches.iter().enumerate() {
            let row_y = inner.y + 2 + i as u16;
            if row_y >= inner.y + inner.height {
                break;
            }
            let selected = i == app.command.selected;
            let style = if selected {
                Style::default().bg(SELECT_BG).fg(Color::White).add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(DIM)
            };
            f.render_widget(
                Paragraph::new(format!(" {cmd}")).style(style),
                Rect::new(inner.x, row_y, inner.width, 1),
            );
        }
    }
}

// ── Main content area ─────────────────────────────────────────────────────────

fn render_content(f: &mut Frame, app: &App, area: Rect) {
    // If there's a view on the stack, render it
    if let Some(view) = app.view_stack.last() {
        match view {
            View::ArtistDetail(detail) => {
                render_artist_detail(f, app, detail, area);
                return;
            }
            View::PlaylistDetail(detail) => {
                render_track_list(f, app, &detail.tracks.items, detail.tracks.selected, true, area, &detail.playlist.title);
                return;
            }
            View::AlbumDetail(detail) => {
                render_album_detail(f, app, detail, area);
                return;
            }
        }
    }

    match app.current_tab {
        Tab::Artists => render_artist_list(f, app, area),
        Tab::Playlists => render_playlist_list(f, app, area),
        Tab::Favorites => render_track_list(
            f, app,
            &app.favorites.items,
            app.favorites.selected,
            true,
            area,
            "Favorites",
        ),
        Tab::Search => render_search_results(f, app, area),
    }
}

// ── Artists list ──────────────────────────────────────────────────────────────

fn render_artist_list(f: &mut Frame, app: &App, area: Rect) {
    let loading = app.artists.loading && app.artists.items.is_empty();
    let spinner = spinner_char(app.tick);

    let block = Block::default()
        .title(if loading {
            format!(" Artists {spinner} ")
        } else {
            format!(" Artists ({}) ", app.artists.total)
        })
        .borders(Borders::ALL)
        .border_style(Style::default().fg(ACCENT));

    let inner = block.inner(area);
    f.render_widget(block, area);

    let height = inner.height as usize;
    let items: Vec<ListItem> = visible_artist_items(&app.artists.items, app.artists.selected, height)
        .iter()
        .enumerate()
        .map(|(i, (abs_idx, artist))| {
            let selected = *abs_idx == app.artists.selected;
            let style = if selected {
                Style::default().bg(HIGHLIGHT_BG).fg(Color::White).add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(Color::White)
            };
            let prefix = if selected { "▶ " } else { "  " };
            ListItem::new(format!("{prefix}{}", artist.name)).style(style)
        })
        .collect();

    if items.is_empty() && !loading {
        let p = Paragraph::new("No followed artists found.")
            .style(Style::default().fg(DIM))
            .alignment(Alignment::Center);
        f.render_widget(p, inner);
        return;
    }

    let list = List::new(items);
    f.render_widget(list, inner);
}

// ── Artist detail (tracks + albums split) ─────────────────────────────────────

fn render_artist_detail(
    f: &mut Frame,
    app: &App,
    detail: &crate::app::ArtistDetail,
    area: Rect,
) {
    let art_col_w: u16 = 22;
    let art_inner_w = art_col_w.saturating_sub(2);
    let art_h = art_inner_w / 2;
    let art_box_h = art_h + 2;

    let cols = Layout::horizontal([
        Constraint::Length(art_col_w),
        Constraint::Min(0),
    ])
    .split(area);

    let left_rows = Layout::vertical([
        Constraint::Length(art_box_h),
        Constraint::Min(0),
    ])
    .split(cols[0]);

    render_artist_art(f, app, detail, left_rows[0]);

    render_artist_bio(f, app, detail, left_rows[1]);

    let panels = Layout::horizontal([Constraint::Percentage(55), Constraint::Percentage(45)])
        .split(cols[1]);

    render_artist_tracks(f, app, detail, panels[0]);
    render_artist_albums(f, app, detail, panels[1]);
}

fn render_artist_art(f: &mut Frame, app: &App, detail: &crate::app::ArtistDetail, area: Rect) {
    let art_block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(DIM));
    let inner = art_block.inner(area);
    f.render_widget(art_block, area);

    let w = inner.width;
    let h = inner.height;
    if w == 0 || h == 0 {
        return;
    }

    if let Some(bytes) = &detail.art_bytes {
        let mut cache = detail.art_cache.borrow_mut();
        let stale = cache.as_ref().map(|(cw, ch, _)| *cw != w || *ch != h).unwrap_or(true);
        if stale {
            let payload = if is_kitty() {
                crate::app::ArtPayload::KittySeq(kitty_image_seq(bytes, w, h, 3))
            } else {
                crate::app::ArtPayload::HalfBlocks(image_to_half_blocks(bytes, w as u32, h as u32))
            };
            *cache = Some((w, h, payload));
        }
        match cache.as_ref() {
            Some((_, _, crate::app::ArtPayload::HalfBlocks(lines))) => {
                f.render_widget(Paragraph::new(lines.clone()), inner);
            }
            Some((_, _, crate::app::ArtPayload::KittySeq(seq))) if !seq.is_empty() => {
                let buf = f.buffer_mut();
                for y in inner.y..inner.y + inner.height {
                    for x in inner.x..inner.x + inner.width {
                        if let Some(cell) = buf.cell_mut((x, y)) { cell.reset(); }
                    }
                }
                if let Some(cell) = buf.cell_mut((inner.x, inner.y)) {
                    cell.set_symbol(seq);
                }
            }
            _ => {}
        }
    } else if detail.art_loading {
        f.render_widget(
            Paragraph::new(spinner_char(app.tick).to_string())
                .style(Style::default().fg(DIM))
                .alignment(Alignment::Center),
            inner,
        );
    } else {
        let ch: String = detail.artist.name.chars().next().unwrap_or('?').to_uppercase().collect();
        f.render_widget(
            Paragraph::new(ch)
                .style(Style::default().fg(Color::Black).bg(ACCENT).add_modifier(Modifier::BOLD))
                .alignment(Alignment::Center),
            inner,
        );
    }
}

fn render_artist_bio(f: &mut Frame, app: &App, detail: &crate::app::ArtistDetail, area: Rect) {
    let focused = detail.focus == ArtistDetailFocus::Bio;
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(if focused { Style::default().fg(ACCENT) } else { Style::default().fg(DIM) });
    let inner = block.inner(area);
    f.render_widget(block, area);

    if inner.height == 0 {
        return;
    }

    // Artist name always at the top.
    f.render_widget(
        Paragraph::new(detail.artist.name.as_str())
            .style(Style::default().fg(ACCENT).add_modifier(Modifier::BOLD))
            .alignment(Alignment::Center),
        Rect::new(inner.x, inner.y, inner.width, 1),
    );

    if inner.height < 3 {
        return;
    }

    let bio_area = Rect::new(inner.x, inner.y + 2, inner.width, inner.height - 2);

    if detail.bio_loading {
        f.render_widget(
            Paragraph::new(spinner_char(app.tick).to_string())
                .style(Style::default().fg(DIM))
                .alignment(Alignment::Center),
            bio_area,
        );
    } else if let Some(bio) = &detail.bio {
        // Strip HTML tags that Tidal sometimes includes.
        let clean: String = {
            let mut out = String::with_capacity(bio.len());
            let mut in_tag = false;
            for ch in bio.chars() {
                match ch {
                    '<' => in_tag = true,
                    '>' => in_tag = false,
                    _ if !in_tag => out.push(ch),
                    _ => {}
                }
            }
            out
        };
        f.render_widget(
            Paragraph::new(clean)
                .style(Style::default().fg(Color::Rgb(180, 180, 180)))
                .wrap(Wrap { trim: true })
                .scroll((detail.bio_scroll, 0)),
            bio_area,
        );
    } else {
        f.render_widget(
            Paragraph::new("No biography available.")
                .style(Style::default().fg(DIM))
                .alignment(Alignment::Center),
            bio_area,
        );
    }
}

fn render_artist_tracks(
    f: &mut Frame,
    app: &App,
    detail: &crate::app::ArtistDetail,
    area: Rect,
) {
    let focused = detail.focus == ArtistDetailFocus::Tracks;
    let spinner = spinner_char(app.tick);
    let loading = detail.tracks.loading;

    let border_style = if focused {
        Style::default().fg(ACCENT)
    } else {
        Style::default().fg(DIM)
    };

    let block = Block::default()
        .title(if loading {
            format!(" Top Tracks {spinner} ")
        } else {
            " Top Tracks ".to_string()
        })
        .borders(Borders::ALL)
        .border_style(border_style);

    let inner = block.inner(area);
    f.render_widget(block, area);

    let height = inner.height as usize;
    let items: Vec<ListItem> = detail.tracks.items
        .iter()
        .enumerate()
        .map(|(i, track)| {
            let selected = i == detail.tracks.selected && focused;
            let style = if selected {
                Style::default().bg(HIGHLIGHT_BG).fg(Color::White).add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(Color::White)
            };
            let prefix = if selected { "▶ " } else { "  " };
            let playing = app.now_playing.track.as_ref().map(|t| t.id == track.id).unwrap_or(false);
            let indicator = if playing { "♪ " } else { "" };
            ListItem::new(format!("{prefix}{indicator}{i:>2}. {} ({})", track.title, track.duration_display()))
                .style(style)
        })
        .collect();

    let list = List::new(items);
    f.render_widget(list, inner);
}

fn render_artist_albums(
    f: &mut Frame,
    app: &App,
    detail: &crate::app::ArtistDetail,
    area: Rect,
) {
    let focused = detail.focus == ArtistDetailFocus::Albums;
    let spinner = spinner_char(app.tick);
    let loading = detail.albums.loading;

    let border_style = if focused {
        Style::default().fg(ACCENT)
    } else {
        Style::default().fg(DIM)
    };

    let block = Block::default()
        .title(if loading {
            format!(" Albums {spinner} ")
        } else {
            " Albums ".to_string()
        })
        .borders(Borders::ALL)
        .border_style(border_style);

    let inner = block.inner(area);
    f.render_widget(block, area);

    let items: Vec<ListItem> = detail.albums.items
        .iter()
        .enumerate()
        .map(|(i, album)| {
            let selected = i == detail.albums.selected && focused;
            let style = if selected {
                Style::default().bg(HIGHLIGHT_BG).fg(Color::White).add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(Color::White)
            };
            let prefix = if selected { "▶ " } else { "  " };
            let year = album.release_date.as_deref().and_then(|d| d.get(..4)).unwrap_or("----");
            let n = album.number_of_tracks.unwrap_or(0);
            ListItem::new(format!("{prefix}{} ({year}, {n} tracks)", album.title)).style(style)
        })
        .collect();

    let list = List::new(items);
    f.render_widget(list, inner);
}

// ── Playlists ─────────────────────────────────────────────────────────────────

fn render_playlist_list(f: &mut Frame, app: &App, area: Rect) {
    let spinner = spinner_char(app.tick);
    let loading = app.playlists.loading && app.playlists.items.is_empty();

    let block = Block::default()
        .title(if loading {
            format!(" Playlists {spinner} ")
        } else {
            format!(" Playlists ({}) ", app.playlists.total)
        })
        .borders(Borders::ALL)
        .border_style(Style::default().fg(ACCENT));

    let inner = block.inner(area);
    f.render_widget(block, area);

    let items: Vec<ListItem> = app.playlists.items
        .iter()
        .enumerate()
        .map(|(i, pl)| {
            let selected = i == app.playlists.selected;
            let style = if selected {
                Style::default().bg(HIGHLIGHT_BG).fg(Color::White).add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(Color::White)
            };
            let prefix = if selected { "▶ " } else { "  " };
            ListItem::new(format!("{prefix}{} ({} tracks)", pl.title, pl.number_of_tracks))
                .style(style)
        })
        .collect();

    if items.is_empty() && !loading {
        let p = Paragraph::new("No playlists found.")
            .style(Style::default().fg(DIM))
            .alignment(Alignment::Center);
        f.render_widget(p, inner);
        return;
    }

    let list = List::new(items);
    f.render_widget(list, inner);
}

// ── Generic track list ────────────────────────────────────────────────────────

fn render_track_list(
    f: &mut Frame,
    app: &App,
    tracks: &[Track],
    selected: usize,
    focused: bool,
    area: Rect,
    title: &str,
) {
    let block = Block::default()
        .title(format!(" {title} ({}) ", tracks.len()))
        .borders(Borders::ALL)
        .border_style(Style::default().fg(ACCENT));

    let inner = block.inner(area);
    f.render_widget(block, area);

    let height = inner.height as usize;
    let offset = scroll_offset(selected, height);

    let items: Vec<ListItem> = tracks
        .iter()
        .enumerate()
        .skip(offset)
        .take(height)
        .map(|(i, track)| {
            let is_selected = i == selected && focused;
            let is_playing = app.now_playing.track.as_ref().map(|t| t.id == track.id).unwrap_or(false);
            let style = if is_selected {
                Style::default().bg(HIGHLIGHT_BG).fg(Color::White).add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(Color::White)
            };
            let prefix = if is_selected { "▶ " } else { "  " };
            let playing = if is_playing { "♪ " } else { "" };
            ListItem::new(format!(
                "{prefix}{playing}{i:>3}. {} — {} ({})",
                track.title,
                track.artist_name(),
                track.duration_display()
            ))
            .style(style)
        })
        .collect();

    if items.is_empty() {
        let p = Paragraph::new("No tracks.")
            .style(Style::default().fg(DIM))
            .alignment(Alignment::Center);
        f.render_widget(p, inner);
        return;
    }

    let list = List::new(items);
    f.render_widget(list, inner);
}

// ── Album detail ──────────────────────────────────────────────────────────────

pub fn is_kitty() -> bool {
    std::env::var("KITTY_WINDOW_ID").is_ok()
}

/// Build a Kitty graphics protocol escape sequence for the given raw image bytes.
/// `image_id`: 1 = sidebar art, 2 = album detail art.
fn kitty_image_seq(bytes: &[u8], cols: u16, rows: u16, image_id: u16) -> String {
    use image::GenericImageView;
    let img = match image::load_from_memory(bytes) {
        Ok(img) => img,
        Err(_) => return String::new(),
    };
    let (w, h) = img.dimensions();
    let rgba = img.to_rgba8().into_raw();
    let b64 = base64_encode(&rgba);

    const CHUNK: usize = 4096;
    let mut seq = String::new();
    let mut pos = 0;
    let mut first = true;
    while pos < b64.len() {
        let end = (pos + CHUNK).min(b64.len());
        let chunk = &b64[pos..end];
        let more = u8::from(end < b64.len());
        if first {
            seq.push_str(&format!(
                "\x1b_Ga=T,f=32,i={image_id},q=2,s={w},v={h},c={cols},r={rows},m={more};{chunk}\x1b\\"
            ));
            first = false;
        } else {
            seq.push_str(&format!("\x1b_Gm={more};{chunk}\x1b\\"));
        }
        pos = end;
    }
    seq
}

fn base64_encode(data: &[u8]) -> String {
    const CHARS: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity((data.len() + 2) / 3 * 4);
    for chunk in data.chunks(3) {
        let b0 = chunk[0] as u32;
        let b1 = if chunk.len() > 1 { chunk[1] as u32 } else { 0 };
        let b2 = if chunk.len() > 2 { chunk[2] as u32 } else { 0 };
        let n = (b0 << 16) | (b1 << 8) | b2;
        out.push(CHARS[(n >> 18) as usize] as char);
        out.push(CHARS[((n >> 12) & 0x3F) as usize] as char);
        out.push(if chunk.len() > 1 { CHARS[((n >> 6) & 0x3F) as usize] as char } else { '=' });
        out.push(if chunk.len() > 2 { CHARS[(n & 0x3F) as usize] as char } else { '=' });
    }
    out
}

fn image_to_half_blocks(data: &[u8], cols: u32, rows: u32) -> Vec<Line<'static>> {
    use image::GenericImageView;

    let Ok(img) = image::load_from_memory(data) else {
        return vec![];
    };
    let img = img.resize_exact(cols, rows * 2, image::imageops::FilterType::Lanczos3);

    (0..rows)
        .map(|row| {
            let spans: Vec<Span<'static>> = (0..cols)
                .map(|col| {
                    let top = img.get_pixel(col, row * 2);
                    let bot = img.get_pixel(col, row * 2 + 1);
                    Span::styled(
                        "▀",
                        Style::default()
                            .fg(Color::Rgb(top[0], top[1], top[2]))
                            .bg(Color::Rgb(bot[0], bot[1], bot[2])),
                    )
                })
                .collect();
            Line::from(spans)
        })
        .collect()
}

fn render_album_detail(f: &mut Frame, app: &App, detail: &crate::app::AlbumDetail, area: Rect) {
    // Left column: art (top) + metadata (below).  Right column: full-height track list.
    let art_cols = (area.width * 2 / 5).max(10);
    let art_rows = (art_cols / 2).max(5).min(area.height.saturating_sub(7)); // cap so metadata fits
    let art_box_h = art_rows + 2; // +2 borders
    let left_col_w = art_cols + 2;

    // Horizontal split: left sidebar | tracks
    let cols = Layout::horizontal([
        Constraint::Length(left_col_w),
        Constraint::Min(0),
    ])
    .split(area);

    // Left sidebar: art (fixed) + metadata (remainder)
    let left_rows = Layout::vertical([
        Constraint::Length(art_box_h),
        Constraint::Min(0),
    ])
    .split(cols[0]);

    // Alias for clarity — art area is left_rows[0], metadata is left_rows[1]
    let header_cols = [left_rows[0], left_rows[1]];

    // ── Album art ─────────────────────────────────────────────────────────────
    let art_block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(ACCENT));
    let art_inner = art_block.inner(header_cols[0]);
    f.render_widget(art_block, header_cols[0]);

    if let Some(bytes) = &detail.art_bytes {
        let w = art_inner.width;
        let h = art_inner.height;
        if w > 0 && h > 0 {
            let mut cache = detail.art_cache.borrow_mut();
            let stale = cache.as_ref()
                .map(|(cw, ch, _)| *cw != w || *ch != h)
                .unwrap_or(true);
            if stale {
                let payload = if is_kitty() {
                    ArtPayload::KittySeq(kitty_image_seq(bytes, w, h, 2))
                } else {
                    ArtPayload::HalfBlocks(image_to_half_blocks(bytes, w as u32, h as u32))
                };
                *cache = Some((w, h, payload));
            }
            match cache.as_ref() {
                Some((_, _, ArtPayload::HalfBlocks(lines))) => {
                    f.render_widget(Paragraph::new(lines.clone()), art_inner);
                }
                Some((_, _, ArtPayload::KittySeq(seq))) if !seq.is_empty() => {
                    let buf = f.buffer_mut();
                    for y in art_inner.y..art_inner.y + art_inner.height {
                        for x in art_inner.x..art_inner.x + art_inner.width {
                            if let Some(cell) = buf.cell_mut((x, y)) {
                                cell.reset();
                            }
                        }
                    }
                    if let Some(cell) = buf.cell_mut((art_inner.x, art_inner.y)) {
                        cell.set_symbol(seq);
                    }
                }
                _ => {}
            }
        }
    } else if detail.art_loading {
        let spinner = spinner_char(app.tick);
        f.render_widget(
            Paragraph::new(format!("{spinner}"))
                .style(Style::default().fg(DIM))
                .alignment(Alignment::Center),
            art_inner,
        );
    } else {
        let ch: String = detail.album.title.chars().next().unwrap_or('?').to_uppercase().collect();
        f.render_widget(
            Paragraph::new(ch)
                .style(Style::default().fg(Color::Black).bg(ACCENT).add_modifier(Modifier::BOLD))
                .alignment(Alignment::Center),
            art_inner,
        );
    }

    // ── Album metadata ────────────────────────────────────────────────────────
    let year = detail.album.release_date.as_deref().and_then(|d| d.get(..4)).unwrap_or("----");
    let n_tracks = detail.album.number_of_tracks.unwrap_or(0);
    let artist_name = detail.album.artist.as_ref().map(|a| a.name.as_str()).unwrap_or("");

    let info = Paragraph::new(vec![
        Line::from(Span::styled(
            detail.album.title.as_str(),
            Style::default().fg(ACCENT).add_modifier(Modifier::BOLD),
        )),
        Line::from(Span::styled(artist_name, Style::default().fg(Color::White))),
        Line::from(Span::styled(
            format!("{year}  •  {n_tracks} tracks"),
            Style::default().fg(DIM),
        )),
    ])
    .block(Block::default().borders(Borders::ALL).border_style(Style::default().fg(DIM)));
    f.render_widget(info, header_cols[1]);

    // ── Track list (full right column) ────────────────────────────────────────
    let spinner = spinner_char(app.tick);
    let title = if detail.tracks.loading {
        format!(" Tracks {spinner} ")
    } else {
        format!(" Tracks ({}) ", detail.tracks.items.len())
    };
    render_track_list(f, app, &detail.tracks.items, detail.tracks.selected, true, cols[1], &title);
}

// ── Search results (three-pane layout) ───────────────────────────────────────

fn render_search_results(f: &mut Frame, app: &App, area: Rect) {
    if app.search.total_results() == 0 && !app.search.loading {
        let hint = if app.search.query.is_empty() {
            "Press / to start searching"
        } else {
            "No results"
        };
        let block = Block::default()
            .title(" Search ")
            .borders(Borders::ALL)
            .border_style(Style::default().fg(ACCENT));
        let inner = block.inner(area);
        f.render_widget(block, area);
        f.render_widget(
            Paragraph::new(hint)
                .style(Style::default().fg(DIM))
                .alignment(Alignment::Center),
            inner,
        );
        return;
    }

    if app.search.loading {
        let spinner = spinner_char(app.tick);
        let block = Block::default()
            .title(format!(" Searching {spinner} "))
            .borders(Borders::ALL)
            .border_style(Style::default().fg(DIM));
        f.render_widget(block, area);
        return;
    }

    let panes = Layout::horizontal([
        Constraint::Percentage(40),
        Constraint::Percentage(30),
        Constraint::Percentage(30),
    ])
    .split(area);

    render_search_pane_tracks(f, app, panes[0]);
    render_search_pane_artists(f, app, panes[1]);
    render_search_pane_playlists(f, app, panes[2]);
}

fn render_search_pane_tracks(f: &mut Frame, app: &App, area: Rect) {
    let active = app.search.pane == SearchPane::Tracks;
    let border_style = if active { Style::default().fg(ACCENT) } else { Style::default().fg(DIM) };
    let block = Block::default()
        .title(format!(" Tracks ({}) ", app.search.tracks.len()))
        .borders(Borders::ALL)
        .border_style(border_style);
    let inner = block.inner(area);
    f.render_widget(block, area);

    let sel = app.search.track_sel;
    let height = inner.height as usize;
    let offset = scroll_offset(sel, height);
    let items: Vec<ListItem> = app.search.tracks
        .iter()
        .enumerate()
        .skip(offset)
        .take(height)
        .map(|(i, t)| {
            let selected = active && i == sel;
            let is_playing = app.now_playing.track.as_ref().map(|np| np.id == t.id).unwrap_or(false);
            let style = if selected {
                Style::default().bg(HIGHLIGHT_BG).fg(Color::White).add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(Color::White)
            };
            let prefix = if selected { "▶ " } else { "  " };
            let playing = if is_playing { "♪ " } else { "" };
            ListItem::new(format!(
                "{prefix}{playing}{} — {} ({})",
                t.title, t.artist_name(), t.duration_display()
            ))
            .style(style)
        })
        .collect();

    if items.is_empty() {
        f.render_widget(
            Paragraph::new("No tracks").style(Style::default().fg(DIM)).alignment(Alignment::Center),
            inner,
        );
    } else {
        f.render_widget(List::new(items), inner);
    }
}

fn render_search_pane_artists(f: &mut Frame, app: &App, area: Rect) {
    let active = app.search.pane == SearchPane::Artists;
    let border_style = if active { Style::default().fg(ACCENT) } else { Style::default().fg(DIM) };
    let block = Block::default()
        .title(format!(" Artists ({}) ", app.search.artists.len()))
        .borders(Borders::ALL)
        .border_style(border_style);
    let inner = block.inner(area);
    f.render_widget(block, area);

    let sel = app.search.artist_sel;
    let height = inner.height as usize;
    let offset = scroll_offset(sel, height);
    let items: Vec<ListItem> = app.search.artists
        .iter()
        .enumerate()
        .skip(offset)
        .take(height)
        .map(|(i, a)| {
            let selected = active && i == sel;
            let style = if selected {
                Style::default().bg(HIGHLIGHT_BG).fg(Color::White).add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(Color::White)
            };
            let prefix = if selected { "▶ " } else { "  " };
            ListItem::new(format!("{prefix}{}", a.name)).style(style)
        })
        .collect();

    if items.is_empty() {
        f.render_widget(
            Paragraph::new("No artists").style(Style::default().fg(DIM)).alignment(Alignment::Center),
            inner,
        );
    } else {
        f.render_widget(List::new(items), inner);
    }
}

fn render_search_pane_playlists(f: &mut Frame, app: &App, area: Rect) {
    let active = app.search.pane == SearchPane::Playlists;
    let border_style = if active { Style::default().fg(ACCENT) } else { Style::default().fg(DIM) };
    let block = Block::default()
        .title(format!(" Playlists ({}) ", app.search.playlists.len()))
        .borders(Borders::ALL)
        .border_style(border_style);
    let inner = block.inner(area);
    f.render_widget(block, area);

    let sel = app.search.playlist_sel;
    let height = inner.height as usize;
    let offset = scroll_offset(sel, height);
    let items: Vec<ListItem> = app.search.playlists
        .iter()
        .enumerate()
        .skip(offset)
        .take(height)
        .map(|(i, pl)| {
            let selected = active && i == sel;
            let style = if selected {
                Style::default().bg(HIGHLIGHT_BG).fg(Color::White).add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(Color::White)
            };
            let prefix = if selected { "▶ " } else { "  " };
            ListItem::new(format!("{prefix}{} ({} tracks)", pl.title, pl.number_of_tracks))
                .style(style)
        })
        .collect();

    if items.is_empty() {
        f.render_widget(
            Paragraph::new("No playlists").style(Style::default().fg(DIM)).alignment(Alignment::Center),
            inner,
        );
    } else {
        f.render_widget(List::new(items), inner);
    }
}


// ── Now playing bar ───────────────────────────────────────────────────────────

fn render_now_playing(f: &mut Frame, app: &App, area: Rect) {
    let block = Block::default()
        .borders(Borders::TOP)
        .border_style(Style::default().fg(DIM));
    let inner = block.inner(area); // height = 6 (7 - 1 border)
    f.render_widget(block, area);

    let sections = Layout::vertical([
        Constraint::Length(3), // lyrics
        Constraint::Length(1), // spacer
        Constraint::Min(0),    // track info / waveform / time
    ])
    .split(inner);

    render_lyrics(f, app, sections[0]);

    let cols = Layout::horizontal([
        Constraint::Percentage(35),
        Constraint::Percentage(30),
        Constraint::Percentage(35),
    ])
    .split(sections[2]);

    let track_info: Vec<Line> = match &app.now_playing.track {
        Some(t) => {
            let quality = t.quality_display();
            let mut album_spans = vec![
                Span::styled(t.album.title.as_str(), Style::default().fg(DIM)),
            ];
            if !quality.is_empty() {
                album_spans.push(Span::styled("  ", Style::default()));
                album_spans.push(Span::styled(
                    quality,
                    Style::default().fg(ACCENT).add_modifier(Modifier::BOLD),
                ));
            }
            vec![
                Line::from(Span::styled(t.title.as_str(), Style::default().fg(Color::White).add_modifier(Modifier::BOLD))),
                Line::from(Span::styled(t.artist_name(), Style::default().fg(Color::White))),
                Line::from(album_spans),
            ]
        }
        None => vec![
            Line::from(Span::styled("No track playing", Style::default().fg(DIM))),
        ],
    };
    f.render_widget(Paragraph::new(track_info), cols[0]);

    f.render_widget(render_squib(app, cols[1].width), cols[1]);

    let time_str = format!("{} / {}", app.now_playing.position_display(), app.now_playing.duration_display());
    f.render_widget(
        Paragraph::new(time_str).alignment(Alignment::Right).style(Style::default().fg(DIM)),
        cols[2],
    );

    if let Some((msg, level)) = &app.status {
        let color = match level { StatusLevel::Error => Color::Red, StatusLevel::Info => Color::Green };
        f.render_widget(
            Paragraph::new(msg.as_str()).style(Style::default().fg(color)).alignment(Alignment::Right),
            cols[2],
        );
    }
}

fn render_lyrics(f: &mut Frame, app: &App, area: Rect) {
    let np = &app.now_playing;

    if np.lyrics_loading {
        let spinner = spinner_char(app.tick);
        f.render_widget(
            Paragraph::new(spinner.to_string()).style(Style::default().fg(DIM)).alignment(Alignment::Center),
            Rect::new(area.x, area.y + 1, area.width, 1),
        );
        return;
    }

    let lines: &[(f64, String)];
    let plain_buf: Vec<(f64, String)>;

    if !np.lyrics_synced.is_empty() {
        lines = &np.lyrics_synced;
    } else if !np.lyrics_plain.is_empty() {
        // Distribute plain lines evenly across the track duration.
        let n = np.lyrics_plain.len() as f64;
        let dur = if np.duration > 0.0 { np.duration } else { n };
        plain_buf = np.lyrics_plain.iter().enumerate()
            .map(|(i, t)| (i as f64 / n * dur, t.clone()))
            .collect();
        lines = &plain_buf;
    } else {
        return;
    }

    // Find the current line: last one whose timestamp <= playback position.
    let pos = np.position;
    let cur = lines.partition_point(|(t, _)| *t <= pos).saturating_sub(1);

    let show: [Option<usize>; 3] = [
        cur.checked_sub(1),
        Some(cur),
        if cur + 1 < lines.len() { Some(cur + 1) } else { None },
    ];

    for (row, opt) in show.iter().enumerate() {
        if let Some(idx) = opt {
            let (_, text) = &lines[*idx];
            let is_cur = row == 1;
            let style = if is_cur {
                Style::default().fg(Color::White).add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(DIM)
            };
            let y = area.y + row as u16;
            if y < area.y + area.height {
                f.render_widget(
                    Paragraph::new(text.as_str()).style(style).alignment(Alignment::Center),
                    Rect::new(area.x, y, area.width, 1),
                );
            }
        }
    }
}

// ── Keybinds bar ─────────────────────────────────────────────────────────────

fn render_keybinds(f: &mut Frame, app: &App, area: Rect) {
    let in_detail = !app.view_stack.is_empty();
    let in_artist = matches!(app.view_stack.last(), Some(View::ArtistDetail(_)));
    let bio_focused = if let Some(View::ArtistDetail(d)) = app.view_stack.last() {
        d.focus == ArtistDetailFocus::Bio
    } else {
        false
    };
    let in_search_tab = app.current_tab == Tab::Search && !in_detail;

    let hints: &[(&str, &str)] = if app.queue_focused {
        &[("↑↓", "navigate"), ("↵", "play from"), ("d", "remove"), ("←/esc", "back"), ("spc", "pause")]
    } else if app.command.active {
        &[("↑↓", "select"), ("tab", "complete"), ("↵", "go"), ("esc", "cancel")]
    } else if app.search.active {
        &[("↵", "search"), ("esc", "cancel")]
    } else if bio_focused {
        &[
            ("↑↓", "scroll"), ("→", "tracks"), ("esc", "back"),
            ("spc", "pause"), ("n/p", "next/prev"), ("/", "command"), ("q", "quit"),
        ]
    } else if in_artist {
        &[
            ("↑↓", "navigate"), ("←→", "panels"), ("← on tracks", "bio"),
            ("↵", "play/open"), ("a", "queue"),
            ("esc", "back"), ("spc", "pause"), ("n/p", "next/prev"), ("/", "command"), ("q", "quit"),
        ]
    } else if in_detail {
        &[
            ("↑↓", "navigate"), ("↵", "play"), ("a", "queue"),
            ("esc", "back"), ("spc", "pause"), ("n/p", "next/prev"), ("/", "command"), ("q", "quit"),
        ]
    } else if in_search_tab {
        &[
            ("↑↓", "navigate"), ("tab/←→", "panes"), ("↵", "open"), ("a", "queue"),
            ("spc", "pause"), ("n/p", "next/prev"), ("/", "command"), ("q", "quit"),
        ]
    } else {
        &[
            ("↑↓", "navigate"), ("↵", "open"), ("a", "queue"), ("→", "focus queue"),
            ("spc", "pause"), ("n/p", "next/prev"), ("/", "command"), ("q", "quit"),
        ]
    };

    let sep = Span::styled("  ·  ", Style::default().fg(Color::Rgb(60, 60, 60)));
    let mut spans: Vec<Span> = Vec::new();
    for (i, (key, desc)) in hints.iter().enumerate() {
        if i > 0 {
            spans.push(sep.clone());
        }
        spans.push(Span::styled(*key, Style::default().fg(ACCENT).add_modifier(Modifier::BOLD)));
        spans.push(Span::styled(format!(" {desc}"), Style::default().fg(DIM)));
    }

    f.render_widget(
        Paragraph::new(Line::from(spans)).alignment(Alignment::Center),
        area,
    );
}

// ── Helpers ───────────────────────────────────────────────────────────────────


fn scroll_offset(selected: usize, height: usize) -> usize {
    if height == 0 || selected < height {
        0
    } else {
        selected - height + 1
    }
}

fn visible_artist_items<'a>(
    items: &'a [crate::api::models::Artist],
    selected: usize,
    height: usize,
) -> Vec<(usize, &'a crate::api::models::Artist)> {
    let offset = scroll_offset(selected, height);
    items
        .iter()
        .enumerate()
        .skip(offset)
        .take(height)
        .collect()
}

const SPINNER: [char; 10] = ['⠋', '⠙', '⠹', '⠸', '⠼', '⠴', '⠦', '⠧', '⠇', '⠏'];

fn spinner_char(tick: u64) -> char {
    SPINNER[(tick / 3) as usize % SPINNER.len()]
}

/// Animated waveform squib: undulates while playing, flat line while paused.
/// The played portion is highlighted in ACCENT, the remainder in DIM.
fn render_squib(app: &App, width: u16) -> Paragraph<'static> {
    const WAVE: [&str; 8] = ["▁", "▂", "▃", "▄", "▅", "▆", "▇", "█"];

    let ratio = app.now_playing.progress_ratio();
    let played_w = ((width as f64 * ratio) as u16).min(width);
    let playing = app.now_playing.track.is_some() && !app.now_playing.paused;

    let spans: Vec<Span<'static>> = (0..width)
        .map(|i| {
            let color = if i < played_w { ACCENT } else { DIM };
            let ch: &'static str = if playing {
                // Sine wave: spatial frequency ~1 cycle per 8 cols, phase advances with tick
                let phase = i as f64 * 0.8 + app.tick as f64 * 0.35;
                let t = (phase.sin() + 1.0) / 2.0; // 0.0 – 1.0
                WAVE[(t * 7.99) as usize]
            } else {
                "▄" // flat mid-height line when paused or idle
            };
            Span::styled(ch, Style::default().fg(color))
        })
        .collect();

    Paragraph::new(Line::from(spans))
}
