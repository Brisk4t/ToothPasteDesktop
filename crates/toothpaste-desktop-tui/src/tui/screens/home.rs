use std::path::PathBuf;

use crossterm::event::{KeyCode, KeyEvent};
use ratatui::{
    Frame,
    layout::{Constraint, Direction, Layout, Rect},
    prelude::*,
    style::{Color, Modifier, Style},
    text::{Line, Text},
    widgets::{List, ListItem, ListState, Paragraph},
};
use tokio::sync::{mpsc, watch};
use toothpaste_desktop_core::AppCommand;

use super::{InputHandler, NavSignal, Screen, ScreenVariant, ScanningScreen};

pub struct HomeScreen {
    list_state: ListState,
    cmd_tx: mpsc::Sender<AppCommand>,
    service_available_rx: watch::Receiver<bool>,
    status: String,
}

impl HomeScreen {
    pub fn new(
        cmd_tx: mpsc::Sender<AppCommand>,
        service_available_rx: watch::Receiver<bool>,
    ) -> Self {
        Self {
            list_state: ListState::default(),
            cmd_tx,
            service_available_rx,
            status: String::new(),
        }
    }

    fn send(&self, cmd: AppCommand) {
        let _ = self.cmd_tx.try_send(cmd);
    }

    fn start_scan(&mut self) -> NavSignal {
        self.send(AppCommand::ScanForDevices);
        NavSignal::Screen(ScreenVariant::Scanning(ScanningScreen::new(self.cmd_tx.clone())))
    }

    fn service_available(&self) -> bool {
        *self.service_available_rx.borrow()
    }
}

impl InputHandler for HomeScreen {
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
            KeyCode::Char('x') | KeyCode::Char('X') => {
                if self.service_available() {
                    self.send(AppCommand::KillService);
                    self.status = "Shutting down service...".into();
                }
                NavSignal::Command
            }
            KeyCode::Char('q') | KeyCode::Char('Q') | KeyCode::Esc => NavSignal::Back,
            _ => NavSignal::Command,
        }
    }

    fn handle_enter_key(&mut self, _key: KeyEvent) -> NavSignal {
        if self.service_available() {
            match self.list_state.selected() {
                Some(0) => self.start_scan(),
                Some(1) => {
                    self.send(AppCommand::KillService);
                    self.status = "Shutting down service...".into();
                    NavSignal::Command
                }
                Some(2) => NavSignal::Back,
                _ => NavSignal::Command,
            }
        } else {
            match self.list_state.selected() {
                Some(0) => {
                    spawn_service();
                    self.status = "Starting service...".into();
                    NavSignal::Command
                }
                Some(1) => NavSignal::Back,
                _ => NavSignal::Command,
            }
        }
    }
}

impl Screen for HomeScreen {
    fn render(&mut self, frame: &mut Frame, area: Rect) {
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Length(3), Constraint::Min(0)])
            .split(area);

        let header = Paragraph::new(Text::from(vec![
            Line::from("Welcome to ToothPaste!").bold(),
            Line::from("Wireless clipboard for your devices.").dim(),
        ]));
        frame.render_widget(header, chunks[0]);

        let items: Vec<ListItem> = if self.service_available() {
            vec![
                ListItem::new("  Scan for Devices"),
                ListItem::new("  Shut down Service"),
                ListItem::new("  Quit"),
            ]
        } else {
            vec![
                ListItem::new("  Start Service"),
                ListItem::new("  Quit"),
            ]
        };

        let list = List::new(items)
            .highlight_style(Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD))
            .highlight_symbol("> ");
        frame.render_stateful_widget(list, chunks[1], &mut self.list_state);
    }

    fn status(&self) -> &str {
        &self.status
    }
}

fn spawn_service() {
    let mut path = std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(|p| p.to_owned()))
        .unwrap_or_else(|| PathBuf::from("."));
    path.push("toothpaste-desktop-service");
    #[cfg(windows)]
    path.set_extension("exe");
    std::process::Command::new(&path)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .ok();
}
