use base64::{Engine, engine::general_purpose::STANDARD as BASE64};
use crossterm::event::{KeyCode, KeyEvent};
use ratatui::{
    Frame,
    layout::{Constraint, Direction, Layout, Rect},
    prelude::*,
    style::{Color, Modifier, Style},
    text::{Line, Text},
    widgets::{Block, Paragraph},
};
use tokio::sync::{mpsc, watch};
use toothpaste_desktop_core::{AppCommand, AppState};

use super::{InputHandler, NavSignal, Screen};

pub struct PairingInputScreen {
    pair_input: String,
    status: String,
    cmd_tx: mpsc::Sender<AppCommand>,
    app_state_rx: watch::Receiver<AppState>,
}

impl PairingInputScreen {
    pub fn new(cmd_tx: mpsc::Sender<AppCommand>, app_state_rx: watch::Receiver<AppState>) -> Self {
        Self {
            pair_input: String::new(),
            status: "Enter peer public key (base64):".into(),
            cmd_tx,
            app_state_rx,
        }
    }

    fn send(&self, cmd: AppCommand) {
        let _ = self.cmd_tx.try_send(cmd);
    }

    fn submit(&mut self) -> NavSignal {
        let trimmed = self.pair_input.trim().to_string();
        match BASE64.decode(&trimmed) {
            Ok(bytes) if bytes.len() == 33 => {
                let app_state = self.app_state_rx.borrow();
                if let Some(device) = app_state.connected_device.clone() {
                    drop(app_state);
                    self.send(AppCommand::PairDevice { device, pub_key: trimmed });
                    self.pair_input.clear();
                    NavSignal::Back
                } else {
                    NavSignal::Command
                }
            }
            Ok(_) => {
                self.status = "Invalid key: expected 33 bytes (compressed P-256).".into();
                NavSignal::Command
            }
            Err(_) => {
                self.status = "Invalid base64. Try again.".into();
                NavSignal::Command
            }
        }
    }
}

impl InputHandler for PairingInputScreen {
    fn handle_back_key(&mut self, _key: KeyEvent) -> NavSignal {
        if self.pair_input.pop().is_some() {
            NavSignal::Command
        } else {
            NavSignal::Back
        }
    }

    fn handle_nav_key(&mut self, key: KeyEvent) -> NavSignal {
        match key.code {
            KeyCode::Char(c) => {
                self.pair_input.push(c);
                NavSignal::Command
            }
            KeyCode::Esc => NavSignal::Back,
            _ => NavSignal::Command,
        }
    }

    fn handle_enter_key(&mut self, _key: KeyEvent) -> NavSignal {
        self.submit()
    }
}

impl Screen for PairingInputScreen {
    fn render(&mut self, frame: &mut Frame, area: Rect) {
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(4),
                Constraint::Length(3),
                Constraint::Min(0),
            ])
            .split(area);

        let prompt = Paragraph::new(Text::from(vec![
            Line::from("Pairing required").bold(),
            Line::from(""),
            Line::from("Enter the device's compressed public key (base64):").dim(),
        ]));
        frame.render_widget(prompt, chunks[0]);

        let input_block = Block::bordered()
            .title(" Public Key ")
            .border_style(Style::default().fg(Color::Cyan));
        let input_inner = input_block.inner(chunks[1]);
        frame.render_widget(input_block, chunks[1]);

        let input_text =
            Paragraph::new(self.pair_input.as_str()).style(Style::default().fg(Color::White));
        frame.render_widget(input_text, input_inner);

        let help = Paragraph::new(Line::from(vec![
            Span::styled("<Enter>", Style::default().fg(Color::Blue).add_modifier(Modifier::BOLD)),
            Span::raw(" confirm  "),
            Span::styled("<Esc>", Style::default().fg(Color::Blue).add_modifier(Modifier::BOLD)),
            Span::raw(" cancel"),
        ]));
        frame.render_widget(help, chunks[2]);
    }

    fn status(&self) -> &str {
        &self.status
    }
}
