use bytes::{BufMut, BytesMut};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct SseEvent {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub event: Option<String>,
    pub data: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub retry: Option<u32>,
}

impl SseEvent {
    pub fn to_bytes(&self) -> BytesMut {
        let mut buffer = BytesMut::new();

        if let Some(id) = &self.id {
            buffer.put_slice(b"id: ");
            buffer.put_slice(id.as_bytes());
            buffer.put_u8(b'\n');
        }

        if let Some(event) = &self.event {
            buffer.put_slice(b"event: ");
            buffer.put_slice(event.as_bytes());
            buffer.put_u8(b'\n');
        }

        if let Some(retry) = self.retry {
            buffer.put_slice(b"retry: ");
            buffer.put_slice(retry.to_string().as_bytes());
            buffer.put_u8(b'\n');
        }

        if !self.data.is_empty() {
            for line in self.data.split('\n') {
                buffer.put_slice(b"data: ");
                buffer.put_slice(line.as_bytes());
                buffer.put_u8(b'\n');
            }
        }

        buffer.put_u8(b'\n');
        buffer
    }
}

/// A parser for Server-Sent Events (SSE) streams.
/// It maintains state between chunks of data to correctly parse events that span multiple chunks.
#[derive(Debug)]
pub struct SseParser {
    buffer: Vec<u8>,
    current_event: SseEvent,
    is_start: bool,
}

impl Default for SseParser {
    fn default() -> Self {
        Self {
            buffer: Vec::new(),
            current_event: SseEvent::default(),
            is_start: true,
        }
    }
}

impl SseParser {
    pub fn new() -> Self {
        Self::default()
    }

    /// Process a chunk of bytes from the SSE stream.
    /// Returns a vector of parsed events.
    pub fn process(&mut self, chunk: &[u8]) -> Vec<SseEvent> {
        self.buffer.extend_from_slice(chunk);

        // Handle Byte Order Mark (BOM) at the beginning of the stream.
        if self.is_start {
            if self.buffer.len() >= 3 {
                if self.buffer[0] == 0xEF && self.buffer[1] == 0xBB && self.buffer[2] == 0xBF {
                    self.buffer.drain(0..3);
                }
                self.is_start = false;
            } else {
                // Check if it could be a BOM (EF BB BF)
                let bom_prefix = [0xEF, 0xBB, 0xBF];
                // If the buffer matches the prefix of BOM so far, wait for more data.
                if self.buffer == bom_prefix[..self.buffer.len()] {
                    return Vec::new();
                }
                self.is_start = false;
            }
        }

        let mut events = Vec::new();

        loop {
            // Find positions of line feed and carriage return
            let lf_pos = self.buffer.iter().position(|&b| b == b'\n');
            let cr_pos = self.buffer.iter().position(|&b| b == b'\r');

            // Determine the end of the line and the length to skip (including the newline char(s))
            let (end_pos, skip_len) = match (lf_pos, cr_pos) {
                (Some(lf), Some(cr)) => {
                    if lf < cr {
                        (lf, 1) // \n comes first
                    } else if lf == cr + 1 {
                        (cr, 2) // \r\n sequence
                    } else {
                        (cr, 1) // \r comes first, treated as newline
                    }
                }
                (Some(lf), None) => (lf, 1), // Only \n found
                (None, Some(cr)) => {
                    // If \r is the last byte, we need to wait for the next chunk to see if \n follows
                    if cr + 1 < self.buffer.len() {
                        (cr, 1) // \r followed by something else
                    } else {
                        break; // Wait for more data
                    }
                }
                (None, None) => break, // No newline found, wait for more data
            };

            let line_bytes = self.buffer[..end_pos].to_vec();
            self.buffer.drain(..end_pos + skip_len);

            // Convert bytes to string, replacing invalid sequences
            let line = String::from_utf8_lossy(&line_bytes);

            if line.is_empty() {
                // Empty line indicates end of event
                if !self.is_event_empty(&self.current_event) {
                    events.push(std::mem::take(&mut self.current_event));
                }
            } else {
                self.parse_line(&line);
            }
        }

        events
    }

    fn is_event_empty(&self, event: &SseEvent) -> bool {
        event.data.is_empty()
            && event.event.is_none()
            && event.id.is_none()
            && event.retry.is_none()
    }

    fn parse_line(&mut self, line: &str) {
        if line.starts_with(':') {
            return; // Ignore comments
        }

        let (field, value) = if let Some((f, v)) = line.split_once(':') {
            (f, v)
        } else {
            (line, "") // Field with no value
        };

        // Remove leading space from value if present
        let value = if value.starts_with(' ') {
            &value[1..]
        } else {
            value
        };

        match field {
            "event" => self.current_event.event = Some(value.to_string()),
            "data" => {
                if !self.current_event.data.is_empty() {
                    self.current_event.data.push('\n');
                }
                self.current_event.data.push_str(value);
            }
            "id" => {
                if !value.contains('\0') {
                    self.current_event.id = Some(value.to_string());
                }
            }
            "retry" => {
                if let Ok(retry) = value.trim().parse::<u32>() {
                    self.current_event.retry = Some(retry);
                }
            }
            _ => {} // Ignore unknown fields
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_basic_parsing() {
        let mut parser = SseParser::new();
        let input = "data: hello world\n\n";
        let events = parser.process(input.as_bytes());

        assert_eq!(events.len(), 1);
        assert_eq!(events[0].data, "hello world");
    }

    #[test]
    fn test_multiple_events() {
        let mut parser = SseParser::new();
        let input = "data: first\n\ndata: second\n\n";
        let events = parser.process(input.as_bytes());

        assert_eq!(events.len(), 2);
        assert_eq!(events[0].data, "first");
        assert_eq!(events[1].data, "second");
    }

    #[test]
    fn test_split_across_chunks() {
        let mut parser = SseParser::new();

        let events1 = parser.process(b"data: hel");
        assert_eq!(events1.len(), 0);

        let events2 = parser.process(b"lo\n\n");
        assert_eq!(events2.len(), 1);
        assert_eq!(events2[0].data, "hello");
    }

    #[test]
    fn test_all_fields() {
        let mut parser = SseParser::new();
        let input = "id: 123\nevent: update\nretry: 1000\ndata: payload\n\n";
        let events = parser.process(input.as_bytes());

        assert_eq!(events.len(), 1);
        assert_eq!(events[0].id.as_deref(), Some("123"));
        assert_eq!(events[0].event.as_deref(), Some("update"));
        assert_eq!(events[0].retry, Some(1000));
        assert_eq!(events[0].data, "payload");
    }

    #[test]
    fn test_multiline_data() {
        let mut parser = SseParser::new();
        let input = "data: line1\ndata: line2\n\n";
        let events = parser.process(input.as_bytes());

        assert_eq!(events.len(), 1);
        assert_eq!(events[0].data, "line1\nline2");
    }

    #[test]
    fn test_comments_ignored() {
        let mut parser = SseParser::new();
        let input = ": this is a comment\ndata: real data\n\n";
        let events = parser.process(input.as_bytes());

        assert_eq!(events.len(), 1);
        assert_eq!(events[0].data, "real data");
    }

    #[test]
    fn test_mixed_line_endings() {
        let mut parser = SseParser::new();
        // \r\n, \r, \n mixed
        let input = "data: e1\r\ndata: e1b\r\n\r\ndata: e2\r\rdata: e3\n\n";
        let events = parser.process(input.as_bytes());

        assert_eq!(events.len(), 3);
        assert_eq!(events[0].data, "e1\ne1b");
        assert_eq!(events[1].data, "e2");
        assert_eq!(events[2].data, "e3");
    }

    #[test]
    fn test_split_crlf_detailed() {
        let mut parser = SseParser::new();

        // "data: test\r\n\r\n" split as "data: test\r" and "\n\r\n"

        let events = parser.process(b"data: test\r");
        assert!(events.is_empty());

        let events = parser.process(b"\n\r\n");
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].data, "test");
    }

    #[test]
    fn test_byte_by_byte() {
        let mut parser = SseParser::new();
        let input = "data: hello\n\n";
        let mut events = Vec::new();

        for b in input.bytes() {
            let chunk = [b];
            events.extend(parser.process(&chunk));
        }

        assert_eq!(events.len(), 1);
        assert_eq!(events[0].data, "hello");
    }

    #[test]
    fn test_utf8_split() {
        let mut parser = SseParser::new();
        // "data: 🚀\n\n"
        // 🚀 is F0 9F 9A 80
        let part1 = vec![b'd', b'a', b't', b'a', b':', b' ', 0xF0, 0x9F];
        let part2 = vec![0x9A, 0x80, b'\n', b'\n'];

        let mut events = parser.process(&part1);
        assert_eq!(events.len(), 0);

        events.extend(parser.process(&part2));
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].data, "🚀");
    }

    #[test]
    fn test_bom_ignored() {
        let mut parser = SseParser::new();
        // BOM: EF BB BF
        let mut input = vec![0xEF, 0xBB, 0xBF];
        input.extend_from_slice(b"data: hello\n\n");

        let events = parser.process(&input);
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].data, "hello");
    }

    #[test]
    fn test_bom_split() {
        let mut parser = SseParser::new();
        // BOM split: EF, BB, BF
        let events = parser.process(&[0xEF]);
        assert!(events.is_empty());

        let events = parser.process(&[0xBB]);
        assert!(events.is_empty());

        let events = parser.process(&[0xBF]);
        assert!(events.is_empty());

        let events = parser.process(b"data: hello\n\n");
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].data, "hello");
    }

    #[test]
    fn test_not_bom_start() {
        let mut parser = SseParser::new();
        // Starts with EF but not BOM
        let events = parser.process(&[0xEF, 0x41]); // EF A
        // Should be treated as data (invalid utf8 maybe, or just bytes)
        // But since we wait for newline, nothing happens yet.
        assert!(events.is_empty());

        let events = parser.process(b"\n\n");
        // The line was "\u{FFFD}A" (replacement char for EF) or similar depending on lossy conversion
        // But since it doesn't have "data:", it's ignored or field name.
        // "EF A" -> field name "ïA" (latin1) or replacement. Value empty.
        // Event is empty.
        assert!(events.is_empty());
    }

    #[test]
    fn test_event_to_bytes() {
        let event = SseEvent {
            id: Some("1".to_string()),
            event: Some("message".to_string()),
            data: "hello\nworld".to_string(),
            retry: Some(123),
        };

        let expected = "id: 1\nevent: message\nretry: 123\ndata: hello\ndata: world\n\n";
        assert_eq!(event.to_bytes(), expected.as_bytes());
    }
}
