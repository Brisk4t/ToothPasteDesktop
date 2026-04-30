mod home;
mod scanning;
mod device_list;
mod connected;
mod pairing_input;

pub use home::HomeScreen;
pub use scanning::ScanningScreen;
pub use device_list::DeviceListScreen;
pub use connected::ConnectedScreen;
pub use pairing_input::PairingInputScreen;

use crossterm::event::KeyEvent;
use ratatui::{Frame, layout::Rect, prelude::*, symbols::border, widgets::Block};

// The signal emitted by input handlers to indicate what navigation action to take.
pub enum NavSignal {
    Back,
    Screen(ScreenVariant),
    Command,
}

// Anything that handles input (for now just screens) must implement this trait.
pub trait InputHandler {
    fn handle_back_key(&mut self, _key: KeyEvent) -> NavSignal {
        NavSignal::Back
    }

    fn handle_nav_key(&mut self, key: KeyEvent) -> NavSignal;
    fn handle_enter_key(&mut self, key: KeyEvent) -> NavSignal;
}

pub enum ScreenVariant {
    Home(HomeScreen),
    Scanning(ScanningScreen),
    DeviceList(DeviceListScreen),
    Connected(ConnectedScreen),
    PairingInput(PairingInputScreen),
}

// Every screen implements this trait. It defines how the screen renders itself and how it handles input.
pub trait Screen: InputHandler {
    fn render(&mut self, frame: &mut Frame, area: Rect);
    fn status(&self) -> &str { "" }
    fn nav_hints(&self) -> Vec<(&'static str, &'static str)> { vec![] }

    fn render_outer_block(frame: &mut Frame, area: Rect, nav_hints: Vec<(&str, &str)>) {
        let title = Line::from(vec![" ".into(), "ToothPaste".bold(), " ".into()]);
        let hint_spans: Vec<Span> = nav_hints
            .into_iter()
            .flat_map(|(key, desc)| vec![" ".into(), key.blue().bold(), desc.into()])
            .collect();
        let instructions = Line::from(
            [
                vec![
                    " ".into(),
                    "<↑↓>".blue().bold(),
                    " Navigate  ".into(),
                    "<Enter>".blue().bold(),
                    " select  ".into(),
                    "<Backspace>".blue().bold(),
                    " Back  ".into(),
                    "<Q>".blue().bold(),
                    " Quit ".into(),
                ],
                hint_spans,
            ]
            .concat(),
        );
        let block = Block::bordered()
            .title(title.centered())
            .title_bottom(instructions.centered())
            .border_set(border::THICK);
        frame.render_widget(block, area);
    }
}

impl InputHandler for ScreenVariant {
    fn handle_back_key(&mut self, key: KeyEvent) -> NavSignal {
        match self {
            ScreenVariant::Home(s) => s.handle_back_key(key),
            ScreenVariant::Scanning(s) => s.handle_back_key(key),
            ScreenVariant::DeviceList(s) => s.handle_back_key(key),
            ScreenVariant::Connected(s) => s.handle_back_key(key),
            ScreenVariant::PairingInput(s) => s.handle_back_key(key),
        }
    }

    fn handle_nav_key(&mut self, key: KeyEvent) -> NavSignal {
        match self {
            ScreenVariant::Home(s) => s.handle_nav_key(key),
            ScreenVariant::Scanning(s) => s.handle_nav_key(key),
            ScreenVariant::DeviceList(s) => s.handle_nav_key(key),
            ScreenVariant::Connected(s) => s.handle_nav_key(key),
            ScreenVariant::PairingInput(s) => s.handle_nav_key(key),
        }
    }

    fn handle_enter_key(&mut self, key: KeyEvent) -> NavSignal {
        match self {
            ScreenVariant::Home(s) => s.handle_enter_key(key),
            ScreenVariant::Scanning(s) => s.handle_enter_key(key),
            ScreenVariant::DeviceList(s) => s.handle_enter_key(key),
            ScreenVariant::Connected(s) => s.handle_enter_key(key),
            ScreenVariant::PairingInput(s) => s.handle_enter_key(key),
        }
    }
}

impl Screen for ScreenVariant {
    fn render(&mut self, frame: &mut Frame, area: Rect) {
        match self {
            ScreenVariant::Home(s) => s.render(frame, area),
            ScreenVariant::Scanning(s) => s.render(frame, area),
            ScreenVariant::DeviceList(s) => s.render(frame, area),
            ScreenVariant::Connected(s) => s.render(frame, area),
            ScreenVariant::PairingInput(s) => s.render(frame, area),
        }
    }

    fn status(&self) -> &str {
        match self {
            ScreenVariant::Home(s) => s.status(),
            ScreenVariant::Scanning(s) => s.status(),
            ScreenVariant::DeviceList(s) => s.status(),
            ScreenVariant::Connected(s) => s.status(),
            ScreenVariant::PairingInput(s) => s.status(),
        }
    }

    fn nav_hints(&self) -> Vec<(&'static str, &'static str)> {
        match self {
            ScreenVariant::Home(s) => s.nav_hints(),
            ScreenVariant::Scanning(s) => s.nav_hints(),
            ScreenVariant::DeviceList(s) => s.nav_hints(),
            ScreenVariant::Connected(s) => s.nav_hints(),
            ScreenVariant::PairingInput(s) => s.nav_hints(),
        }
    }
}
