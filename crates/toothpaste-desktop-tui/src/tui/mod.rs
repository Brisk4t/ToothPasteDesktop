mod ui;
use ui::ui;

use std::io;
use std::time::Duration;

use base64::{engine::general_purpose::STANDARD as BASE64, Engine};
use crossterm::event::{self, DisableMouseCapture, EnableMouseCapture, Event, KeyCode, KeyEvent, KeyEventKind};
use crossterm::execute;
use crossterm::terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen};
use ratatui::backend::CrosstermBackend;
use ratatui::widgets::ListState;
use ratatui::DefaultTerminal;
use tokio::sync::{mpsc, watch};
use toothpaste_desktop_core::{AppCommand, AppState, DeviceState, AuthState};

pub enum CurrentScreen {
    Home,
    Scanning,
    DeviceList,
    Connected,
    PairingInput,
}

pub struct ToothPasteTUI {
    pub current_screen: CurrentScreen,
    pub app_state: AppState,
    pub list_state: ListState,
    pub pair_input: String,
    pub status: String,
    pub exit: bool,

    app_state_rx: watch::Receiver<AppState>,
    cmd_tx: mpsc::Sender<AppCommand>,
    pair_req_rx: mpsc::Receiver<()>,
    pair_resp_tx: mpsc::Sender<[u8; 33]>,
}

impl ToothPasteTUI {
    fn new(
        app_state_rx: watch::Receiver<AppState>,
        cmd_tx: mpsc::Sender<AppCommand>,
        pair_req_rx: mpsc::Receiver<()>,
        pair_resp_tx: mpsc::Sender<[u8; 33]>,
    ) -> Self {
        let app_state = app_state_rx.borrow().clone();
        Self {
            current_screen: CurrentScreen::Home,
            app_state,
            list_state: ListState::default(),
            pair_input: String::new(),
            status: String::new(),
            exit: false,
            app_state_rx,
            cmd_tx,
            pair_req_rx,
            pair_resp_tx,
        }
    }

    fn run(&mut self, terminal: &mut DefaultTerminal) -> io::Result<()> {
        while !self.exit {
            self.sync_app_state();

            if let Ok(()) = self.pair_req_rx.try_recv() {
                self.current_screen = CurrentScreen::PairingInput;
                self.pair_input.clear();
                self.status = "Device requires pairing. Enter peer public key (base64):".into();
            }

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
        if self.app_state_rx.has_changed().unwrap_or(false) {
            self.app_state = (*self.app_state_rx.borrow_and_update()).clone();

            // Auto-transition: scanning → device list when results arrive
            if matches!(self.current_screen, CurrentScreen::Scanning)
                && !self.app_state.devices.is_empty()
            {
                self.current_screen = CurrentScreen::DeviceList;
                self.list_state.select(Some(0));
                self.status = format!("{} device(s) found", self.app_state.devices.len());
            }

            // Auto-transition: device list → connected when device connects
            if matches!(self.current_screen, CurrentScreen::DeviceList | CurrentScreen::Scanning)
                && self.app_state.connected_device.is_some()
            {
                self.current_screen = CurrentScreen::Connected;
                self.list_state.select(None);
                self.status = String::new();
            }
        }
    }

    fn handle_keypress(&mut self, key: KeyEvent) {
        match self.current_screen {
            CurrentScreen::PairingInput => self.handle_pairing_key(key),
            _ => self.handle_nav_key(key),
        }
    }

    fn handle_nav_key(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Char('q') | KeyCode::Char('Q') | KeyCode::Esc => {
                if matches!(self.current_screen, CurrentScreen::Home) {
                    self.exit = true;
                } else {
                    self.current_screen = CurrentScreen::Home;
                    self.list_state.select(None);
                    self.status = String::new();
                }
            }
            KeyCode::Down => self.list_state.select_next(),
            KeyCode::Up => self.list_state.select_previous(),
            KeyCode::Char('s') | KeyCode::Char('S') => {
                if matches!(
                    self.current_screen,
                    CurrentScreen::Home | CurrentScreen::DeviceList
                ) {
                    self.send_command(AppCommand::ScanForDevices);
                    self.current_screen = CurrentScreen::Scanning;
                    self.status = "Scanning for devices...".into();
                    self.list_state.select(None);
                }
            }
            KeyCode::Char('d') | KeyCode::Char('D') => {
                if matches!(self.current_screen, CurrentScreen::Connected) {
                    self.send_command(AppCommand::DisconnectDevice);
                    self.current_screen = CurrentScreen::Home;
                    self.status = String::new();
                }
            }
            KeyCode::Enter => self.handle_enter(),
            _ => {}
        }
    }

    fn handle_enter(&mut self) {
        match self.current_screen {
            CurrentScreen::Home => match self.list_state.selected() {
                Some(0) => {
                    self.send_command(AppCommand::ScanForDevices);
                    self.current_screen = CurrentScreen::Scanning;
                    self.status = "Scanning for devices...".into();
                    self.list_state.select(None);
                }
                Some(1) => self.exit = true,
                _ => {}
            },
            CurrentScreen::Scanning => {
                if !self.app_state.devices.is_empty() {
                    self.current_screen = CurrentScreen::DeviceList;
                    self.list_state.select(Some(0));
                    self.status = format!("{} device(s) found", self.app_state.devices.len());
                }
            }
            CurrentScreen::DeviceList => {
                if let Some(idx) = self.list_state.selected() {
                    if let Some(device) = self.app_state.devices.get(idx).cloned() {
                        self.status = format!("Connecting to {}…", device.name);
                        self.send_command(AppCommand::ConnectToDevice(device));
                    }
                }
            }
            CurrentScreen::PairingInput => self.submit_pair_input(),
            CurrentScreen::Connected => {}
        }
    }

    fn handle_pairing_key(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Enter => self.submit_pair_input(),
            KeyCode::Backspace => {
                self.pair_input.pop();
            }
            KeyCode::Esc => {
                self.pair_input.clear();
                self.current_screen = CurrentScreen::Connected;
                self.status = "Pairing cancelled.".into();
            }
            KeyCode::Char(c) => self.pair_input.push(c),
            _ => {}
        }
    }

    fn submit_pair_input(&mut self) {
        match BASE64.decode(self.pair_input.trim()) {
            Ok(bytes) => match bytes.try_into() {
                Ok(arr) => {
                    let _ = self.pair_resp_tx.try_send(arr);
                    self.pair_input.clear();
                    self.current_screen = CurrentScreen::Connected;
                    self.status = "Pairing key sent.".into();
                }
                Err(_) => {
                    self.status = "Invalid key: expected 33 bytes (compressed P-256).".into();
                }
            },
            Err(_) => {
                self.status = "Invalid base64. Try again.".into();
            }
        }
    }

    fn send_command(&self, cmd: AppCommand) {
        let _ = self.cmd_tx.try_send(cmd);
    }
}

// ── public helpers used by ui.rs ──────────────────────────────────────────────

pub fn auth_label(state: &AppState) -> &'static str {
    match &state.connected_device {
        Some(d) => match &d.state {
            DeviceState::Connected { auth_state, .. } => match auth_state {
                AuthState::Authenticated { .. } => "Authenticated",
                AuthState::NotAuthenticated => "Authenticating…",
                AuthState::AuthenticationFailed => "Auth failed",
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

// ── entry point ───────────────────────────────────────────────────────────────

pub fn start_tui(
    app_state_rx: watch::Receiver<AppState>,
    cmd_tx: mpsc::Sender<AppCommand>,
    pair_req_rx: mpsc::Receiver<()>,
    pair_resp_tx: mpsc::Sender<[u8; 33]>,
) -> io::Result<()> {
    enable_raw_mode()?;
    execute!(io::stdout(), EnterAlternateScreen, EnableMouseCapture)?;

    let backend = CrosstermBackend::new(io::stdout());
    let mut terminal = ratatui::Terminal::new(backend)?;

    let mut app = ToothPasteTUI::new(app_state_rx, cmd_tx, pair_req_rx, pair_resp_tx);
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
