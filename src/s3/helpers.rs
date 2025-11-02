use std::collections::HashMap;
use std::path::Path;

/// Detect Content-Type based on file extension
///
/// Returns the MIME type for common file formats. Falls back to
/// "application/octet-stream" for unknown types.
#[allow(dead_code)] // Ready for Phase 5 integration
pub fn detect_content_type(path: &Path) -> String {
    match path.extension().and_then(|e| e.to_str()) {
        // Video formats
        Some("mp4") => "video/mp4",
        Some("mov") => "video/quicktime",
        Some("avi") => "video/x-msvideo",
        Some("mkv") => "video/x-matroska",
        Some("webm") => "video/webm",
        Some("flv") => "video/x-flv",
        Some("wmv") => "video/x-ms-wmv",
        Some("m4v") => "video/x-m4v",

        // Image formats
        Some("jpg") | Some("jpeg") => "image/jpeg",
        Some("png") => "image/png",
        Some("gif") => "image/gif",
        Some("bmp") => "image/bmp",
        Some("svg") => "image/svg+xml",
        Some("webp") => "image/webp",
        Some("ico") => "image/x-icon",
        Some("tif") | Some("tiff") => "image/tiff",

        // Audio formats
        Some("mp3") => "audio/mpeg",
        Some("wav") => "audio/wav",
        Some("ogg") => "audio/ogg",
        Some("flac") => "audio/flac",
        Some("aac") => "audio/aac",
        Some("m4a") => "audio/mp4",

        // Document formats
        Some("pdf") => "application/pdf",
        Some("doc") => "application/msword",
        Some("docx") => "application/vnd.openxmlformats-officedocument.wordprocessingml.document",
        Some("xls") => "application/vnd.ms-excel",
        Some("xlsx") => "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet",
        Some("ppt") => "application/vnd.ms-powerpoint",
        Some("pptx") => "application/vnd.openxmlformats-officedocument.presentationml.presentation",

        // Text formats
        Some("txt") => "text/plain",
        Some("html") | Some("htm") => "text/html",
        Some("css") => "text/css",
        Some("js") => "text/javascript",
        Some("json") => "application/json",
        Some("xml") => "application/xml",
        Some("csv") => "text/csv",
        Some("md") => "text/markdown",

        // Archive formats
        Some("zip") => "application/zip",
        Some("tar") => "application/x-tar",
        Some("gz") => "application/gzip",
        Some("bz2") => "application/x-bzip2",
        Some("7z") => "application/x-7z-compressed",
        Some("rar") => "application/vnd.rar",

        // Binary/executable
        Some("exe") => "application/x-msdownload",
        Some("dmg") => "application/x-apple-diskimage",
        Some("iso") => "application/x-iso9660-image",

        // Default
        _ => "application/octet-stream",
    }
    .to_string()
}

/// Parse metadata string into HashMap
///
/// Expected format: "key1=value1,key2=value2"
///
/// # Examples
///
/// ```
/// let metadata = parse_metadata("author=John,project=Demo");
/// assert_eq!(metadata.get("author"), Some(&"John".to_string()));
/// ```
#[allow(dead_code)] // Ready for Phase 5 integration
pub fn parse_metadata(metadata_str: &str) -> HashMap<String, String> {
    metadata_str
        .split(',')
        .filter_map(|pair| {
            let mut parts = pair.split('=');
            let key = parts.next()?.trim();
            let value = parts.next()?.trim();

            if key.is_empty() || value.is_empty() {
                None
            } else {
                Some((key.to_string(), value.to_string()))
            }
        })
        .collect()
}

/// Parse tags string into HashMap
///
/// Expected format: "key1=value1,key2=value2"
///
/// S3 tags have restrictions:
/// - Tag keys and values are case sensitive
/// - Maximum key length: 128 characters
/// - Maximum value length: 256 characters
#[allow(dead_code)] // Ready for Phase 5 integration
pub fn parse_tags(tags_str: &str) -> HashMap<String, String> {
    tags_str
        .split(',')
        .filter_map(|pair| {
            let mut parts = pair.split('=');
            let key = parts.next()?.trim();
            let value = parts.next()?.trim();

            // Validate tag constraints
            if key.is_empty() || value.is_empty() {
                return None;
            }

            if key.len() > 128 || value.len() > 256 {
                eprintln!(
                    "Warning: Tag key '{}' or value '{}' exceeds AWS limits (key: 128, value: 256 chars). Skipping.",
                    key, value
                );
                return None;
            }

            Some((key.to_string(), value.to_string()))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn test_detect_content_type_video() {
        assert_eq!(
            detect_content_type(&PathBuf::from("video.mp4")),
            "video/mp4"
        );
        assert_eq!(
            detect_content_type(&PathBuf::from("movie.mov")),
            "video/quicktime"
        );
        assert_eq!(
            detect_content_type(&PathBuf::from("clip.avi")),
            "video/x-msvideo"
        );
    }

    #[test]
    fn test_detect_content_type_image() {
        assert_eq!(
            detect_content_type(&PathBuf::from("photo.jpg")),
            "image/jpeg"
        );
        assert_eq!(
            detect_content_type(&PathBuf::from("image.png")),
            "image/png"
        );
    }

    #[test]
    fn test_detect_content_type_unknown() {
        assert_eq!(
            detect_content_type(&PathBuf::from("file.unknown")),
            "application/octet-stream"
        );
        assert_eq!(
            detect_content_type(&PathBuf::from("no_extension")),
            "application/octet-stream"
        );
    }

    #[test]
    fn test_parse_metadata() {
        let metadata = parse_metadata("author=John Doe,project=Demo,version=1.0");

        assert_eq!(metadata.len(), 3);
        assert_eq!(metadata.get("author"), Some(&"John Doe".to_string()));
        assert_eq!(metadata.get("project"), Some(&"Demo".to_string()));
        assert_eq!(metadata.get("version"), Some(&"1.0".to_string()));
    }

    #[test]
    fn test_parse_metadata_empty() {
        let metadata = parse_metadata("");
        assert_eq!(metadata.len(), 0);
    }

    #[test]
    fn test_parse_metadata_malformed() {
        let metadata = parse_metadata("author=John,invalid,project=");

        // Should only parse valid key=value pairs
        assert_eq!(metadata.len(), 1);
        assert_eq!(metadata.get("author"), Some(&"John".to_string()));
    }

    #[test]
    fn test_parse_tags() {
        let tags = parse_tags("env=prod,type=video");

        assert_eq!(tags.len(), 2);
        assert_eq!(tags.get("env"), Some(&"prod".to_string()));
        assert_eq!(tags.get("type"), Some(&"video".to_string()));
    }

    #[test]
    fn test_parse_tags_length_validation() {
        // Tag key too long (> 128 chars)
        let long_key = format!("{}=value", "k".repeat(129));
        let tags = parse_tags(&long_key);
        assert_eq!(tags.len(), 0);

        // Tag value too long (> 256 chars)
        let long_value = format!("key={}", "v".repeat(257));
        let tags = parse_tags(&long_value);
        assert_eq!(tags.len(), 0);
    }
}
