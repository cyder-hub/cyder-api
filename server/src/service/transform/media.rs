use base64::{Engine as _, engine::general_purpose::STANDARD};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum InputAudioFormat {
    Wav,
    Mp3,
}

impl InputAudioFormat {
    pub(crate) const fn as_str(self) -> &'static str {
        match self {
            Self::Wav => "wav",
            Self::Mp3 => "mp3",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum InlineMediaKind {
    Image,
    Audio(InputAudioFormat),
    File,
    Video,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct ParsedBase64DataUrl<'a> {
    pub(crate) mime_type: &'a str,
    pub(crate) data: &'a str,
}

pub(crate) fn classify_inline_mime(mime_type: &str) -> Option<InlineMediaKind> {
    match mime_type.trim().to_ascii_lowercase().as_str() {
        "image/png" | "image/jpeg" | "image/jpg" | "image/webp" | "image/gif" => {
            Some(InlineMediaKind::Image)
        }
        "audio/wav" | "audio/x-wav" | "audio/wave" => {
            Some(InlineMediaKind::Audio(InputAudioFormat::Wav))
        }
        "audio/mpeg" | "audio/mp3" => Some(InlineMediaKind::Audio(InputAudioFormat::Mp3)),
        "application/pdf" | "text/plain" | "text/markdown" | "text/csv" | "application/json" => {
            Some(InlineMediaKind::File)
        }
        value if value.starts_with("video/") => Some(InlineMediaKind::Video),
        _ => None,
    }
}

pub(crate) fn mime_type_from_filename(filename: &str) -> Option<&'static str> {
    let extension = filename.rsplit_once('.')?.1.to_ascii_lowercase();
    match extension.as_str() {
        "pdf" => Some("application/pdf"),
        "txt" => Some("text/plain"),
        "md" | "markdown" => Some("text/markdown"),
        "csv" => Some("text/csv"),
        "json" => Some("application/json"),
        _ => None,
    }
}

pub(crate) fn default_filename_for_mime(mime_type: &str) -> Option<&'static str> {
    match mime_type {
        "application/pdf" => Some("document.pdf"),
        "text/plain" => Some("document.txt"),
        "text/markdown" => Some("document.md"),
        "text/csv" => Some("document.csv"),
        "application/json" => Some("document.json"),
        _ => None,
    }
}

pub(crate) fn is_valid_base64(data: &str) -> bool {
    !data.is_empty() && STANDARD.decode(data).is_ok()
}

pub(crate) fn parse_base64_data_url(value: &str) -> Option<ParsedBase64DataUrl<'_>> {
    let rest = value.strip_prefix("data:")?;
    let (mime_type, data) = rest.split_once(";base64,")?;
    if mime_type.is_empty() || !is_valid_base64(data) {
        return None;
    }
    Some(ParsedBase64DataUrl { mime_type, data })
}

pub(crate) fn is_valid_http_url(value: &str) -> bool {
    reqwest::Url::parse(value)
        .is_ok_and(|url| matches!(url.scheme(), "http" | "https") && url.host_str().is_some())
}

pub(crate) fn is_valid_image_reference(value: &str) -> bool {
    if is_valid_http_url(value) {
        return true;
    }
    parse_base64_data_url(value).is_some_and(|data_url| {
        classify_inline_mime(data_url.mime_type) == Some(InlineMediaKind::Image)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn media_contract_accepts_only_registered_inline_formats() {
        assert_eq!(
            classify_inline_mime("image/png"),
            Some(InlineMediaKind::Image)
        );
        assert_eq!(
            classify_inline_mime("audio/mpeg"),
            Some(InlineMediaKind::Audio(InputAudioFormat::Mp3))
        );
        assert_eq!(
            classify_inline_mime("application/pdf"),
            Some(InlineMediaKind::File)
        );
        assert_eq!(
            classify_inline_mime("video/mp4"),
            Some(InlineMediaKind::Video)
        );
        assert_eq!(classify_inline_mime("application/octet-stream"), None);
    }

    #[test]
    fn image_references_require_http_or_valid_image_data_urls() {
        assert!(is_valid_image_reference("https://example.com/image.png"));
        assert!(is_valid_image_reference("data:image/png;base64,ZmFrZQ=="));
        assert!(!is_valid_image_reference("file:///tmp/image.png"));
        assert!(!is_valid_image_reference("data:video/mp4;base64,ZmFrZQ=="));
        assert!(!is_valid_image_reference(
            "data:image/png;base64,not-base64"
        ));
    }
}
