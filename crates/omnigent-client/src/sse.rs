//! An incremental decoder of `text/event-stream` bodies.
//!
//! Fed the body chunk by chunk ([`SseDecoder::push`]), wherever the chunks split: inside a
//! line, a UTF-8 sequence or a `\r\n`. Lines are decoded only once complete. A frame is
//! dispatched at a blank line, its `data:` lines joined with `\n`. Comments (`:`) and the
//! `id:`/`retry:` fields are ignored. A `data: [DONE]` frame ends the stream; nothing after
//! it is decoded.

/// A decoded frame.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Frame {
    /// An event: its `event:` name, if any, and its data.
    Event {
        /// The `event:` field.
        event: Option<String>,
        /// The `data:` lines, joined with `\n`.
        data: String,
    },
    /// The `data: [DONE]` sentinel: the server closed the stream.
    Done,
}

/// The decoder's state between chunks.
#[derive(Debug, Default)]
pub struct SseDecoder {
    line: Vec<u8>,
    event: Option<String>,
    data: Option<String>,
    done: bool,
}

impl SseDecoder {
    /// A decoder at the start of a stream.
    pub fn new() -> Self {
        Self::default()
    }

    /// Whether `[DONE]` was decoded.
    pub fn is_done(&self) -> bool {
        self.done
    }

    /// Decode the frames that `chunk` completes.
    pub fn push(&mut self, chunk: &[u8]) -> Vec<Frame> {
        let mut frames = Vec::new();
        for &byte in chunk {
            if self.done {
                break;
            }
            if byte == b'\n' {
                let mut line = std::mem::take(&mut self.line);
                if line.last() == Some(&b'\r') {
                    line.pop();
                }
                self.line_complete(&line, &mut frames);
            } else {
                self.line.push(byte);
            }
        }
        frames
    }

    /// The end of the body: dispatch a frame left without its blank line.
    pub fn finish(&mut self) -> Vec<Frame> {
        let mut frames = Vec::new();
        if !self.done {
            let mut line = std::mem::take(&mut self.line);
            if line.last() == Some(&b'\r') {
                line.pop();
            }
            if !line.is_empty() {
                self.line_complete(&line, &mut frames);
            }
            self.dispatch(&mut frames);
        }
        frames
    }

    fn line_complete(&mut self, line: &[u8], frames: &mut Vec<Frame>) {
        if line.is_empty() {
            self.dispatch(frames);
            return;
        }
        if line[0] == b':' {
            return;
        }
        let line = String::from_utf8_lossy(line);
        let (field, value) = match line.split_once(':') {
            Some((field, value)) => (field, value.strip_prefix(' ').unwrap_or(value)),
            None => (line.as_ref(), ""),
        };
        match field {
            "event" => self.event = Some(value.to_owned()),
            "data" => {
                // The server sends `[DONE]` bare, with no `event:` line and maybe no
                // blank line after it.
                if value.trim() == "[DONE]" && self.data.is_none() {
                    self.event = None;
                    self.done = true;
                    frames.push(Frame::Done);
                    return;
                }
                match &mut self.data {
                    Some(data) => {
                        data.push('\n');
                        data.push_str(value);
                    }
                    None => self.data = Some(value.to_owned()),
                }
            }
            _ => {}
        }
    }

    fn dispatch(&mut self, frames: &mut Vec<Frame>) {
        let event = self.event.take();
        if let Some(data) = self.data.take() {
            frames.push(Frame::Event { event, data });
        }
    }
}
