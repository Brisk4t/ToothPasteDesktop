use crossterm::event::{KeyCode, KeyEvent};
use ratatui::{
    Frame,
    layout::{Constraint, Direction, Layout, Rect},
    prelude::*,
    style::{Color, Modifier, Style},
    text::{Line, Text},
    widgets::{List, ListItem, ListState, Paragraph, Wrap},
};
use tokio::sync::{mpsc, watch};
use toothpaste_desktop_core::{AppCommand, AppState, AuthState, DeviceState};

use super::{InputHandler, NavSignal, PairingInputScreen, Screen, ScreenVariant};

pub struct ConnectedScreen {
    list_state: ListState,
    cmd_tx: mpsc::Sender<AppCommand>,
    app_state_rx: watch::Receiver<AppState>,
    status: String,
}

impl ConnectedScreen {
    pub fn new(cmd_tx: mpsc::Sender<AppCommand>, app_state_rx: watch::Receiver<AppState>) -> Self {
        let mut list_state = ListState::default();
        list_state.select(Some(0));
        Self { list_state, cmd_tx, app_state_rx, status: String::new() }
    }

    fn send(&self, cmd: AppCommand) {
        let _ = self.cmd_tx.try_send(cmd);
    }

    fn is_authenticated(app_state: &AppState) -> bool {
        matches!(
            app_state.connected_device.as_ref().and_then(|d| match &d.state {
                DeviceState::Connected { auth_state, .. } => Some(auth_state),
                _ => None,
            }),
            Some(AuthState::Authenticated { .. })
        )
    }
}

impl InputHandler for ConnectedScreen {
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
            KeyCode::Char('d') | KeyCode::Char('D') => {
                self.send(AppCommand::DisconnectDevice);
                NavSignal::Back
            }
            KeyCode::Char('q') | KeyCode::Char('Q') | KeyCode::Esc => NavSignal::Back,
            _ => NavSignal::Command,
        }
    }

    fn handle_enter_key(&mut self, _key: KeyEvent) -> NavSignal {
        let app_state = self.app_state_rx.borrow();
        let authenticated = Self::is_authenticated(&app_state);
        drop(app_state);

        match self.list_state.selected() {
            Some(0) if !authenticated => NavSignal::Screen(ScreenVariant::PairingInput(
                PairingInputScreen::new(self.cmd_tx.clone(), self.app_state_rx.clone()),
            )),
            Some(idx) if (authenticated && idx == 0) || (!authenticated && idx == 1) => {
                self.send(AppCommand::DisconnectDevice);
                NavSignal::Back
            }
            _ => NavSignal::Command,
        }
    }
}

impl Screen for ConnectedScreen {
    fn render(&mut self, frame: &mut Frame, area: Rect) {
        let app_state = self.app_state_rx.borrow();

        let dev = match &app_state.connected_device {
            Some(d) => d,
            None => return,
        };

        let (auth_label, fw_label) = match &dev.state {
            DeviceState::Connected { auth_state, firmware_version } => {
                let auth = match auth_state {
                    AuthState::Authenticated { .. } => "Authenticated",
                    AuthState::NotAuthenticated => "Authenticating…",
                    AuthState::AuthenticationFailed => "Pairing required",
                };
                (auth, firmware_version.clone())
            }
            DeviceState::Disconnected => ("Disconnected", "—".into()),
        };

        let auth_color = match auth_label {
            "Authenticated" => Color::Green,
            "Pairing required" => Color::Red,
            _ => Color::Yellow,
        };

        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Length(6), Constraint::Min(0)])
            .split(area);

        let info = Text::from(vec![
            Line::from(vec![
                Span::styled("Connected  ", Style::default().add_modifier(Modifier::BOLD)),
                Span::styled(
                    &dev.name,
                    Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD),
                ),
            ]),
            Line::from(""),
            Line::from(vec![
                Span::raw("  Address:   "),
                Span::styled(&dev.address, Style::default().fg(Color::White)),
            ]),
            Line::from(vec![
                Span::raw("  Firmware:  "),
                Span::styled(fw_label, Style::default().fg(Color::White)),
            ]),
            Line::from(vec![
                Span::raw("  Auth:      "),
                Span::styled(auth_label, Style::default().fg(auth_color)),
            ]),
        ]);
        frame.render_widget(Paragraph::new(info).wrap(Wrap { trim: false }), chunks[0]);

        let authenticated = auth_label == "Authenticated";
        let mut items = vec![];
        if !authenticated {
            items.push(ListItem::new("  Pair Device"));
        }
        items.push(ListItem::new("  Disconnect"));
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
