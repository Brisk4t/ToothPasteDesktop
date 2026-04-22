
mod ui;
use ui::ui;

use std::io;

use clap::Parser;
use crossterm::{event::{self, EnableMouseCapture, Event, KeyCode, KeyEvent, KeyEventKind}, execute, terminal::{disable_raw_mode, enable_raw_mode}};
use ratatui::{
    prelude::*,
    DefaultTerminal, Frame, buffer::Buffer, layout::Rect, style::{Modifier, Stylize}, symbols::border, text::{Line, Text}, widgets::{Block, List, Paragraph, Widget, ListState}
};

#[derive(Parser)]
struct Cli {
    pattern: String,
    input: std::path::PathBuf,
}

pub enum DeviceState {
    Connected,
    Disconnected,
    Ready
}
pub enum CurrentScreen {
    Home,
    Settings,
    Connect
}

// The main application widget
#[derive(Default)]
pub struct App{
    currentSelection: u8,
    exit: bool,
    list_state: ListState,
    //deviceState: DeviceState
}

impl App {
    // Main application loop - continuously call draw() and handle_events() until exit is true
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
            _ => {}
            // Move up and down with arrow keys
        }
    }
}

// impl Widget for &mut App {
//     fn render(self, area: Rect, buf: &mut Buffer) {
       

//     }
// }


pub fn start_tui() -> std::io::Result<()> {
    enable_raw_mode()?;

    execute!(io::stdout(), crossterm::terminal::EnterAlternateScreen, EnableMouseCapture)?;
    let backend = ratatui::backend::CrosstermBackend::new(io::stdout());
    let mut terminal = DefaultTerminal::new(backend)?;

    // Run the tui
    let res = ratatui::run(|terminal| App::default().run(terminal));

    disable_raw_mode()?;
    execute!(terminal.backend_mut(), crossterm::terminal::LeaveAlternateScreen, crossterm::event::DisableMouseCapture)?;
    terminal.show_cursor()?;

    if res .is_err() {
        eprintln!("Error: {}", res.err().unwrap());
    }
    Ok(())
}
