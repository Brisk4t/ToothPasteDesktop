
use std::collections::HashSet;
use rdev::{Event, EventType, Key};
use tokio::sync::{mpsc, watch};
use toothpaste_desktop_core::{AppCommand, AppState};
use arboard::Clipboard;

pub enum InputEvent {
    RDevEvent(Event),
    Clipboard(String),
}

pub struct SysInputHandler {
    pressed_keys: HashSet<Key>,
    input_event_tx: mpsc::Sender<InputEvent>,
    listen_state_rx: watch::Receiver<AppState>,
    app_state_tx: watch::Sender<AppState>,
    
    mouse_last_position: (f64, f64),
    accumulated_delta: (f64, f64),
    mouse_debounce_threshold_ms: u64,
    scroll_debounce_threshold_ms: u64,
    mouse_last_event_time: std::time::SystemTime,
}

impl SysInputHandler {
    pub fn new(input_event_tx: mpsc::Sender<InputEvent>,listen_state_rx: watch::Receiver<AppState>, app_state_tx: watch::Sender<AppState>) -> Self {
        Self {
            pressed_keys: HashSet::new(),
            input_event_tx,
            listen_state_rx,
            app_state_tx,
            mouse_last_position: (0.0, 0.0),
            accumulated_delta: (0.0, 0.0),
            mouse_debounce_threshold_ms: 50,
            scroll_debounce_threshold_ms: 100,
            mouse_last_event_time: std::time::SystemTime::now(),
        }
    }

    pub fn handle_event(&mut self, event: Event) {
        match event.event_type {
            // Keyboard event
            EventType::KeyPress(_) | EventType::KeyRelease(_) => {
                self.handle_keyboard_event(event);
                
            }
            EventType::MouseMove{..} => {
                self.handle_mouse_move(event);
            }

            EventType::ButtonPress(_) | EventType::ButtonRelease(_) => {
                self.handle_mouse_click(event);
            }

            // Disable wheel events for now
            // EventType::Wheel {..} => {
            //     self.handle_scroll_event(event);
            // }
            _ => {
                println!("Received non-key event: {:?}", event);
            }
        }

    }

    fn handle_keyboard_event(&mut self, event: Event) {
        match event.event_type {
            EventType::KeyPress(key) => {
                self.pressed_keys.insert(key);
                self.handle_disable_capture_event(key);
                self.handle_clipboard_event(key);
            }
            EventType::KeyRelease(key) => {
                self.pressed_keys.remove(&key);
            }
            _ => {}
        }
                
        if self.listen_state_rx.borrow().enable_key_capture {
            let _ = self.input_event_tx.try_send(InputEvent::RDevEvent(event));
        }
    }


    fn handle_mouse_click(&mut self, event: Event) {
        if self.listen_state_rx.borrow().enable_key_capture {
            let _ = self.input_event_tx.try_send(InputEvent::RDevEvent(event));
        }
    }

    fn handle_scroll_event(&mut self, event: Event) {
        if let EventType::Wheel { delta_x, delta_y } = event.event_type {
            println!("Scroll event detected: delta_x={}, delta_y={}", delta_x, delta_y);
            self.process_accumulated_event(
                (delta_x as f64, delta_y as f64),
                event.time,
                self.scroll_debounce_threshold_ms,
                |delta| EventType::Wheel {
                    delta_x: delta.0 as i64,
                    delta_y: delta.1 as i64,
                },
            );
        }
    }

    fn handle_mouse_move(&mut self, event: Event) {
        if let EventType::MouseMove { x, y } = event.event_type {
            // Calculate delta from last observed position (every event)
            let delta_x = x - self.mouse_last_position.0;
            let delta_y = y - self.mouse_last_position.1;
            
            // Always update to current position
            self.mouse_last_position = (x, y);
            
            self.process_accumulated_event(
                (delta_x, delta_y),
                event.time,
                self.mouse_debounce_threshold_ms,
                |delta| EventType::MouseMove {
                    x: delta.0,
                    y: delta.1,
                },
            );
        }
    }

    /// Generic accumulator handler for events with directional deltas (mouse, scroll, etc.)
    /// Sends accumulated delta on direction change or debounce threshold.
    fn process_accumulated_event(
        &mut self,
        delta: (f64, f64),
        event_time: std::time::SystemTime,
        debounce_threshold_ms: u64,
        create_event: impl Fn((f64, f64)) -> EventType,
    ) {
        // Check if direction changed
        let x_direction_changed = (delta.0 > 0.0 && self.accumulated_delta.0 < 0.0) || 
                                  (delta.0 < 0.0 && self.accumulated_delta.0 > 0.0);
        let y_direction_changed = (delta.1 > 0.0 && self.accumulated_delta.1 < 0.0) || 
                                  (delta.1 < 0.0 && self.accumulated_delta.1 > 0.0);

        if x_direction_changed || y_direction_changed {
            // Direction changed, send accumulated delta immediately
            if (self.accumulated_delta.0 != 0.0 || self.accumulated_delta.1 != 0.0) 
                && self.listen_state_rx.borrow().enable_key_capture {
                let _ = self.input_event_tx.try_send(InputEvent::RDevEvent(Event {
                    time: event_time,
                    name: None,
                    event_type: create_event(self.accumulated_delta),
                }));
            }
            // Start new accumulation with current delta
            self.accumulated_delta = delta;
            self.mouse_last_event_time = event_time;
        } else {
            // Direction is monotonic, accumulate
            self.accumulated_delta.0 += delta.0;
            self.accumulated_delta.1 += delta.1;
            
            // Check if debounce threshold passed
            if let Ok(elapsed) = event_time.duration_since(self.mouse_last_event_time) {
                if elapsed.as_millis() >= debounce_threshold_ms as u128 {
                    // Send accumulated movement if key capture is enabled
                    if (self.accumulated_delta.0 != 0.0 || self.accumulated_delta.1 != 0.0)
                        && self.listen_state_rx.borrow().enable_key_capture {
                        let _ = self.input_event_tx.try_send(InputEvent::RDevEvent(Event {
                            time: event_time,
                            name: None,
                            event_type: create_event(self.accumulated_delta),
                        }));
                    }
                    // Reset accumulated delta
                    self.accumulated_delta = (0.0, 0.0);
                    self.mouse_last_event_time = event_time;
                }
            }
        }
    }

    fn handle_clipboard_event(&mut self, key: Key) {
        if key == rdev::Key::KeyV {
            let has_ctrl = self.pressed_keys.contains(&rdev::Key::ControlLeft) || self.pressed_keys.contains(&rdev::Key::ControlRight);
            if has_ctrl {
                let mut clipboard = Clipboard::new().unwrap();
                if let Ok(contents) = clipboard.get_text() {
                    println!("Clipboard content captured: {}", contents);
                    let _ = self.input_event_tx.try_send(InputEvent::Clipboard(contents));
                }
            }
        }
    }

    fn handle_disable_capture_event(&mut self, key: Key) {
        // Check for Ctrl+Alt+C combo
        if key == rdev::Key::KeyC {
            let has_ctrl = self.pressed_keys.contains(&rdev::Key::ControlLeft) || self.pressed_keys.contains(&rdev::Key::ControlRight);
            let has_alt = self.pressed_keys.contains(&rdev::Key::Alt);
            
            if has_ctrl && has_alt {
                let current_state = self.listen_state_rx.borrow().clone();
                let new_state = AppState {
                    enable_key_capture: !current_state.enable_key_capture,
                    ..current_state
                };
                println!("Ctrl+Alt+C pressed - toggling key capture to: {}", new_state.enable_key_capture);
                let _ = self.app_state_tx.send(new_state);
            }
        }
    }
}