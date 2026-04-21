
use std::io;

use clap::Parser;
use crossterm::event::{self, Event, KeyCode, KeyEvent, KeyEventKind};
use ratatui::{
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

#[derive(Default)]
struct App{
    currentSelection: u8,
    exit: bool,
    list_state: ListState,
    //deviceState: DeviceState
}

impl App {
    // Main application loop
    pub fn run(&mut self, terminal: &mut DefaultTerminal) -> std::io::Result<()> {
        while !self.exit {
            terminal.draw(|frame| self.draw(frame))?;
            
            self.handle_events()?;
        }
        
       
        Ok(())
    }

    fn draw(&mut self, frame: &mut Frame) { 
        render_list(frame, vec!["Item 1", "Item 2", "Item 3"], &mut self.list_state);
        //frame.render_widget(self, frame.area());
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

impl Widget for &App {
    fn render(self, area: Rect, buf: &mut Buffer) {
        let title = Line::from(" Counter App Tutorial ".bold());
        let instructions = Line::from(vec![
            " Decrement ".into(),
            "<Left>".blue().bold(),
            " Increment ".into(),
            "<Right>".blue().bold(),
            " Quit ".into(),
            "<Q> ".blue().bold(),
        ]);
        let block = Block::bordered()
            .title(title.centered())
            .title_bottom(instructions.centered())
            .border_set(border::THICK);

        // let counter_text = Text::from(vec![Line::from(vec![
        //     "Value: ".into(),
        //     self.counter.to_string().yellow(),
        // ])]);

        // Paragraph::new(counter_text)
        //     .block(block)
        //     .render(area, buf);

    }
}


pub fn render_list(frame: &mut Frame, items: Vec<&str>, list_state: &mut ListState) {
    let list = List::new(items)
        .highlight_style(Modifier::REVERSED)
        .highlight_symbol(">");

    frame.render_stateful_widget(list, frame.area(), list_state);
    // Render the list in your terminal
}

pub fn start_tui() -> std::io::Result<()> {
    ratatui::run(|terminal| App::default().run(terminal))
}
