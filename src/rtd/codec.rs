#[derive(Debug, Clone)]
pub struct SerialRtdFramer {
    state: FrameState,
    next_index: usize,
}

#[derive(Debug, Clone, Copy)]
enum FrameState {
    ReadingSyncIdle,
    ReadingData,
}

impl Default for SerialRtdFramer {
    fn default() -> Self {
        Self {
            state: FrameState::ReadingSyncIdle,
            next_index: 0,
        }
    }
}

impl SerialRtdFramer {
    pub fn push_bytes(&mut self, buffer: &mut Vec<u8>, chunk: &[u8]) {
        buffer.extend_from_slice(chunk);
    }

    pub fn pop_frame(&mut self, buffer: &mut Vec<u8>) -> Option<Vec<u8>> {
        loop {
            match self.state {
                FrameState::ReadingSyncIdle => {
                    let pos = buffer.iter().position(|b| *b == 0x16)?;
                    buffer.drain(..=pos);
                    self.state = FrameState::ReadingData;
                    self.next_index = 0;
                    if buffer.is_empty() {
                        return None;
                    }
                }
                FrameState::ReadingData => {
                    if let Some(offset) = buffer[self.next_index..].iter().position(|b| *b == 0x17)
                    {
                        let position = self.next_index + offset;
                        let frame = buffer.drain(..position).collect::<Vec<_>>();
                        buffer.drain(..1);
                        self.state = FrameState::ReadingSyncIdle;
                        self.next_index = 0;
                        return Some(frame);
                    }
                    self.next_index = buffer.len();
                    return None;
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extracts_frame_split_across_reads() {
        let mut framer = SerialRtdFramer::default();
        let mut buffer = Vec::new();

        framer.push_bytes(&mut buffer, b"garbage\x16abc");
        assert!(framer.pop_frame(&mut buffer).is_none());

        framer.push_bytes(&mut buffer, b"def\x17");
        let frame = framer.pop_frame(&mut buffer).expect("frame should exist");
        assert_eq!(frame, b"abcdef");
    }
}
