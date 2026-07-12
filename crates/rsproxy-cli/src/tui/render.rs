use super::format::{footer, json_str, json_u64, plain_detail, truncate};
use super::state::{DetailTab, TuiApp, TuiSnapshot};
use ratatui::layout::{Constraint, Direction, Layout};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Cell, Paragraph, Row, Table, Wrap};
use serde_json::Value as JsonValue;

pub(super) fn render_frame(frame: &mut ratatui::Frame, app: &TuiApp) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(5),
            Constraint::Min(8),
            Constraint::Length(9),
            Constraint::Length(2),
        ])
        .split(frame.area());
    let main = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(62), Constraint::Percentage(38)])
        .split(chunks[1]);

    frame.render_widget(status_panel(&app.snapshot, &app.api), chunks[0]);
    frame.render_widget(
        sessions_table(&app.snapshot.sessions, app.selected),
        main[0],
    );
    frame.render_widget(
        detail_panel(app.snapshot.selected_detail.as_ref(), app.detail_tab),
        main[1],
    );
    frame.render_widget(footer(app), chunks[3]);
}

fn status_panel(snapshot: &TuiSnapshot, api: &str) -> Paragraph<'static> {
    let trace = snapshot.status.get("trace").unwrap_or(&JsonValue::Null);
    let lines = vec![
        Line::from(vec![
            Span::styled("rsproxy ", Style::default().add_modifier(Modifier::BOLD)),
            Span::raw(
                json_str(&snapshot.status, "status")
                    .unwrap_or("unknown")
                    .to_string(),
            ),
            Span::raw(format!("  api={api}")),
        ]),
        Line::from(format!(
            "proxy={}  storage={}",
            json_str(&snapshot.status, "proxy").unwrap_or("-"),
            json_str(&snapshot.status, "storage").unwrap_or("-")
        )),
        Line::from(format!(
            "sessions={} spilled={} dropped={} spill={} errors={}",
            json_u64(trace, "sessions").unwrap_or(0),
            json_u64(trace, "spilled").unwrap_or(0),
            json_u64(trace, "dropped").unwrap_or(0),
            json_str(trace, "spill_compression").unwrap_or("none"),
            json_u64(trace, "spill_errors").unwrap_or(0)
        )),
    ];
    Paragraph::new(lines).block(Block::default().borders(Borders::ALL).title("Status"))
}

fn sessions_table(sessions: &[JsonValue], selected: usize) -> Table<'static> {
    let rows = sessions.iter().enumerate().map(|(index, session)| {
        let marker = if index == selected { ">" } else { " " };
        let style = if index == selected {
            Style::default().fg(Color::Yellow)
        } else {
            Style::default()
        };
        Row::new(vec![
            Cell::from(marker.to_string()),
            Cell::from(json_u64(session, "id").unwrap_or(0).to_string()),
            Cell::from(json_str(session, "kind").unwrap_or("-").to_string()),
            Cell::from(json_u64(session, "status").map_or("-".to_string(), |v| v.to_string())),
            Cell::from(json_u64(session, "duration_ms").unwrap_or(0).to_string()),
            Cell::from(json_u64(session, "response_bytes").unwrap_or(0).to_string()),
            Cell::from(json_str(session, "method").unwrap_or("-").to_string()),
            Cell::from(truncate(json_str(session, "url").unwrap_or("-"), 80)),
        ])
        .style(style)
    });
    Table::new(
        rows,
        [
            Constraint::Length(1),
            Constraint::Length(5),
            Constraint::Length(9),
            Constraint::Length(6),
            Constraint::Length(7),
            Constraint::Length(8),
            Constraint::Length(7),
            Constraint::Min(20),
        ],
    )
    .header(
        Row::new(vec![
            ">", "ID", "KIND", "STATUS", "DUR", "BYTES", "METHOD", "URL",
        ])
        .style(Style::default().add_modifier(Modifier::BOLD)),
    )
    .block(
        Block::default()
            .borders(Borders::ALL)
            .title("Recent Sessions"),
    )
}

fn detail_panel(detail: Option<&JsonValue>, tab: DetailTab) -> Paragraph<'static> {
    let Some(detail) = detail else {
        return Paragraph::new("no session selected")
            .block(Block::default().borders(Borders::ALL).title("Detail"));
    };
    let title = format!("Detail: {}", tab.name());
    let body = plain_detail(detail, tab);
    Paragraph::new(body)
        .wrap(Wrap { trim: false })
        .block(Block::default().borders(Borders::ALL).title(title))
}
