use bytes::{Buf, BufMut, BytesMut};
use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::config::SseResponseConfig;

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
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

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SseFrame {
    Event(SseEvent),
    NonDispatch,
}

#[derive(Clone, Debug, Error, PartialEq, Eq)]
pub enum SseParseError {
    #[error("SSE stream contains invalid UTF-8")]
    InvalidUtf8,
    #[error("SSE line exceeded {limit_bytes} bytes (observed at least {observed_bytes})")]
    LineLimitExceeded {
        limit_bytes: usize,
        observed_bytes: usize,
    },
    #[error("SSE event exceeded {limit_bytes} bytes (observed at least {observed_bytes})")]
    EventLimitExceeded {
        limit_bytes: usize,
        observed_bytes: usize,
    },
    #[error(
        "SSE retained buffer exceeded {limit_bytes} bytes (observed at least {observed_bytes})"
    )]
    BufferLimitExceeded {
        limit_bytes: usize,
        observed_bytes: usize,
    },
    #[error("SSE frame count exceeded {limit_frames} frames (observed at least {observed_frames})")]
    FrameCountLimitExceeded {
        limit_frames: u64,
        observed_frames: u64,
    },
    #[error("SSE parser was fed after EOF")]
    FeedAfterFinish,
}

#[derive(Debug)]
pub struct SseParser {
    limits: SseResponseConfig,
    buffer: BytesMut,
    current_event: SseEvent,
    current_event_bytes: usize,
    current_has_data: bool,
    frame_count: u64,
    is_start: bool,
    finishing: bool,
    finished: bool,
}

impl SseParser {
    pub fn new(limits: SseResponseConfig) -> Self {
        Self {
            limits,
            buffer: BytesMut::new(),
            current_event: SseEvent::default(),
            current_event_bytes: 0,
            current_has_data: false,
            frame_count: 0,
            is_start: true,
            finishing: false,
            finished: false,
        }
    }

    pub fn feed(&mut self, chunk: &[u8]) -> Result<Option<SseFrame>, SseParseError> {
        if self.finishing || self.finished {
            return Err(SseParseError::FeedAfterFinish);
        }
        self.append_chunk(chunk)?;
        if !self.resolve_bom(false)? {
            return Ok(None);
        }
        self.next_frame(false)
    }

    pub fn finish(&mut self) -> Result<Option<SseFrame>, SseParseError> {
        if self.finished {
            return Ok(None);
        }
        self.finishing = true;
        self.resolve_bom(true)?;
        if let Some(frame) = self.next_frame(true)? {
            return Ok(Some(frame));
        }

        if std::str::from_utf8(&self.buffer).is_err() {
            return Err(SseParseError::InvalidUtf8);
        }
        self.buffer.clear();
        self.current_event = SseEvent::default();
        self.current_event_bytes = 0;
        self.current_has_data = false;
        self.finished = true;
        Ok(None)
    }

    pub const fn frame_count(&self) -> u64 {
        self.frame_count
    }

    fn append_chunk(&mut self, chunk: &[u8]) -> Result<(), SseParseError> {
        let retained = self
            .current_event_bytes
            .checked_add(self.buffer.len())
            .and_then(|value| value.checked_add(chunk.len()))
            .unwrap_or(usize::MAX);
        if retained > self.limits.buffer_limit_bytes {
            return Err(SseParseError::BufferLimitExceeded {
                limit_bytes: self.limits.buffer_limit_bytes,
                observed_bytes: self.limits.buffer_limit_bytes.saturating_add(1),
            });
        }
        self.buffer.extend_from_slice(chunk);
        Ok(())
    }

    fn resolve_bom(&mut self, eof: bool) -> Result<bool, SseParseError> {
        if !self.is_start {
            return Ok(true);
        }
        const BOM: &[u8; 3] = b"\xef\xbb\xbf";
        if self.buffer.len() < BOM.len()
            && self.buffer.as_ref() == &BOM[..self.buffer.len()]
            && !eof
        {
            return Ok(false);
        }
        if self.buffer.starts_with(BOM) {
            self.buffer.advance(BOM.len());
        }
        self.is_start = false;
        Ok(true)
    }

    fn next_frame(&mut self, eof: bool) -> Result<Option<SseFrame>, SseParseError> {
        loop {
            let Some((line_end, terminator_bytes)) = self.next_line_boundary(eof) else {
                self.enforce_pending_line_limit()?;
                return Ok(None);
            };
            if line_end > self.limits.line_limit_bytes {
                return Err(SseParseError::LineLimitExceeded {
                    limit_bytes: self.limits.line_limit_bytes,
                    observed_bytes: self.limits.line_limit_bytes.saturating_add(1),
                });
            }
            let consumed = line_end.checked_add(terminator_bytes).unwrap_or(usize::MAX);
            let event_bytes = self
                .current_event_bytes
                .checked_add(consumed)
                .unwrap_or(usize::MAX);
            if event_bytes > self.limits.event_limit_bytes {
                return Err(SseParseError::EventLimitExceeded {
                    limit_bytes: self.limits.event_limit_bytes,
                    observed_bytes: self.limits.event_limit_bytes.saturating_add(1),
                });
            }

            let line_bytes = self.buffer.split_to(line_end);
            self.buffer.advance(terminator_bytes);
            let line = std::str::from_utf8(&line_bytes).map_err(|_| SseParseError::InvalidUtf8)?;
            self.current_event_bytes = event_bytes;

            if line.is_empty() {
                self.frame_count = self.frame_count.checked_add(1).unwrap_or(u64::MAX);
                if self.frame_count > self.limits.frame_count_limit {
                    return Err(SseParseError::FrameCountLimitExceeded {
                        limit_frames: self.limits.frame_count_limit,
                        observed_frames: self.limits.frame_count_limit.saturating_add(1),
                    });
                }
                let event = std::mem::take(&mut self.current_event);
                let dispatch = self.current_has_data;
                self.current_has_data = false;
                self.current_event_bytes = 0;
                return Ok(Some(if dispatch {
                    SseFrame::Event(event)
                } else {
                    SseFrame::NonDispatch
                }));
            }
            self.parse_line(line);
        }
    }

    fn next_line_boundary(&self, eof: bool) -> Option<(usize, usize)> {
        for (index, byte) in self.buffer.iter().copied().enumerate() {
            match byte {
                b'\n' => return Some((index, 1)),
                b'\r' if index + 1 < self.buffer.len() => {
                    return Some((
                        index,
                        if self.buffer[index + 1] == b'\n' {
                            2
                        } else {
                            1
                        },
                    ));
                }
                b'\r' if eof => return Some((index, 1)),
                b'\r' => return None,
                _ => {}
            }
        }
        None
    }

    fn enforce_pending_line_limit(&self) -> Result<(), SseParseError> {
        let pending_line_bytes = self
            .buffer
            .iter()
            .position(|byte| *byte == b'\r')
            .unwrap_or(self.buffer.len());
        if pending_line_bytes > self.limits.line_limit_bytes {
            return Err(SseParseError::LineLimitExceeded {
                limit_bytes: self.limits.line_limit_bytes,
                observed_bytes: self.limits.line_limit_bytes.saturating_add(1),
            });
        }
        Ok(())
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
                let had_data = self.current_has_data;
                self.current_has_data = true;
                if had_data {
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

    fn limits(line: usize, event: usize, buffer: usize, frames: u64) -> SseResponseConfig {
        SseResponseConfig {
            line_limit_bytes: line,
            event_limit_bytes: event,
            buffer_limit_bytes: buffer,
            frame_count_limit: frames,
        }
    }

    fn generous_limits() -> SseResponseConfig {
        limits(1024, 4096, 8192, 1024)
    }

    fn drain_feed(
        parser: &mut SseParser,
        chunk: &[u8],
        frames: &mut Vec<SseFrame>,
    ) -> Result<(), SseParseError> {
        let mut next = parser.feed(chunk)?;
        while let Some(frame) = next {
            frames.push(frame);
            next = parser.feed(&[])?;
        }
        Ok(())
    }

    fn collect(
        chunks: &[&[u8]],
        limits: SseResponseConfig,
    ) -> Result<Vec<SseFrame>, SseParseError> {
        let mut parser = SseParser::new(limits);
        let mut frames = Vec::new();
        for chunk in chunks {
            drain_feed(&mut parser, chunk, &mut frames)?;
        }
        while let Some(frame) = parser.finish()? {
            frames.push(frame);
        }
        Ok(frames)
    }

    fn events(frames: &[SseFrame]) -> Vec<&SseEvent> {
        frames
            .iter()
            .filter_map(|frame| match frame {
                SseFrame::Event(event) => Some(event),
                SseFrame::NonDispatch => None,
            })
            .collect()
    }

    #[test]
    fn valid_fields_comments_multiline_bom_and_line_endings_are_preserved() {
        let input = b"\xef\xbb\xbf: comment\r\nid: 123\revent: update\nretry: 1000\r\ndata: line1\ndata: line2\r\n\r\n";
        let frames = collect(&[input], generous_limits()).unwrap();
        let parsed = events(&frames);
        assert_eq!(frames.len(), 1);
        assert_eq!(parsed.len(), 1);
        assert_eq!(parsed[0].id.as_deref(), Some("123"));
        assert_eq!(parsed[0].event.as_deref(), Some("update"));
        assert_eq!(parsed[0].retry, Some(1000));
        assert_eq!(parsed[0].data, "line1\nline2");
    }

    #[test]
    fn every_blank_line_counts_a_frame_but_only_data_fields_dispatch() {
        let input = b":comment\n\nunknown: value\n\nid: 1\n\ndata:\n\ndata: value\n\n\n";
        let frames = collect(&[input], generous_limits()).unwrap();
        assert_eq!(frames.len(), 6);
        assert!(matches!(frames[0], SseFrame::NonDispatch));
        assert!(matches!(frames[1], SseFrame::NonDispatch));
        assert!(matches!(frames[2], SseFrame::NonDispatch));
        assert!(matches!(&frames[3], SseFrame::Event(event) if event.data.is_empty()));
        assert!(matches!(&frames[4], SseFrame::Event(event) if event.data == "value"));
        assert!(matches!(frames[5], SseFrame::NonDispatch));
    }

    #[test]
    fn split_bom_crlf_and_utf8_codepoint_are_chunk_invariant() {
        let chunks: Vec<&[u8]> = vec![
            &[0xef],
            &[0xbb],
            &[0xbf, b'd', b'a', b't', b'a', b':', b' '],
            &[0xf0, 0x9f],
            &[0x9a, 0x80, b'\r'],
            &[b'\n', b'\r'],
            &[b'\n'],
        ];
        let frames = collect(&chunks, generous_limits()).unwrap();
        assert!(matches!(&frames[0], SseFrame::Event(event) if event.data == "🚀"));
    }

    #[test]
    fn invalid_utf8_fails_in_line_across_chunks_and_at_eof_without_replacement() {
        assert_eq!(
            collect(&[b"data: \xff\n\n"], generous_limits()),
            Err(SseParseError::InvalidUtf8)
        );
        assert_eq!(
            collect(&[b"data: \xf0\x9f", b"x\n\n"], generous_limits()),
            Err(SseParseError::InvalidUtf8)
        );
        assert_eq!(
            collect(&[b"data: \xf0\x9f"], generous_limits()),
            Err(SseParseError::InvalidUtf8)
        );
    }

    #[test]
    fn line_event_buffer_and_frame_limits_allow_exact_and_reject_plus_one() {
        assert!(collect(&[b"data:x\n\n"], limits(6, 8, 8, 1)).is_ok());
        assert_eq!(
            collect(&[b"data:xx\n\n"], limits(6, 9, 9, 1)),
            Err(SseParseError::LineLimitExceeded {
                limit_bytes: 6,
                observed_bytes: 7,
            })
        );

        assert!(collect(&[b"data:x\n\n"], limits(6, 8, 8, 1)).is_ok());
        assert_eq!(
            collect(&[b"data:xx\n\n"], limits(7, 8, 9, 1)),
            Err(SseParseError::EventLimitExceeded {
                limit_bytes: 8,
                observed_bytes: 9,
            })
        );

        let mut parser = SseParser::new(limits(6, 6, 6, 1));
        assert_eq!(parser.feed(b"data:x").unwrap(), None);
        let mut parser = SseParser::new(limits(7, 7, 6, 1));
        assert_eq!(
            parser.feed(b"data:xx"),
            Err(SseParseError::BufferLimitExceeded {
                limit_bytes: 6,
                observed_bytes: 7,
            })
        );

        assert_eq!(collect(&[b"\n\n"], limits(1, 1, 2, 2)).unwrap().len(), 2);
        assert_eq!(
            collect(&[b"\n\n\n"], limits(1, 1, 3, 2)),
            Err(SseParseError::FrameCountLimitExceeded {
                limit_frames: 2,
                observed_frames: 3,
            })
        );
    }

    #[test]
    fn terminators_count_toward_event_and_retained_limits() {
        assert!(collect(&[b"data:x\r\n\r\n"], limits(6, 10, 10, 1)).is_ok());
        assert_eq!(
            collect(&[b"data:x\r\n\r\n"], limits(6, 9, 10, 1)),
            Err(SseParseError::EventLimitExceeded {
                limit_bytes: 9,
                observed_bytes: 10,
            })
        );
    }

    #[test]
    fn eof_drops_unterminated_event_but_terminal_cr_can_complete_a_blank_line() {
        assert!(events(&collect(&[b"data: value\n"], generous_limits()).unwrap()).is_empty());
        let frames = collect(&[b"data: value\r\r"], generous_limits()).unwrap();
        assert!(matches!(&frames[0], SseFrame::Event(event) if event.data == "value"));
        let frames = collect(&[b"data: value\n\n"], generous_limits()).unwrap();
        assert!(matches!(&frames[0], SseFrame::Event(event) if event.data == "value"));
    }

    #[test]
    fn whole_single_byte_and_fixed_random_splits_have_identical_results() {
        let input = b"data: one\n\n:comment\n\ndata: two\ndata: three\r\n\r\n";
        let whole = collect(&[input], generous_limits()).unwrap();
        let single = input.iter().map(std::slice::from_ref).collect::<Vec<_>>();
        assert_eq!(collect(&single, generous_limits()).unwrap(), whole);

        let mut chunks = Vec::new();
        let mut offset = 0;
        let widths = [3usize, 1, 7, 2, 5];
        let mut index = 0;
        while offset < input.len() {
            let end = (offset + widths[index % widths.len()]).min(input.len());
            chunks.push(&input[offset..end]);
            offset = end;
            index += 1;
        }
        assert_eq!(collect(&chunks, generous_limits()).unwrap(), whole);

        let invalid = b"data: valid\ndata: \xf0\x9fbroken\n\n";
        let whole_error = collect(&[invalid], generous_limits()).unwrap_err();
        let single = invalid.iter().map(std::slice::from_ref).collect::<Vec<_>>();
        assert_eq!(
            collect(&single, generous_limits()).unwrap_err(),
            whole_error
        );
    }

    #[test]
    fn feed_and_finish_each_deliver_at_most_one_frame_per_call() {
        let mut parser = SseParser::new(generous_limits());
        assert!(matches!(
            parser.feed(b"data: one\n\ndata: two\n\n").unwrap(),
            Some(SseFrame::Event(_))
        ));
        assert!(matches!(
            parser.feed(&[]).unwrap(),
            Some(SseFrame::Event(_))
        ));
        assert_eq!(parser.feed(&[]).unwrap(), None);
        assert_eq!(parser.finish().unwrap(), None);
        assert_eq!(parser.frame_count(), 2);
        assert_eq!(
            parser.feed(b"data: late\n\n"),
            Err(SseParseError::FeedAfterFinish)
        );
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
