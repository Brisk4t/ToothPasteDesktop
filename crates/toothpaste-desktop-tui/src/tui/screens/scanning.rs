use crossterm::event::{KeyCode, KeyEvent};
use ratatui::{Frame, layout::{Constraint, Layout, Rect}};
use throbber_widgets_tui::{Throbber, ThrobberState};
use tokio::sync::mpsc;
use toothpaste_desktop_core::AppCommand;

use super::{InputHandler, NavSignal, Screen};

pub struct ScanningScreen {
    #[allow(dead_code)]
    cmd_tx: mpsc::Sender<AppCommand>,
    throbber_state: ThrobberState,
}

impl ScanningScreen {
    pub fn new(cmd_tx: mpsc::Sender<AppCommand>) -> Self {
        Self { cmd_tx, throbber_state: ThrobberState::default() }
    }
}

impl InputHandler for ScanningScreen {
    fn handle_nav_key(&mut self, key: KeyEvent) -> NavSignal {
        match key.code {
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
        let [row, _] = Layout::vertical([Constraint::Length(1), Constraint::Min(0)]).areas(area);

        self.throbber_state.calc_next();
        let throbber = Throbber::default().label("  Scanning for ToothPaste devices…");
        frame.render_stateful_widget(throbber, row, &mut self.throbber_state);
    }


}
