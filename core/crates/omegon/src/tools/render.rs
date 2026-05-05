//! render_diagram — Render D2, Mermaid, GraphViz, or PlantUML diagrams to PNG/SVG.
//!
//! Detects available CLI backends at first call and selects the right one
//! based on the `format` parameter or auto-detection from source content.
//! Output is saved to ~/.omegon/visuals/ for persistence across sessions.

use anyhow::{Result, anyhow};
use async_trait::async_trait;
use omegon_traits::{ContentBlock, ToolCapability, ToolDefinition, ToolProvider, ToolResult};
use serde_json::json;
use std::path::{Path, PathBuf};
use std::process::Command;

use crate::tool_registry::render as reg;

/// Tool provider for deterministic diagram rendering.
pub struct RenderProvider;

impl RenderProvider {
    pub fn new() -> Self {
        Self
    }
}

#[async_trait]
impl ToolProvider for RenderProvider {
    fn tools(&self) -> Vec<ToolDefinition> {
        vec![ToolDefinition {
            name: reg::RENDER_DIAGRAM.into(),
            label: reg::RENDER_DIAGRAM.into(),
            description: tool_description(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "source": {
                        "type": "string",
                        "description": "Diagram source code (D2, Mermaid, GraphViz, or PlantUML)"
                    },
                    "format": {
                        "type": "string",
                        "description": "Diagram format: d2, mermaid, graphviz, plantuml (auto-detected if omitted)"
                    },
                    "title": {
                        "type": "string",
                        "description": "Optional title — used for the filename and image header"
                    },
                    "output_path": {
                        "type": "string",
                        "description": "Where to write the image (default: ~/.omegon/visuals/)"
                    },
                    "output_format": {
                        "type": "string",
                        "description": "Output image format: png or svg (default: png)"
                    }
                },
                "required": ["source"]
            }),
            capabilities: vec![ToolCapability::StateChanging],
        }]
    }

    async fn execute(
        &self,
        tool_name: &str,
        _call_id: &str,
        args: serde_json::Value,
        _cancel: tokio_util::sync::CancellationToken,
    ) -> Result<ToolResult> {
        match tool_name {
            reg::RENDER_DIAGRAM => {
                let source = args["source"]
                    .as_str()
                    .ok_or_else(|| anyhow!("missing 'source' argument"))?;
                let format = args["format"].as_str();
                let title = args["title"].as_str();
                let output_path = args["output_path"].as_str();
                let output_format = args["output_format"].as_str();
                execute(source, format, title, output_path, output_format).await
            }
            _ => Err(anyhow!("unknown tool: {tool_name}")),
        }
    }
}

/// Output directory for rendered visuals.
fn visuals_dir() -> PathBuf {
    dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("/tmp"))
        .join(".omegon/visuals")
}

fn ensure_visuals_dir() -> Result<PathBuf> {
    let dir = visuals_dir();
    std::fs::create_dir_all(&dir)?;
    Ok(dir)
}

fn timestamp() -> String {
    chrono::Utc::now().format("%Y%m%dT%H%M%S").to_string()
}

/// Check if a CLI tool is available.
fn has_cmd(cmd: &str) -> bool {
    Command::new(cmd)
        .arg("--version")
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .is_ok_and(|s| s.success())
}

/// Detect which diagram backends are available.
pub fn detect_backends() -> Vec<(&'static str, &'static str)> {
    let mut available = Vec::new();
    if has_cmd("d2") {
        available.push(("d2", "D2 — architecture, flowcharts, ER, sequence diagrams"));
    }
    if has_cmd("mmdc") {
        available.push((
            "mermaid",
            "Mermaid — flowcharts, sequence, class, state diagrams",
        ));
    }
    if has_cmd("dot") {
        available.push(("graphviz", "GraphViz — directed/undirected graphs"));
    }
    if has_cmd("plantuml") {
        available.push(("plantuml", "PlantUML — UML diagrams"));
    }
    available
}

/// Build the tool description dynamically based on available backends.
pub fn tool_description() -> String {
    let backends = detect_backends();
    if backends.is_empty() {
        return "Render a diagram to PNG/SVG. No diagram backends are currently installed. \
                Install one: `brew install d2` (recommended), `npm install -g @mermaid-js/mermaid-cli`, \
                `brew install graphviz`, or `brew install plantuml`."
            .to_string();
    }
    let list = backends
        .iter()
        .map(|(name, desc)| format!("  - {name}: {desc}"))
        .collect::<Vec<_>>()
        .join("\n");
    format!(
        "Render a diagram from source code to a PNG or SVG image file. \
         Output is saved to ~/.omegon/visuals/ for persistence.\n\n\
         Available backends:\n{list}\n\n\
         The format is auto-detected from source content if not specified."
    )
}

/// Auto-detect diagram format from source content.
/// Order matters: most specific formats checked first, D2 last (most permissive).
fn detect_format(source: &str) -> Option<&'static str> {
    let trimmed = source.trim();

    // PlantUML: @startuml (most distinctive marker)
    if trimmed.starts_with("@startuml") {
        return Some("plantuml");
    }

    // GraphViz: starts with digraph/strict, or "graph {" with "--" edges
    if trimmed.starts_with("digraph ") || trimmed.starts_with("strict ") {
        return Some("graphviz");
    }
    if trimmed.starts_with("graph ") && trimmed.contains('{') && trimmed.contains("--") {
        return Some("graphviz");
    }

    // Mermaid: starts with a diagram type keyword
    if trimmed.starts_with("graph ")
        || trimmed.starts_with("flowchart ")
        || trimmed.starts_with("sequenceDiagram")
        || trimmed.starts_with("classDiagram")
        || trimmed.starts_with("stateDiagram")
        || trimmed.starts_with("erDiagram")
        || trimmed.starts_with("gantt")
        || trimmed.starts_with("pie")
        || trimmed.starts_with("gitGraph")
    {
        return Some("mermaid");
    }

    // D2: most permissive — arrows, colons, or braces with structure
    if trimmed.contains("->") || trimmed.contains(':') {
        return Some("d2");
    }

    None
}

/// Render a D2 diagram via the `d2` CLI.
fn render_d2(source: &str, output_path: &Path, output_format: &str) -> Result<()> {
    let tmp_input = output_path.with_extension("d2");
    std::fs::write(&tmp_input, source)?;

    let mut args = vec![
        "-l".to_string(),
        "elk".to_string(),
        "-t".to_string(),
        "200".to_string(), // dark theme
        "--pad".to_string(),
        "40".to_string(),
    ];

    if output_format == "svg" {
        // d2 outputs SVG by default when output has .svg extension
        let svg_out = output_path.with_extension("svg");
        args.push(tmp_input.display().to_string());
        args.push(svg_out.display().to_string());
    } else {
        args.push(tmp_input.display().to_string());
        args.push(output_path.display().to_string());
    }

    let result = Command::new("d2").args(&args).output()?;

    // Clean up temp input
    let _ = std::fs::remove_file(&tmp_input);

    if !result.status.success() {
        let stderr = String::from_utf8_lossy(&result.stderr);
        return Err(anyhow!("d2 failed (exit {}):\n{}", result.status, stderr));
    }
    Ok(())
}

/// Render a Mermaid diagram via the `mmdc` CLI.
fn render_mermaid(source: &str, output_path: &Path, output_format: &str) -> Result<()> {
    let tmp_input = output_path.with_extension("mmd");
    std::fs::write(&tmp_input, source)?;

    let ext = if output_format == "svg" { "svg" } else { "png" };
    let out = output_path.with_extension(ext);

    let result = Command::new("mmdc")
        .args([
            "-i",
            &tmp_input.display().to_string(),
            "-o",
            &out.display().to_string(),
            "-t",
            "dark",
            "-b",
            "transparent",
        ])
        .output()?;

    let _ = std::fs::remove_file(&tmp_input);

    if !result.status.success() {
        let stderr = String::from_utf8_lossy(&result.stderr);
        return Err(anyhow!("mmdc failed (exit {}):\n{}", result.status, stderr));
    }
    Ok(())
}

/// Render a GraphViz diagram via the `dot` CLI.
fn render_graphviz(source: &str, output_path: &Path, output_format: &str) -> Result<()> {
    let fmt_flag = if output_format == "svg" {
        "-Tsvg"
    } else {
        "-Tpng"
    };
    let result = Command::new("dot")
        .args([fmt_flag, "-o", &output_path.display().to_string()])
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .and_then(|mut child| {
            use std::io::Write;
            if let Some(ref mut stdin) = child.stdin {
                stdin.write_all(source.as_bytes())?;
            }
            child.wait_with_output()
        })?;

    if !result.status.success() {
        let stderr = String::from_utf8_lossy(&result.stderr);
        return Err(anyhow!("dot failed (exit {}):\n{}", result.status, stderr));
    }
    Ok(())
}

/// Render a PlantUML diagram via the `plantuml` CLI.
fn render_plantuml(source: &str, output_path: &Path, output_format: &str) -> Result<()> {
    let tmp_input = output_path.with_extension("puml");
    std::fs::write(&tmp_input, source)?;

    let fmt_flag = if output_format == "svg" {
        "-tsvg"
    } else {
        "-tpng"
    };
    let result = Command::new("plantuml")
        .args([fmt_flag, &tmp_input.display().to_string()])
        .output()?;

    let _ = std::fs::remove_file(&tmp_input);

    if !result.status.success() {
        let stderr = String::from_utf8_lossy(&result.stderr);
        return Err(anyhow!(
            "plantuml failed (exit {}):\n{}",
            result.status,
            stderr
        ));
    }

    // PlantUML outputs to the same directory with the diagram extension
    let expected_out = tmp_input.with_extension(if output_format == "svg" { "svg" } else { "png" });
    if expected_out != *output_path && expected_out.exists() {
        std::fs::rename(&expected_out, output_path)?;
    }
    Ok(())
}

/// Execute the render_diagram tool.
pub async fn execute(
    source: &str,
    format: Option<&str>,
    title: Option<&str>,
    output_path: Option<&str>,
    output_format: Option<&str>,
) -> Result<ToolResult> {
    let fmt = format
        .or_else(|| detect_format(source))
        .ok_or_else(|| {
            anyhow!(
                "Could not auto-detect diagram format. Specify format: d2, mermaid, graphviz, or plantuml."
            )
        })?;

    // Verify backend is installed
    let cmd = match fmt {
        "d2" => "d2",
        "mermaid" => "mmdc",
        "graphviz" => "dot",
        "plantuml" => "plantuml",
        other => {
            return Err(anyhow!(
                "Unknown diagram format: {other}. Use: d2, mermaid, graphviz, plantuml."
            ));
        }
    };
    if !has_cmd(cmd) {
        let install_hint = match fmt {
            "d2" => "brew install d2",
            "mermaid" => "npm install -g @mermaid-js/mermaid-cli",
            "graphviz" => "brew install graphviz",
            "plantuml" => "brew install plantuml",
            _ => "",
        };
        return Err(anyhow!(
            "{cmd} is not installed. Install it with: {install_hint}"
        ));
    }

    let out_fmt = output_format.unwrap_or("png");
    let ext = if out_fmt == "svg" { "svg" } else { "png" };

    // Determine output path
    let out_path = if let Some(p) = output_path {
        PathBuf::from(p)
    } else {
        let dir = ensure_visuals_dir()?;
        let slug = title
            .unwrap_or("diagram")
            .chars()
            .map(|c| if c.is_alphanumeric() { c } else { '_' })
            .take(40)
            .collect::<String>();
        dir.join(format!("{}_{}.{}", timestamp(), slug, ext))
    };

    // Create parent dirs
    if let Some(parent) = out_path.parent() {
        std::fs::create_dir_all(parent)?;
    }

    // Render
    let start = std::time::Instant::now();
    match fmt {
        "d2" => render_d2(source, &out_path, out_fmt)?,
        "mermaid" => render_mermaid(source, &out_path, out_fmt)?,
        "graphviz" => render_graphviz(source, &out_path, out_fmt)?,
        "plantuml" => render_plantuml(source, &out_path, out_fmt)?,
        _ => unreachable!(),
    }
    let elapsed = start.elapsed().as_secs_f64();

    // Read output and encode as data URI for inline display
    let file_data = std::fs::read(&out_path)?;
    let mime = if out_fmt == "svg" {
        "image/svg+xml"
    } else {
        "image/png"
    };

    let title_prefix = title.map(|t| format!("{t} — ")).unwrap_or_default();
    let summary = format!(
        "{title_prefix}{fmt} diagram ({out_fmt}, {elapsed:.1}s). Saved: {}",
        out_path.display()
    );

    use base64::Engine;
    let b64 = base64::engine::general_purpose::STANDARD.encode(&file_data);

    Ok(ToolResult {
        content: vec![
            ContentBlock::Text { text: summary },
            ContentBlock::Image {
                url: format!("data:{mime};base64,{b64}"),
                media_type: mime.to_string(),
            },
        ],
        details: serde_json::json!({
            "format": fmt,
            "output_format": out_fmt,
            "output_path": out_path.display().to_string(),
            "elapsed_secs": elapsed,
        }),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detect_d2_syntax() {
        assert_eq!(detect_format("a -> b: hello"), Some("d2"));
        assert_eq!(
            detect_format("server: {\n  shape: rectangle\n}"),
            Some("d2")
        );
    }

    #[test]
    fn detect_mermaid_syntax() {
        assert_eq!(detect_format("graph TD\n  A --> B"), Some("mermaid"));
        assert_eq!(detect_format("flowchart LR\n  A --> B"), Some("mermaid"));
        assert_eq!(
            detect_format("sequenceDiagram\n  Alice->>Bob: Hello"),
            Some("mermaid")
        );
    }

    #[test]
    fn detect_graphviz_syntax() {
        assert_eq!(detect_format("digraph G {\n  a -> b;\n}"), Some("graphviz"));
    }

    #[test]
    fn detect_plantuml_syntax() {
        assert_eq!(
            detect_format("@startuml\nAlice -> Bob\n@enduml"),
            Some("plantuml")
        );
    }

    #[test]
    fn detect_unknown_returns_none() {
        assert_eq!(detect_format("just some text"), None);
    }

    #[test]
    fn visuals_dir_is_under_home() {
        let dir = visuals_dir();
        assert!(dir.to_str().unwrap().contains(".omegon/visuals"));
    }
}
