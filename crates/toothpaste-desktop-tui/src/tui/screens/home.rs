use crossterm::event::{KeyCode, KeyEvent};
use ratatui::{
    Frame,
    layout::{Constraint, Direction, Layout, Rect},
    prelude::*,
    style::{Color, Modifier, Style},
    text::{Line, Text},
    widgets::{List, ListItem, ListState, Paragraph},
};
use tokio::sync::mpsc;
use toothpaste_desktop_core::AppCommand;

use super::{InputHandler, NavSignal, Screen, ScreenVariant, ScanningScreen};

pub struct HomeScreen {
    list_state: ListState,
    cmd_tx: mpsc::Sender<AppCommand>,
    status: String,
}

impl HomeScreen {
    pub fn new(cmd_tx: mpsc::Sender<AppCommand>) -> Self {
        Self {
            list_state: ListState::default(),
            cmd_tx,
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
                self.send(AppCommand::KillService);
                self.status = "Shutting down service...".into();
                NavSignal::Command
            }
            KeyCode::Char('q') | KeyCode::Char('Q') | KeyCode::Esc => NavSignal::Back,
            _ => NavSignal::Command,
        }
    }

    fn handle_enter_key(&mut self, _key: KeyEvent) -> NavSignal {
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

        let items = vec![
            ListItem::new("  Scan for Devices"),
            ListItem::new("  Shut down Service"),
            ListItem::new("  Quit"),
        ];
        let list = List::new(items)
            .highlight_style(Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD))
            .highlight_symbol("> ");
        frame.render_stateful_widget(list, chunks[1], &mut self.list_state);
    }

    fn status(&self) -> &str {
        &self.status
    }
}
