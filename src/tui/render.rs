// ratatui draw: title | header | list | footer.

use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, List, ListItem, Paragraph};
use ratatui::Frame;

use super::app::{App, SettingsAction, ViewMode, SETTINGS_ACTIONS};
use crate::format::{format_duration, format_size};

const KEY_HINT: &str =
    "↑/↓ navigate · Enter/l open · Backspace/h parent · r reload · c settings · q quit";
const SETTINGS_HINT: &str = "↑/↓ navigate · Enter/Space change · s save · Esc cancel · q quit";

pub(crate) fn render(f: &mut Frame, app: &mut App) {
    match app.view {
        ViewMode::List => render_list(f, app),
        ViewMode::Settings => render_settings(f, app),
    }
}

fn render_list(f: &mut Frame, app: &mut App) {
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

    let path = app.current_dir.display().to_string();
    let title = if app.scanning {
        Line::from(vec![
            Span::styled("rll", Style::default().add_modifier(Modifier::BOLD)),
            Span::raw(format!("  {path}  ")),
            Span::styled("scanning…", Style::default().fg(Color::Yellow)),
        ])
    } else {
        Line::from(vec![
            Span::styled("rll", Style::default().add_modifier(Modifier::BOLD)),
            Span::raw(format!("  {path}  ")),
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
    let footer = if let Some(status) = &app.status {
        Line::from(vec![
            Span::styled(status.clone(), Style::default().fg(Color::Green)),
            Span::raw("   "),
            Span::styled(KEY_HINT, Style::default().fg(Color::DarkGray)),
        ])
    } else if let (Some(summary), Some(elapsed)) = (app.summary, app.elapsed) {
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
            Span::styled(KEY_HINT, Style::default().fg(Color::DarkGray)),
        ])
    } else {
        Line::from(vec![
            Span::styled(
                format!("{} items… ", app.rows.len()),
                Style::default().fg(Color::Yellow),
            ),
            Span::styled(KEY_HINT, Style::default().fg(Color::DarkGray)),
        ])
    };
    f.render_widget(Paragraph::new(footer), chunks[3]);

    if app.confirm_leave_root.is_some() {
        render_leave_root_modal(f);
    }
}

fn render_settings(f: &mut Frame, app: &mut App) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1),
            Constraint::Min(1),
            Constraint::Length(1),
        ])
        .split(f.area());

    f.render_widget(
        Paragraph::new(Line::from(vec![
            Span::styled(
                "rll settings",
                Style::default().add_modifier(Modifier::BOLD),
            ),
            Span::raw(format!("  {}", app.current_dir.display())),
        ])),
        chunks[0],
    );

    let draft = app.draft_config();
    let items: Vec<ListItem> = SETTINGS_ACTIONS
        .iter()
        .map(|action| {
            let text = match action {
                SettingsAction::ShowHidden => format!(
                    "Show hidden files: {}",
                    if draft.show_hidden { "on" } else { "off" }
                ),
                SettingsAction::SortField => format!("Sort field: {}", draft.sort_field.as_str()),
                SettingsAction::SortDirection => {
                    format!("Sort direction: {}", draft.sort_direction.as_str())
                }
                SettingsAction::Save => "Save and close".to_owned(),
                SettingsAction::Cancel => "Cancel".to_owned(),
            };
            ListItem::new(Line::from(text))
        })
        .collect();

    let list = List::new(items)
        .block(Block::default().borders(Borders::ALL).title("Config"))
        .highlight_style(
            Style::default()
                .bg(Color::DarkGray)
                .add_modifier(Modifier::BOLD),
        )
        .highlight_symbol("» ");
    let mut state = ratatui::widgets::ListState::default();
    state.select(Some(app.settings_selected));
    f.render_stateful_widget(list, chunks[1], &mut state);

    let footer = if let Some(status) = &app.status {
        Line::from(vec![
            Span::styled(status.clone(), Style::default().fg(Color::Red)),
            Span::raw("   "),
            Span::styled(SETTINGS_HINT, Style::default().fg(Color::DarkGray)),
        ])
    } else {
        Line::from(Span::styled(
            SETTINGS_HINT,
            Style::default().fg(Color::DarkGray),
        ))
    };
    f.render_widget(Paragraph::new(footer), chunks[2]);
}

fn render_leave_root_modal(f: &mut Frame) {
    let area = centered_rect(f.area());
    f.render_widget(Clear, area);
    let modal = Paragraph::new("Leave initial directory?\ny/Enter confirm · n/Esc cancel")
        .style(Style::default().fg(Color::Yellow))
        .block(Block::default().borders(Borders::ALL).title("Confirm"));
    f.render_widget(modal, area);
}

fn centered_rect(area: Rect) -> Rect {
    let vertical = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Percentage(40),
            Constraint::Length(4),
            Constraint::Percentage(40),
        ])
        .split(area);
    Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage(15),
            Constraint::Percentage(70),
            Constraint::Percentage(15),
        ])
        .split(vertical[1])[1]
}
