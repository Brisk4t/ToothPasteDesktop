use super::App;
use crossterm::{event::{self, EnableMouseCapture, Event, KeyCode, KeyEvent, KeyEventKind}, execute, terminal::{disable_raw_mode, enable_raw_mode}};
use ratatui::{
    prelude::*,
    DefaultTerminal, Frame, buffer::Buffer, layout::Rect, style::{Modifier, Stylize}, symbols::border, text::{Line, Text}, widgets::{Block, List, Paragraph, Widget, ListState}
};



pub fn ui(frame: &mut Frame, app: &mut App) {
    let layout = Layout::default()
        .direction(Direction::Vertical)
        .constraints(vec![
            Constraint::Percentage(50),
            Constraint::Percentage(50),
        ])
    .split(frame.area());

    render_list(frame, vec!["Item 1", "Item 2", "Item 3"], &mut app.list_state, layout[1]);
    render_ui(frame, app,layout[0]);
}


pub fn render_list(frame: &mut Frame, items: Vec<&str>, list_state: &mut ListState, area: Rect) {
    let list = List::new(items)
        .highlight_style(Modifier::REVERSED)
        .highlight_symbol(">");

    frame.render_stateful_widget(list, area, list_state);
}


pub fn render_ui(frame: &mut Frame, app: &mut App, area: Rect) {
    let title = Line::from(" ToothPaste ".bold());
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

    frame.render_widget(block, area);

    // let counter_text = Text::from(vec![Line::from(vec![
    //     "Value: ".into(),
    //     self.counter.to_string().yellow(),
    // ])]);

    // Paragraph::new(counter_text)
    //     .block(block)
    //     .render(area, buf);
}