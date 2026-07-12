use crate::cli::api::api_request;
use crate::cli::args::{has_flag, option_value};
use crate::cli::config::runtime_config;
use crossterm::event::{self, Event, KeyCode};
use crossterm::execute;
use crossterm::terminal::{
    EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode,
};
use ratatui::Terminal;
use ratatui::backend::CrosstermBackend;
use serde_json::Value as JsonValue;
use std::io;
use std::time::{Duration, Instant};

mod format;
mod render;
mod state;

use format::*;
use render::render_frame;
use state::*;

pub fn tui_cmd(args: Vec<String>) -> Result<(), String> {
    let api = runtime_config(&args)?.api;
    let limit = option_value(&args, "--limit")
        .or_else(|| option_value(&args, "-n"))
        .and_then(|value| value.parse::<usize>().ok())
        .unwrap_or(20);
    let filter = option_value(&args, "--filter").unwrap_or_default();
    let detail_tab = option_value(&args, "--tab")
        .as_deref()
        .map(DetailTab::parse)
        .transpose()?
        .unwrap_or(DetailTab::Overview);
    let interval_ms = option_value(&args, "--interval-ms")
        .and_then(|value| value.parse::<u64>().ok())
        .unwrap_or(1000);
    if has_flag(&args, "--once") {
        let snapshot = fetch_snapshot(&api, limit, None, &filter)?;
        if has_flag(&args, "--json") {
            println!(
                "{}",
                serde_json::json!({
                    "status": snapshot.status,
                    "sessions": snapshot.sessions,
                    "selected": snapshot.selected_detail,
                    "error": snapshot.error,
                    "filter": filter,
                    "tab": detail_tab.name(),
                })
            );
        } else {
            print!("{}", plain_snapshot(&snapshot, detail_tab, &filter, None));
        }
        return Ok(());
    }
    run_interactive(
        &api,
        limit,
        filter,
        detail_tab,
        Duration::from_millis(interval_ms.max(100)),
    )
}

fn fetch_snapshot(
    api: &str,
    limit: usize,
    selected_id: Option<u64>,
    filter: &str,
) -> Result<TuiSnapshot, String> {
    let status_body = api_request("GET", api, "/api/status", "")?;
    let status: JsonValue = serde_json::from_str(&status_body)
        .map_err(|error| format!("parse status json: {error}"))?;
    let fetch_limit = limit.saturating_mul(5).max(limit).max(20);
    let sessions_body = api_request(
        "GET",
        api,
        &format!("/api/sessions?limit={fetch_limit}"),
        "",
    )?;
    let sessions_json: JsonValue = serde_json::from_str(&sessions_body)
        .map_err(|error| format!("parse sessions json: {error}"))?;
    let mut sessions = sessions_json.as_array().cloned().unwrap_or_default();
    sessions.retain(|session| session_matches_filter(session, filter));
    sessions.truncate(limit);
    let selected_id =
        selected_id.or_else(|| sessions.first().and_then(|item| json_u64(item, "id")));
    let selected_detail = selected_id
        .and_then(|id| api_request("GET", api, &format!("/api/sessions/{id}"), "").ok())
        .and_then(|body| serde_json::from_str(&body).ok());
    Ok(TuiSnapshot {
        status,
        sessions,
        selected_detail,
        error: None,
    })
}

fn run_interactive(
    api: &str,
    limit: usize,
    filter: String,
    detail_tab: DetailTab,
    interval: Duration,
) -> Result<(), String> {
    let snapshot = fetch_snapshot(api, limit, None, &filter)?;
    enable_raw_mode().map_err(|error| error.to_string())?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen).map_err(|error| {
        let _ = disable_raw_mode();
        error.to_string()
    })?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend).map_err(|error| {
        let _ = disable_raw_mode();
        error.to_string()
    })?;

    let mut app = TuiApp {
        api: api.to_string(),
        limit,
        selected: 0,
        filter,
        editing_filter: false,
        detail_tab,
        replay_status: None,
        snapshot,
        last_refresh: Instant::now(),
    };
    let result = run_tui_loop(&mut terminal, &mut app, interval);

    let _ = disable_raw_mode();
    let _ = execute!(terminal.backend_mut(), LeaveAlternateScreen);
    let _ = terminal.show_cursor();
    result
}

fn run_tui_loop(
    terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
    app: &mut TuiApp,
    interval: Duration,
) -> Result<(), String> {
    loop {
        terminal
            .draw(|frame| render_frame(frame, app))
            .map_err(|error| error.to_string())?;

        if event::poll(Duration::from_millis(100)).map_err(|error| error.to_string())? {
            match event::read().map_err(|error| error.to_string())? {
                Event::Key(key) => match key.code {
                    KeyCode::Esc if app.editing_filter => app.editing_filter = false,
                    KeyCode::Char('q') | KeyCode::Esc => return Ok(()),
                    KeyCode::Char('/') => {
                        app.filter.clear();
                        app.editing_filter = true;
                    }
                    KeyCode::Enter if app.editing_filter => {
                        app.editing_filter = false;
                        app.selected = 0;
                        app.refresh();
                    }
                    KeyCode::Backspace if app.editing_filter => {
                        app.filter.pop();
                    }
                    KeyCode::Char(character) if app.editing_filter => {
                        app.filter.push(character);
                    }
                    KeyCode::Char('R') => app.refresh(),
                    KeyCode::Char('r') => app.replay_selected(),
                    KeyCode::Tab => app.detail_tab = app.detail_tab.next(),
                    KeyCode::BackTab => app.detail_tab = app.detail_tab.previous(),
                    KeyCode::Up => app.selected = app.selected.saturating_sub(1),
                    KeyCode::Down if app.selected + 1 < app.snapshot.sessions.len() => {
                        app.selected += 1;
                    }
                    _ => {}
                },
                Event::Resize(_, _) => {}
                _ => {}
            }
        }
        if app.last_refresh.elapsed() >= interval {
            app.refresh();
        }
    }
}

#[cfg(test)]
#[path = "tui/tests/mod.rs"]
mod tests;
