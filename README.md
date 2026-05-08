# Typhoon

A terminal UI music player for Tidal, built with Rust.

<img width="1920" height="1080" alt="Screenshot_2026-05-08-160905" src="https://github.com/user-attachments/assets/70684d59-be7d-4793-938a-8f619334f0ad" />


## Features

- Browse your Tidal library: favorites, artists, playlists, and albums
- Full-text search across tracks, artists, and playlists
- Synchronized lyrics
- Album art in the sidebar and album detail view (pixel-perfect in Kitty terminal, half-block fallback elsewhere)
- Artist pictures and biography
- Queue management — add tracks, navigate to any position, remove entries, play from any point
- Gapless playback via mpv
- Audio quality indicator (Hi-Res, FLAC, MQA, AAC)
- Animated waveform progress bar

## Requirements

- **Rust** 1.85+ (2024 edition) — to build from source
- **mpv** — used as the audio backend; must be on your `PATH`
- A **Tidal** account (HiFi or HiFi Plus recommended for lossless quality)

### Installing mpv

| Platform              | Command                |
|-----------------------|------------------------|
| Linux (Debian/Ubuntu) | `sudo apt install mpv` |
| Linux (Arch)          | `sudo pacman -S mpv`   |
| Linux (Fedora)        | `sudo dnf install mpv` |

## Installation

```bash
git clone https://github.com/yourname/typhoon
cd typhoon
cargo install --path .
```

The `typhoon` binary will be placed in `~/.cargo/bin/`. Make sure that directory is on your `PATH`.

## First run & authentication

Typhoon uses Tidal's OAuth device-authorization flow. On first launch it will print a URL and a short code:

```
╔══════════════════════════════════════════╗
║           Tidal Authorization            ║
╠══════════════════════════════════════════╣
║  Open:                                   ║
║  https://link.tidal.com/XXXXX            ║
╠══════════════════════════════════════════╣
║  Code: ABCD-1234                         ║
╚══════════════════════════════════════════╝

Waiting for authorization…
```

Open the URL in a browser, log in with your Tidal account, and enter the code. Typhoon will save your tokens to the config file and launch immediately. You will not need to authenticate again unless your refresh token expires.

## Configuration

The config file lives at:

| Platform | Path                                                |
|----------|-----------------------------------------------------|
| Linux    | `~/.config/typhoon/config.json`                     |

It is created automatically on first run. Example:

```json
{
  "client_id": null,
  "client_secret": null,
  "access_token": "...",
  "refresh_token": "...",
  "expires_at": "2025-01-01T00:00:00+00:00",
  "user_id": 12345678,
  "country_code": "US",
  "session_id": "..."
}
```

### Using your own OAuth credentials

Typhoon ships with built-in fallback credentials (provided by the open-source [tidalapi](https://github.com/tamland/python-tidal) project). If those credentials are ever revoked you can substitute your own:

1. Register a device-authorization client at [developer.tidal.com](https://developer.tidal.com)
2. Add your credentials to `config.json`:

```json
{
  "client_id": "your-client-id",
  "client_secret": "your-client-secret"
}
```

3. Delete `access_token` and `refresh_token` from the file (or delete the file entirely) to trigger a fresh login with your credentials.

## Keybindings

### Global

| Key       | Action                  |
|-----------|-------------------------|
| `q` / `Q` | Quit                    |
| `Space`   | Pause / resume          |
| `n`       | Next track              |
| `p`       | Previous track          |
| `/`       | Open command palette    |
| `Esc`     | Go back / close overlay |
| `Tab`     | Cycle to next tab       |

### Navigation

| Key       | Action                      |
|-----------|-----------------------------|
| `↑` / `k` | Move up                     |
| `↓` / `j` | Move down                   |
| `←` / `h` | Move left / previous pane   |
| `→` / `l` | Move right / next pane      |
| `Enter`   | Open / play selected item   |
| `a`       | Add selected track to queue |

### Command palette (`/`)

Type the start of a destination and press `Enter` (or `Tab` to autocomplete):

| Command     | Action          |
|-------------|-----------------|
| `favorites` | Go to Favorites |
| `artists`   | Go to Artists   |
| `playlists` | Go to Playlists |
| `search`    | Open search     |

### Search

Press `/` → `search` (or use the command palette) to open the search overlay. Type your query and press `Enter`. Results are shown in three panes — Tracks, Artists, Playlists — switchable with `Tab` or `←`/`→`.

### Artist detail

| Key                                 | Action                         |
|-------------------------------------|--------------------------------|
| `←` / `h` on Tracks                 | Focus bio pane                 |
| `→` / `l` on Bio                    | Focus tracks pane              |
| `←` / `→` between Tracks and Albums | Switch panel focus             |
| `↑` / `↓` in Bio                    | Scroll biography               |
| `Enter` on a track                  | Play track                     |
| `Enter` on an album                 | Open album                     |
| `a`                                 | Add highlighted track to queue |

### Queue panel

Press `→` / `l` from any main tab to focus the queue.

| Key                | Action                    |
|--------------------|---------------------------|
| `↑` / `k`          | Move cursor up            |
| `↓` / `j`          | Move cursor down          |
| `Enter`            | Play from cursor position |
| `d` / `Delete`     | Remove track from queue   |
| `←` / `h` or `Esc` | Return to main view       |

## Kitty terminal graphics

If you run Typhoon inside [Kitty](https://sw.kovidgoyal.net/kitty/), album art and artist pictures are rendered at full pixel resolution using the Kitty graphics protocol. In any other terminal, a half-block (`▀`) approximation is used instead.

## License

This project is licensed under the [GNU General Public License v3.0](LICENSE).

[![License: GPL v3](https://img.shields.io/badge/License-GPLv3-blue.svg)](https://www.gnu.org/licenses/gpl-3.0)
