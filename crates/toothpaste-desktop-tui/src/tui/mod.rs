mod screens;
mod ui;

use ui::ui;
use screens::{
    ConnectedScreen, DeviceListScreen, HomeScreen, InputHandler, NavSignal, PairingInputScreen,
    Screen, ScreenVariant,
};

use std::io;
use std::time::Duration;

use crossterm::event::{
    self, DisableMouseCapture, EnableMouseCapture, Event, KeyCode, KeyEvent, KeyEventKind,
};
use crossterm::execute;
use crossterm::terminal::{
    EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode,
};
use ratatui::DefaultTerminal;
use ratatui::backend::CrosstermBackend;
use tokio::sync::{mpsc, watch};
use toothpaste_desktop_core::{AppCommand, AppState, AuthState, DeviceState};


pub struct ToothPasteTUI {
    current_screen: ScreenVariant,
    screen_stack: Vec<ScreenVariant>,
    pub app_state: AppState,
    exit: bool,

    app_state_rx: watch::Receiver<AppState>,
    cmd_tx: mpsc::Sender<AppCommand>,
}


impl ToothPasteTUI {
    fn new(app_state_rx: watch::Receiver<AppState>, cmd_tx: mpsc::Sender<AppCommand>) -> Self {
        let app_state = app_state_rx.borrow().clone();
        Self {
            current_screen: ScreenVariant::Home(HomeScreen::new(cmd_tx.clone())),
            screen_stack: Vec::new(),
            app_state,
            exit: false,
            app_state_rx,
            cmd_tx,
        }
    }

    fn run(&mut self, terminal: &mut DefaultTerminal) -> io::Result<()> {
        while !self.exit {
            self.sync_app_state();
            terminal.draw(|frame| ui(frame, self))?;

            if event::poll(Duration::from_millis(100))? {
                if let Event::Key(key) = event::read()? {
                    if key.kind == KeyEventKind::Press {
                        self.handle_keypress(key);
                    }
                }
            }
        }
        Ok(())
    }

    fn sync_app_state(&mut self) {
        if !self.app_state_rx.has_changed().unwrap_or(false) {
            return;
        }
        self.app_state = (*self.app_state_rx.borrow_and_update()).clone();

        // Auto-transition: Scanning → DeviceList when devices are found
        if matches!(self.current_screen, ScreenVariant::Scanning(_))
            && !self.app_state.devices.is_empty()
        {
            self.current_screen = ScreenVariant::DeviceList(DeviceListScreen::new(
                self.cmd_tx.clone(),
                self.app_state_rx.clone(),
                &self.app_state,
            ));
        }

        // Auto-transition: Scanning/DeviceList → Connected when a device connects
        if matches!(
            self.current_screen,
            ScreenVariant::Scanning(_) | ScreenVariant::DeviceList(_)
        ) && self.app_state.connected_device.is_some()
        {
            self.current_screen = ScreenVariant::Connected(ConnectedScreen::new(
                self.cmd_tx.clone(),
                self.app_state_rx.clone(),
            ));
        }

        // Auto-transition: Connected → PairingInput when device signals auth failure
        if matches!(self.current_screen, ScreenVariant::Connected(_)) {
            if let Some(device) = &self.app_state.connected_device {
                if matches!(
                    &device.state,
                    DeviceState::Connected {
                        auth_state: AuthState::AuthenticationFailed,
                        ..
                    }
                ) {
                    let connected = std::mem::replace(
                        &mut self.current_screen,
                        ScreenVariant::PairingInput(PairingInputScreen::new(
                            self.cmd_tx.clone(),
                            self.app_state_rx.clone(),
                        )),
                    );
                    self.screen_stack.push(connected);
                }
            }
        }
    }

    fn handle_keypress(&mut self, key: KeyEvent) {
        let signal = match key.code {
            KeyCode::Backspace => self.current_screen.handle_back_key(key),
            KeyCode::Enter => self.current_screen.handle_enter_key(key),
            _ => self.current_screen.handle_nav_key(key),
        };
        self.apply_nav_signal(signal);
    }

    fn apply_nav_signal(&mut self, signal: NavSignal) {
        match signal {
            NavSignal::Back => {
                if let Some(prev) = self.screen_stack.pop() {
                    self.current_screen = prev;
                } else {
                    self.exit = true;
                }
            }
            NavSignal::Screen(new_screen) => {
                let prev = std::mem::replace(&mut self.current_screen, new_screen);
                self.screen_stack.push(prev);
            }
            NavSignal::Command => {}
        }
    }

    pub fn current_screen(&mut self) -> &mut ScreenVariant {
        &mut self.current_screen
    }

    pub fn status(&self) -> &str {
        self.current_screen.status()
    }

    pub fn nav_hints(&self) -> Vec<&'static str> {
        self.current_screen.nav_hints()
    }
}

// ── public helpers used by ui.rs ──────────────────────────────────────────────

pub fn auth_label(state: &AppState) -> &'static str {
    match &state.connected_device {
        Some(d) => match &d.state {
            DeviceState::Connected { auth_state, .. } => match auth_state {
                AuthState::Authenticated { .. } => "Authenticated",
                AuthState::NotAuthenticated => "Authenticating…",
                AuthState::AuthenticationFailed => "Pairing required",
            },
            DeviceState::Disconnected => "Disconnected",
        },
        None => "—",
    }
}

pub fn firmware_label(state: &AppState) -> String {
    match &state.connected_device {
        Some(d) => match &d.state {
            DeviceState::Connected { firmware_version, .. } => firmware_version.clone(),
            _ => "—".into(),
        },
        None => "—".into(),
    }
}

pub fn capture_label(state: &AppState) -> &'static str {
    if state.enable_key_capture { "Enabled" } else { "Disabled" }
}

// ── entry point ───────────────────────────────────────────────────────────────

pub fn start_tui(
    app_state_rx: watch::Receiver<AppState>,
    cmd_tx: mpsc::Sender<AppCommand>,
) -> io::Result<()> {
    enable_raw_mode()?;
    execute!(io::stdout(), EnterAlternateScreen, EnableMouseCapture)?;

    let backend = CrosstermBackend::new(io::stdout());
    let mut terminal = ratatui::Terminal::new(backend)?;

    let mut app = ToothPasteTUI::new(app_state_rx, cmd_tx);
    let result = app.run(&mut terminal);

    disable_raw_mode()?;
    execute!(
        terminal.backend_mut(),
        LeaveAlternateScreen,
        DisableMouseCapture
    )?;
    terminal.show_cursor()?;

    result
}
