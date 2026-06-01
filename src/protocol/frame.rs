#![allow(dead_code)]

use thiserror::Error;

const HEADER_LEN: usize = 4;
const MAX_FRAME_LEN: usize = 8 * 1024 * 1024;

#[derive(Debug, Error, PartialEq, Eq)]
pub enum FrameError {
    #[error("native pipe frame length exceeds limit")]
    FrameTooLarge,
}

#[derive(Debug, Default)]
pub struct FrameDecoder {
    pending: Vec<u8>,
}

impl FrameDecoder {
    pub fn push(&mut self, bytes: &[u8]) -> Result<Vec<String>, FrameError> {
        self.pending.extend_from_slice(bytes);
        let mut messages = Vec::new();
        let mut offset = 0;

        while self.pending.len().saturating_sub(offset) >= HEADER_LEN {
            let len = read_native_u32(&self.pending[offset..offset + HEADER_LEN]) as usize;
            if len > MAX_FRAME_LEN {
                return Err(FrameError::FrameTooLarge);
            }

            let frame_len = HEADER_LEN + len;
            if self.pending.len().saturating_sub(offset) < frame_len {
                break;
            }

            let payload_start = offset + HEADER_LEN;
            let payload_end = payload_start + len;
            messages.push(String::from_utf8_lossy(&self.pending[payload_start..payload_end]).into());
            offset += frame_len;
        }

        if offset > 0 {
            self.pending.drain(..offset);
        }

        Ok(messages)
    }
}

pub fn encode_frame(message: &str) -> Vec<u8> {
    let payload = message.as_bytes();
    let mut frame = Vec::with_capacity(HEADER_LEN + payload.len());
    frame.extend_from_slice(&native_u32_bytes(payload.len() as u32));
    frame.extend_from_slice(payload);
    frame
}

fn native_u32_bytes(value: u32) -> [u8; HEADER_LEN] {
    if cfg!(target_endian = "little") {
        value.to_le_bytes()
    } else {
        value.to_be_bytes()
    }
}

fn read_native_u32(bytes: &[u8]) -> u32 {
    let array = [bytes[0], bytes[1], bytes[2], bytes[3]];
    if cfg!(target_endian = "little") {
        u32::from_le_bytes(array)
    } else {
        u32::from_be_bytes(array)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;

    #[test]
    fn decodes_complete_frame() {
        let mut decoder = FrameDecoder::default();
        let messages = decoder.push(&encode_frame(r#"{"id":1}"#)).unwrap();
        assert_eq!(messages, vec![r#"{"id":1}"#]);
    }

    #[test]
    fn decodes_fragmented_frame() {
        let frame = encode_frame("hello");
        let mut decoder = FrameDecoder::default();
        assert!(decoder.push(&frame[..2]).unwrap().is_empty());
        assert_eq!(decoder.push(&frame[2..]).unwrap(), vec!["hello"]);
    }

    #[test]
    fn decodes_multiple_frames() {
        let mut bytes = encode_frame("one");
        bytes.extend_from_slice(&encode_frame("two"));
        let mut decoder = FrameDecoder::default();
        assert_eq!(decoder.push(&bytes).unwrap(), vec!["one", "two"]);
    }

    #[test]
    fn rejects_huge_frame() {
        let mut decoder = FrameDecoder::default();
        let huge = if cfg!(target_endian = "little") {
            ((MAX_FRAME_LEN as u32) + 1).to_le_bytes()
        } else {
            ((MAX_FRAME_LEN as u32) + 1).to_be_bytes()
        };
        assert_eq!(decoder.push(&huge), Err(FrameError::FrameTooLarge));
    }
}
