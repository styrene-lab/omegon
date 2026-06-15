//! Read tool — file contents with offset/limit support.

use anyhow::Result;
use omegon_traits::{ContentBlock, ToolResult};
use std::path::Path;

const MAX_LINES: usize = 2000;
const MAX_BYTES: usize = 50 * 1024;

/// Read timeout — 30 seconds should handle any local file system.
/// Network-mounted filesystems that stall will hit this.
const READ_TIMEOUT_SECS: u64 = 30;

pub async fn execute(
    path: &Path,
    offset: Option<usize>,
    limit: Option<usize>,
) -> Result<ToolResult> {
    if !path.exists() {
        anyhow::bail!("File not found: {}", path.display());
    }

    let timeout = std::time::Duration::from_secs(READ_TIMEOUT_SECS);

    // Check if it's an image
    if is_image(path) {
        let data = tokio::time::timeout(timeout, tokio::fs::read(path))
            .await
            .map_err(|_| {
                anyhow::anyhow!(
                    "Read timed out after {READ_TIMEOUT_SECS}s: {}",
                    path.display()
                )
            })??;
        let base64 = base64_encode(&data);
        let media_type = mime_from_ext(path);
        return Ok(ToolResult {
            content: vec![ContentBlock::Image {
                url: format!("data:{media_type};base64,{base64}"),
                media_type: media_type.clone(),
            }],
            details: serde_json::json!({
                "path": path.display().to_string(),
                "bytes": data.len(),
                "media_type": media_type,
                "rendered": true,
            }),
        });
    }

    let data = tokio::time::timeout(timeout, tokio::fs::read(path))
        .await
        .map_err(|_| {
            anyhow::anyhow!(
                "Read timed out after {READ_TIMEOUT_SECS}s: {}",
                path.display()
            )
        })??;
    if looks_binary(&data) {
        anyhow::bail!(
            "Cannot read {} as text: file appears to be binary ({} bytes). Use `view` for supported images or `bash`/`xxd` for byte-level inspection.",
            path.display(),
            data.len()
        );
    }
    let content = String::from_utf8(data).map_err(|err| {
        anyhow::anyhow!(
            "Cannot read {} as UTF-8 text: invalid byte sequence starting at byte {}. Use `bash`/`file`/`xxd` for byte-level inspection, or convert the file to UTF-8 before using `read`.",
            path.display(),
            err.utf8_error().valid_up_to()
        )
    })?;
    let lines: Vec<&str> = content.lines().collect();
    let total_lines = lines.len();

    let start = offset.unwrap_or(1).saturating_sub(1); // 1-indexed to 0-indexed
    let max = limit.unwrap_or(MAX_LINES).min(MAX_LINES);

    let selected: Vec<&str> = lines.iter().skip(start).take(max).copied().collect();

    let mut text = selected.join("\n");

    // Truncate by bytes if needed, but only at UTF-8 character boundaries.
    if text.len() > MAX_BYTES {
        text.truncate(text.floor_char_boundary(MAX_BYTES));
        if let Some(last_newline) = text.rfind('\n') {
            text.truncate(last_newline);
        }
    }

    let shown_lines = text.lines().count();
    let remaining = total_lines.saturating_sub(start + shown_lines);

    if remaining > 0 {
        text.push_str(&format!(
            "\n\n[{remaining} more lines in file. Use offset={} to continue.]",
            start + shown_lines + 1
        ));
    }

    Ok(ToolResult {
        content: vec![ContentBlock::Text { text }],
        details: serde_json::json!({
            "path": path.display().to_string(),
            "totalLines": total_lines,
            "shownLines": shown_lines,
            "offset": start + 1,
        }),
    })
}

fn is_image(path: &Path) -> bool {
    matches!(
        path.extension().and_then(|e| e.to_str()),
        Some("jpg" | "jpeg" | "png" | "gif" | "webp" | "svg")
    )
}

fn mime_from_ext(path: &Path) -> String {
    match path.extension().and_then(|e| e.to_str()) {
        Some("jpg" | "jpeg") => "image/jpeg".to_string(),
        Some("png") => "image/png".to_string(),
        Some("gif") => "image/gif".to_string(),
        Some("webp") => "image/webp".to_string(),
        Some("svg") => "image/svg+xml".to_string(),
        _ => "application/octet-stream".to_string(),
    }
}

fn looks_binary(data: &[u8]) -> bool {
    data.iter().take(8192).any(|byte| *byte == 0)
}

fn base64_encode(data: &[u8]) -> String {
    use std::io::Write;
    let mut buf = Vec::with_capacity(data.len() * 4 / 3 + 4);
    let mut encoder = Base64Encoder::new(&mut buf);
    encoder.write_all(data).unwrap();
    encoder.finish().unwrap();
    String::from_utf8(buf).unwrap()
}

struct Base64Encoder<W: std::io::Write> {
    writer: W,
    buf: [u8; 3],
    len: usize,
}

const B64_CHARS: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

impl<W: std::io::Write> Base64Encoder<W> {
    fn new(writer: W) -> Self {
        Self {
            writer,
            buf: [0; 3],
            len: 0,
        }
    }

    fn finish(mut self) -> std::io::Result<W> {
        if self.len > 0 {
            for i in self.len..3 {
                self.buf[i] = 0;
            }
            let mut out = [b'='; 4];
            out[0] = B64_CHARS[((self.buf[0] >> 2) & 0x3F) as usize];
            out[1] = B64_CHARS[(((self.buf[0] & 0x03) << 4) | (self.buf[1] >> 4)) as usize];
            if self.len > 1 {
                out[2] = B64_CHARS[(((self.buf[1] & 0x0F) << 2) | (self.buf[2] >> 6)) as usize];
            }
            if self.len > 2 {
                out[3] = B64_CHARS[(self.buf[2] & 0x3F) as usize];
            }
            self.writer.write_all(&out)?;
        }
        Ok(self.writer)
    }
}

impl<W: std::io::Write> std::io::Write for Base64Encoder<W> {
    fn write(&mut self, data: &[u8]) -> std::io::Result<usize> {
        let mut consumed = 0;
        for &byte in data {
            self.buf[self.len] = byte;
            self.len += 1;
            if self.len == 3 {
                let out = [
                    B64_CHARS[((self.buf[0] >> 2) & 0x3F) as usize],
                    B64_CHARS[(((self.buf[0] & 0x03) << 4) | (self.buf[1] >> 4)) as usize],
                    B64_CHARS[(((self.buf[1] & 0x0F) << 2) | (self.buf[2] >> 6)) as usize],
                    B64_CHARS[(self.buf[2] & 0x3F) as usize],
                ];
                self.writer.write_all(&out)?;
                self.len = 0;
            }
            consumed += 1;
        }
        Ok(consumed)
    }

    fn flush(&mut self) -> std::io::Result<()> {
        self.writer.flush()
    }
}

// ─── Tests ──────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn image_detection() {
        assert!(is_image(Path::new("photo.jpg")));
        assert!(is_image(Path::new("icon.png")));
        assert!(is_image(Path::new("logo.svg")));
        assert!(is_image(Path::new("anim.gif")));
        assert!(is_image(Path::new("hero.webp")));
        assert!(!is_image(Path::new("code.rs")));
        assert!(!is_image(Path::new("readme.md")));
        assert!(!is_image(Path::new("data.json")));
    }

    #[test]
    fn mime_types() {
        assert_eq!(mime_from_ext(Path::new("a.jpg")), "image/jpeg");
        assert_eq!(mime_from_ext(Path::new("a.jpeg")), "image/jpeg");
        assert_eq!(mime_from_ext(Path::new("a.png")), "image/png");
        assert_eq!(mime_from_ext(Path::new("a.gif")), "image/gif");
        assert_eq!(mime_from_ext(Path::new("a.webp")), "image/webp");
        assert_eq!(mime_from_ext(Path::new("a.svg")), "image/svg+xml");
        assert_eq!(
            mime_from_ext(Path::new("a.txt")),
            "application/octet-stream"
        );
    }

    #[test]
    fn base64_encode_basic() {
        assert_eq!(base64_encode(b""), "");
        assert_eq!(base64_encode(b"f"), "Zg==");
        assert_eq!(base64_encode(b"fo"), "Zm8=");
        assert_eq!(base64_encode(b"foo"), "Zm9v");
        assert_eq!(base64_encode(b"foobar"), "Zm9vYmFy");
        assert_eq!(base64_encode(b"Hello, World!"), "SGVsbG8sIFdvcmxkIQ==");
    }

    #[tokio::test]
    async fn read_with_offset_and_limit() {
        let dir = std::env::temp_dir().join("omegon-test-read");
        let _ = std::fs::create_dir_all(&dir);
        let file = dir.join("test.txt");
        std::fs::write(&file, "line1\nline2\nline3\nline4\nline5\n").unwrap();

        // Read all
        let result = execute(&file, None, None).await.unwrap();
        let text = match &result.content[0] {
            ContentBlock::Text { text } => text.clone(),
            _ => panic!("expected text"),
        };
        assert!(text.contains("line1"));
        assert!(text.contains("line5"));

        // Offset 3 (1-indexed) = start from line3
        let result = execute(&file, Some(3), None).await.unwrap();
        let text = match &result.content[0] {
            ContentBlock::Text { text } => text.clone(),
            _ => panic!("expected text"),
        };
        assert!(!text.contains("line1"));
        assert!(!text.contains("line2"));
        assert!(text.contains("line3"));

        // Limit 2
        let result = execute(&file, Some(1), Some(2)).await.unwrap();
        let text = match &result.content[0] {
            ContentBlock::Text { text } => text.clone(),
            _ => panic!("expected text"),
        };
        assert!(text.contains("line1"));
        assert!(text.contains("line2"));
        assert!(!text.contains("line3"));

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[tokio::test]
    async fn read_nonexistent_file() {
        let result = execute(Path::new("/tmp/omegon-nonexistent-file.xyz"), None, None).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn read_non_utf8_file_reports_path_and_offset() {
        let dir =
            std::env::temp_dir().join(format!("omegon-test-read-nonutf8-{}", std::process::id()));
        let _ = std::fs::create_dir_all(&dir);
        let file = dir.join("latin1.txt");
        std::fs::write(&file, b"abc\xffdef").unwrap();

        let err = execute(&file, None, None).await.unwrap_err();
        let message = err.to_string();
        assert!(message.contains("latin1.txt"), "{message}");
        assert!(message.contains("UTF-8"), "{message}");
        assert!(message.contains("byte 3"), "{message}");

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[tokio::test]
    async fn read_binary_file_reports_path() {
        let dir =
            std::env::temp_dir().join(format!("omegon-test-read-binary-{}", std::process::id()));
        let _ = std::fs::create_dir_all(&dir);
        let file = dir.join("data.bin");
        std::fs::write(&file, b"abc\0def").unwrap();

        let err = execute(&file, None, None).await.unwrap_err();
        let message = err.to_string();
        assert!(message.contains("data.bin"), "{message}");
        assert!(message.contains("binary"), "{message}");

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[tokio::test]
    async fn read_shows_remaining_count() {
        let dir = std::env::temp_dir().join("omegon-test-read-remaining");
        let _ = std::fs::create_dir_all(&dir);
        let file = dir.join("big.txt");
        let content: String = (1..=100).map(|i| format!("line {i}\n")).collect();
        std::fs::write(&file, &content).unwrap();

        let result = execute(&file, Some(1), Some(5)).await.unwrap();
        let text = match &result.content[0] {
            ContentBlock::Text { text } => text.clone(),
            _ => panic!("expected text"),
        };
        assert!(
            text.contains("more lines in file"),
            "should show remaining: {text}"
        );
        assert!(text.contains("offset=6"), "should suggest next offset");

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[tokio::test]
    async fn read_offset_near_end_of_file() {
        let dir = std::env::temp_dir().join("omegon-test-read-tail");
        let _ = std::fs::create_dir_all(&dir);
        let file = dir.join("large.txt");
        let content: String = (1..=1500).map(|i| format!("line {i}\n")).collect();
        std::fs::write(&file, &content).unwrap();

        // Read near the end — offset 1495 (1-indexed), limit 3
        let result = execute(&file, Some(1495), Some(3)).await.unwrap();
        let text = match &result.content[0] {
            ContentBlock::Text { text } => text.clone(),
            _ => panic!("expected text"),
        };
        assert!(
            text.contains("line 1495"),
            "should start at line 1495: {text}"
        );
        assert!(
            text.contains("line 1497"),
            "should include line 1497: {text}"
        );
        assert!(
            !text.starts_with("line 1\n"),
            "must not reset to beginning: {text}"
        );
        assert!(
            !text.contains("line 1498"),
            "should respect limit=3: {text}"
        );

        // Read the very last line
        let result = execute(&file, Some(1500), Some(1)).await.unwrap();
        let text = match &result.content[0] {
            ContentBlock::Text { text } => text.clone(),
            _ => panic!("expected text"),
        };
        assert!(
            text.contains("line 1500"),
            "should return last line: {text}"
        );
        assert!(
            !text.contains("line 1499"),
            "should not include prior line: {text}"
        );

        // Offset beyond EOF — should return empty (no crash, no reset)
        let result = execute(&file, Some(2000), Some(5)).await.unwrap();
        let text = match &result.content[0] {
            ContentBlock::Text { text } => text.clone(),
            _ => panic!("expected text"),
        };
        assert!(
            !text.starts_with("line 1\n"),
            "must not reset to beginning on OOB offset: {text}"
        );

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[tokio::test]
    async fn byte_truncation_handles_emoji_boundary() {
        let dir =
            std::env::temp_dir().join(format!("omegon-test-read-emoji-{}", std::process::id()));
        let _ = std::fs::create_dir_all(&dir);
        let file = dir.join("emoji-large.txt");
        let content = format!("{}✓tail", "x".repeat(MAX_BYTES - 1));
        std::fs::write(&file, content).unwrap();

        let result = execute(&file, None, None).await.unwrap();
        let text = match &result.content[0] {
            ContentBlock::Text { text } => text,
            _ => panic!("expected text"),
        };
        assert!(text.is_char_boundary(text.len()));

        let _ = std::fs::remove_dir_all(&dir);
    }
}
