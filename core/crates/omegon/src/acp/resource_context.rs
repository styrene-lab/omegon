use agent_client_protocol::schema::*;

const ACP_RESOURCE_MAX_LINES: usize = 2000;
const ACP_RESOURCE_MAX_BYTES: usize = 50 * 1024;
const ACP_DIRECTORY_MAX_ENTRIES: usize = 200;
const ACP_DIRECTORY_MAX_DEPTH: usize = 2;

fn resource_link_to_prompt_text(link: &ResourceLink, cwd: &std::path::Path) -> String {
    let mut out = format!("[Referenced resource: {}]", link.name);
    if let Some(title) = &link.title
        && title != &link.name
    {
        out.push_str(&format!("\nTitle: {title}"));
    }
    out.push_str(&format!("\nURI: {}", link.uri));
    if let Some(mime_type) = &link.mime_type {
        out.push_str(&format!("\nMIME: {mime_type}"));
    }
    if let Some(description) = &link.description {
        out.push_str(&format!("\nDescription: {description}"));
    }

    match read_acp_resource_text(
        &link.uri,
        Some(&link.name),
        link.title.as_deref(),
        link.mime_type.as_deref(),
        cwd,
    ) {
        Ok(Some(contents)) => {
            out.push_str("\n\nContents:\n");
            out.push_str(&contents);
        }
        Ok(None) => {}
        Err(e) => out.push_str(&format!("\n\n[Resource contents unavailable: {e}]")),
    }
    out
}

fn read_acp_resource_text(
    uri: &str,
    name: Option<&str>,
    title: Option<&str>,
    mime_type: Option<&str>,
    cwd: &std::path::Path,
) -> anyhow::Result<Option<String>> {
    let Some(path) = acp_resource_path(uri, name, title, cwd)? else {
        return virtual_resource_marker(uri);
    };
    if path.is_dir() {
        return Ok(Some(format_directory_listing(&path)?));
    }
    let bytes = std::fs::read(&path)?;
    if !should_inject_resource_as_text(mime_type, &path, &bytes) {
        return Ok(None);
    }
    let selection = line_selection(uri);
    let truncated_bytes = bytes.len() > ACP_RESOURCE_MAX_BYTES;
    let mut text =
        String::from_utf8_lossy(&bytes[..bytes.len().min(ACP_RESOURCE_MAX_BYTES)]).to_string();
    if truncated_bytes && let Some(last_newline) = text.rfind('\n') {
        text.truncate(last_newline);
    }

    let total_lines = text.lines().count();
    let (mut text, truncated_lines) = if let Some(range) = selection {
        let start = range.start.min(total_lines.max(1));
        let end = range.end.min(total_lines.max(1));
        let selected = text
            .lines()
            .enumerate()
            .filter_map(|(idx, line)| {
                let line_no = idx + 1;
                (line_no >= start && line_no <= end).then_some(line)
            })
            .collect::<Vec<_>>()
            .join("\n");
        (
            format!(
                "[Selected lines {start}-{end} of {}]\n{selected}",
                path.display()
            ),
            false,
        )
    } else {
        let truncated_lines = total_lines > ACP_RESOURCE_MAX_LINES;
        (
            text.lines()
                .take(ACP_RESOURCE_MAX_LINES)
                .collect::<Vec<_>>()
                .join("\n"),
            truncated_lines,
        )
    };
    if truncated_bytes || truncated_lines {
        text.push_str("\n\n[Referenced resource truncated]");
    }
    Ok(Some(text))
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct LineSelection {
    start: usize,
    end: usize,
}

fn line_selection(uri: &str) -> Option<LineSelection> {
    let fragment = uri.split_once('#')?.1;
    let fragment = fragment.split('?').next().unwrap_or(fragment);
    parse_line_selection(fragment)
}

fn parse_line_selection(fragment: &str) -> Option<LineSelection> {
    let range = fragment.strip_prefix('L').unwrap_or(fragment);
    let (start, end) = if let Some((start, end)) = range.split_once(':') {
        (start, end)
    } else if let Some((start, end)) = range.split_once('-') {
        (start, end.strip_prefix('L').unwrap_or(end))
    } else {
        (range, range)
    };
    let start = start.parse::<usize>().ok()?.max(1);
    let end = end.parse::<usize>().ok()?.max(start);
    Some(LineSelection { start, end })
}

fn format_directory_listing(path: &std::path::Path) -> anyhow::Result<String> {
    let mut lines = vec![format!("[Directory listing: {}]", path.display())];
    let mut count = 0usize;
    collect_directory_listing(path, 0, &mut count, &mut lines)?;
    if count >= ACP_DIRECTORY_MAX_ENTRIES {
        lines.push(format!(
            "[Directory listing truncated at {ACP_DIRECTORY_MAX_ENTRIES} entries]"
        ));
    }
    Ok(lines.join("\n"))
}

fn collect_directory_listing(
    path: &std::path::Path,
    depth: usize,
    count: &mut usize,
    lines: &mut Vec<String>,
) -> anyhow::Result<()> {
    if depth >= ACP_DIRECTORY_MAX_DEPTH || *count >= ACP_DIRECTORY_MAX_ENTRIES {
        return Ok(());
    }
    let mut entries = std::fs::read_dir(path)?
        .filter_map(Result::ok)
        .filter(|entry| !is_ignored_directory_entry(entry.file_name().to_string_lossy().as_ref()))
        .collect::<Vec<_>>();
    entries.sort_by_key(|entry| entry.file_name());
    for entry in entries {
        if *count >= ACP_DIRECTORY_MAX_ENTRIES {
            break;
        }
        let file_type = entry.file_type().ok();
        let is_dir = file_type.as_ref().is_some_and(|t| t.is_dir());
        let suffix = if is_dir { "/" } else { "" };
        lines.push(format!(
            "{}{}{}",
            "  ".repeat(depth),
            entry.file_name().to_string_lossy(),
            suffix
        ));
        *count += 1;
        if is_dir {
            collect_directory_listing(&entry.path(), depth + 1, count, lines)?;
        }
    }
    Ok(())
}

fn is_ignored_directory_entry(name: &str) -> bool {
    matches!(
        name,
        ".git" | "target" | "node_modules" | ".venv" | "__pycache__" | ".DS_Store"
    )
}

fn virtual_resource_marker(uri: &str) -> anyhow::Result<Option<String>> {
    if uri.starts_with("zed:///agent/") {
        return Ok(Some(format!(
            "[Virtual Zed resource referenced but no embedded content was provided: {uri}]"
        )));
    }
    Ok(None)
}

fn should_inject_resource_as_text(
    acp_mime: Option<&str>,
    path: &std::path::Path,
    bytes: &[u8],
) -> bool {
    if content_inspector::inspect(bytes).is_binary() {
        return false;
    }

    if acp_mime.is_some_and(is_textish_mime) {
        return true;
    }

    let guessed = mime_guess::from_path(path).first_raw();
    if guessed.is_some_and(is_textish_mime) {
        return true;
    }

    std::str::from_utf8(bytes).is_ok()
}

fn is_textish_mime(mime: &str) -> bool {
    let lower = mime.to_ascii_lowercase();
    lower.starts_with("text/")
        || lower.contains("json")
        || lower.contains("xml")
        || lower.contains("yaml")
        || lower.contains("toml")
        || lower.contains("markdown")
        || lower.contains("javascript")
        || lower.contains("typescript")
        || lower.contains("x-sh")
}

fn acp_resource_path(
    uri: &str,
    name: Option<&str>,
    title: Option<&str>,
    cwd: &std::path::Path,
) -> anyhow::Result<Option<std::path::PathBuf>> {
    let cwd = canonicalize_existing_root(cwd)?;
    for candidate in acp_resource_candidates(uri, name, title, &cwd) {
        let Some(path) = resolve_resource_candidate_inside_root(&cwd, &candidate)? else {
            continue;
        };
        if path.is_file() || path.is_dir() {
            return Ok(Some(path));
        }
    }
    Ok(None)
}

fn acp_resource_candidates(
    uri: &str,
    name: Option<&str>,
    title: Option<&str>,
    cwd: &std::path::Path,
) -> Vec<std::path::PathBuf> {
    let mut candidates = Vec::new();
    let labels: Vec<&str> = [name, title]
        .into_iter()
        .flatten()
        .filter(|label| !label.trim().is_empty())
        .collect();
    if let Ok(Some(path)) = uri_to_local_path(uri) {
        if labels.is_empty() {
            push_resource_candidate(&mut candidates, path.clone());
        }
        for label in &labels {
            push_resource_candidate(&mut candidates, path.join(label));
            push_resource_candidate(&mut candidates, path.join(format!("{label}.md")));
        }
        push_resource_candidate(&mut candidates, path.clone());
    }
    for label in labels {
        let label_path = std::path::PathBuf::from(label);
        if label_path.is_absolute() {
            push_resource_candidate(&mut candidates, label_path.clone());
            push_resource_candidate(&mut candidates, label_path.with_extension("md"));
        } else {
            push_resource_candidate(&mut candidates, cwd.join(label));
            push_resource_candidate(&mut candidates, cwd.join(format!("{label}.md")));
        }
    }
    candidates
}

fn push_resource_candidate(candidates: &mut Vec<std::path::PathBuf>, path: std::path::PathBuf) {
    if !candidates.iter().any(|p| p == &path) {
        candidates.push(path);
    }
}

fn canonicalize_existing_root(path: &std::path::Path) -> anyhow::Result<std::path::PathBuf> {
    path.canonicalize()
        .map_err(|e| anyhow::anyhow!("cannot resolve ACP resource root {}: {e}", path.display()))
}

fn resolve_resource_candidate_inside_root(
    root: &std::path::Path,
    candidate: &std::path::Path,
) -> anyhow::Result<Option<std::path::PathBuf>> {
    let resolved = match candidate.canonicalize() {
        Ok(path) => path,
        Err(_) => return Ok(None),
    };
    if resolved == root || resolved.starts_with(root) {
        Ok(Some(resolved))
    } else {
        Err(anyhow::anyhow!(
            "ACP resource path {} escapes session root {}",
            candidate.display(),
            root.display()
        ))
    }
}

fn uri_to_local_path(uri: &str) -> anyhow::Result<Option<std::path::PathBuf>> {
    if let Some(path) = zed_uri_path(uri)? {
        return Ok(Some(path));
    }

    let uri_no_fragment = uri.split('#').next().unwrap_or(uri);
    let uri_path = uri_no_fragment.split('?').next().unwrap_or(uri_no_fragment);
    if let Some(rest) = uri_path.strip_prefix("file://") {
        let rest = rest.strip_prefix("localhost").unwrap_or(rest);
        let decoded = percent_decode(rest)?;
        return Ok(Some(std::path::PathBuf::from(decoded)));
    }
    if uri_path.starts_with("http://") || uri_path.starts_with("https://") {
        return Ok(None);
    }
    let path = std::path::PathBuf::from(uri_path);
    if path.is_absolute() || path.exists() {
        return Ok(Some(path));
    }
    Ok(None)
}

fn zed_uri_path(uri: &str) -> anyhow::Result<Option<std::path::PathBuf>> {
    if !(uri.starts_with("zed:///agent/file")
        || uri.starts_with("zed:///agent/directory")
        || uri.starts_with("zed:///agent/selection")
        || uri.starts_with("zed:///agent/symbol/"))
    {
        return Ok(None);
    }
    let Some(query) = uri.split('?').nth(1) else {
        return Ok(None);
    };
    let query = query.split('#').next().unwrap_or(query);
    for pair in query.split('&') {
        let Some((key, value)) = pair.split_once('=') else {
            continue;
        };
        if key == "path" {
            return Ok(Some(std::path::PathBuf::from(percent_decode(value)?)));
        }
    }
    Ok(None)
}

fn percent_decode(input: &str) -> anyhow::Result<String> {
    let bytes = input.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            let hex = std::str::from_utf8(&bytes[i + 1..i + 3])?;
            out.push(u8::from_str_radix(hex, 16)?);
            i += 3;
        } else {
            out.push(bytes[i]);
            i += 1;
        }
    }
    Ok(String::from_utf8(out)?)
}

fn text_resource_to_prompt_text(text: &TextResourceContents, cwd: &std::path::Path) -> String {
    let mut out = format!(
        "[Embedded resource: {}{}]",
        text.uri,
        text.mime_type
            .as_ref()
            .map(|m| format!(" ({m})"))
            .unwrap_or_default()
    );

    let label = embedded_resource_label_candidate(&text.text);
    let dereferenced = read_acp_resource_text(
        &text.uri,
        label.as_deref(),
        None,
        text.mime_type.as_deref(),
        cwd,
    );
    match dereferenced {
        Ok(Some(contents)) if embedded_text_is_label_like(&text.text, &contents) => {
            out.push_str("\nContents:\n");
            out.push_str(&contents);
        }
        Ok(Some(contents)) if text.text.trim().is_empty() => {
            out.push_str("\nContents:\n");
            out.push_str(&contents);
        }
        Ok(_) => {
            out.push('\n');
            out.push_str(&text.text);
        }
        Err(e) => {
            out.push('\n');
            out.push_str(&text.text);
            out.push_str(&format!(
                "\n\n[Embedded resource dereference unavailable: {e}]"
            ));
        }
    }
    out
}

fn embedded_resource_label_candidate(text: &str) -> Option<String> {
    let trimmed = text.trim();
    if trimmed.is_empty() || trimmed.len() > 256 || trimmed.lines().count() > 3 {
        return None;
    }
    // Treat tiny embedded payloads from editor @-mentions as display labels.
    // Substantive embedded resources are preserved as their own content.
    if trimmed.chars().any(|c| c == '\0') {
        return None;
    }
    Some(trimmed.to_string())
}

fn embedded_text_is_label_like(embedded: &str, dereferenced: &str) -> bool {
    let trimmed = embedded.trim();
    !trimmed.is_empty()
        && trimmed.len() <= 256
        && !dereferenced.trim().is_empty()
        && dereferenced.trim() != trimmed
        && trimmed.lines().count() <= 3
}

pub(super) fn prompt_blocks_to_text(blocks: &[ContentBlock], cwd: &std::path::Path) -> String {
    blocks
        .iter()
        .filter_map(|block| content_block_to_prompt_text(block, cwd))
        .collect::<Vec<_>>()
        .join("\n")
}

fn content_block_to_prompt_text(block: &ContentBlock, cwd: &std::path::Path) -> Option<String> {
    match block {
        ContentBlock::Text(text) => Some(text.text.clone()),
        ContentBlock::ResourceLink(link) => Some(resource_link_to_prompt_text(link, cwd)),
        ContentBlock::Resource(resource) => match &resource.resource {
            EmbeddedResourceResource::TextResourceContents(text) => {
                Some(text_resource_to_prompt_text(text, cwd))
            }
            EmbeddedResourceResource::BlobResourceContents(blob) => Some(format!(
                "[Embedded binary resource: {}{}; {} base64 bytes]",
                blob.uri,
                blob.mime_type
                    .as_ref()
                    .map(|m| format!(" ({m})"))
                    .unwrap_or_default(),
                blob.blob.len()
            )),
            _ => Some("[Unsupported embedded resource]".to_string()),
        },
        ContentBlock::Image(image) => Some(format!(
            "[Image attachment{}: {}; {} base64 bytes]",
            image
                .uri
                .as_ref()
                .map(|u| format!(" {u}"))
                .unwrap_or_default(),
            image.mime_type,
            image.data.len()
        )),
        ContentBlock::Audio(audio) => Some(format!(
            "[Audio attachment: {}; {} base64 bytes]",
            audio.mime_type,
            audio.data.len()
        )),
        _ => Some("[Unsupported ACP content block]".to_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn resource_link_reads_file_uri_contents() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("note.md");
        std::fs::write(
            &file,
            "# Note

hello from resource",
        )
        .unwrap();
        let uri = format!("file://{}", file.display());
        let block = ContentBlock::ResourceLink(ResourceLink::new("note.md", uri));

        let text = prompt_blocks_to_text(&[block], dir.path());

        assert!(
            text.contains(
                "Contents:
# Note"
            ),
            "{text}"
        );
        assert!(text.contains("hello from resource"), "{text}");
    }

    #[test]
    fn embedded_resource_label_dereferences_uri_directory() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join("0.24.0.md"),
            "# 0.24.0

actual contents",
        )
        .unwrap();
        let uri = format!("file://{}", dir.path().display());
        let block = ContentBlock::Resource(EmbeddedResource::new(
            EmbeddedResourceResource::TextResourceContents(TextResourceContents::new(
                "0.24.0", uri,
            )),
        ));

        let text = prompt_blocks_to_text(&[block], dir.path());

        assert!(
            text.contains(
                "Contents:
# 0.24.0"
            ),
            "{text}"
        );
        assert!(text.contains("actual contents"), "{text}");
    }

    #[test]
    fn binary_resource_link_does_not_inject_bytes() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("image.png");
        std::fs::write(
            &file,
            [
                0x89, b'P', b'N', b'G', b'\r', b'\n', 0x1a, b'\n', 0, b'b', b'i', b'n',
            ],
        )
        .unwrap();
        let uri = format!("file://{}", file.display());
        let block = ContentBlock::ResourceLink(ResourceLink::new("image.png", uri));

        let text = prompt_blocks_to_text(&[block], dir.path());

        assert!(!text.contains("Contents:"), "{text}");
        assert!(text.contains("URI: file://"), "{text}");
    }

    #[test]
    fn resource_link_honors_line_fragment_selection() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("selection.rs");
        std::fs::write(
            &file,
            "one
two
three
four
",
        )
        .unwrap();
        let uri = format!("file://{}#L2:3", file.display());
        let block = ContentBlock::ResourceLink(ResourceLink::new("selection.rs", uri));

        let text = prompt_blocks_to_text(&[block], dir.path());

        assert!(text.contains("Selected lines 2-3"), "{text}");
        assert!(
            text.contains(
                "two
three"
            ),
            "{text}"
        );
        assert!(
            !text.contains(
                "one
two
three
four"
            ),
            "{text}"
        );
    }

    #[test]
    fn directory_resource_link_injects_bounded_listing() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join("a.rs"),
            "fn main() {}
",
        )
        .unwrap();
        std::fs::create_dir(dir.path().join("src")).unwrap();
        std::fs::write(
            dir.path().join("src").join("lib.rs"),
            "pub fn x() {}
",
        )
        .unwrap();
        std::fs::create_dir(dir.path().join("target")).unwrap();
        std::fs::write(
            dir.path().join("target").join("ignored"),
            "ignore
",
        )
        .unwrap();
        let uri = format!("file://{}/", dir.path().display());
        let block = ContentBlock::ResourceLink(ResourceLink::new("fixture", uri));

        let text = prompt_blocks_to_text(&[block], dir.path());

        assert!(text.contains("[Directory listing:"), "{text}");
        assert!(text.contains("a.rs"), "{text}");
        assert!(text.contains("src/"), "{text}");
        assert!(text.contains("lib.rs"), "{text}");
        assert!(!text.contains("target/"), "{text}");
    }

    #[test]
    fn virtual_zed_resource_gets_explicit_marker() {
        let dir = tempfile::tempdir().unwrap();
        let block = ContentBlock::ResourceLink(ResourceLink::new(
            "Diagnostics",
            "zed:///agent/diagnostics".to_string(),
        ));

        let text = prompt_blocks_to_text(&[block], dir.path());

        assert!(text.contains("Virtual Zed resource referenced"), "{text}");
    }

    #[test]
    fn resource_link_rejects_paths_outside_session_root() {
        let root = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        let file = outside.path().join("secret.md");
        std::fs::write(&file, "secret").unwrap();
        let block = ContentBlock::ResourceLink(ResourceLink::new(
            "secret.md",
            format!("file://{}", file.display()),
        ));

        let text = prompt_blocks_to_text(&[block], root.path());

        assert!(text.contains("escapes session root"), "{text}");
        assert!(
            !text.contains(
                "Contents:
secret"
            ),
            "{text}"
        );
    }

    #[test]
    fn resource_link_rejects_symlink_escape() {
        let root = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        let file = outside.path().join("secret.md");
        std::fs::write(&file, "secret").unwrap();
        let link = root.path().join("link.md");
        #[cfg(unix)]
        std::os::unix::fs::symlink(&file, &link).unwrap();
        #[cfg(windows)]
        std::os::windows::fs::symlink_file(&file, &link).unwrap();
        let block = ContentBlock::ResourceLink(ResourceLink::new(
            "link.md",
            format!("file://{}", link.display()),
        ));

        let text = prompt_blocks_to_text(&[block], root.path());

        assert!(text.contains("escapes session root"), "{text}");
        assert!(
            !text.contains(
                "Contents:
secret"
            ),
            "{text}"
        );
    }

    #[test]
    fn resource_link_reads_zed_agent_file_uri() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("zed-file.canvas");
        std::fs::write(&file, r#"{"version":1,"cells":[]}"#).unwrap();
        let uri = format!(
            "zed:///agent/file?path={}",
            percent_encode_for_test(&file.display().to_string())
        );
        let block = ContentBlock::ResourceLink(ResourceLink::new("zed-file.canvas", uri));

        let text = prompt_blocks_to_text(&[block], dir.path());

        assert!(text.contains("Contents:"), "{text}");
        assert!(text.contains(r#""cells":[]"#), "{text}");
    }

    #[test]
    fn resource_link_honors_zed_agent_selection_uri() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("selection.flow");
        std::fs::write(
            &file,
            "alpha
beta
gamma
delta
",
        )
        .unwrap();
        let uri = format!(
            "zed:///agent/selection?path={}#L2:3",
            percent_encode_for_test(&file.display().to_string())
        );
        let block = ContentBlock::ResourceLink(ResourceLink::new("selection.flow", uri));

        let text = prompt_blocks_to_text(&[block], dir.path());

        assert!(text.contains("Selected lines 2-3"), "{text}");
        assert!(
            text.contains(
                "beta
gamma"
            ),
            "{text}"
        );
        assert!(
            !text.contains(
                "alpha
beta
gamma
delta"
            ),
            "{text}"
        );
    }

    #[test]
    fn directory_resource_link_reads_zed_agent_directory_uri() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("board.flow"), r#"{"nodes":[]}"#).unwrap();
        let uri = format!(
            "zed:///agent/directory?path={}",
            percent_encode_for_test(&dir.path().display().to_string())
        );
        let block = ContentBlock::ResourceLink(ResourceLink::new("fixture", uri));

        let text = prompt_blocks_to_text(&[block], dir.path());

        assert!(text.contains("[Directory listing:"), "{text}");
        assert!(text.contains("board.flow"), "{text}");
    }

    #[test]
    fn text_like_ecosystem_resources_are_injected() {
        let dir = tempfile::tempdir().unwrap();
        for (name, body) in [
            ("mock.canvas", r#"{"version":1}"#),
            ("scene.excalidraw", r#"{"type":"excalidraw"}"#),
            ("graph.flow", r#"{"nodes":[]}"#),
            ("diagram.d2", "a -> b"),
            ("config.pkl", r#"name = "demo""#),
        ] {
            let file = dir.path().join(name);
            std::fs::write(&file, body).unwrap();
            let block = ContentBlock::ResourceLink(ResourceLink::new(
                name,
                format!("file://{}", file.display()),
            ));
            let text = prompt_blocks_to_text(&[block], dir.path());
            assert!(text.contains(body), "{name}: {text}");
        }
    }

    fn percent_encode_for_test(input: &str) -> String {
        input
            .replace('%', "%25")
            .replace(' ', "%20")
            .replace('#', "%23")
            .replace('?', "%3F")
            .replace('&', "%26")
            .replace('=', "%3D")
    }
}
