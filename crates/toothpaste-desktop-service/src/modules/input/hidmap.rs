use rdev::Key;

/// Get HID code for a key using compile-time dispatch (match statement)
pub fn key_to_hid(key: &Key) -> Option<u8> {
    match key {
        // Modifiers
        Key::ControlLeft => Some(0x80),
        Key::ControlRight => Some(0x84),
        Key::ShiftLeft => Some(0x81),
        Key::ShiftRight => Some(0x85),
        Key::Alt => Some(0x82),
        Key::AltGr => Some(0x86),
        Key::MetaLeft => Some(0x83),
        Key::MetaRight => Some(0x87),
        
        // Navigation
        Key::UpArrow => Some(0xDA),
        Key::DownArrow => Some(0xD9),
        Key::LeftArrow => Some(0xD8),
        Key::RightArrow => Some(0xD7),
        Key::Home => Some(0xD2),
        Key::End => Some(0xD5),
        Key::PageUp => Some(0xD3),
        Key::PageDown => Some(0xD6),
        
        // Editing
        Key::Backspace => Some(0xB2),
        Key::Tab => Some(0xB3),
        Key::Return => Some(0xB0),
        Key::Escape => Some(0xB1),
        Key::Insert => Some(0xD1),
        Key::Delete => Some(0xD4),
        
        // Function keys
        Key::F1 => Some(0xC2),
        Key::F2 => Some(0xC3),
        Key::F3 => Some(0xC4),
        Key::F4 => Some(0xC5),
        Key::F5 => Some(0xC6),
        Key::F6 => Some(0xC7),
        Key::F7 => Some(0xC8),
        Key::F8 => Some(0xC9),
        Key::F9 => Some(0xCA),
        Key::F10 => Some(0xCB),
        Key::F11 => Some(0xCC),
        Key::F12 => Some(0xCD),
        
        _ => None,
    }
}

/// Map Key to ASCII code for printing characters (letters, numbers, and symbols)
pub fn key_to_ascii(key: &Key) -> Option<u8> {
    match key {
        // Space
        Key::Space => Some(0x20),
        
        // Letter keys (A-Z) -> ASCII 0x61-0x7A (a-z)
        Key::KeyA => Some(0x61),
        Key::KeyB => Some(0x62),
        Key::KeyC => Some(0x63),
        Key::KeyD => Some(0x64),
        Key::KeyE => Some(0x65),
        Key::KeyF => Some(0x66),
        Key::KeyG => Some(0x67),
        Key::KeyH => Some(0x68),
        Key::KeyI => Some(0x69),
        Key::KeyJ => Some(0x6A),
        Key::KeyK => Some(0x6B),
        Key::KeyL => Some(0x6C),
        Key::KeyM => Some(0x6D),
        Key::KeyN => Some(0x6E),
        Key::KeyO => Some(0x6F),
        Key::KeyP => Some(0x70),
        Key::KeyQ => Some(0x71),
        Key::KeyR => Some(0x72),
        Key::KeyS => Some(0x73),
        Key::KeyT => Some(0x74),
        Key::KeyU => Some(0x75),
        Key::KeyV => Some(0x76),
        Key::KeyW => Some(0x77),
        Key::KeyX => Some(0x78),
        Key::KeyY => Some(0x79),
        Key::KeyZ => Some(0x7A),
        
        // Number keys (0-9) -> ASCII 0x30-0x39
        Key::Num0 => Some(0x30),
        Key::Num1 => Some(0x31),
        Key::Num2 => Some(0x32),
        Key::Num3 => Some(0x33),
        Key::Num4 => Some(0x34),
        Key::Num5 => Some(0x35),
        Key::Num6 => Some(0x36),
        Key::Num7 => Some(0x37),
        Key::Num8 => Some(0x38),
        Key::Num9 => Some(0x39),
        
        // Symbol/punctuation keys (common ASCII symbols)
        Key::Comma => Some(0x2C),           // ,
        Key::Minus => Some(0x2D),           // -
        //Key:: => Some(0x2E),          // .
        Key::Slash => Some(0x2F),           // /
        //Key::Semicolon => Some(0x3B),       // ;
        Key::Equal => Some(0x3D),           // =
        //Key::BracketLeft => Some(0x5B),     // [
        //Key::Backslash => Some(0x5C),       // \
        //Key::BracketRight => Some(0x5D),    // ]
        Key::BackQuote => Some(0x60),       // `
        
        _ => None,
    }
}
