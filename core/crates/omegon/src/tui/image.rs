//! Inline image rendering via ratatui-image.
//!
//! Uses the Picker to detect terminal graphics protocol (Kitty/Sixel/iTerm2)
//! at startup. Images from tool results are decoded with the `image` crate
//! and rendered as StatefulImage widgets inside the conversation view.
//!
//! The StatefulProtocol state is stored per-segment-index in ImageCache,
//! separate from the Segment enum (which needs Clone + Debug).

use std::collections::HashMap;
use std::path::Path;
use std::sync::OnceLock;

use ratatui::prelude::*;
use ratatui_image::picker::Picker;
use ratatui_image::protocol::StatefulProtocol;
use ratatui_image::StatefulImage;

/// Global picker — initialized once at startup.
static PICKER: OnceLock<Option<Picker>> = OnceLock::new();

/// Initialize the image picker by querying the terminal.
/// Call this once before entering the TUI main loop.
pub fn init_picker() {
    PICKER.get_or_init(|| {
        match Picker::from_query_stdio() {
            Ok(picker) => {
                tracing::info!(
                    "Image protocol detected: using ratatui-image"
                );
                Some(picker)
            }
            Err(e) => {
                tracing::debug!("No image protocol available: {e} — using placeholders");
                None
            }
        }
    });
}

/// Check if image rendering is available.
pub fn is_available() -> bool {
    PICKER.get().is_some_and(|p| p.is_some())
}

/// Cache of StatefulProtocol instances, keyed by segment index.
/// Lives alongside the ConversationView, not inside Segment.
#[derive(Default)]
pub struct ImageCache {
    protocols: HashMap<usize, StatefulProtocol>,
}

impl ImageCache {
    /// Get or create the StatefulProtocol for a segment index.
    /// Returns None if image rendering is unavailable or the file can't be decoded.
    pub fn get_or_create(&mut self, segment_idx: usize, path: &Path) -> Option<&mut StatefulProtocol> {
        use std::collections::hash_map::Entry;
        match self.protocols.entry(segment_idx) {
            Entry::Occupied(e) => Some(e.into_mut()),
            Entry::Vacant(e) => {
                let protocol = create_protocol(path)?;
                Some(e.insert(protocol))
            }
        }
    }

    /// Remove cached protocol for a segment (e.g., when segments are cleared).
    pub fn clear(&mut self) {
        self.protocols.clear();
    }

    /// Remove entries beyond a certain index (for truncation).
    pub fn truncate_from(&mut self, idx: usize) {
        self.protocols.retain(|k, _| *k < idx);
    }
}

/// Render an image into the given area using the cached protocol.
pub fn render_image(
    area: Rect,
    frame: &mut ratatui::Frame,
    protocol: &mut StatefulProtocol,
) {
    let widget = StatefulImage::default();
    frame.render_stateful_widget(widget, area, protocol);
}

/// Create a StatefulProtocol from an image file path.
fn create_protocol(path: &Path) -> Option<StatefulProtocol> {
    let picker = PICKER.get()?.as_ref()?;

    let dyn_img = match image::ImageReader::open(path) {
        Ok(reader) => match reader.decode() {
            Ok(img) => img,
            Err(e) => {
                tracing::debug!("Failed to decode image {}: {e}", path.display());
                return None;
            }
        },
        Err(e) => {
            tracing::debug!("Failed to open image {}: {e}", path.display());
            return None;
        }
    };

    Some(picker.new_resize_protocol(dyn_img))
}

/// Check if a file path looks like an image based on extension.
pub fn is_image_path(path: &str) -> bool {
    let lower = path.to_lowercase();
    lower.ends_with(".png")
        || lower.ends_with(".jpg")
        || lower.ends_with(".jpeg")
        || lower.ends_with(".gif")
        || lower.ends_with(".webp")
        || lower.ends_with(".bmp")
        || lower.ends_with(".svg") // Won't decode but good to detect
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn is_image_path_detects_images() {
        assert!(is_image_path("photo.png"));
        assert!(is_image_path("PHOTO.PNG"));
        assert!(is_image_path("/tmp/test.jpg"));
        assert!(is_image_path("output.webp"));
        assert!(is_image_path("diagram.gif"));
        assert!(!is_image_path("code.rs"));
        assert!(!is_image_path("data.json"));
        assert!(!is_image_path("image")); // no extension
    }

    #[test]
    fn image_cache_default_empty() {
        let cache = ImageCache::default();
        assert!(cache.protocols.is_empty());
    }

    #[test]
    fn image_cache_clear() {
        let mut cache = ImageCache::default();
        // Can't easily create StatefulProtocol without a picker, but we can test the container
        cache.clear();
        assert!(cache.protocols.is_empty());
    }

    #[test]
    fn is_available_before_init() {
        // Before init_picker is called, should return false
        // (OnceLock not yet set in this test context)
        // Note: this test is order-dependent and may pass/fail depending on test execution order
        // Just verify it doesn't panic
        let _ = is_available();
    }

    #[test]
    fn create_protocol_missing_file() {
        // init_picker may not be called, so this should return None
        let result = create_protocol(Path::new("/nonexistent/image.png"));
        assert!(result.is_none());
    }
}
