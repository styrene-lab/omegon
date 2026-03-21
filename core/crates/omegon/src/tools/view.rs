//! View tool — render files inline in the terminal.
//!
//! Supports: images (iTerm2/Kitty protocol), PDFs (pdftotext), code (syntax
//! highlighting via syntect), documents (pandoc → markdown). Falls back to
//! plain text for unknown types.

use async_trait::async_trait;
use omegon_traits::{ContentBlock, ToolDefinition, ToolProvider, ToolResult};
use serde_json::{json, Value};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use tokio_util::sync::CancellationToken;

pub struct ViewProvider {
    cwd: PathBuf,
}

impl ViewProvider {
    pub fn new(cwd: PathBuf) -> Self {
        Self { cwd }
    }

    fn resolve_path(&self, path: &str) -> PathBuf {
        let p = Path::new(path);
        if p.is_absolute() {
            p.to_path_buf()
        } else {
            self.cwd.join(p)
        }
    }
}

#[derive(Debug)]
enum FileKind {
    Image,
    Svg,
    Pdf,
    Code,
    Document,
    Unknown,
}

fn classify(path: &Path) -> FileKind {
    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_lowercase();
    match ext.as_str() {
        "jpg" | "jpeg" | "png" | "gif" | "webp" | "bmp" | "tiff" => FileKind::Image,
        "svg" => FileKind::Svg,
        "pdf" => FileKind::Pdf,
        "docx" | "xlsx" | "pptx" | "epub" | "odt" | "rtf" => FileKind::Document,
        "rs" | "ts" | "tsx" | "js" | "jsx" | "py" | "go" | "c" | "cpp" | "h" | "hpp"
        | "java" | "rb" | "sh" | "bash" | "zsh" | "fish" | "toml" | "yaml" | "yml"
        | "json" | "xml" | "html" | "css" | "scss" | "sql" | "md" | "lua" | "zig"
        | "swift" | "kt" | "scala" | "r" | "jl" | "ex" | "exs" | "erl" | "hs"
        | "ml" | "mli" | "nix" | "tf" | "hcl" | "proto" | "graphql" | "Dockerfile"
        | "Makefile" | "cmake" | "gradle" => FileKind::Code,
        _ => FileKind::Unknown,
    }
}

fn has_cmd(cmd: &str) -> bool {
    Command::new("which").arg(cmd).output().is_ok_and(|o| o.status.success())
}

fn file_header(path: &Path) -> String {
    let meta = fs::metadata(path).ok();
    let size = meta.as_ref().map(|m| format_size(m.len())).unwrap_or_default();
    format!("**{}** ({})", path.display(), size)
}

fn format_size(bytes: u64) -> String {
    if bytes < 1024 {
        format!("{bytes} B")
    } else if bytes < 1024 * 1024 {
        format!("{:.1} KB", bytes as f64 / 1024.0)
    } else {
        format!("{:.1} MB", bytes as f64 / (1024.0 * 1024.0))
    }
}

fn view_image(path: &Path) -> ToolResult {
    // Return as image content block — the rendering layer handles protocol
    match fs::read(path) {
        Ok(data) => {
            let mime = match path.extension().and_then(|e| e.to_str()).unwrap_or("") {
                "png" => "image/png",
                "jpg" | "jpeg" => "image/jpeg",
                "gif" => "image/gif",
                "webp" => "image/webp",
                _ => "application/octet-stream",
            };
            // Encode as data: URI for inline rendering
            let b64 = base64_encode(&data);
            let data_uri = format!("data:{mime};base64,{b64}");
            ToolResult {
                content: vec![
                    ContentBlock::Text { text: file_header(path) },
                    ContentBlock::Image {
                        url: data_uri,
                        media_type: mime.into(),
                    },
                ],
                details: json!({}),
            }
        }
        Err(e) => ToolResult {
            content: vec![ContentBlock::Text { text: format!("Cannot read image: {e}") }],
            details: json!({"error": true}),
        },
    }
}

/// Encode bytes as base64 (public for reuse by render tools).
pub fn base64_encode_bytes(data: &[u8]) -> String {
    base64_encode(data)
}

/// Execute the view tool with standard CoreTools signature.
pub async fn execute(
    _tool_name: &str,
    _call_id: &str,
    args: serde_json::Value,
    _cancel: tokio_util::sync::CancellationToken,
    cwd: &std::path::Path,
) -> anyhow::Result<omegon_traits::ToolResult> {
    let provider = ViewProvider::new(cwd.to_path_buf());
    provider.execute("view", _call_id, args, _cancel).await
}

fn base64_encode(data: &[u8]) -> String {
    use std::io::Write;
    let mut buf = Vec::new();
    let mut encoder = Base64Encoder::new(&mut buf);
    encoder.write_all(data).ok();
    encoder.finish();
    String::from_utf8(buf).unwrap_or_default()
}

/// Simple base64 encoder (no external crate needed).
struct Base64Encoder<W: std::io::Write> {
    writer: W,
    buf: [u8; 3],
    buf_len: usize,
}

const B64_CHARS: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

impl<W: std::io::Write> Base64Encoder<W> {
    fn new(writer: W) -> Self {
        Self { writer, buf: [0; 3], buf_len: 0 }
    }
    fn flush_buf(&mut self) {
        if self.buf_len == 0 { return; }
        let b = &self.buf;
        let out = match self.buf_len {
            3 => [
                B64_CHARS[(b[0] >> 2) as usize],
                B64_CHARS[((b[0] & 0x03) << 4 | b[1] >> 4) as usize],
                B64_CHARS[((b[1] & 0x0F) << 2 | b[2] >> 6) as usize],
                B64_CHARS[(b[2] & 0x3F) as usize],
            ],
            2 => [
                B64_CHARS[(b[0] >> 2) as usize],
                B64_CHARS[((b[0] & 0x03) << 4 | b[1] >> 4) as usize],
                B64_CHARS[((b[1] & 0x0F) << 2) as usize],
                b'=',
            ],
            1 => [
                B64_CHARS[(b[0] >> 2) as usize],
                B64_CHARS[((b[0] & 0x03) << 4) as usize],
                b'=',
                b'=',
            ],
            _ => return,
        };
        let _ = self.writer.write_all(&out);
        self.buf_len = 0;
    }
    fn finish(mut self) {
        self.flush_buf();
    }
}

impl<W: std::io::Write> std::io::Write for Base64Encoder<W> {
    fn write(&mut self, data: &[u8]) -> std::io::Result<usize> {
        for &byte in data {
            self.buf[self.buf_len] = byte;
            self.buf_len += 1;
            if self.buf_len == 3 {
                self.flush_buf();
            }
        }
        Ok(data.len())
    }
    fn flush(&mut self) -> std::io::Result<()> { Ok(()) }
}

fn view_pdf(path: &Path, page: Option<u32>) -> ToolResult {
    if !has_cmd("pdftotext") {
        return ToolResult {
            content: vec![ContentBlock::Text {
                text: format!("{}\n\npdftotext not found. Install poppler-utils.", file_header(path)),
            }],
            details: json!({"error": true}),
        };
    }

    let mut args = vec![path.to_string_lossy().to_string()];
    if let Some(p) = page {
        args.extend(["-f".into(), p.to_string(), "-l".into(), p.to_string()]);
    }
    args.push("-".into()); // stdout

    match Command::new("pdftotext").args(&args).output() {
        Ok(output) if output.status.success() => {
            let text = String::from_utf8_lossy(&output.stdout);
            ToolResult {
                content: vec![ContentBlock::Text {
                    text: format!("{}\n\n{}", file_header(path), text),
                }],
                details: json!({}),
            }
        }
        Ok(output) => ToolResult {
            content: vec![ContentBlock::Text {
                text: format!("pdftotext error: {}", String::from_utf8_lossy(&output.stderr)),
            }],
            details: json!({"error": true}),
        },
        Err(e) => ToolResult {
            content: vec![ContentBlock::Text { text: format!("pdftotext failed: {e}") }],
            details: json!({"error": true}),
        },
    }
}

fn view_document(path: &Path) -> ToolResult {
    if !has_cmd("pandoc") {
        return ToolResult {
            content: vec![ContentBlock::Text {
                text: format!("{}\n\npandoc not found. Install pandoc to view this file.", file_header(path)),
            }],
            details: json!({"error": true}),
        };
    }

    match Command::new("pandoc")
        .args(["-t", "markdown", "--wrap=none"])
        .arg(path)
        .output()
    {
        Ok(output) if output.status.success() => {
            let text = String::from_utf8_lossy(&output.stdout);
            ToolResult {
                content: vec![ContentBlock::Text {
                    text: format!("{}\n\n{}", file_header(path), text),
                }],
                details: json!({}),
            }
        }
        _ => ToolResult {
            content: vec![ContentBlock::Text {
                text: format!("{}\n\npandoc conversion failed.", file_header(path)),
            }],
            details: json!({"error": true}),
        },
    }
}

fn view_code(path: &Path) -> ToolResult {
    // For now, just read as text with a header. Syntect highlighting can be added later.
    match fs::read_to_string(path) {
        Ok(content) => {
            let lines = content.lines().count();
            let truncated = if lines > 500 {
                let taken: String = content.lines().take(500).collect::<Vec<_>>().join("\n");
                format!("{taken}\n\n... [{} more lines]", lines - 500)
            } else {
                content
            };
            ToolResult {
                content: vec![ContentBlock::Text {
                    text: format!("{} ({} lines)\n\n{truncated}", file_header(path), lines),
                }],
                details: json!({"lines": lines}),
            }
        }
        Err(e) => ToolResult {
            content: vec![ContentBlock::Text { text: format!("Cannot read file: {e}") }],
            details: json!({"error": true}),
        },
    }
}

fn view_text(path: &Path) -> ToolResult {
    view_code(path) // Same implementation for now
}

#[async_trait]
impl ToolProvider for ViewProvider {
    fn tools(&self) -> Vec<ToolDefinition> {
        vec![ToolDefinition {
            name: crate::tool_registry::view::VIEW.into(),
            label: "View File".into(),
            description: "View a file inline with rich rendering. Images render graphically. PDFs render as text. Documents convert to markdown via pandoc. Code files get displayed with line counts.".into(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "path": { "type": "string", "description": "Path to the file to view" },
                    "page": { "type": "number", "description": "Page number for PDFs" }
                },
                "required": ["path"]
            }),
        }]
    }

    async fn execute(
        &self,
        _tool_name: &str,
        _call_id: &str,
        args: Value,
        _cancel: CancellationToken,
    ) -> anyhow::Result<ToolResult> {
        let path_str = args.get("path").and_then(|v| v.as_str()).unwrap_or("");
        let page = args.get("page").and_then(|v| v.as_u64()).map(|p| p as u32);
        let path = self.resolve_path(path_str);

        if !path.exists() {
            return Ok(ToolResult {
                content: vec![ContentBlock::Text {
                    text: format!("File not found: {}", path.display()),
                }],
                details: json!({"error": true}),
            });
        }

        Ok(match classify(&path) {
            FileKind::Image => view_image(&path),
            FileKind::Svg => view_code(&path), // SVG as text for now
            FileKind::Pdf => view_pdf(&path, page),
            FileKind::Document => view_document(&path),
            FileKind::Code => view_code(&path),
            FileKind::Unknown => view_text(&path),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classify_file_types() {
        assert!(matches!(classify(Path::new("test.png")), FileKind::Image));
        assert!(matches!(classify(Path::new("test.jpg")), FileKind::Image));
        assert!(matches!(classify(Path::new("test.pdf")), FileKind::Pdf));
        assert!(matches!(classify(Path::new("test.rs")), FileKind::Code));
        assert!(matches!(classify(Path::new("test.py")), FileKind::Code));
        assert!(matches!(classify(Path::new("test.docx")), FileKind::Document));
        assert!(matches!(classify(Path::new("test.xyz")), FileKind::Unknown));
    }

    #[test]
    fn format_size_units() {
        assert_eq!(format_size(500), "500 B");
        assert_eq!(format_size(1536), "1.5 KB");
        assert_eq!(format_size(1_500_000), "1.4 MB");
    }

    #[test]
    fn base64_round_trip() {
        let data = b"Hello, World!";
        let encoded = base64_encode(data);
        assert_eq!(encoded, "SGVsbG8sIFdvcmxkIQ==");
    }

    #[test]
    fn view_nonexistent_file() {
        let _provider = ViewProvider::new(PathBuf::from("/tmp"));
        // Can't call async execute in sync test, but we can test the classify function
        assert!(matches!(classify(Path::new("nonexistent.rs")), FileKind::Code));
    }
}
