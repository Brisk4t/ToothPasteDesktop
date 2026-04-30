use crossterm::event::{KeyCode, KeyEvent};
use ratatui::{
    Frame,
    layout::{Constraint, Direction, Layout, Rect},
    prelude::*,
    style::{Color, Modifier, Style},
    text::Line,
    widgets::{List, ListItem, ListState, Paragraph},
};
use tokio::sync::{mpsc, watch};
use toothpaste_desktop_core::{AppCommand, AppState};

use super::{InputHandler, NavSignal, Screen};

pub struct DeviceListScreen {
    list_state: ListState,
    cmd_tx: mpsc::Sender<AppCommand>,
    app_state_rx: watch::Receiver<AppState>,
    status: String,
}

impl DeviceListScreen {
    pub fn new(
        cmd_tx: mpsc::Sender<AppCommand>,
        app_state_rx: watch::Receiver<AppState>,
        initial_state: &AppState,
    ) -> Self {
        let mut list_state = ListState::default();
        list_state.select(Some(0));
        Self {
            list_state,
            cmd_tx,
            app_state_rx,
            status: format!("{} device(s) found", initial_state.devices.len()),
        }
    }

    fn send(&self, cmd: AppCommand) {
        let _ = self.cmd_tx.try_send(cmd);
    }
}

impl InputHandler for DeviceListScreen {
    fn handle_nav_key(&mut self, key: KeyEvent) -> NavSignal {
        match key.code {
            KeyCode::Up => {
                self.list_state.select_previous();
                NavSignal::Command
            }
            KeyCode::Down => {
                self.list_state.select_next();
                NavSignal::Command
            }
            KeyCode::Char('q') | KeyCode::Char('Q') | KeyCode::Esc => NavSignal::Back,
            _ => NavSignal::Command,
        }
    }

    fn handle_enter_key(&mut self, _key: KeyEvent) -> NavSignal {
        let app_state = self.app_state_rx.borrow();
        if let Some(idx) = self.list_state.selected() {
            if let Some(device) = app_state.devices.get(idx).cloned() {
                drop(app_state);
                self.status = format!("Connecting to {}…", device.name);
                self.send(AppCommand::ConnectToDevice(device));
            }
        }
        NavSignal::Command
    }
}

impl Screen for DeviceListScreen {
    fn render(&mut self, frame: &mut Frame, area: Rect) {
        let app_state = self.app_state_rx.borrow();

        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Length(2), Constraint::Min(0)])
            .split(area);

        let header = Paragraph::new(Line::from(vec![
            Span::styled(
                "Discovered devices",
                Style::default().add_modifier(Modifier::BOLD),
            ),
            Span::raw(format!("  ({})", app_state.devices.len())),
        ]));
        frame.render_widget(header, chunks[0]);

        let items: Vec<ListItem> = app_state
            .devices
            .iter()
            .map(|d| {
                ListItem::new(Line::from(vec![
                    Span::raw(format!("  {:<22}", d.name)),
                    Span::styled(
                        format!("{:>5} dBm", d.signal_strength),
                        Style::default().fg(rssi_color(d.signal_strength)),
                    ),
                ]))
            })
            .collect();
        drop(app_state);

        let list = List::new(items)
            .highlight_style(Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD))
            .highlight_symbol("> ");
        frame.render_stateful_widget(list, chunks[1], &mut self.list_state);
    }

    fn status(&self) -> &str {
        &self.status
    }
}

fn rssi_color(rssi: i32) -> Color {
    match rssi {
        i32::MIN..=-80 => Color::Red,
        -79..=-65 => Color::Yellow,
        _ => Color::Green,
    }
}
