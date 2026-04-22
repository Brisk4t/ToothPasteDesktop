use super::App;
use super::CurrentScreen;
use crossterm::{event::{self, EnableMouseCapture, Event, KeyCode, KeyEvent, KeyEventKind}, execute, terminal::{disable_raw_mode, enable_raw_mode}};
use ratatui::{
    prelude::*,
    DefaultTerminal, Frame, buffer::Buffer, layout::Rect, style::{Modifier, Stylize}, symbols::border, text::{Line, Text}, widgets::{Block, List, Paragraph, Widget, ListState}
};



pub fn ui(frame: &mut Frame, app: &mut App) {
    let layout = Layout::default()
        .direction(Direction::Vertical)
        .constraints(vec![Constraint::Percentage(100)])
        .split(frame.area());

    render_parent_container(frame, app, frame.area());
    

    let inner_layout = Layout::default()
        .direction(Direction::Horizontal)
        .margin(2)
        .constraints(vec![Constraint::Percentage(50), Constraint::Percentage(50)])
        .split(layout[0]);
    
    match app.CurrentScreen {
        CurrentScreen::Home => render_home_screen(frame, app, inner_layout[0]),
        CurrentScreen::Connect => render_connect_screen(frame, app, inner_layout[0]),
        _ => render_home_screen(frame, app, inner_layout[0]),
    }
}


pub fn render_list(frame: &mut Frame, items: Vec<&str>, list_state: &mut ListState, area: Rect) {
    let list = List::new(items)
        .highlight_style(Modifier::REVERSED)
        .highlight_symbol(">");

    frame.render_stateful_widget(list, area, list_state);
}


pub fn render_parent_container(frame: &mut Frame, app: &mut App, area: Rect) {
    let title = Line::from(" ToothPaste ".bold());
    let instructions = Line::from(vec![
        " Navigate ".into(),
        "<Up>".blue().bold(),
        "<Down>".blue().bold(),
        " Select ".into(),
        "<Enter>".blue().bold(),
        " Quit ".into(),
        "<Q> ".blue().bold(),
    ]);
    
    let block = Block::bordered()
        .title(title.centered())
        .title_bottom(instructions.centered())
        .border_set(border::THICK);

    frame.render_widget(block, area);

    // let counter_text = Text::from(vec![Line::from(vec![
    //     "Value: ".into(),
    //     self.counter.to_string().yellow(),
    // ])]);

    // Paragraph::new(counter_text)
    //     .block(block)
    //     .render(area, buf);
}

pub fn render_home_screen(frame: &mut Frame, app: &mut App, area: Rect) {
    let text = Text::from(vec![
        Line::from("Welcome to ToothPaste!").bold(),
        Line::from("This is the home screen.").italic(),
    ]);

    Paragraph::new(text)
        .render(area, frame.buffer_mut());

    render_list(frame, vec!["Connect to Device", "Item 2", "Item 3"], &mut app.list_state, area);
}

pub fn render_connect_screen(frame: &mut Frame, app: &mut App, area: Rect) {
    let items = vec!["Device 1", "Device 2", "Device 3"];
    render_list(frame, items, &mut app.list_state, area);

    //frame.render_stateful_widget(list, area, app.list_state);
}