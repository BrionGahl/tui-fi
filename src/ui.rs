use ratatui::{
    Frame,
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, List, ListItem, ListState, Paragraph},
};

use crate::app::{self, App, Mode, Panel, RepeatMode, TagEditor};
use crate::history::HistoryEntry;
use crate::player::{Player, TrackInfo, fmt_duration};

pub fn draw(frame: &mut Frame, app: &App, player: &Player) {
    let area = frame.area();

    let rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Min(0),
            Constraint::Length(1),
            Constraint::Length(1),
        ])
        .split(area);

    let cols = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
        .split(rows[0]);

    draw_browser(frame, app, cols[0]);
    draw_playlist(frame, app, cols[1]);
    draw_player_bar(frame, app, player, rows[1]);
    draw_controls_bar(frame, app, rows[2]);

    match app.mode {
        Mode::Naming => draw_naming_popup(frame, app, area),
        Mode::UrlInput => draw_url_popup(frame, app, area),
        Mode::TagEditing => {
            if let Some(ed) = &app.tag_editor {
                draw_tag_editor(frame, ed, area);
            }
        }
        Mode::Normal | Mode::Searching => {}
    }

    if app.show_history {
        draw_history(frame, app, area);
    }
}

fn draw_browser(frame: &mut Frame, app: &App, area: Rect) {
    let focused = app.focused_panel == Panel::Browser;
    let searching = app.mode == Mode::Searching && focused;

    let border_style = if focused {
        Style::default().fg(Color::Cyan)
    } else {
        Style::default().fg(Color::DarkGray)
    };

    let title = if searching {
        format!(" Browser  /{}_ ", app.input_buffer)
    } else {
        format!(" Browser — {} ", app.browser.current_dir.display())
    };

    let block = Block::default()
        .title(title)
        .borders(Borders::ALL)
        .border_style(border_style);

    let visible = app.browser_visible_indices();

    let items: Vec<ListItem> = visible
        .iter()
        .map(|&i| {
            let p = &app.browser.entries[i];
            let name = p.file_name().unwrap_or_default().to_string_lossy();
            let (prefix, style) = if p.is_dir() {
                (
                    "  ",
                    Style::default()
                        .fg(Color::Blue)
                        .add_modifier(Modifier::BOLD),
                )
            } else if crate::app::is_m3u(p) {
                ("  ", Style::default().fg(Color::Magenta))
            } else {
                ("  ", Style::default().fg(Color::Green))
            };
            ListItem::new(Line::from(vec![
                Span::raw(prefix),
                Span::styled(name.to_string(), style),
            ]))
        })
        .collect();

    let filtered_pos = visible.iter().position(|&i| i == app.browser.selected);
    let mut state = ListState::default();
    state.select(filtered_pos);

    frame.render_stateful_widget(
        List::new(items)
            .block(block)
            .highlight_style(
                Style::default()
                    .bg(Color::DarkGray)
                    .add_modifier(Modifier::BOLD),
            )
            .highlight_symbol("> "),
        area,
        &mut state,
    );
}

fn draw_playlist(frame: &mut Frame, app: &App, area: Rect) {
    let focused = app.focused_panel == Panel::Playlist;
    let searching = app.mode == Mode::Searching && focused;
    let border_style = if focused {
        Style::default().fg(Color::Cyan)
    } else {
        Style::default().fg(Color::DarkGray)
    };

    let pl = app.current_playlist();
    let title = if searching {
        format!(" Playlist: {}  /{}_ ", pl.name, app.input_buffer)
    } else {
        format!(
            " Playlist: {} ({}/{}) ",
            pl.name,
            app.active_playlist + 1,
            app.playlists.len()
        )
    };

    let block = Block::default()
        .title(title)
        .borders(Borders::ALL)
        .border_style(border_style);

    let visible = app.playlist_visible_indices();

    let items: Vec<ListItem> = visible
        .iter()
        .map(|&i| {
            let t = &pl.tracks[i];
            ListItem::new(Line::from(Span::styled(
                &t.name,
                Style::default().fg(Color::Yellow),
            )))
        })
        .collect();

    let filtered_pos = visible.iter().position(|&i| i == pl.selected);
    let mut state = ListState::default();
    state.select(filtered_pos);

    frame.render_stateful_widget(
        List::new(items)
            .block(block)
            .highlight_style(
                Style::default()
                    .bg(Color::DarkGray)
                    .add_modifier(Modifier::BOLD),
            )
            .highlight_symbol("> "),
        area,
        &mut state,
    );
}

fn draw_player_bar(frame: &mut Frame, app: &App, player: &Player, area: Rect) {
    let now_playing = match &player.track_info {
        Some(TrackInfo { title: Some(t), artist: Some(a), .. }) => format!("{} — {}", a, t),
        Some(TrackInfo { title: Some(t), .. }) => t.clone(),
        _ => player
            .now_playing
            .as_ref()
            .map(|p| p.file_name().unwrap_or_default().to_string_lossy().to_string())
            .unwrap_or_else(|| "—".to_string()),
    };

    let pause_str = if player.paused { "⏸" } else { "▶" };
    let vol = (player.volume * 100.0) as u32;

    let time_str = match (player.position(), player.total_duration) {
        (Some(pos), Some(total)) => format!("  {} / {}", fmt_duration(pos), fmt_duration(total)),
        (Some(pos), None) => format!("  {}", fmt_duration(pos)),
        _ => String::new(),
    };

    let shuffle_span = if app.shuffle {
        Span::styled(" ⇀ ", Style::default().fg(Color::Cyan))
    } else {
        Span::raw("   ")
    };

    let repeat_span = match app.repeat {
        RepeatMode::Off => Span::raw(""),
        RepeatMode::All => Span::styled(" ⟳ ", Style::default().fg(Color::Cyan)),
        RepeatMode::One => Span::styled(" ⟳¹", Style::default().fg(Color::Yellow)),
    };

    let viz_str = {
        let blocks = ['▁', '▂', '▃', '▄', '▅', '▆', '▇', '█'];
        let s: String = if player.now_playing.is_some() && !player.paused {
            let t = app.viz_tick as f64;
            (0..5)
                .map(|i| {
                    let v = (t * 0.0018 + i as f64 * 1.1).sin();
                    blocks[((v + 1.0) / 2.0 * 7.0) as usize]
                })
                .collect()
        } else {
            std::iter::repeat(blocks[0]).take(5).collect()
        };
        format!(" {}", s)
    };

    let line = Line::from(vec![
        Span::styled(format!(" {} ", pause_str), Style::default().fg(Color::Cyan)),
        Span::styled(
            now_playing,
            Style::default()
                .fg(Color::White)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(viz_str, Style::default().fg(Color::Cyan)),
        Span::styled(time_str, Style::default().fg(Color::DarkGray)),
        Span::styled(
            format!("  vol: {}%", vol),
            Style::default().fg(Color::DarkGray),
        ),
        shuffle_span,
        repeat_span,
    ]);

    frame.render_widget(
        Paragraph::new(line).style(Style::default().bg(Color::Black)),
        area,
    );
}

fn draw_controls_bar(frame: &mut Frame, app: &App, area: Rect) {
    let text = if !app.status_msg.is_empty() {
        Span::styled(
            format!(" {}", app.status_msg),
            Style::default().fg(Color::Yellow),
        )
    } else {
        Span::styled(
            " [Tab] panel  [a] add  [d] del  [J/K] reorder  [e] edit tags  [X] export m3u  [Enter] play/open/import  [Space] pause  [s] stop  [+/-] vol  [[/]] prev/next song  [z] shuffle  [p] repeat  [/] search  [H] history  [y] download  [r] refresh  [n] new playlist  [</>] prev/next playlist  [q] quit",
            Style::default().fg(Color::DarkGray),
        )
    };

    frame.render_widget(
        Paragraph::new(Line::from(text)).style(Style::default().bg(Color::Black)),
        area,
    );
}

fn draw_naming_popup(frame: &mut Frame, app: &App, area: Rect) {
    let popup = centered_rect(40, 3, area);
    frame.render_widget(Clear, popup);
    frame.render_widget(
        Paragraph::new(format!(" New playlist: {}_", app.input_buffer))
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .border_style(Style::default().fg(Color::Cyan)),
            )
            .style(Style::default().fg(Color::White)),
        popup,
    );
}

fn draw_url_popup(frame: &mut Frame, app: &App, area: Rect) {
    let dir = app::App::download_dir();
    let popup = centered_rect(70, 5, area);
    frame.render_widget(Clear, popup);
    let text = format!(
        " YouTube URL:\n {}_\n\n  → saves to {}",
        app.input_buffer,
        dir.display()
    );
    frame.render_widget(
        Paragraph::new(text)
            .block(
                Block::default()
                    .title(" Download Audio ")
                    .borders(Borders::ALL)
                    .border_style(Style::default().fg(Color::Yellow)),
            )
            .style(Style::default().fg(Color::White)),
        popup,
    );
}

fn draw_history(frame: &mut Frame, app: &App, area: Rect) {
    let popup = centered_rect(60, 27, area);
    frame.render_widget(Clear, popup);

    let items: Vec<ListItem> = if app.history.entries.is_empty() {
        vec![ListItem::new(Span::styled(" No history yet", Style::default().fg(Color::DarkGray)))]
    } else {
        app.history.entries.iter().map(|e: &HistoryEntry| {
            ListItem::new(Line::from(Span::styled(&e.name, Style::default().fg(Color::Yellow))))
        }).collect()
    };

    let mut state = ListState::default();
    if !app.history.entries.is_empty() {
        state.select(Some(app.history_selected));
    }

    frame.render_stateful_widget(
        List::new(items)
            .block(Block::default()
                .title(" Recently Played  [H/Esc] close  [Enter] play ")
                .borders(Borders::ALL)
                .border_style(Style::default().fg(Color::Cyan)))
            .highlight_style(Style::default().bg(Color::DarkGray).add_modifier(Modifier::BOLD))
            .highlight_symbol("> "),
        popup,
        &mut state,
    );
}

fn draw_tag_editor(frame: &mut Frame, ed: &TagEditor, area: Rect) {
    let popup = centered_rect(60, 7, area);
    frame.render_widget(Clear, popup);
    let filename = ed.path.file_name().unwrap_or_default().to_string_lossy();

    let lines: Vec<Line> = TagEditor::LABELS.iter().enumerate().map(|(i, label)| {
        let active = i == ed.active;
        let cursor = if active { "_" } else { "" };
        let label_span = Span::styled(
            format!(" {:>6}: ", label),
            Style::default().fg(if active { Color::Cyan } else { Color::DarkGray }),
        );
        let value_span = Span::styled(
            format!("{}{}", ed.fields[i], cursor),
            Style::default().fg(Color::White).add_modifier(if active { Modifier::BOLD } else { Modifier::empty() }),
        );
        Line::from(vec![label_span, value_span])
    }).collect();

    let mut all_lines = vec![Line::from(Span::styled(
        format!(" {}", filename),
        Style::default().fg(Color::DarkGray),
    )), Line::raw("")];
    all_lines.extend(lines);
    all_lines.push(Line::raw(""));
    all_lines.push(Line::from(Span::styled(
        " [Tab] next field   [Enter] save   [Esc] cancel",
        Style::default().fg(Color::DarkGray),
    )));

    frame.render_widget(
        Paragraph::new(all_lines)
            .block(Block::default().title(" Edit Tags ").borders(Borders::ALL)
                .border_style(Style::default().fg(Color::Cyan))),
        popup,
    );
}

fn centered_rect(width: u16, height: u16, area: Rect) -> Rect {
    let x = area.x + area.width.saturating_sub(width) / 2;
    let y = area.y + area.height.saturating_sub(height) / 2;
    Rect::new(x, y, width.min(area.width), height.min(area.height))
}
