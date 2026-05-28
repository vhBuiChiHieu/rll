// ratatui draw: title | header | list | footer.

use ratatui::layout::{Constraint, Direction, Layout};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{List, ListItem, Paragraph};
use ratatui::Frame;

use super::app::App;
use crate::format::{format_duration, format_size};

pub(crate) fn render(f: &mut Frame, app: &mut App) {
    // Vertical stack: title | header | list | footer.
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1),
            Constraint::Length(1),
            Constraint::Min(1),
            Constraint::Length(1),
        ])
        .split(f.area());

    // Cache the list viewport height so PgUp/PgDn track the visible page.
    app.list_height = chunks[2].height as usize;

    // Title.
    let title = if app.scanning {
        Line::from(vec![
            Span::styled("rll", Style::default().add_modifier(Modifier::BOLD)),
            Span::raw("  ./  "),
            Span::styled("scanning…", Style::default().fg(Color::Yellow)),
        ])
    } else {
        Line::from(vec![
            Span::styled("rll", Style::default().add_modifier(Modifier::BOLD)),
            Span::raw("  ./  "),
            Span::styled(
                format!("{} entries", app.rows.len()),
                Style::default().fg(Color::DarkGray),
            ),
        ])
    };
    f.render_widget(Paragraph::new(title), chunks[0]);

    // Column header — same column widths as the CLI table.
    let header = Line::from(Span::styled(
        format!("{:<5} {:<10} {}", "TYPE", "SIZE", "NAME"),
        Style::default().add_modifier(Modifier::DIM),
    ));
    f.render_widget(Paragraph::new(header), chunks[1]);

    // Rows.
    let items: Vec<ListItem> = app
        .rows
        .iter()
        .map(|r| {
            let size = r.size.map(format_size).unwrap_or_else(|| "?".to_owned());
            let type_color = match r.type_name {
                "DIR" => Color::Cyan,
                "FILE" => Color::White,
                _ => Color::DarkGray,
            };
            ListItem::new(Line::from(vec![
                Span::styled(
                    format!("{:<5}", r.type_name),
                    Style::default().fg(type_color),
                ),
                Span::raw(" "),
                Span::styled(format!("{size:<10}"), Style::default().fg(Color::DarkGray)),
                Span::raw(" "),
                Span::raw(r.name.clone()),
            ]))
        })
        .collect();

    let list = List::new(items)
        .highlight_style(
            Style::default()
                .bg(Color::DarkGray)
                .add_modifier(Modifier::BOLD),
        )
        .highlight_symbol("");
    // ListState carries the scroll offset; cloning is cheap (single usize + Option<usize>).
    let mut state = app.state.clone();
    f.render_stateful_widget(list, chunks[2], &mut state);
    app.state = state;

    // Footer.
    let footer = if let (Some(summary), Some(elapsed)) = (app.summary, app.elapsed) {
        Line::from(vec![
            Span::styled(
                format!("TOTAL {} ", summary.total()),
                Style::default().add_modifier(Modifier::BOLD),
            ),
            Span::styled(
                format!(
                    "({} files, {} dirs, {} other) ",
                    summary.files, summary.dirs, summary.others
                ),
                Style::default().fg(Color::DarkGray),
            ),
            Span::styled(
                format!("in {}", format_duration(elapsed)),
                Style::default().fg(Color::Green),
            ),
            Span::raw("   "),
            Span::styled(
                "↑/↓ navigate · q quit",
                Style::default().fg(Color::DarkGray),
            ),
        ])
    } else {
        Line::from(vec![
            Span::styled(
                format!("{} items… ", app.rows.len()),
                Style::default().fg(Color::Yellow),
            ),
            Span::styled(
                "↑/↓ navigate · q quit",
                Style::default().fg(Color::DarkGray),
            ),
        ])
    };
    f.render_widget(Paragraph::new(footer), chunks[3]);
}
