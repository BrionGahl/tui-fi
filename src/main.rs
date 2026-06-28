mod app;
mod config;
mod history;
mod m3u;
mod mpris;
mod player;
mod playlist;
mod ui;
mod utils;

use std::io;
use std::path::PathBuf;
use std::sync::mpsc;
use std::time::{Duration, Instant};

use crossterm::{
    event::{self, DisableMouseCapture, EnableMouseCapture, Event, KeyCode, KeyModifiers},
    execute,
    terminal::{EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode},
};
use ratatui::{Terminal, backend::CrosstermBackend};

use app::{App, Mode, Panel};
use player::{Player, save_tags};

fn main() -> io::Result<()> {
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen, EnableMouseCapture)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    let mut app = App::new();
    let cfg = config::Config::load();
    let mut player = Player::new(cfg.volume).expect("Failed to init audio");
    let mut last_tick = Instant::now();

    let (mpris_cmd_tx, mpris_cmd_rx) = mpsc::channel::<mpris::Cmd>();
    let mpris = mpris::MprisHandle::spawn(mpris_cmd_tx);
    let mut last_mpris_path: Option<PathBuf> = None;
    let mut last_mpris_paused = false;

    loop {
        terminal.draw(|f| ui::draw(f, &app, &player))?;

        if event::poll(Duration::from_millis(100))?
            && let Event::Key(key) = event::read()?
        {
            handle_key(&mut app, &mut player, key.code, key.modifiers);
        }

        let delta = last_tick.elapsed();
        last_tick = Instant::now();
        app.tick_play_time(delta, player.now_playing.as_ref(), player.paused);

        app.poll_download();
        app.tick_status();
        if player.now_playing.is_some() && !player.paused {
            app.viz_tick = app.viz_tick.wrapping_add(delta.as_millis() as u64);
        }

        if player.now_playing.is_some() && player.is_finished() {
            if let Some(path) = app.advance_track(1) {
                player.play(path);
            } else {
                player.stop();
            }
        }

        // Handle commands from the taskbar/media keys via MPRIS.
        if let Some(ref handle) = mpris {
            while let Ok(cmd) = mpris_cmd_rx.try_recv() {
                match cmd {
                    mpris::Cmd::Play => { if player.paused { player.toggle_pause(); } }
                    mpris::Cmd::Pause => { if !player.paused && player.now_playing.is_some() { player.toggle_pause(); } }
                    mpris::Cmd::Toggle => player.toggle_pause(),
                    mpris::Cmd::Stop => player.stop(),
                    mpris::Cmd::Next => { if let Some(path) = app.advance_track(1) { player.play(path); } }
                    mpris::Cmd::Previous => { if let Some(path) = app.advance_track(-1) { player.play(path); } }
                }
            }
            // Push state to MPRIS whenever track or pause state changes.
            let path_changed = player.now_playing != last_mpris_path;
            let pause_changed = player.now_playing.is_some() && player.paused != last_mpris_paused;
            if path_changed || pause_changed {
                handle.update(&player);
                last_mpris_path = player.now_playing.clone();
                last_mpris_paused = player.paused;
            }
        }

        if app.should_quit {
            config::Config { volume: player.volume }.save();
            break;
        }
    }

    disable_raw_mode()?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen, DisableMouseCapture)?;
    terminal.show_cursor()?;
    Ok(())
}

fn handle_key(app: &mut App, player: &mut Player, key: KeyCode, _mods: KeyModifiers) {
    if app.show_history {
        match key {
            KeyCode::Char('H') | KeyCode::Esc => app.show_history = false,
            KeyCode::Char('j') | KeyCode::Down => {
                if app.history_selected + 1 < app.history.entries.len() {
                    app.history_selected += 1;
                }
            }
            KeyCode::Char('k') | KeyCode::Up => {
                app.history_selected = app.history_selected.saturating_sub(1);
            }
            KeyCode::Enter => {
                if let Some(entry) = app.history.entries.get(app.history_selected) {
                    let path = entry.path.clone();
                    player.play(path);
                    app.show_history = false;
                }
            }
            _ => {}
        }
        return;
    }

    match app.mode {
        Mode::Naming => match key {
            KeyCode::Esc => {
                app.mode = Mode::Normal;
                app.input_buffer.clear();
            }
            KeyCode::Enter => {
                let name = app.input_buffer.trim().to_string();
                if !name.is_empty() {
                    app.new_playlist(&name);
                    app.set_status(format!("Created playlist '{}'", name));
                }
                app.input_buffer.clear();
                app.mode = Mode::Normal;
            }
            KeyCode::Backspace => { app.input_buffer.pop(); }
            KeyCode::Char(c) => { app.input_buffer.push(c); }
            _ => {}
        },

        Mode::UrlInput => match key {
            KeyCode::Esc => {
                app.mode = Mode::Normal;
                app.input_buffer.clear();
            }
            KeyCode::Enter => {
                let url = app.input_buffer.trim().to_string();
                if !url.is_empty() {
                    app.start_download(&url);
                }
                app.input_buffer.clear();
                app.mode = Mode::Normal;
            }
            KeyCode::Backspace => { app.input_buffer.pop(); }
            KeyCode::Char(c) => { app.input_buffer.push(c); }
            _ => {}
        },

        Mode::TagEditing => {
            let Some(ed) = &mut app.tag_editor else { return };
            match key {
                KeyCode::Esc => {
                    app.tag_editor = None;
                    app.mode = Mode::Normal;
                }
                KeyCode::Enter => {
                    let ed = app.tag_editor.take().unwrap();
                    if save_tags(&ed.path, &ed.fields[0], &ed.fields[1], &ed.fields[2]) {
                        app.set_status("Tags saved");
                        player.reload_track_info();
                    } else {
                        app.set_status("Failed to save tags");
                    }
                    app.mode = Mode::Normal;
                }
                KeyCode::Tab => {
                    ed.active = (ed.active + 1) % app::TagEditor::LABELS.len();
                }
                KeyCode::BackTab => {
                    ed.active = ed.active.checked_sub(1).unwrap_or(app::TagEditor::LABELS.len() - 1);
                }
                KeyCode::Backspace => {
                    ed.fields[ed.active].pop();
                }
                KeyCode::Char(c) => {
                    ed.fields[ed.active].push(c);
                }
                _ => {}
            }
        }

        Mode::Searching => match key {
            KeyCode::Esc => {
                app.mode = Mode::Normal;
                app.input_buffer.clear();
            }
            KeyCode::Enter => {
                match app.focused_panel {
                    Panel::Browser => {
                        if let Some(path) = app.browser.selected_path().cloned() {
                            if path.is_dir() {
                                app.browser.enter_selected();
                            } else {
                                player.play(path);
                            }
                        }
                    }
                    Panel::Playlist => {
                        let pl_idx = app.active_playlist;
                        let tr_idx = app.current_playlist().selected;
                        if let Some(path) = app.play_playlist_track(pl_idx, tr_idx) {
                            player.play(path);
                        }
                    }
                }
                app.mode = Mode::Normal;
                app.input_buffer.clear();
            }
            KeyCode::Backspace => {
                app.input_buffer.pop();
                match app.focused_panel {
                    Panel::Browser => app.snap_browser_to_visible(),
                    Panel::Playlist => app.snap_playlist_to_visible(),
                }
            }
            KeyCode::Char(c) => {
                app.input_buffer.push(c);
                match app.focused_panel {
                    Panel::Browser => app.snap_browser_to_visible(),
                    Panel::Playlist => app.snap_playlist_to_visible(),
                }
            }
            KeyCode::Down => match app.focused_panel {
                Panel::Browser => app.browser_nav(1),
                Panel::Playlist => app.playlist_nav(1),
            },
            KeyCode::Up => match app.focused_panel {
                Panel::Browser => app.browser_nav(-1),
                Panel::Playlist => app.playlist_nav(-1),
            },
            _ => {}
        },

        Mode::Normal => match key {
            KeyCode::Char('q') => app.should_quit = true,
            KeyCode::Tab => {
                app.focused_panel = match app.focused_panel {
                    Panel::Browser => Panel::Playlist,
                    Panel::Playlist => Panel::Browser,
                };
                app.status_msg.clear();
            }
            KeyCode::Char('/') => {
                app.mode = Mode::Searching;
                app.input_buffer.clear();
            }
            KeyCode::Char('n') => {
                app.mode = Mode::Naming;
                app.input_buffer.clear();
            }
            KeyCode::Char('<') | KeyCode::Char(',') => {
                app.prev_playlist();
                app.status_msg.clear();
            }
            KeyCode::Char('>') | KeyCode::Char('.') => {
                app.next_playlist();
                app.status_msg.clear();
            }
            KeyCode::Char('+') | KeyCode::Char('=') => player.volume_up(),
            KeyCode::Char('-') => player.volume_down(),
            KeyCode::Char(' ') => player.toggle_pause(),
            KeyCode::Char('s') => player.stop(),
            KeyCode::Char('y') => {
                if !app.is_downloading() {
                    app.mode = Mode::UrlInput;
                    app.input_buffer.clear();
                }
            }
            KeyCode::Char('r') => {
                app.browser.refresh();
                app.set_status("Refreshed");
            }
            KeyCode::Char('H') => {
                app.show_history = true;
                app.history_selected = 0;
            }
            KeyCode::Char('z') => app.toggle_shuffle(),
            KeyCode::Char('p') => app.toggle_repeat(),
            KeyCode::Char(']') => {
                if let Some(path) = app.advance_track(1) { player.play(path); }
            }
            KeyCode::Char('[') => {
                if let Some(path) = app.advance_track(-1) { player.play(path); }
            }
            _ => match app.focused_panel {
                Panel::Browser => handle_browser_key(app, player, key),
                Panel::Playlist => handle_playlist_key(app, player, key),
            },
        },
    }
}

fn open_tag_editor_for(app: &mut App, player: &Player, path: PathBuf) {
    let info = player.now_playing.as_ref()
        .filter(|p| *p == &path)
        .and(player.track_info.as_ref());
    let tags = player::read_tags(&path);
    app.open_tag_editor(path, tags.as_ref().or(info));
}

fn handle_browser_key(app: &mut App, player: &mut Player, key: KeyCode) {
    match key {
        KeyCode::Down | KeyCode::Char('j') => app.browser_nav(1),
        KeyCode::Up | KeyCode::Char('k') => app.browser_nav(-1),
        KeyCode::Enter | KeyCode::Right | KeyCode::Char('l') => {
            if let Some(path) = app.browser.selected_path().cloned() {
                if path.is_dir() {
                    app.browser.enter_selected();
                } else if crate::app::is_m3u(&path) {
                    match app.import_m3u(&path) {
                        Some(n) => {
                            let name = app.current_playlist().name.clone();
                            app.set_status(format!("Imported '{}' ({} tracks)", name, n));
                        }
                        None => app.set_status("Failed to import M3U"),
                    }
                } else {
                    player.play(path);
                    app.status_msg.clear();
                }
            }
        }
        KeyCode::Left | KeyCode::Char('h') | KeyCode::Backspace => {
            app.browser.go_up();
        }
        KeyCode::Char('a') => {
            app.add_selected_to_playlist();
        }
        KeyCode::Char('e') => {
            if let Some(path) = app.browser.selected_path().cloned()
                && crate::app::is_audio(&path)
            {
                open_tag_editor_for(app, player, path);
            }
        }
        _ => {}
    }
}

fn handle_playlist_key(app: &mut App, player: &mut Player, key: KeyCode) {
    match key {
        KeyCode::Down | KeyCode::Char('j') => app.playlist_nav(1),
        KeyCode::Up | KeyCode::Char('k') => app.playlist_nav(-1),
        KeyCode::Char('J') => app.move_track_down(),
        KeyCode::Char('K') => app.move_track_up(),
        KeyCode::Enter => {
            let pl_idx = app.active_playlist;
            let tr_idx = app.current_playlist().selected;
            if let Some(path) = app.play_playlist_track(pl_idx, tr_idx) {
                player.play(path);
                app.status_msg.clear();
            }
        }
        KeyCode::Char('X') => {
            match app.export_m3u() {
                Ok(path) => app.set_status(format!("Exported to {}", path.display())),
                Err(_) => app.set_status("Export failed"),
            }
        }
        KeyCode::Char('d') => {
            app.remove_selected_from_playlist();
        }
        KeyCode::Char('e') => {
            let path = app.current_playlist().tracks.get(app.current_playlist().selected)
                .map(|t| t.path.clone());
            if let Some(path) = path {
                open_tag_editor_for(app, player, path);
            }
        }
        _ => {}
    }
}
