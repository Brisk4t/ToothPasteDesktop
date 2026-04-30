use crossterm::event::{KeyCode, KeyEvent};
use ratatui::{
    Frame,
    layout::{Constraint, Direction, Layout, Rect},
    prelude::*,
    style::{Color, Modifier, Style},
    text::Line,
    widgets::{List, ListItem, ListState, Paragraph},
};
use throbber_widgets_tui::{Throbber, ThrobberState};
use tokio::sync::{mpsc, watch};
use toothpaste_desktop_core::{AppCommand, AppState};

use super::{InputHandler, NavSignal, Screen, ScreenVariant, ScanningScreen};

pub struct DeviceListScreen {
    list_state: ListState,
    cmd_tx: mpsc::Sender<AppCommand>,
    app_state_rx: watch::Receiver<AppState>,
    connecting_to: Option<String>,
    throbber_state: ThrobberState,
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
            connecting_to: None,
            throbber_state: ThrobberState::default(),
            status: format!("{} device(s) found", initial_state.devices.len()),
        }
    }

    fn send(&self, cmd: AppCommand) {
        let _ = self.cmd_tx.try_send(cmd);
    }
}

impl InputHandler for DeviceListScreen {
    fn handle_nav_key(&mut self, key: KeyEvent) -> NavSignal {
        if self.connecting_to.is_some() {
            return NavSignal::Command;
        }
        match key.code {
            KeyCode::Up => {
                self.list_state.select_previous();
                NavSignal::Command
            }
            KeyCode::Down => {
                self.list_state.select_next();
                NavSignal::Command
            }
            KeyCode::Char('s') | KeyCode::Char('S') => {
                self.send(AppCommand::ScanForDevices);
                NavSignal::Screen(ScreenVariant::Scanning(ScanningScreen::new(self.cmd_tx.clone())))
            }
            KeyCode::Char('q') | KeyCode::Char('Q') | KeyCode::Esc => NavSignal::Back,
            _ => NavSignal::Command,
        }
    }

    fn handle_enter_key(&mut self, _key: KeyEvent) -> NavSignal {
        if self.connecting_to.is_some() {
            return NavSignal::Command;
        }
        let app_state = self.app_state_rx.borrow();
        if let Some(idx) = self.list_state.selected() {
            if let Some(device) = app_state.devices.get(idx).cloned() {
                drop(app_state);
                self.connecting_to = Some(device.name.clone());
                self.send(AppCommand::ConnectToDevice(device));
            }
        }
        NavSignal::Command
    }
}

impl Screen for DeviceListScreen {
    fn render(&mut self, frame: &mut Frame, area: Rect) {
        if let Some(name) = &self.connecting_to {
            let [row, _] =
                Layout::vertical([Constraint::Length(1), Constraint::Min(0)]).areas(area);
            self.throbber_state.calc_next();
            let throbber = Throbber::default().label(format!("  Connecting to {}…", name));
            frame.render_stateful_widget(throbber, row, &mut self.throbber_state);
            return;
        }

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
        if self.connecting_to.is_some() { "" } else { &self.status }
    }

    fn nav_hints(&self) -> Vec<(&'static str, &'static str)> {
        if self.connecting_to.is_some() {
            vec![]
        } else {
            vec![("<S>", " Rescan")]
        }
    }
}

fn rssi_color(rssi: i32) -> Color {
    match rssi {
        i32::MIN..=-80 => Color::Red,
        -79..=-65 => Color::Yellow,
        _ => Color::Green,
    }
}
