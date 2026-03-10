use std::{fmt::Display, num::ParseIntError, str::Utf8Error};

use super::packet::Packet;

#[derive(Debug, Clone)]
pub struct RtdState {
    data: Vec<u8>,
}

#[derive(Debug, Clone, Copy)]
pub enum RtdFieldJustification {
    Left,
    Right,
    None,
}

impl RtdState {
    pub fn with_capacity(capacity: usize) -> Self {
        Self {
            data: vec![b' '; capacity],
        }
    }

    pub fn apply_packet(&mut self, packet: &Packet) -> Result<(), RtdStateError> {
        let start = packet.start_index as usize;
        let end = start + packet.data.len();
        if end > self.data.len() {
            return Err(RtdStateError::OutOfBounds {
                start,
                len: packet.data.len(),
                capacity: self.data.len(),
            });
        }
        self.data[start..end].copy_from_slice(&packet.data);
        Ok(())
    }

    pub fn field_str(
        &self,
        item: usize,
        length: usize,
        justify: RtdFieldJustification,
    ) -> Result<&str, RtdStateFieldError> {
        let real_index = item - 1;
        let bytes = self
            .data
            .get(real_index..real_index + length)
            .ok_or(RtdStateFieldError::OutOfBounds)?;
        let mut value = std::str::from_utf8(bytes).map_err(RtdStateFieldError::Utf8Error)?;
        value = match justify {
            RtdFieldJustification::Left => value.trim_end(),
            RtdFieldJustification::Right => value.trim_start(),
            RtdFieldJustification::None => value,
        };
        if value.is_empty() {
            Err(RtdStateFieldError::NoData)
        } else {
            Ok(value)
        }
    }

    pub fn field_i32(
        &self,
        item: usize,
        length: usize,
        justify: RtdFieldJustification,
    ) -> Result<i32, RtdStateFieldError> {
        self.field_str(item, length, justify).and_then(|field| {
            field
                .parse::<i32>()
                .map_err(RtdStateFieldError::ParseIntError)
        })
    }

    pub fn field_bool(&self, item: usize) -> Result<bool, RtdStateFieldError> {
        self.field_str(item, 1, RtdFieldJustification::None)
            .map(|c| !c.trim().is_empty())
    }

    pub fn snapshot(&self) -> &[u8] {
        &self.data
    }
}

#[derive(Debug)]
pub enum RtdStateError {
    OutOfBounds {
        start: usize,
        len: usize,
        capacity: usize,
    },
}

impl Display for RtdStateError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            RtdStateError::OutOfBounds {
                start,
                len,
                capacity,
            } => write!(
                f,
                "packet write out of bounds: start={start}, len={len}, capacity={capacity}"
            ),
        }
    }
}

impl std::error::Error for RtdStateError {}

#[derive(Debug)]
pub enum RtdStateFieldError {
    NoData,
    ParseIntError(ParseIntError),
    Utf8Error(Utf8Error),
    OutOfBounds,
}

impl Display for RtdStateFieldError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            RtdStateFieldError::NoData => write!(f, "no data can be read"),
            RtdStateFieldError::ParseIntError(e) => write!(f, "failed to parse int: {e}"),
            RtdStateFieldError::Utf8Error(e) => write!(f, "failed to parse string: {e}"),
            RtdStateFieldError::OutOfBounds => write!(f, "field lookup out of bounds"),
        }
    }
}

impl std::error::Error for RtdStateFieldError {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn updates_state_from_packet() {
        let mut state = RtdState::with_capacity(16);
        let packet = Packet {
            start_index: 4,
            data: b"12".to_vec(),
        };

        state.apply_packet(&packet).expect("apply should work");
        assert_eq!(&state.snapshot()[4..6], b"12");
    }

    #[test]
    fn returns_out_of_bounds_error() {
        let mut state = RtdState::with_capacity(4);
        let packet = Packet {
            start_index: 3,
            data: b"abcd".to_vec(),
        };
        assert!(matches!(
            state.apply_packet(&packet),
            Err(RtdStateError::OutOfBounds { .. })
        ));
    }
}
