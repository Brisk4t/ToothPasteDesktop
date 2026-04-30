
use std::collections::HashSet;
use rdev::{Event, EventType, Key};
use tokio::sync::{mpsc, watch};
use toothpaste_desktop_core::{AppCommand, AppState};

pub struct SysInputHandler {
    pressed_keys: HashSet<Key>,
    input_event_tx: mpsc::Sender<Event>,
    listen_state_rx: watch::Receiver<AppState>,
    app_state_tx: watch::Sender<AppState>,
    
    mouse_last_position: (f64, f64),
    accumulated_delta: (f64, f64),
    mouse_debounce_threshold_ms: u64,
    mouse_last_event_time: std::time::SystemTime,
}

impl SysInputHandler {
    pub fn new(input_event_tx: mpsc::Sender<Event>,listen_state_rx: watch::Receiver<AppState>, app_state_tx: watch::Sender<AppState>) -> Self {
        Self {
            pressed_keys: HashSet::new(),
            input_event_tx,
            listen_state_rx,
            app_state_tx,
            mouse_last_position: (0.0, 0.0),
            accumulated_delta: (0.0, 0.0),
            mouse_debounce_threshold_ms: 50,
            mouse_last_event_time: std::time::SystemTime::now(),
        }
    }

    pub fn handle_event(&mut self, event: Event) {
        match event.event_type {
            // If a key is pressed
            EventType::KeyPress(key) => {
                self.handle_keyboard_event(event);
                
            }
            // If a key is released, remove it from the pressed keys set
            EventType::KeyRelease(key) => {
                self.handle_keyboard_event(event);
            }

            EventType::MouseMove{..} => {
                self.handle_mouse_event(event);
            }

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
            }
            EventType::KeyRelease(key) => {
                self.pressed_keys.remove(&key);
            }
            _ => {}
        }
                
        if self.listen_state_rx.borrow().enable_key_capture {
            let _ = self.input_event_tx.try_send(event);
        }
    }

    fn handle_mouse_event(&mut self, event: Event) {
        if let EventType::MouseMove { x, y } = event.event_type {
            // Calculate delta from last observed position (every event)
            let delta_x = x - self.mouse_last_position.0;
            let delta_y = y - self.mouse_last_position.1;
            
            // Always update to current position
            self.mouse_last_position = (x, y);

            // Check if direction changed
            let x_direction_changed = (delta_x > 0.0 && self.accumulated_delta.0 < 0.0) || 
                                      (delta_x < 0.0 && self.accumulated_delta.0 > 0.0);
            let y_direction_changed = (delta_y > 0.0 && self.accumulated_delta.1 < 0.0) || 
                                      (delta_y < 0.0 && self.accumulated_delta.1 > 0.0);

            if x_direction_changed || y_direction_changed {
                // Direction changed, send accumulated delta immediately
                if (self.accumulated_delta.0 != 0.0 || self.accumulated_delta.1 != 0.0) 
                    && self.listen_state_rx.borrow().enable_key_capture {
                    let _ = self.input_event_tx.try_send(Event {
                        time: event.time,
                        name: None,
                        event_type: EventType::MouseMove {
                            x: self.accumulated_delta.0,
                            y: self.accumulated_delta.1,
                        },
                    });
                }
                // Start new accumulation with current delta
                self.accumulated_delta = (delta_x, delta_y);
                self.mouse_last_event_time = event.time;
            } else {
                // Direction is monotonic, accumulate
                self.accumulated_delta.0 += delta_x;
                self.accumulated_delta.1 += delta_y;
                
                // Check if debounce threshold passed
                if let Ok(elapsed) = event.time.duration_since(self.mouse_last_event_time) {
                    if elapsed.as_millis() >= self.mouse_debounce_threshold_ms as u128 {
                        // Send accumulated movement if key capture is enabled
                        if (self.accumulated_delta.0 != 0.0 || self.accumulated_delta.1 != 0.0)
                            && self.listen_state_rx.borrow().enable_key_capture {
                            let _ = self.input_event_tx.try_send(Event {
                                time: event.time,
                                name: None,
                                event_type: EventType::MouseMove {
                                    x: self.accumulated_delta.0,
                                    y: self.accumulated_delta.1,
                                },
                            });
                        }
                        // Reset accumulated delta
                        self.accumulated_delta = (0.0, 0.0);
                        self.mouse_last_event_time = event.time;
                    }
                }
            }
        }
    }

    fn handle_clipboard_event(&mut self, event: Event) {
        // Implementation for handling clipboard events 
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