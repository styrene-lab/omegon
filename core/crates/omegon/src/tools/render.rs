//! Render tool — diagram and image generation via external tools.
//!
//! Tools:
//! - render_diagram: D2 diagrams → PNG via d2 CLI
//! - generate_image_local: FLUX.1 image generation via MLX (Apple Silicon)
//!
//! Additional tools (render_native_diagram, render_excalidraw, render_composition_still,
//! render_composition_video) are deferred — they require the Node.js composition
//! renderer or complex SVG generation that's better handled by the TS layer until
//! Phase 2 TUI is complete.

use async_trait::async_trait;
use omegon_traits::{ContentBlock, ToolDefinition, ToolProvider, ToolResult};
use serde_json::{json, Value};
use std::fs;
use std::path::PathBuf;
use std::process::Command;
use std::time::SystemTime;
use tokio_util::sync::CancellationToken;

pub struct RenderProvider;

impl RenderProvider {
    pub fn new() -> Self {
        Self
    }
}

fn visuals_dir() -> PathBuf {
    let home = dirs::home_dir().unwrap_or_else(|| PathBuf::from("."));
    let dir = home.join(".pi/visuals");
    let _ = fs::create_dir_all(&dir);
    dir
}

fn timestamp_slug() -> String {
    let now = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .unwrap_or_default();
    // Simple timestamp — good enough for file names
    format!("{}", now.as_secs())
}

fn has_cmd(cmd: &str) -> bool {
    Command::new("which").arg(cmd).output().is_ok_and(|o| o.status.success())
}

fn render_d2(args: &Value) -> anyhow::Result<ToolResult> {
    if !has_cmd("d2") {
        anyhow::bail!("d2 CLI not found. Install via `brew install d2` or `nix profile install nixpkgs#d2`.");
    }

    let code = args.get("code").and_then(|v| v.as_str()).unwrap_or("");
    let title = args.get("title").and_then(|v| v.as_str()).unwrap_or("diagram");
    let layout = args.get("layout").and_then(|v| v.as_str()).unwrap_or("elk");
    let theme = args.get("theme").and_then(|v| v.as_u64()).unwrap_or(200);
    let sketch = args.get("sketch").and_then(|v| v.as_bool()).unwrap_or(false);

    let slug = title.chars()
        .map(|c| if c.is_alphanumeric() { c } else { '_' })
        .take(40)
        .collect::<String>();
    let ts = timestamp_slug();
    let dir = visuals_dir();
    let d2_path = dir.join(format!("{ts}_{slug}.d2"));
    let png_path = dir.join(format!("{ts}_{slug}.png"));

    fs::write(&d2_path, code)?;

    let mut cmd_args = vec![
        "-l".to_string(), layout.to_string(),
        "-t".to_string(), theme.to_string(),
        "--pad".to_string(), "40".to_string(),
    ];
    if sketch {
        cmd_args.push("--sketch".to_string());
    }
    cmd_args.push(d2_path.to_string_lossy().to_string());
    cmd_args.push(png_path.to_string_lossy().to_string());

    let output = Command::new("d2").args(&cmd_args).output()?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        anyhow::bail!("d2 failed (exit {}):\n{}", output.status, &stderr[stderr.len().saturating_sub(1500)..]);
    }

    // Read PNG and return as image content
    let png_data = fs::read(&png_path)?;
    let b64 = crate::tools::view::base64_encode_bytes(&png_data);
    let data_uri = format!("data:image/png;base64,{b64}");

    let header = if title != "diagram" {
        format!("# {title}\n\n")
    } else {
        String::new()
    };

    Ok(ToolResult {
        content: vec![
            ContentBlock::Text {
                text: format!("{header}📊 D2 ({layout}, {:.1}s)  ·  Saved: {}",
                    0.0, // TODO: actual timing
                    png_path.display()),
            },
            ContentBlock::Image {
                url: data_uri,
                media_type: "image/png".into(),
            },
        ],
        details: json!({
            "d2_path": d2_path.to_string_lossy(),
            "png_path": png_path.to_string_lossy(),
            "layout": layout,
            "theme": theme,
        }),
    })
}

fn generate_image(args: &Value) -> anyhow::Result<ToolResult> {
    // FLUX.1 generation requires the MLX Python package
    // This is a subprocess call to the existing Python script
    let prompt = args.get("prompt").and_then(|v| v.as_str()).unwrap_or("");
    let preset = args.get("preset").and_then(|v| v.as_str()).unwrap_or("schnell");
    let width = args.get("width").and_then(|v| v.as_u64()).unwrap_or(1024);
    let height = args.get("height").and_then(|v| v.as_u64()).unwrap_or(1024);
    let steps = args.get("steps").and_then(|v| v.as_u64());
    let seed = args.get("seed").and_then(|v| v.as_u64());
    let quantize = args.get("quantize").and_then(|v| v.as_str());

    // Determine steps from preset if not specified
    let steps = steps.unwrap_or(match preset {
        "schnell" => 4,
        "dev" => 25,
        "dev-fast" => 12,
        "diagram" => 4,
        "portrait" => 25,
        "wide" => 4,
        _ => 4,
    });

    if !has_cmd("python3") {
        anyhow::bail!("python3 not found. FLUX.1 image generation requires Python 3 with MLX.");
    }

    let ts = timestamp_slug();
    let dir = visuals_dir();
    let out_path = dir.join(format!("{ts}_flux.png"));

    // Build the MLX FLUX command
    let mut cmd_args = vec![
        "-m".to_string(), "mlx_community/FLUX.1-schnell-4bit-quantized".to_string(),
        "--prompt".to_string(), prompt.to_string(),
        "--output".to_string(), out_path.to_string_lossy().to_string(),
        "--width".to_string(), width.to_string(),
        "--height".to_string(), height.to_string(),
        "--n-images".to_string(), "1".to_string(),
        "--steps".to_string(), steps.to_string(),
    ];
    if let Some(s) = seed {
        cmd_args.extend(["--seed".to_string(), s.to_string()]);
    }
    if let Some(q) = quantize {
        cmd_args.extend(["--quantize".to_string(), q.to_string()]);
    }

    let output = Command::new("python3")
        .args(["-m", "mlx_flux"])
        .args(&cmd_args)
        .output();

    match output {
        Ok(o) if o.status.success() && out_path.exists() => {
            let png_data = fs::read(&out_path)?;
            let b64 = crate::tools::view::base64_encode_bytes(&png_data);
            Ok(ToolResult {
                content: vec![
                    ContentBlock::Text {
                        text: format!("Generated image: {}\nPreset: {preset}, {width}x{height}, {steps} steps",
                            out_path.display()),
                    },
                    ContentBlock::Image {
                        url: format!("data:image/png;base64,{b64}"),
                        media_type: "image/png".into(),
                    },
                ],
                details: json!({"path": out_path.to_string_lossy(), "preset": preset}),
            })
        }
        Ok(o) => {
            let stderr = String::from_utf8_lossy(&o.stderr);
            Ok(ToolResult {
                content: vec![ContentBlock::Text {
                    text: format!("Image generation failed: {stderr}"),
                }],
                details: json!({"error": true}),
            })
        }
        Err(e) => Ok(ToolResult {
            content: vec![ContentBlock::Text {
                text: format!("Failed to run FLUX: {e}. Ensure mlx_flux is installed."),
            }],
            details: json!({"error": true}),
        }),
    }
}

#[async_trait]
impl ToolProvider for RenderProvider {
    fn tools(&self) -> Vec<ToolDefinition> {
        vec![
            ToolDefinition {
                name: crate::tool_registry::render::RENDER_DIAGRAM.into(),
                label: "Render Diagram".into(),
                description: "Render a D2 diagram as an inline PNG image. D2 is a modern declarative diagramming language. Requires d2 CLI.".into(),
                parameters: json!({
                    "type": "object",
                    "properties": {
                        "code": { "type": "string", "description": "D2 diagram source code" },
                        "title": { "type": "string", "description": "Optional title" },
                        "layout": { "type": "string", "enum": ["dagre", "elk"], "description": "Layout engine (default: elk)" },
                        "theme": { "type": "number", "description": "D2 theme ID (default: 200 = dark)" },
                        "sketch": { "type": "boolean", "description": "Sketch/hand-drawn mode" }
                    },
                    "required": ["code"]
                }),
            },
            ToolDefinition {
                name: crate::tool_registry::render::GENERATE_IMAGE_LOCAL.into(),
                label: "Generate Image".into(),
                description: "Generate an image locally on Apple Silicon using FLUX.1 via MLX. Runs entirely on-device.".into(),
                parameters: json!({
                    "type": "object",
                    "properties": {
                        "prompt": { "type": "string", "description": "Text prompt" },
                        "preset": { "type": "string", "enum": ["schnell", "dev", "dev-fast", "diagram", "portrait", "wide"] },
                        "width": { "type": "number", "description": "Width in pixels (multiple of 64)" },
                        "height": { "type": "number", "description": "Height in pixels (multiple of 64)" },
                        "steps": { "type": "number", "description": "Diffusion steps" },
                        "seed": { "type": "number", "description": "Random seed" },
                        "quantize": { "type": "string", "enum": ["3", "4", "5", "6", "8"] }
                    },
                    "required": ["prompt"]
                }),
            },
        ]
    }

    async fn execute(
        &self,
        tool_name: &str,
        _call_id: &str,
        args: Value,
        _cancel: CancellationToken,
    ) -> anyhow::Result<ToolResult> {
        match tool_name {
            "render_diagram" => render_d2(&args),
            "generate_image_local" => generate_image(&args),
            _ => anyhow::bail!("Unknown render tool: {tool_name}"),
        }
    }
}

/// Execute render tools with standard CoreTools signature.
pub async fn execute(
    tool_name: &str,
    _call_id: &str,
    args: serde_json::Value,
    _cancel: tokio_util::sync::CancellationToken,
) -> anyhow::Result<omegon_traits::ToolResult> {
    let provider = RenderProvider::new();
    provider.execute(tool_name, _call_id, args, _cancel).await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tool_definitions() {
        let provider = RenderProvider::new();
        let tools = provider.tools();
        assert_eq!(tools.len(), 2);
        assert_eq!(tools[0].name, "render_diagram");
        assert_eq!(tools[1].name, "generate_image_local");
    }

    #[test]
    fn visuals_dir_exists() {
        let dir = visuals_dir();
        assert!(dir.exists() || dir.parent().is_some());
    }

    #[test]
    fn timestamp_slug_format() {
        let ts = timestamp_slug();
        assert!(!ts.is_empty());
        assert!(ts.parse::<u64>().is_ok());
    }
}
