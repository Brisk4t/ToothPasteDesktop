mod ui;
use ui::ui;

use toothpaste_desktop_service::ble_discover_toothpaste;
use std::io;

use crossterm::{
    event::{self, EnableMouseCapture, Event, KeyCode, KeyEvent, KeyEventKind},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode},
};

use ratatui::{
    DefaultTerminal, Frame,
    widgets::ListState,
};

pub enum CurrentScreen {
    Home,
    Settings,
    Connect,
}

pub struct ToothPasteTUI {
    pub current_screen: CurrentScreen,
    pub ble_devices: Vec<String>,
    pub exit: bool,
    pub list_state: ListState,
}

impl ToothPasteTUI {
    pub fn run(&mut self, terminal: &mut DefaultTerminal) -> std::io::Result<()> {
        while !self.exit {
            terminal.draw(|frame| self.draw(frame))?;
            self.handle_events()?;
        }
        Ok(())
    }

    fn draw(&mut self, frame: &mut Frame) {
        ui(frame, self);
    }

    fn handle_events(&mut self) -> io::Result<()> {
        match event::read()? {
            Event::Key(key_event) if key_event.kind == KeyEventKind::Press => {
                self.handle_keypress(key_event);
            }
            _ => {}
        };
        Ok(())
    }

    fn handle_keypress(&mut self, key_event: KeyEvent) {
        match key_event.code {
            KeyCode::Char('q') | KeyCode::Char('Q') => self.exit = true,
            KeyCode::Down => self.list_state.select_next(),
            KeyCode::Up => self.list_state.select_previous(),
            KeyCode::Enter => {
                self.current_screen = match self.list_state.selected().unwrap_or(0) {
                    0 => CurrentScreen::Connect,
                    _ => CurrentScreen::Home,
                };
            }
            KeyCode::Char('s') | KeyCode::Char('S') => self.scan_ble_devices(),
            _ => {}
        }
    }

    pub fn scan_ble_devices(&mut self) {
        let rt = tokio::runtime::Runtime::new().unwrap();
        rt.block_on(async {
            self.ble_devices = ble_discover_toothpaste().await.unwrap_or_else(|_| Vec::new());
        });
    }
}

pub fn start_tui() -> std::io::Result<()> {
    enable_raw_mode()?;

    execute!(io::stdout(), crossterm::terminal::EnterAlternateScreen, EnableMouseCapture)?;
    let backend = ratatui::backend::CrosstermBackend::new(io::stdout());
    let mut terminal = DefaultTerminal::new(backend)?;

    let mut app = ToothPasteTUI {
        current_screen: CurrentScreen::Home,
        exit: false,
        list_state: ListState::default(),
        ble_devices: Vec::new(),
    };
    let res = ratatui::run(|terminal| app.run(terminal));

    disable_raw_mode()?;
    execute!(
        terminal.backend_mut(),
        crossterm::terminal::LeaveAlternateScreen,
        crossterm::event::DisableMouseCapture
    )?;
    terminal.show_cursor()?;

    if res.is_err() {
        eprintln!("Error: {}", res.err().unwrap());
    }
    Ok(())
}
