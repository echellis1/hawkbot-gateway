use std::{fmt, num::ParseIntError, str::Utf8Error};

const HEADER_PREFIX: &[u8] = b"004210";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Packet {
    pub start_index: u32,
    pub data: Vec<u8>,
}

impl Packet {
    pub fn parse(frame: &[u8]) -> Result<Self, PacketParseError> {
        let mut cursor = 0usize;

        let header_start = frame[cursor..]
            .iter()
            .position(|b| *b == 0x01)
            .ok_or(PacketParseError::IllFormed)?;
        cursor += header_start + 1;

        let header_end_offset = frame[cursor..]
            .iter()
            .position(|b| *b == 0x02)
            .or_else(|| frame[cursor..].iter().position(|b| *b == 0x04))
            .ok_or(PacketParseError::IllFormed)?;
        let header = &frame[cursor..cursor + header_end_offset];
        cursor += header_end_offset + 1;

        let header_no_prefix = header.strip_prefix(HEADER_PREFIX).ok_or_else(|| {
            PacketParseError::UnsupportedPacket {
                header_bytes: header.to_vec(),
            }
        })?;
        let start_index = std::str::from_utf8(header_no_prefix)
            .map_err(PacketParseError::BadTextEncoding)?
            .parse::<u32>()
            .map_err(PacketParseError::NumberParseFailure)?;

        let data_end_offset = frame[cursor..]
            .iter()
            .position(|b| *b == 0x04)
            .ok_or(PacketParseError::IllFormed)?;
        let data = frame[cursor..cursor + data_end_offset].to_vec();

        Ok(Packet { start_index, data })
    }
}

#[derive(Debug)]
pub enum PacketParseError {
    UnsupportedPacket { header_bytes: Vec<u8> },
    IllFormed,
    BadTextEncoding(Utf8Error),
    NumberParseFailure(ParseIntError),
}

impl fmt::Display for PacketParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            PacketParseError::UnsupportedPacket { .. } => write!(f, "unsupported packet type"),
            PacketParseError::IllFormed => write!(f, "packet is ill-formed"),
            PacketParseError::BadTextEncoding(err) => {
                write!(f, "bad text encoding in packet: {err}")
            }
            PacketParseError::NumberParseFailure(err) => write!(f, "couldn't parse number: {err}"),
        }
    }
}

impl std::error::Error for PacketParseError {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_valid_packet() {
        let frame = b"abc\x010042100012\x02HELLO\x04AA";
        let packet = Packet::parse(frame).expect("packet should parse");
        assert_eq!(packet.start_index, 12);
        assert_eq!(packet.data, b"HELLO");
    }

    #[test]
    fn rejects_bad_header() {
        let frame = b"\x010042110012\x02HELLO\x04AA";
        assert!(matches!(
            Packet::parse(frame),
            Err(PacketParseError::UnsupportedPacket { .. })
        ));
    }

    #[test]
    fn rejects_ill_formed_packet() {
        let frame = b"\x010042100012\x02HELLO";
        assert!(matches!(
            Packet::parse(frame),
            Err(PacketParseError::IllFormed)
        ));
    }
}
