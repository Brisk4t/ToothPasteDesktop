use std::collections::HashSet;
use rdev::{Event, EventType, Key};
use tokio::sync::{mpsc, watch};
use toothpaste_desktop_core::{AppCommand, AppState};
use arboard::Clipboard;
use notify_rust::Notification;

use super::hidmap::{key_to_hid, key_to_ascii};

pub enum InputEvent {
    RDevEvent(Event),
    Clipboard(String),
    Keycode(Vec<u8>),
    ConsumerControl(u32),
}

/// Represents a key combo (modifiers + target key)
struct KeyCombo {
    requires_ctrl: bool,
    requires_alt: bool,
    requires_shift: bool,
    target_key: Key,
}

pub struct SysInputHandler {
    pressed_keys: HashSet<Key>,
    input_event_tx: mpsc::Sender<InputEvent>,
    listen_state_rx: watch::Receiver<AppState>,
    app_state_tx: watch::Sender<AppState>,
    current_state: AppState,
    
    mouse_last_position: (f64, f64),
    accumulated_delta: (f64, f64),
    mouse_debounce_threshold_ms: u64,
    scroll_debounce_threshold_ms: u64,
    mouse_last_event_time: std::time::SystemTime,
}

impl SysInputHandler {
    pub fn new(input_event_tx: mpsc::Sender<InputEvent>, listen_state_rx: watch::Receiver<AppState>, app_state_tx: watch::Sender<AppState>) -> Self {
        let current_state = listen_state_rx.borrow().clone();
        Self {
            pressed_keys: HashSet::new(),
            input_event_tx,
            listen_state_rx,
            app_state_tx,
            current_state,
            mouse_last_position: (0.0, 0.0),
            accumulated_delta: (0.0, 0.0),
            mouse_debounce_threshold_ms: 50,
            scroll_debounce_threshold_ms: 100,
            mouse_last_event_time: std::time::SystemTime::now(),
        }
    }

    fn get_state(&self) -> AppState {
        self.listen_state_rx.borrow().clone()
    }

    fn update_state(&self, new_state: AppState) {
        let _ = self.app_state_tx.send(new_state);
    }

    /// Check if a key combo is currently active (modifiers pressed)
    fn is_combo_active(&self, combo: &KeyCombo) -> bool {
        let has_ctrl = (combo.requires_ctrl) && 
            (self.pressed_keys.contains(&Key::ControlLeft) || self.pressed_keys.contains(&Key::ControlRight));
        let has_alt = (combo.requires_alt) && 
            (self.pressed_keys.contains(&Key::Alt) || self.pressed_keys.contains(&Key::AltGr));
        let has_shift = (combo.requires_shift) && 
            (self.pressed_keys.contains(&Key::ShiftLeft) || self.pressed_keys.contains(&Key::ShiftRight));
        
        let ctrl_ok = !combo.requires_ctrl || has_ctrl;
        let alt_ok = !combo.requires_alt || has_alt;
        let shift_ok = !combo.requires_shift || has_shift;
        
        ctrl_ok && alt_ok && shift_ok
    }

    /// Generate HID report for a key combo (8-byte format: modifiers[0-4], key[5], reserved[6-7])
    /// Uses HID map for special keys, falls back to ASCII code for printing characters
    fn build_hid_report(&self, combo: &KeyCombo) -> Option<[u8; 8]> {
        let mut report = [0u8; 8];
        let mut modifier_idx = 0;
        
        // Add modifiers at indices 0-4
        if combo.requires_ctrl {
            if let Some(code) = key_to_hid(&Key::ControlLeft) {
                if modifier_idx < 5 {
                    report[modifier_idx] = code;
                    modifier_idx += 1;
                }
            }
        }
        if combo.requires_shift {
            if let Some(code) = key_to_hid(&Key::ShiftLeft) {
                if modifier_idx < 5 {
                    report[modifier_idx] = code;
                    modifier_idx += 1;
                }
            }
        }
        if combo.requires_alt {
            if let Some(code) = key_to_hid(&Key::Alt) {
                if modifier_idx < 5 {
                    report[modifier_idx] = code;
                    modifier_idx += 1;
                }
            }
        }
        
        // Add key at index 5 - try HID map first, then ASCII code for printing chars
        let key_code = key_to_hid(&combo.target_key)
            .or_else(|| key_to_ascii(&combo.target_key))?;
        report[5] = key_code;
        
        Some(report)
    }

    pub fn handle_event(&mut self, event: Event) {
        self.current_state = self.get_state();
        
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
        println!("Received keyboard event: {:?}", event);
        match event.event_type {
            EventType::KeyPress(key) => {
                self.pressed_keys.insert(key);
                
                // Check for special combos first (these don't send to device, they control the app)
                self.handle_disable_key_capture_event(key);
                self.handle_disable_clipboard_capture_event(key);
                self.handle_disable_mouse_capture_event(key);
                self.handle_clipboard_event(key);   // Handle clipboard capture (Ctrl+V)

                if self.current_state.enable_key_capture {
                    
                    // Build combo with current modifiers and send to device
                    let combo = KeyCombo {
                        requires_ctrl: self.pressed_keys.contains(&Key::ControlLeft) || self.pressed_keys.contains(&Key::ControlRight),
                        requires_alt: self.pressed_keys.contains(&Key::Alt) || self.pressed_keys.contains(&Key::AltGr),
                        requires_shift: self.pressed_keys.contains(&Key::ShiftLeft) || self.pressed_keys.contains(&Key::ShiftRight),
                        target_key: key,
                    };
                    
                    println!("Current pressed keys: {:?}, constructed combo: ctrl={}, alt={}, shift={}, key={:?}", 
                        self.pressed_keys, combo.requires_ctrl, combo.requires_alt, combo.requires_shift, combo.target_key);
                    
                    let has_modifiers = combo.requires_ctrl || combo.requires_alt || combo.requires_shift;
                    let is_special = key_to_hid(&combo.target_key).is_some();
                    let is_printable = key_to_ascii(&combo.target_key).is_some();
                    
                    if has_modifiers || is_special {
                        // Send as HID report (has modifiers OR is special key)
                        if let Some(hid_report) = self.build_hid_report(&combo) {
                            println!("Sending HID keycode report (hex): {:02x?}", hid_report);
                            let _ = self.input_event_tx.try_send(InputEvent::Keycode(hid_report.to_vec()));
                        }
                    } else if is_printable {
                        // No modifiers, not special - send as printing character
                        let _ = self.input_event_tx.try_send(InputEvent::RDevEvent(event));
                    }
                }
            }
            EventType::KeyRelease(key) => {
                self.pressed_keys.remove(&key);
            }
            _ => {}
        }
    }


    fn handle_mouse_click(&mut self, event: Event) {
        if self.current_state.enable_mouse_capture {
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
                self.current_state.enable_mouse_capture,
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
                self.current_state.enable_mouse_capture,
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
        key_capture_enabled: bool,
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
                && key_capture_enabled {
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
                        && key_capture_enabled {
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
        if self.current_state.enable_clipboard_capture {
            if key == rdev::Key::KeyV {
                let has_ctrl = self.pressed_keys.contains(&rdev::Key::ControlLeft) || self.pressed_keys.contains(&rdev::Key::ControlRight);
                if has_ctrl {
                    let mut clipboard = Clipboard::new().unwrap();
                    if let Ok(contents) = clipboard.get_text() {
                        let _ = self.input_event_tx.try_send(InputEvent::Clipboard(contents));
                    }
                }
            }
        }
    }

    fn handle_disable_clipboard_capture_event(&mut self, key: Key) {
        // Check for Ctrl+Alt+V combo
        let combo = KeyCombo {
            requires_ctrl: true,
            requires_alt: true,
            requires_shift: false,
            target_key: rdev::Key::KeyV,
        };
        
        if key == rdev::Key::KeyV && self.is_combo_active(&combo) {
            let new_state = AppState {
                enable_clipboard_capture: !self.current_state.enable_clipboard_capture,
                ..self.current_state.clone()
            };
            println!("Ctrl+Alt+V pressed - toggling clipboard capture to: {}", new_state.enable_clipboard_capture);
            Notification::new()
                .summary(&format!("Clipboard Capture {}", 
                if new_state.enable_clipboard_capture { "Enabled" } else { "Disabled" }))
                .timeout(notify_rust::Timeout::Milliseconds(2000))
                .show()
                .ok();

            self.update_state(new_state);
        }
    }

    fn handle_disable_mouse_capture_event(&mut self, key: Key) {
        // Check for Ctrl+Alt+M combo
        let combo = KeyCombo {
            requires_ctrl: true,
            requires_alt: true,
            requires_shift: false,
            target_key: rdev::Key::KeyM,
        };

        if key == rdev::Key::KeyM && self.is_combo_active(&combo) {
            let new_state = AppState {
                enable_mouse_capture: !self.current_state.enable_mouse_capture,
                ..self.current_state.clone()
            };
            println!("Ctrl+Alt+M pressed - toggling mouse capture to: {}", new_state.enable_mouse_capture);
            Notification::new()
                .summary(&format!("Mouse Capture {}",
                if new_state.enable_mouse_capture { "Enabled" } else { "Disabled" }))
                .timeout(notify_rust::Timeout::Milliseconds(2000))
                .show()
                .ok();

            self.update_state(new_state);
        }
    }

    fn handle_disable_key_capture_event(&mut self, key: Key) {
        // Check for Ctrl+Alt+C combo
        let combo = KeyCombo {
            requires_ctrl: true,
            requires_alt: true,
            requires_shift: false,
            target_key: rdev::Key::KeyC,
        };
        
        if key == rdev::Key::KeyC && self.is_combo_active(&combo) {
            let new_state = AppState {
                enable_key_capture: !self.current_state.enable_key_capture,
                ..self.current_state.clone()
            };
            println!("Ctrl+Alt+C pressed - toggling key capture to: {}", new_state.enable_key_capture);
            Notification::new()
                .summary(&format!("Key Capture {}", 
                if new_state.enable_key_capture { "Enabled" } else { "Disabled" }))
                .timeout(notify_rust::Timeout::Milliseconds(2000))
                .show()
                .ok();

            self.update_state(new_state);
        }
    }
}