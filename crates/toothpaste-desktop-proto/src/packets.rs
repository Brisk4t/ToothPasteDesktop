use crate::toothpaste::{
    ConsumerControlPacket, DataPacket, EncryptedData, Frame, KeyboardPacket, KeycodePacket,
    MouseJigglePacket, MousePacket, RenamePacket, ResponsePacket, data_packet, encrypted_data,
};
use prost::Message;

// Create an unencrypted ToothPaste DataPacket from the input string
pub fn create_unencrypted_packet(input: &str) -> Vec<u8> {
    let text_data = input.as_bytes().to_vec();
    let len = text_data.len() as u32;
    DataPacket {
        packet_id: data_packet::PacketId::AuthPacket as i32,
        packet_number: 1,
        total_packets: 0,
        slow_mode: true,
        iv: vec![0u8; 12],
        data_len: len,
        encrypted_data: text_data,
        tag: vec![0u8; 16],
    }
    .encode_to_vec()
}

pub fn create_mouse_packet(x: f64, y: f64, left_click: bool, right_click: bool, scroll_delta: i32) -> EncryptedData {
    let mouse_packet = MousePacket {
        num_frames: 1,
        frames: vec![Frame {
            x: x.round() as i32,
            y: y.round() as i32,
        }],
        l_click: left_click as i32,
        r_click: right_click as i32,
        wheel: scroll_delta,
    };
    EncryptedData {
        packet_type: encrypted_data::PacketType::Mouse as i32,
        packet_data: Some(encrypted_data::PacketData::MousePacket(mouse_packet)),
    }
}

pub fn create_mouse_stream(
    frames: &[(f64, f64)], left_click: bool, right_click: bool, scroll_delta: i32,
) -> EncryptedData {
    let pb_frames: Vec<Frame> = frames
        .iter()
        .map(|(x, y)| Frame {
            x: x.round() as i32,
            y: y.round() as i32,
        })
        .collect();
    let mouse_packet = MousePacket {
        num_frames: pb_frames.len() as u32,
        frames: pb_frames,
        l_click: left_click as i32,
        r_click: right_click as i32,
        wheel: scroll_delta,
    };
    EncryptedData {
        packet_type: encrypted_data::PacketType::Mouse as i32,
        packet_data: Some(encrypted_data::PacketData::MousePacket(mouse_packet)),
    }
}

pub fn create_keyboard_packet(key_string: &str) -> EncryptedData {
    EncryptedData {
        packet_type: encrypted_data::PacketType::KeyboardString as i32,
        packet_data: Some(encrypted_data::PacketData::KeyboardPacket(KeyboardPacket {
            length: key_string.chars().count() as u32,
            message: key_string.to_string(),
        })),
    }
}

pub fn create_keyboard_stream(input: &str) -> Vec<EncryptedData> {
    let chars: Vec<char> = input.chars().collect();
    chars
        .chunks(100)
        .map(|chunk| create_keyboard_packet(&chunk.iter().collect::<String>()))
        .collect()
}

pub fn create_keycode_packet(code: &[u8]) -> EncryptedData {
    EncryptedData {
        packet_type: encrypted_data::PacketType::KeyboardKeycode as i32,
        packet_data: Some(encrypted_data::PacketData::KeycodePacket(KeycodePacket {
            length: code.len() as u32,
            code: code.to_vec(),
        })),
    }
}

pub fn create_rename_packet(new_name: &str) -> EncryptedData {
    EncryptedData {
        packet_type: encrypted_data::PacketType::Rename as i32,
        packet_data: Some(encrypted_data::PacketData::RenamePacket(RenamePacket {
            length: new_name.chars().count() as u32,
            message: new_name.to_string(),
        })),
    }
}

pub fn create_consumer_control_packet(code: u32) -> EncryptedData {
    EncryptedData {
        packet_type: encrypted_data::PacketType::ConsumerControl as i32,
        packet_data: Some(encrypted_data::PacketData::ConsumerControlPacket(
            ConsumerControlPacket {
                code: vec![code],
                length: 1,
            },
        )),
    }
}

pub fn create_mouse_jiggle_packet(enable: bool) -> EncryptedData {
    EncryptedData {
        packet_type: encrypted_data::PacketType::Composite as i32,
        packet_data: Some(encrypted_data::PacketData::MouseJigglePacket(
            MouseJigglePacket { enable },
        )),
    }
}

pub fn unpack_response_packet(bytes: &[u8]) -> Result<ResponsePacket, prost::DecodeError> {
    ResponsePacket::decode(bytes)
}
