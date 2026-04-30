use crossterm::event::{KeyCode, KeyEvent};
use ratatui::{
    Frame,
    layout::Rect,
    prelude::*,
    text::{Line, Text},
    widgets::{Paragraph, Wrap},
};
use tokio::sync::mpsc;
use toothpaste_desktop_core::AppCommand;

use super::{InputHandler, NavSignal, Screen};

pub struct ScanningScreen {
    #[allow(dead_code)]
    cmd_tx: mpsc::Sender<AppCommand>,
}

impl ScanningScreen {
    pub fn new(cmd_tx: mpsc::Sender<AppCommand>) -> Self {
        Self { cmd_tx }
    }
}

impl InputHandler for ScanningScreen {
    fn handle_nav_key(&mut self, key: KeyEvent) -> NavSignal {
        match key.code {
            KeyCode::Char('s') | KeyCode::Char('S') => {
                let _ = self.cmd_tx.try_send(AppCommand::ScanForDevices);
                NavSignal::Command
            }
            KeyCode::Char('q') | KeyCode::Char('Q') | KeyCode::Esc => NavSignal::Back,
            _ => NavSignal::Command,
        }
    }

    fn handle_enter_key(&mut self, _key: KeyEvent) -> NavSignal {
        NavSignal::Command
    }
}

impl Screen for ScanningScreen {
    fn render(&mut self, frame: &mut Frame, area: Rect) {
        let para = Paragraph::new(Text::from(vec![
            Line::from("Scanning for ToothPaste devices…").bold(),
            Line::from(""),
            Line::from("This may take up to 5 seconds.").dim(),
        ]))
        .wrap(Wrap { trim: false });
        frame.render_widget(para, area);
    }

    fn status(&self) -> &str {
        "Scanning for devices..."
    }

    fn nav_hints(&self) -> Vec<&'static str> {
        vec!["<S> Rescan"]
    }
}
