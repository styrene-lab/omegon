use std::time::{Duration, Instant};

const FNV_OFFSET: u64 = 0xcbf29ce484222325;
const FNV_PRIME: u64 = 0x100000001b3;

pub(super) const DEFAULT_MAX_BYTES: usize = 64 * 1024;
pub(super) const DEFAULT_MAX_RECORDS: usize = 64;
pub(super) const DEFAULT_MAX_ROWS: usize = 1_000;
pub(super) const DEFAULT_PREPARE_BUDGET: Duration = Duration::from_millis(5);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct PublicationIdentity {
    pub(super) attachment_epoch: u64,
    pub(super) base_revision: u64,
    pub(super) target_revision: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct PreparedPublication {
    pub(super) identity: PublicationIdentity,
    pub(super) range: std::ops::Range<usize>,
    pub(super) text: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum DeliveryResult {
    Committed,
    KnownFailure,
    Ambiguous,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum SettlementError {
    StaleAttachment,
    NonContiguous,
    WrongRevision,
    Degraded,
}

#[derive(Debug, Clone, Copy)]
pub(super) struct PreparationBudget {
    pub(super) max_bytes: usize,
    pub(super) max_records: usize,
    pub(super) max_rows: usize,
    pub(super) max_elapsed: Duration,
}

impl Default for PreparationBudget {
    fn default() -> Self {
        Self {
            max_bytes: DEFAULT_MAX_BYTES,
            max_records: DEFAULT_MAX_RECORDS,
            max_rows: DEFAULT_MAX_ROWS,
            max_elapsed: DEFAULT_PREPARE_BUDGET,
        }
    }
}

#[derive(Debug)]
pub(super) struct NativePublicationState {
    pub(super) automatic: AutomaticPublication,
    attachment_epoch: u64,
    committed_revision: u64,
    committed_prefix_revision: u64,
    committed_byte: usize,
    degraded: bool,
}

impl Default for NativePublicationState {
    fn default() -> Self {
        Self {
            automatic: AutomaticPublication::default(),
            attachment_epoch: 1,
            committed_revision: 0,
            committed_prefix_revision: FNV_OFFSET,
            committed_byte: 0,
            degraded: false,
        }
    }
}

/// Cursor into the canonical source, not another transcript. A batch owns only
/// the text that can fit in one bounded terminal insertion.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
struct InlineCursor {
    generation: u64,
    notice: Option<String>,
    segment: usize,
    field: usize,
    byte: usize,
    detail: Option<crate::surfaces::layout::UiPresentationLevel>,
    scan: Option<OutcomeScan>,
    summary: Option<String>,
    summary_end: Option<usize>,
    pending_line: String,
    markdown: super::markdown_publication::NativeMarkdown,
    control: ControlState,
}

/// Escape parsing survives event and preparation boundaries.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
enum ControlState {
    #[default]
    Ground,
    Escape,
    Csi,
    Osc,
    OscEscape,
    StringControl,
    StringEscape,
}
impl ControlState {
    fn consume(&mut self, text: &str) -> String {
        let mut visible = String::new();
        for ch in text.chars() {
            match *self {
                Self::Ground => match ch {
                    '\u{1b}' => *self = Self::Escape,
                    '\n' => visible.push(ch),
                    '\t' => visible.push(' '),
                    _ if !ch.is_control() => visible.push(ch),
                    _ => {}
                },
                Self::Escape => {
                    *self = match ch {
                        '[' => Self::Csi,
                        ']' => Self::Osc,
                        'P' | '^' | '_' | 'X' => Self::StringControl,
                        _ => Self::Ground,
                    }
                }
                Self::Csi => {
                    if ('@'..='~').contains(&ch) {
                        *self = Self::Ground;
                    }
                }
                Self::Osc => match ch {
                    '\u{7}' => *self = Self::Ground,
                    '\u{1b}' => *self = Self::OscEscape,
                    _ => {}
                },
                Self::StringControl => {
                    if ch == '\u{1b}' {
                        *self = Self::StringEscape;
                    }
                }
                Self::StringEscape => {
                    *self = match ch {
                        '\\' => Self::Ground,
                        '\u{1b}' => Self::StringEscape,
                        _ => Self::StringControl,
                    }
                }
                Self::OscEscape => {
                    *self = match ch {
                        '\\' | '\u{7}' => Self::Ground,
                        '\u{1b}' => Self::OscEscape,
                        _ => Self::Osc,
                    }
                }
            }
        }
        visible
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct OutcomeScan {
    index: usize,
    coordinate: Option<(Option<u64>, u32)>,
    summary: crate::surfaces::episodes::OutcomeSummary,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct HeldSource {
    generation: u64,
    segment: usize,
    field: usize,
    byte: usize,
    text_len: usize,
    thinking_len: usize,
    complete: bool,
    finalized: usize,
    width: u16,
    detail: u8,
}

#[derive(Debug, Default)]
pub(super) struct AutomaticPublication {
    cursor: InlineCursor,
    degraded: bool,
    unsupported_grapheme: std::cell::Cell<bool>,
    source_changed: bool,
    held_source: std::cell::Cell<Option<HeldSource>>,
    proven_held_tail: std::cell::Cell<bool>,
}

pub(super) struct InlineBatch {
    base: InlineCursor,
    next: InlineCursor,
    pub(super) lines: Vec<String>,
    pub(super) styled: Vec<(usize, ratatui::text::Line<'static>)>,
}

pub(super) fn safe_inline_text(text: &str) -> String {
    text.split('\n')
        .map(super::segments::strip_terminal_control)
        .collect::<Vec<_>>()
        .join("\n")
}

impl AutomaticPublication {
    pub(super) fn generation(&self) -> u64 {
        self.cursor.generation
    }
    pub(super) fn has_pending(&self, boundary: usize) -> bool {
        self.cursor.notice.is_some() || self.cursor.segment < boundary
    }
    pub(super) fn apply_prune(&mut self, prune: &super::conversation::PublicationPrune) -> bool {
        if self.cursor.generation != prune.from_generation {
            return false;
        }
        for index in &prune.removed {
            if *index < self.cursor.segment {
                self.cursor.segment -= 1;
            } else if *index == self.cursor.segment && self.cursor.notice.is_none() {
                self.cursor.field = 0;
                self.cursor.byte = 0;
                self.cursor.detail = None;
                self.cursor.summary = None;
                self.cursor.scan = None;
                self.cursor.summary_end = None;
                self.cursor.pending_line.clear();
                self.cursor.markdown = Default::default();
                self.cursor.control = ControlState::Ground;
            }
            if let Some(end) = &mut self.cursor.summary_end
                && *index < *end
            {
                *end -= 1;
            }
            if let Some(scan) = &mut self.cursor.scan
                && *index < scan.index
            {
                scan.index -= 1;
            }
        }
        // Also invalidate prepared batches when deletion was after the cursor.
        self.cursor.generation = prune.to_generation;
        true
    }

    pub(super) fn reconcile(
        &mut self,
        generation: u64,
        changed_at: usize,
        _boundary: usize,
    ) -> bool {
        if changed_at > self.cursor.segment
            || (changed_at == self.cursor.segment
                && self.cursor.field == 0
                && self.cursor.byte == 0)
        {
            self.cursor.generation = generation;
            if self.cursor.scan.is_some() {
                self.cursor.scan = None;
                self.cursor.summary = None;
            }
            false
        } else {
            // An old turn boundary can now trail already published live text.
            // Without precise mapping, stop instead of replaying guessed offsets.
            self.cursor.generation = generation;
            self.source_changed = true;
            self.held_source.set(None);
            true
        }
    }
    pub(super) fn source_replaced(&mut self, generation: u64, boundary: usize) {
        self.attach(generation, boundary);
        self.cursor.notice =
            Some("Conversation boundary changed · previous output remains in scrollback".into());
    }

    pub(super) fn attach(&mut self, generation: u64, boundary: usize) {
        self.held_source.set(None);
        self.unsupported_grapheme.set(false);
        self.source_changed = false;
        self.cursor = InlineCursor {
            generation,
            segment: boundary,
            notice: (boundary > 0).then(|| {
                format!("Session attached · {boundary} prior records available in fullscreen")
            }),
            ..Default::default()
        };
        // Uncertain physical output is not made retryable by a source rewrite.
    }

    pub(super) fn pending_text(&self) -> &str {
        if self.cursor.markdown.pending.is_empty() {
            &self.cursor.pending_line
        } else {
            &self.cursor.markdown.pending
        }
    }

    pub(super) fn preview(&self, width: u16, rows: usize) -> Vec<ratatui::text::Line<'static>> {
        self.cursor.markdown.preview(width, rows)
    }

    pub(super) fn degradation_message(&self) -> &'static str {
        if self.unsupported_grapheme.get() {
            "Text exceeds inline buffer limit · fullscreen or /session-export for complete output"
        } else if self.source_changed {
            "Conversation changed · inline publication paused · fullscreen or /session-export for history"
        } else {
            "Scrollback delivery uncertain · /session-export or fullscreen for history"
        }
    }

    pub(super) fn is_degraded(&self) -> bool {
        self.degraded || self.unsupported_grapheme.get() || self.source_changed
    }

    pub(super) fn prepare(
        &self,
        generation: u64,
        segments: &[super::segments::Segment],
        finalized: usize,
        detail: crate::surfaces::layout::UiPresentationLevel,
        width: u16,
        budget: PreparationBudget,
    ) -> Option<InlineBatch> {
        let key = match segments
            .get(self.cursor.segment)
            .map(|segment| &segment.content)
        {
            Some(super::segments::SegmentContent::AssistantText {
                text,
                thinking,
                complete,
            }) => Some(HeldSource {
                generation,
                segment: self.cursor.segment,
                field: self.cursor.field,
                byte: self.cursor.byte,
                text_len: text.len(),
                thinking_len: thinking.len(),
                complete: *complete,
                finalized,
                width,
                detail: detail as u8,
            }),
            _ => None,
        };
        if key.is_some() && self.held_source.get() == key {
            return None;
        }
        self.proven_held_tail.set(false);
        let started = Instant::now();
        let result = self.prepare_with_elapsed(
            generation,
            segments,
            finalized,
            detail,
            width,
            budget,
            || started.elapsed(),
        );
        self.held_source
            .set(if result.is_none() && self.proven_held_tail.get() {
                key
            } else {
                None
            });
        result
    }

    #[allow(clippy::too_many_arguments)] // Same boundary with an injected cooperative clock for deterministic tests.
    fn prepare_with_elapsed(
        &self,
        generation: u64,
        segments: &[super::segments::Segment],
        finalized: usize,
        detail: crate::surfaces::layout::UiPresentationLevel,
        width: u16,
        budget: PreparationBudget,
        mut elapsed: impl FnMut() -> Duration,
    ) -> Option<InlineBatch> {
        use super::segments::SegmentContent;
        use unicode_segmentation::UnicodeSegmentation;
        use unicode_width::UnicodeWidthStr;
        if self.is_degraded() || width < 2 || generation != self.cursor.generation {
            return None;
        }
        let mut next = self.cursor.clone();
        let mut lines = Vec::new();
        let mut styled = Vec::new();
        let mut line = std::mem::take(&mut next.pending_line);

        let mut bytes = 0;
        let mut records = 0;
        let max_rows = budget.max_rows.min(65_536 / usize::from(width));
        next.markdown.push("", width);
        drain_markdown(
            &mut next.markdown,
            &mut lines,
            &mut styled,
            width,
            max_rows,
            DEFAULT_MAX_BYTES,
            budget.max_elapsed,
            &mut elapsed,
        );
        if next.markdown.has_publishable(width) {
            return (next != self.cursor).then(|| InlineBatch {
                base: self.cursor.clone(),
                next,
                lines,
                styled,
            });
        }
        while line.width() > usize::from(width)
            && lines.len() < max_rows
            && bytes < budget.max_bytes
            && elapsed() < budget.max_elapsed
        {
            let mut cells = 0;
            let split = line
                .grapheme_indices(true)
                .find_map(|(index, grapheme)| {
                    cells += grapheme.width();
                    (cells > usize::from(width)).then_some(index)
                })
                .unwrap_or(line.len());
            if split == 0 {
                self.unsupported_grapheme.set(true);
                return None;
            }
            if bytes + split > budget.max_bytes {
                break;
            }
            let tail = line.split_off(split);
            bytes += split;
            lines.push(std::mem::replace(&mut line, tail));
        }
        if line.width() > usize::from(width) {
            next.pending_line = line;
            return (next != self.cursor).then(|| InlineBatch {
                base: self.cursor.clone(),
                next,
                lines,
                styled,
            });
        }
        'records: while (next.notice.is_some() || next.segment < segments.len())
            && records < budget.max_records
            && lines.len() < max_rows
            && bytes < budget.max_bytes
            && elapsed() < budget.max_elapsed
        {
            let notice = next.notice.as_ref().map(super::segments::Segment::system);
            let segment = if let Some(ref notice) = notice {
                notice
            } else {
                &segments[next.segment]
            };
            if notice.is_none()
                && matches!(&segment.content,
                SegmentContent::SystemNotification { text } if super::segments::is_plan_progress_text(text))
            {
                next.segment += 1;
                next.detail = None;
                records += 1;
                continue;
            }
            if notice.is_none()
                && next.segment >= finalized
                && matches!(
                    segment.content,
                    SegmentContent::ToolCard {
                        complete: false,
                        ..
                    }
                )
            {
                break;
            }
            let level = *next.detail.get_or_insert(detail);
            if matches!(segment.content, SegmentContent::ToolCard { .. }) {
                let coordinate = segment
                    .meta
                    .turn
                    .map(|turn| (segment.meta.runtime_turn, turn));
                if next.summary.is_none() {
                    let scan = next.scan.get_or_insert_with(|| OutcomeScan {
                        index: next.segment,
                        coordinate,
                        summary: Default::default(),
                    });
                    loop {
                        let candidate = segments.get(scan.index);
                        if next.segment >= finalized
                            && scan.index >= finalized
                            && candidate.is_none()
                        {
                            break 'records;
                        }
                        let done = candidate.is_none()
                            || candidate.is_some_and(|candidate| {
                                !matches!(candidate.content, SegmentContent::ToolCard { .. })
                                    || candidate
                                        .meta
                                        .turn
                                        .map(|turn| (candidate.meta.runtime_turn, turn))
                                        != coordinate
                            });
                        if done {
                            next.summary = Some(scan.summary.display());
                            next.summary_end = Some(scan.index);
                            next.scan = None;
                            break;
                        }
                        if scan.index >= finalized
                            && candidate.is_some_and(|candidate| {
                                matches!(
                                    candidate.content,
                                    SegmentContent::ToolCard {
                                        complete: false,
                                        ..
                                    }
                                )
                            })
                        {
                            break 'records;
                        }
                        if records >= budget.max_records || elapsed() >= budget.max_elapsed {
                            break 'records;
                        }
                        let candidate = candidate?;
                        if let SegmentContent::ToolCard {
                            name,
                            result_summary,
                            is_error,
                            ..
                        } = &candidate.content
                        {
                            let cost = name.len().min(512)
                                + result_summary.as_ref().map_or(0, |v| v.len().min(512));
                            if bytes + cost > budget.max_bytes {
                                break 'records;
                            }
                            bytes += cost;
                            scan.summary
                                .observe(name, result_summary.as_deref(), *is_error);
                        }
                        scan.index += 1;
                        records += 1;
                    }
                }
            }
            // Borrow source fields; do not export/clone an unbounded record.
            let fields: Vec<&str> = match &segment.content {
                SegmentContent::UserPrompt { text } => vec!["› ", text, "\n"],
                SegmentContent::AssistantText { text, thinking, .. } => {
                    if level == crate::surfaces::layout::UiPresentationLevel::Full
                        && !thinking.is_empty()
                    {
                        vec![text, "\n", "Thinking: ", thinking, "\n"]
                    } else {
                        vec![text, "\n"]
                    }
                }
                SegmentContent::ToolCard {
                    name,
                    args_summary,
                    detail_args,
                    result_summary,
                    detail_result,
                    is_error,
                    ..
                } => {
                    if level != crate::surfaces::layout::UiPresentationLevel::Full
                        && let Some(summary) = next.summary.as_deref()
                    {
                        vec![summary, "\n"]
                    } else {
                        let result = if level == crate::surfaces::layout::UiPresentationLevel::Full
                        {
                            detail_result.as_deref().or(result_summary.as_deref())
                        } else {
                            result_summary.as_deref()
                        };
                        vec![
                            if *is_error { "✗ " } else { "✓ " },
                            name,
                            " ",
                            detail_args
                                .as_deref()
                                .or(args_summary.as_deref())
                                .unwrap_or(""),
                            ": ",
                            result.unwrap_or("completed"),
                            "\n",
                        ]
                    }
                }
                SegmentContent::SystemNotification { text }
                | SegmentContent::LifecycleEvent { text, .. } => vec![text, "\n"],
                SegmentContent::PeerAgentText { label, text, .. } => vec![label, ": ", text, "\n"],
                SegmentContent::OperatorCopyBlock { label, text, .. } => {
                    vec![label, ":\n", text, "\n"]
                }
                SegmentContent::Image { path, alt, .. } => vec![
                    "[image] ",
                    alt,
                    " · ",
                    path.to_str().unwrap_or("non-UTF-8 path"),
                    "\n",
                ],
                SegmentContent::SkillEvent {
                    active_ref,
                    reason,
                    resolution,
                    ..
                } => vec![
                    "Skill: ", active_ref, " · ", reason, " · ", resolution, "\n",
                ],
                SegmentContent::TurnSeparator => vec!["\n"],
            };
            let streaming = notice.is_none()
                && next.segment >= finalized
                && matches!(
                    segment.content,
                    SegmentContent::AssistantText {
                        complete: false,
                        ..
                    }
                );
            while next.field < fields.len() {
                if streaming && next.field > 0 {
                    break 'records;
                }
                let field = fields[next.field];
                let markdown = matches!(segment.content, SegmentContent::AssistantText { .. })
                    && next.field == 0;
                if next.byte > field.len() || !field.is_char_boundary(next.byte) {
                    return None;
                }
                // Bound segmentation of pathological unfinished graphemes too.
                let limit = field.floor_char_boundary(
                    next.byte
                        .saturating_add(budget.max_bytes.saturating_sub(bytes))
                        .min(field.len()),
                );
                let truncated = limit < field.len();
                let mut graphemes = field[next.byte..limit].graphemes(true).peekable();
                while let Some(grapheme) = graphemes.next() {
                    if (truncated || streaming)
                        && graphemes.peek().is_none()
                        && !grapheme.chars().any(char::is_control)
                    {
                        self.proven_held_tail.set(streaming && !truncated);
                        if truncated && grapheme.len() >= DEFAULT_MAX_BYTES.saturating_sub(4) {
                            self.unsupported_grapheme.set(true);
                            return None;
                        }
                        break 'records;
                    }
                    if bytes + grapheme.len() > budget.max_bytes
                        || lines.len() >= max_rows
                        || elapsed() >= budget.max_elapsed
                    {
                        break 'records;
                    }
                    let mut control = next.control;
                    let visible = control.consume(grapheme);
                    if markdown {
                        if next.markdown.pending.len().saturating_add(visible.len())
                            > DEFAULT_MAX_BYTES
                        {
                            self.unsupported_grapheme.set(true);
                            return None;
                        }
                        let produced = next.markdown.push(&visible, width);
                        next.byte += grapheme.len();
                        bytes += grapheme.len();
                        next.control = control;
                        if produced {
                            drain_markdown(
                                &mut next.markdown,
                                &mut lines,
                                &mut styled,
                                width,
                                max_rows,
                                DEFAULT_MAX_BYTES,
                                budget.max_elapsed,
                                &mut elapsed,
                            );
                        }
                        if next.markdown.has_publishable(width) {
                            break 'records;
                        }
                        continue;
                    }
                    let mut candidate = line.clone();
                    if !visible.contains('\n') {
                        candidate.push_str(&visible);
                    }
                    if candidate.len() > DEFAULT_MAX_BYTES {
                        self.unsupported_grapheme.set(true);
                        return None;
                    }
                    if !visible.contains('\n') && candidate.width() > usize::from(width) {
                        // Sanitation can join raw graphemes across ANSI controls.
                        // Keep the complete final sanitized cluster together.
                        let split = candidate
                            .grapheme_indices(true)
                            .next_back()
                            .map_or(0, |(index, _)| index);
                        if split == 0 {
                            self.unsupported_grapheme.set(true);
                            return None;
                        }
                        let tail = candidate.split_off(split);
                        lines.push(candidate);
                        candidate = tail;
                    }
                    next.byte += grapheme.len();
                    bytes += grapheme.len();
                    next.control = control;
                    if visible.contains('\n') {
                        lines.push(std::mem::take(&mut line));
                    } else {
                        line = candidate;
                    }
                }
                if streaming || truncated {
                    break 'records;
                }
                if markdown {
                    next.markdown.finish(width);
                    drain_markdown(
                        &mut next.markdown,
                        &mut lines,
                        &mut styled,
                        width,
                        max_rows,
                        DEFAULT_MAX_BYTES,
                        budget.max_elapsed,
                        &mut elapsed,
                    );
                    // Do not advance beyond the source until all its rendered
                    // rows have been delivered under the same cursor settlement.
                    if next.markdown.has_ready() {
                        break 'records;
                    }
                    next.markdown = Default::default();
                    // finish() supplies the final line; the following separator
                    // field would otherwise add an extra blank after every reply.
                    next.field += 1;
                }
                next.field += 1;
                next.byte = 0;
            }
            // Unterminated control strings belong only to this source record.
            if next.control != ControlState::Ground {
                next.control = ControlState::Ground;
                if !line.is_empty() {
                    lines.push(std::mem::take(&mut line));
                }
            }
            records += 1;
            if next.summary.is_some() {
                let end = next.summary_end.expect("completed tool run has an end");
                next.segment = if level == crate::surfaces::layout::UiPresentationLevel::Full {
                    next.segment + 1
                } else {
                    end
                };
                if next.segment >= end {
                    next.summary = None;
                    next.summary_end = None;
                }
            } else if next.notice.take().is_none() {
                next.segment += 1;
            }
            next.field = 0;
            if next.summary.is_none() {
                next.detail = None;
            }
        }
        next.pending_line = line;
        if next == self.cursor {
            None
        } else {
            Some(InlineBatch {
                base: self.cursor.clone(),
                next,
                lines,
                styled,
            })
        }
    }

    pub(super) fn settle(&mut self, batch: InlineBatch, outcome: DeliveryResult) -> bool {
        if self.degraded || batch.base != self.cursor {
            return false;
        }
        match outcome {
            DeliveryResult::Committed => self.cursor = batch.next,
            DeliveryResult::KnownFailure => {}
            DeliveryResult::Ambiguous => self.degraded = true,
        }
        true
    }
}

#[allow(clippy::too_many_arguments)] // One cooperative publication budget spans source consumption and row delivery.
fn drain_markdown(
    markdown: &mut super::markdown_publication::NativeMarkdown,
    lines: &mut Vec<String>,
    styled: &mut Vec<(usize, ratatui::text::Line<'static>)>,
    width: u16,
    max_rows: usize,
    max_bytes: usize,
    max_elapsed: Duration,
    elapsed: &mut impl FnMut() -> Duration,
) {
    let mut bytes = lines.iter().map(String::len).sum::<usize>();
    while lines.len() < max_rows && bytes < max_bytes && elapsed() < max_elapsed {
        let Some(row) = markdown.pop_row(width, max_bytes - bytes) else {
            break;
        };
        let plain = row
            .spans
            .iter()
            .map(|span| span.content.as_ref())
            .collect::<String>();
        bytes += plain.len();
        styled.push((lines.len(), row));
        lines.push(plain);
    }
}

impl NativePublicationState {
    pub(super) fn begin_attachment(&mut self) {
        self.attachment_epoch = self
            .attachment_epoch
            .checked_add(1)
            .expect("native publication attachment epoch exhausted");
        self.committed_revision = 0;
        self.committed_prefix_revision = FNV_OFFSET;
        self.committed_byte = 0;
        self.degraded = false;
    }

    pub(super) fn prepare(
        &self,
        canonical: &str,
        target_revision: u64,
        budget: PreparationBudget,
    ) -> Result<Option<PreparedPublication>, SettlementError> {
        if self.degraded {
            return Err(SettlementError::Degraded);
        }
        if self.committed_byte > canonical.len() || !canonical.is_char_boundary(self.committed_byte)
        {
            return Err(SettlementError::NonContiguous);
        }
        if self.committed_byte > 0
            && canonical_prefix_revision(&canonical[..self.committed_byte])
                != self.committed_prefix_revision
        {
            return Err(SettlementError::NonContiguous);
        }
        if self.committed_byte == canonical.len() {
            return Ok(None);
        }

        let started = Instant::now();
        let suffix = &canonical[self.committed_byte..];
        let mut bytes = 0;
        let mut rows = 0;
        for (records, record) in suffix.split_inclusive('\n').enumerate() {
            if records >= budget.max_records
                || rows >= budget.max_rows
                || started.elapsed() >= budget.max_elapsed
            {
                break;
            }
            let remaining = budget.max_bytes.saturating_sub(bytes);
            if remaining == 0 {
                break;
            }
            let take = floor_char_boundary(record, remaining.min(record.len()));
            if take == 0 {
                break;
            }
            bytes += take;
            rows += record[..take]
                .chars()
                .filter(|ch| *ch == '\n')
                .count()
                .max(1);
            if take < record.len() || bytes >= budget.max_bytes {
                break;
            }
        }
        if bytes == 0 {
            return Ok(None);
        }
        let end = self.committed_byte + bytes;
        Ok(Some(PreparedPublication {
            identity: PublicationIdentity {
                attachment_epoch: self.attachment_epoch,
                base_revision: self.committed_revision,
                target_revision,
            },
            range: self.committed_byte..end,
            text: canonical[self.committed_byte..end].to_owned(),
        }))
    }

    pub(super) fn settle(
        &mut self,
        prepared: &PreparedPublication,
        result: DeliveryResult,
    ) -> Result<(), SettlementError> {
        if self.degraded {
            return Err(SettlementError::Degraded);
        }
        if prepared.identity.attachment_epoch != self.attachment_epoch {
            return Err(SettlementError::StaleAttachment);
        }
        if prepared.identity.base_revision != self.committed_revision {
            return Err(SettlementError::WrongRevision);
        }
        if prepared.range.start != self.committed_byte {
            return Err(SettlementError::NonContiguous);
        }
        match result {
            DeliveryResult::Committed => {
                self.committed_byte = prepared.range.end;
                self.committed_prefix_revision = canonical_prefix_revision_from_parts(
                    self.committed_prefix_revision,
                    prepared.text.as_bytes(),
                );
                self.committed_revision = prepared.identity.target_revision;
            }
            DeliveryResult::KnownFailure => {}
            DeliveryResult::Ambiguous => self.degraded = true,
        }
        Ok(())
    }

    pub(super) fn is_degraded(&self) -> bool {
        self.degraded
    }

    #[cfg(test)]
    fn committed_byte(&self) -> usize {
        self.committed_byte
    }
}

fn canonical_prefix_revision(text: &str) -> u64 {
    canonical_prefix_revision_from_parts(FNV_OFFSET, text.as_bytes())
}

fn canonical_prefix_revision_from_parts(mut revision: u64, bytes: &[u8]) -> u64 {
    for byte in bytes {
        revision ^= u64::from(*byte);
        revision = revision.wrapping_mul(FNV_PRIME);
    }
    revision
}

fn floor_char_boundary(text: &str, mut index: usize) -> usize {
    index = index.min(text.len());
    while index > 0 && !text.is_char_boundary(index) {
        index -= 1;
    }
    index
}

#[cfg(test)]
mod tests {
    use super::*;

    fn prepare_view(
        cursor: &AutomaticPublication,
        view: &super::super::conversation::ConversationView,
        budget: PreparationBudget,
    ) -> InlineBatch {
        cursor
            .prepare(
                view.publication_generation(),
                view.segments(),
                view.segments().len(),
                crate::surfaces::layout::UiPresentationLevel::Active,
                120,
                budget,
            )
            .unwrap()
    }

    #[test]
    fn notification_prune_preserves_surviving_partial_record_and_rejects_stale_batch() {
        let mut view = super::super::conversation::ConversationView::new();
        for index in 0..64 {
            view.push_user(&format!("old prompt {index}"));
            view.push_system(&format!("old notice {index}"));
        }
        let assistant_index = view.segments().len();
        let text = "SURVIVING-ASSISTANT-PARTIAL-CONTENT-ONCE";
        view.append_streaming(text);
        view.finalize_message();
        let mut cursor = AutomaticPublication::default();
        cursor.cursor.segment = assistant_index;
        let first = prepare_view(&cursor, &view, inline_budget());
        let mut rendered = first.lines.concat();
        cursor.settle(first, DeliveryResult::Committed);
        let old_byte = cursor.cursor.byte;
        let stale = prepare_view(&cursor, &view, inline_budget());
        view.push_system("new 65");
        view.push_system("new 66");
        let prune = view.take_publication_prune().unwrap();
        assert!(cursor.apply_prune(&prune));
        assert_eq!(cursor.cursor.byte, old_byte);
        assert_eq!(cursor.cursor.segment, assistant_index - 2);
        assert!(!cursor.settle(stale, DeliveryResult::Committed));
        let remaining = prepare_view(&cursor, &view, PreparationBudget::default());
        rendered.push_str(&remaining.lines.concat());
        assert_eq!(rendered.matches(text).count(), 1, "{rendered}");
        assert!(!rendered.contains("old notice"));
        assert!(rendered.contains("new 65") && rendered.contains("new 66"));
    }

    #[test]
    fn notification_prune_resets_evicted_partial_record_but_not_synthetic_notice() {
        let mut view = super::super::conversation::ConversationView::new();
        view.push_system(&"OLD-LONG-CONTENT-".repeat(10));
        for index in 1..64 {
            view.push_system(&format!("retained-{index}"));
        }
        let mut cursor = AutomaticPublication::default();
        let first = prepare_view(&cursor, &view, inline_budget());
        cursor.settle(first, DeliveryResult::Committed);
        assert!(cursor.cursor.byte > 0);
        view.push_system("new 65");
        let prune = view.take_publication_prune().unwrap();
        assert!(cursor.apply_prune(&prune));
        assert_eq!(cursor.cursor.byte, 0);
        let rendered = prepare_view(&cursor, &view, PreparationBudget::default())
            .lines
            .concat();
        assert!(rendered.contains("retained-1"));
        assert!(!rendered.contains("OLD-LONG"));

        let mut synthetic = AutomaticPublication::default();
        synthetic.cursor.notice = Some("synthetic partial attachment".into());
        synthetic.cursor.byte = 7;
        assert!(synthetic.apply_prune(&prune));
        assert_eq!(synthetic.cursor.byte, 7);
    }

    #[test]
    fn notification_prune_after_cursor_invalidates_prepared_batch() {
        let mut view = super::super::conversation::ConversationView::new();
        view.push_user("UNPUBLISHED-PROMPT");
        for index in 0..64 {
            view.push_system(&format!("notice {index}"));
        }
        let mut cursor = AutomaticPublication::default();
        let batch = prepare_view(&cursor, &view, inline_budget());
        view.push_system("new notice");
        let prune = view.take_publication_prune().unwrap();
        assert!(cursor.apply_prune(&prune));
        assert_eq!(cursor.cursor.segment, 0);
        assert!(!cursor.settle(batch, DeliveryResult::Committed));
        assert!(
            prepare_view(&cursor, &view, PreparationBudget::default())
                .lines
                .concat()
                .contains("UNPUBLISHED-PROMPT")
        );
    }

    fn inline_budget() -> PreparationBudget {
        PreparationBudget {
            max_bytes: 16,
            max_rows: 3,
            max_elapsed: Duration::from_secs(1),
            ..Default::default()
        }
    }

    #[test]
    fn automatic_chunks_bound_rows_and_preserve_unicode_source() {
        let source = "界abcé".repeat(200);
        let segments = vec![super::super::segments::Segment::system(&source)];
        let mut cursor = AutomaticPublication::default();
        let mut output = String::new();
        for _ in 0..2000 {
            let Some(batch) = cursor.prepare(
                0,
                &segments,
                1,
                crate::surfaces::layout::UiPresentationLevel::Active,
                4,
                inline_budget(),
            ) else {
                break;
            };
            assert!(batch.lines.len() <= 3);
            for line in &batch.lines {
                assert!(unicode_width::UnicodeWidthStr::width(line.as_str()) <= 4);
                output.push_str(line);
            }
            assert!(cursor.settle(batch, DeliveryResult::Committed));
        }
        assert_eq!(output, source);
        assert_eq!(cursor.cursor.segment, 1);
    }

    fn drain_stream(
        cursor: &mut AutomaticPublication,
        source: &[super::super::segments::Segment],
        boundary: usize,
        width: u16,
        detail: crate::surfaces::layout::UiPresentationLevel,
    ) -> Vec<String> {
        let mut lines = Vec::new();
        for _ in 0..1000 {
            let Some(batch) = cursor.prepare(0, source, boundary, detail, width, inline_budget())
            else {
                return lines;
            };
            lines.extend(batch.lines.clone());
            assert!(cursor.settle(batch, DeliveryResult::Committed));
        }
        panic!("publication did not converge");
    }

    #[test]
    fn streaming_tool_consolidation_waits_for_run_closure_then_releases_answer() {
        use super::super::{conversation::ConversationView, segments::SegmentContent};
        use crate::surfaces::layout::UiPresentationLevel::Active;
        let mut view = ConversationView::new();
        let mut cursor = AutomaticPublication::default();
        view.push_tool_start("a", "read", Some("first"), None);
        view.push_tool_end("a", false, Some("first result"));
        let held = cursor
            .prepare(0, view.segments(), 0, Active, 120, inline_budget())
            .unwrap();
        assert!(held.lines.is_empty());
        cursor.settle(held, DeliveryResult::Committed);
        view.push_tool_start("b", "read", Some("second"), None);
        view.push_tool_end("b", false, Some("second result"));
        let generation = view.publication_generation();
        cursor.reconcile(generation, view.take_publication_change(), 0);
        assert!(
            !cursor.is_degraded(),
            "unpublished run can reconcile consolidation"
        );
        assert_eq!(
            view.segments().len(),
            1,
            "compatible tools consolidate canonically"
        );
        let mut expected = crate::surfaces::episodes::OutcomeSummary::default();
        if let SegmentContent::ToolCard {
            name,
            result_summary,
            detail_result,
            is_error,
            ..
        } = &view.segments()[0].content
        {
            expected.observe(name, result_summary.as_deref(), *is_error);
            assert!(detail_result.as_ref().unwrap().contains("second result"));
        }
        view.append_streaming("answer after tools\n");
        let mut lines = Vec::new();
        while let Some(batch) =
            cursor.prepare(generation, view.segments(), 0, Active, 120, inline_budget())
        {
            lines.extend(batch.lines.clone());
            cursor.settle(batch, DeliveryResult::Committed);
        }
        assert_eq!(lines, [expected.display(), "answer after tools".into()]);
    }

    #[test]
    fn streaming_full_tool_run_pins_detail_across_batches_and_later_runs_publish() {
        use super::super::segments::{Segment, SegmentContent};
        use crate::surfaces::layout::UiPresentationLevel::{Active, Full};
        let mut source = Vec::new();
        for (id, args) in [("a", "args-one"), ("b", "args-two")] {
            let mut tool = Segment::tool_card(id, "read");
            tool.meta.turn = Some(1);
            if let SegmentContent::ToolCard {
                complete,
                args_summary,
                ..
            } = &mut tool.content
            {
                *complete = true;
                *args_summary = Some(args.into());
            }
            source.push(tool);
        }
        let mut answer = Segment::assistant_text();
        if let SegmentContent::AssistantText { text, complete, .. } = &mut answer.content {
            *text = "between tools\n".into();
            *complete = true;
        }
        source.push(answer);
        let mut cursor = AutomaticPublication::default();
        let budget = PreparationBudget {
            max_records: 1,
            ..inline_budget()
        };
        let mut lines = Vec::new();
        for _ in 0..10 {
            let batch = cursor.prepare(0, &source, 0, Full, 120, budget).unwrap();
            lines.extend(batch.lines.clone());
            cursor.settle(batch, DeliveryResult::Committed);
            if lines.concat().contains("args-one") {
                break;
            }
        }
        assert!(lines.concat().contains("args-one"));
        lines.extend(drain_stream(&mut cursor, &source, 0, 120, Active));
        assert_eq!(lines.concat().matches("args-one").count(), 1);
        assert_eq!(lines.concat().matches("args-two").count(), 1);
        let mut later = Segment::tool_card("c", "write");
        later.meta.turn = Some(1);
        if let SegmentContent::ToolCard { complete, .. } = &mut later.content {
            *complete = true;
        }
        source.push(later);
        source.push(Segment::system("following notice"));
        let later_lines = drain_stream(&mut cursor, &source, 0, 120, Active).concat();
        assert!(later_lines.contains("write"));
        assert!(later_lines.contains("following notice"));
    }

    #[test]
    fn streaming_mutable_plan_is_omitted_without_blocking_notices_or_answer() {
        use super::super::segments::{Segment, SegmentContent};
        use crate::surfaces::layout::UiPresentationLevel::Full;
        let mut answer = Segment::assistant_text();
        if let SegmentContent::AssistantText { text, .. } = &mut answer.content {
            *text = "visible answer\n".into();
        }
        let source = [
            Segment::system("Plan progress\nPlan mode: executing\nProgress: 1/2"),
            Segment::system("visible notice"),
            answer,
        ];
        let mut cursor = AutomaticPublication::default();
        assert_eq!(
            drain_stream(&mut cursor, &source, 0, 120, Full),
            ["visible notice", "visible answer"]
        );
    }

    #[test]
    fn streaming_generic_rewrite_pauses_without_replaying_prior_prefix() {
        use super::super::segments::{Segment, SegmentContent};
        use crate::surfaces::layout::UiPresentationLevel::Active;
        let mut answer = Segment::assistant_text();
        if let SegmentContent::AssistantText { text, .. } = &mut answer.content {
            *text = "already visible\npartial".into();
        }
        let mut cursor = AutomaticPublication::default();
        assert_eq!(
            drain_stream(&mut cursor, &[answer.clone()], 0, 120, Active),
            ["already visible"]
        );
        assert!(cursor.reconcile(1, 0, 0));
        assert!(
            cursor
                .degradation_message()
                .contains("Conversation changed")
        );
        assert!(
            cursor
                .prepare(1, &[answer], 0, Active, 120, inline_budget())
                .is_none()
        );
        cursor.attach(2, 0);
        assert!(!cursor.is_degraded());
    }

    #[test]
    fn streaming_controls_are_inert_across_chunks_and_do_not_poison_next_record() {
        use super::super::segments::{Segment, SegmentContent};
        use crate::surfaces::layout::UiPresentationLevel::Active;
        let mut source = vec![Segment::assistant_text()];
        let mut cursor = AutomaticPublication::default();
        let mut lines = Vec::new();
        for delta in [
            "A👩",
            "\x1b[",
            "0m\u{200d}",
            "💻B\n",
            "\x1b]52;c;private",
            "payload\x1b",
            "\\ok\n",
            "\x1bPhidden\x1b\\",
            "\x1b^hidden\x1b\\",
            "\x1b_hidden\x1b\\",
            "\x1bXhidden\x1b\\",
            "tail\x1b]unterminated",
        ] {
            if let SegmentContent::AssistantText { text, .. } = &mut source[0].content {
                text.push_str(delta);
            }
            lines.extend(drain_stream(&mut cursor, &source, 0, 2, Active));
        }
        if let SegmentContent::AssistantText { complete, .. } = &mut source[0].content {
            *complete = true;
        }
        source.push(Segment::system("next record"));
        lines.extend(drain_stream(&mut cursor, &source, source.len(), 2, Active));
        assert_eq!(lines.concat(), "A👩\u{200d}💻Boktailnext record");
        assert!(lines.iter().any(|line| line == "👩\u{200d}💻"));
    }

    #[test]
    fn streaming_resize_reflows_pending_row_and_full_thinking_arrives_after_answer() {
        use super::super::segments::{Segment, SegmentContent};
        use crate::surfaces::layout::UiPresentationLevel::Full;
        let mut source = vec![Segment::assistant_text()];
        let mut cursor = AutomaticPublication::default();
        if let SegmentContent::AssistantText { text, .. } = &mut source[0].content {
            text.push_str("abcdefghijklmnop");
        }
        assert!(drain_stream(&mut cursor, &source, 0, 80, Full).is_empty());
        let mut lines = drain_stream(&mut cursor, &source, 0, 4, Full);
        assert!(!lines.is_empty());
        if let SegmentContent::AssistantText {
            thinking, complete, ..
        } = &mut source[0].content
        {
            thinking.push_str("late thoughts");
            *complete = true;
        }
        lines.extend(drain_stream(&mut cursor, &source, 0, 4, Full));
        assert_eq!(lines.concat(), "abcdefghijklmnopThinking: late thoughts");
        assert!(
            lines
                .iter()
                .all(|line| unicode_width::UnicodeWidthStr::width(line.as_str()) <= 4)
        );
    }

    #[test]
    fn streaming_oversized_cluster_degrades_explicitly_and_new_attachment_recovers() {
        use super::super::segments::{Segment, SegmentContent};
        use crate::surfaces::layout::UiPresentationLevel::Active;
        let mut segment = Segment::assistant_text();
        if let SegmentContent::AssistantText { text, complete, .. } = &mut segment.content {
            *text = format!("e{}", "\u{301}".repeat(DEFAULT_MAX_BYTES));
            *complete = true;
        }
        let mut cursor = AutomaticPublication::default();
        assert!(
            cursor
                .prepare(0, &[segment], 1, Active, 80, PreparationBudget::default())
                .is_none()
        );
        assert!(cursor.is_degraded());
        assert!(
            cursor
                .degradation_message()
                .contains("Text exceeds inline buffer limit")
        );
        cursor.attach(1, 0);
        assert!(!cursor.is_degraded());
        let batch = cursor
            .prepare(
                1,
                &[Segment::system("recovered")],
                1,
                Active,
                80,
                PreparationBudget::default(),
            )
            .unwrap();
        assert_eq!(batch.lines, ["recovered"]);
    }

    #[test]
    fn streaming_budget_exhaustion_does_not_cache_a_permanent_stall() {
        use super::super::segments::{Segment, SegmentContent};
        use crate::surfaces::layout::UiPresentationLevel::Active;
        let mut segment = Segment::assistant_text();
        if let SegmentContent::AssistantText { text, .. } = &mut segment.content {
            *text = "ready row\n".into();
        }
        let source = [segment];
        let cursor = AutomaticPublication::default();
        assert!(
            cursor
                .prepare(
                    0,
                    &source,
                    0,
                    Active,
                    80,
                    PreparationBudget {
                        max_elapsed: Duration::ZERO,
                        ..inline_budget()
                    }
                )
                .is_none()
        );
        assert_eq!(
            cursor
                .prepare(0, &source, 0, Active, 80, inline_budget())
                .unwrap()
                .lines,
            ["ready row"]
        );
    }

    #[test]
    fn streaming_markdown_source_backpressure_bounds_ready_table_rows() {
        use super::super::segments::{Segment, SegmentContent};
        use crate::surfaces::layout::UiPresentationLevel::Active;
        let mut source = vec![Segment::assistant_text()];
        if let SegmentContent::AssistantText { text, .. } = &mut source[0].content {
            *text = "| Name | Long content |\n| --- | --- |\n".into();
            for index in 0..100 {
                text.push_str(&format!(
                    "| entry{index:03} | {} |\n",
                    "ordinary words ".repeat(40)
                ));
            }
        }
        let mut cursor = AutomaticPublication::default();
        let mut delivered = String::new();
        for _ in 0..10_000 {
            let Some(batch) = cursor.prepare(
                0,
                &source,
                0,
                Active,
                80,
                PreparationBudget {
                    max_rows: 1,
                    max_elapsed: Duration::from_secs(1),
                    ..Default::default()
                },
            ) else {
                break;
            };
            assert!(batch.lines.len() <= 1);
            assert!(
                batch.next.markdown.retained_bytes() < 8192,
                "one-row sink must stop source admission instead of buffering the whole table"
            );
            delivered.push_str(&batch.lines.concat());
            assert!(cursor.settle(batch, DeliveryResult::Committed));
        }
        for index in 0..100 {
            assert_eq!(delivered.matches(&format!("entry{index:03}")).count(), 1);
        }
        assert!(!cursor.is_degraded());
    }

    #[test]
    fn streaming_markdown_renders_heading_emphasis_and_code_before_completion() {
        use super::super::segments::{Segment, SegmentContent};
        use crate::surfaces::layout::UiPresentationLevel::Active;
        let mut source = vec![Segment::assistant_text()];
        if let SegmentContent::AssistantText { text, .. } = &mut source[0].content {
            *text = "## What distinguishes it\nThis is **Omegon** with `cargo test`.\n\n".into();
        }
        let output = drain_stream(&mut AutomaticPublication::default(), &source, 0, 80, Active);
        assert_eq!(
            output,
            [
                "What distinguishes it",
                "This is Omegon with cargo test.",
                ""
            ]
        );
    }

    #[test]
    fn streaming_markdown_wraps_prose_at_words_before_completion() {
        use super::super::segments::{Segment, SegmentContent};
        use crate::surfaces::layout::UiPresentationLevel::Active;
        let mut source = vec![Segment::assistant_text()];
        if let SegmentContent::AssistantText { text, .. } = &mut source[0].content {
            *text = "Persistent project context keeps engineering decisions readable.\n".into();
        }
        let output = drain_stream(&mut AutomaticPublication::default(), &source, 0, 24, Active);
        for word in [
            "Persistent",
            "project",
            "context",
            "keeps",
            "engineering",
            "decisions",
            "readable.",
        ] {
            assert!(
                output.iter().any(|line| line.contains(word)),
                "word split: {word}; {output:?}"
            );
        }
    }

    #[test]
    fn streaming_answer_publishes_stable_rows_before_message_end_without_replay() {
        use super::super::segments::{Segment, SegmentContent};
        let mut source = vec![Segment::assistant_text()];
        let mut cursor = AutomaticPublication::default();
        let mut delivered = Vec::new();
        for index in 0..24 {
            if let SegmentContent::AssistantText { text, .. } = &mut source[0].content {
                text.push_str(&format!("stream row {index:02}\n"));
            }
            let batch = cursor
                .prepare(
                    0,
                    &source,
                    0,
                    crate::surfaces::layout::UiPresentationLevel::Active,
                    80,
                    inline_budget(),
                )
                .expect("complete streaming rows belong in primary scrollback before MessageEnd");
            delivered.extend(batch.lines.clone());
            cursor.settle(batch, DeliveryResult::Committed);
        }
        assert_eq!(delivered.len(), 24);
        if let SegmentContent::AssistantText { text, complete, .. } = &mut source[0].content {
            text.push_str("final fragment");
            *complete = true;
        }
        while let Some(batch) = cursor.prepare(
            0,
            &source,
            0,
            crate::surfaces::layout::UiPresentationLevel::Active,
            80,
            inline_budget(),
        ) {
            delivered.extend(batch.lines.clone());
            cursor.settle(batch, DeliveryResult::Committed);
        }
        assert_eq!(
            delivered
                .iter()
                .filter(|line| line.as_str() == "final fragment")
                .count(),
            1
        );
        assert_eq!(
            delivered
                .iter()
                .filter(|line| line.as_str() == "stream row 00")
                .count(),
            1
        );
        assert!(
            cursor
                .prepare(
                    0,
                    &source,
                    source.len(),
                    crate::surfaces::layout::UiPresentationLevel::Active,
                    80,
                    inline_budget()
                )
                .is_none()
        );
    }

    #[test]
    fn streaming_unbroken_answer_publishes_rows_and_preserves_split_graphemes() {
        use super::super::segments::{Segment, SegmentContent};
        let mut source = vec![Segment::assistant_text()];
        let mut cursor = AutomaticPublication::default();
        let mut delivered = Vec::new();
        for delta in ["abcdefghijklmnop", "e", "\u{301}👩", "\u{200d}💻", " tail"] {
            if let SegmentContent::AssistantText { text, .. } = &mut source[0].content {
                text.push_str(delta);
            }
            while let Some(batch) = cursor.prepare(
                0,
                &source,
                0,
                crate::surfaces::layout::UiPresentationLevel::Active,
                8,
                inline_budget(),
            ) {
                delivered.extend(batch.lines.clone());
                cursor.settle(batch, DeliveryResult::Committed);
            }
        }
        assert!(
            !delivered.is_empty(),
            "unbroken output cannot wait until completion"
        );
        if let SegmentContent::AssistantText { complete, .. } = &mut source[0].content {
            *complete = true;
        }
        while let Some(batch) = cursor.prepare(
            0,
            &source,
            1,
            crate::surfaces::layout::UiPresentationLevel::Active,
            8,
            inline_budget(),
        ) {
            delivered.extend(batch.lines.clone());
            cursor.settle(batch, DeliveryResult::Committed);
        }
        assert_eq!(
            delivered.concat(),
            "abcdefghijklmnope\u{301}👩\u{200d}💻 tail"
        );
        assert!(delivered.iter().any(|line| line.contains("e\u{301}")));
        assert!(delivered.iter().any(|line| line.contains("👩\u{200d}💻")));
    }

    #[test]
    fn automatic_holds_unfinished_row_and_does_not_replay_uncertain_delivery() {
        let mut segment = super::super::segments::Segment::assistant_text();
        if let super::super::segments::SegmentContent::AssistantText { text, .. } =
            &mut segment.content
        {
            *text = "answer".into();
        }
        let source = vec![segment];
        let mut cursor = AutomaticPublication::default();
        let prepare = |cursor: &AutomaticPublication, boundary| {
            cursor.prepare(
                0,
                &source,
                boundary,
                crate::surfaces::layout::UiPresentationLevel::Active,
                80,
                inline_budget(),
            )
        };
        let held = prepare(&cursor, 0).expect("unfinished tail can advance bounded scratch");
        assert!(held.lines.is_empty());
        cursor.settle(held, DeliveryResult::Committed);
        let batch = prepare(&cursor, 1).unwrap();
        assert!(cursor.settle(batch, DeliveryResult::KnownFailure));
        assert_eq!(cursor.cursor.segment, 0);
        let batch = prepare(&cursor, 1).unwrap();
        assert!(cursor.settle(batch, DeliveryResult::Ambiguous));
        assert!(prepare(&cursor, 1).is_none());
        cursor.attach(1, 0);
        assert!(cursor.is_degraded());
    }

    #[test]
    fn automatic_rejects_stale_generation_and_zero_geometry() {
        let source = vec![super::super::segments::Segment::system("answer")];
        let mut cursor = AutomaticPublication::default();
        assert!(
            cursor
                .prepare(
                    0,
                    &source,
                    1,
                    crate::surfaces::layout::UiPresentationLevel::Full,
                    0,
                    inline_budget()
                )
                .is_none()
        );
        let batch = cursor
            .prepare(
                0,
                &source,
                1,
                crate::surfaces::layout::UiPresentationLevel::Full,
                80,
                inline_budget(),
            )
            .unwrap();
        cursor.attach(1, 1);
        assert!(!cursor.settle(batch, DeliveryResult::Committed));
    }

    #[test]
    fn automatic_resume_and_replacement_publish_only_a_bounded_boundary_notice() {
        let source = vec![super::super::segments::Segment::system(
            "OLD_HISTORY_MUST_NOT_REPLAY",
        )];
        let mut cursor = AutomaticPublication::default();
        cursor.attach(7, 1);
        let batch = cursor
            .prepare(
                7,
                &source,
                1,
                crate::surfaces::layout::UiPresentationLevel::Active,
                90,
                PreparationBudget::default(),
            )
            .unwrap();
        assert!(batch.lines.concat().contains("1 prior records"));
        assert!(!batch.lines.concat().contains("OLD_HISTORY"));
        cursor.settle(batch, DeliveryResult::Committed);
        assert!(
            cursor
                .prepare(
                    7,
                    &source,
                    1,
                    crate::surfaces::layout::UiPresentationLevel::Active,
                    90,
                    PreparationBudget::default()
                )
                .is_none()
        );
        cursor.source_replaced(8, 1);
        let batch = cursor
            .prepare(
                8,
                &source,
                1,
                crate::surfaces::layout::UiPresentationLevel::Active,
                90,
                PreparationBudget::default(),
            )
            .unwrap();
        assert!(
            batch
                .lines
                .concat()
                .contains("Conversation boundary changed")
        );
        assert!(!batch.lines.concat().contains("OLD_HISTORY"));
    }

    #[test]
    fn automatic_clock_limits_work_without_scanning_committed_history() {
        let mut source = (0..10000)
            .map(|_| super::super::segments::Segment::system("old"))
            .collect::<Vec<_>>();
        source.push(super::super::segments::Segment::system("new".repeat(10000)));
        let mut cursor = AutomaticPublication::default();
        cursor.cursor.segment = 10000;
        let mut ticks = 0;
        let batch = cursor
            .prepare_with_elapsed(
                0,
                &source,
                source.len(),
                crate::surfaces::layout::UiPresentationLevel::Full,
                80,
                PreparationBudget {
                    max_elapsed: Duration::from_millis(5),
                    ..Default::default()
                },
                || {
                    ticks += 1;
                    Duration::from_millis(ticks)
                },
            )
            .unwrap();
        assert_eq!(batch.next.segment, 10000);
        assert!(batch.next.byte > 0 && batch.next.byte < 10);
        assert!(!batch.lines.concat().contains("old"));
        assert!(ticks <= 5);
    }

    #[test]
    fn automatic_outcome_scan_is_bounded_and_shares_semantic_reducer() {
        use super::super::segments::{Segment, SegmentContent};
        let mut expected = crate::surfaces::episodes::OutcomeSummary::default();
        let source = (0..100)
            .map(|index| {
                let mut segment = Segment::tool_card(index.to_string(), "read");
                segment.meta.runtime_turn = Some(1);
                segment.meta.turn = Some(1);
                if let SegmentContent::ToolCard {
                    result_summary,
                    complete,
                    is_error,
                    ..
                } = &mut segment.content
                {
                    *result_summary = Some(format!("result {index}"));
                    *complete = true;
                    *is_error = index == 33;
                    expected.observe("read", result_summary.as_deref(), *is_error);
                }
                segment
            })
            .collect::<Vec<_>>();
        let mut cursor = AutomaticPublication::default();
        let budget = PreparationBudget {
            max_records: 3,
            max_elapsed: Duration::from_secs(1),
            ..Default::default()
        };
        let mut output = Vec::new();
        let mut batches = 0;
        while let Some(batch) = cursor.prepare(
            0,
            &source,
            source.len(),
            crate::surfaces::layout::UiPresentationLevel::Active,
            120,
            budget,
        ) {
            output.extend(batch.lines.clone());
            assert!(cursor.settle(batch, DeliveryResult::Committed));
            batches += 1;
            assert!(batches < 200);
        }
        assert!(batches > 30);
        assert_eq!(output.join(""), expected.display());
    }

    #[test]
    fn automatic_rewrite_distinguishes_pending_coalescence_from_replacement() {
        let mut cursor = AutomaticPublication::default();
        cursor.attach(0, 5);
        assert!(!cursor.reconcile(1, 5, 5));
        assert_eq!(cursor.cursor.segment, 5);
        assert_eq!(cursor.generation(), 1);
        assert!(cursor.reconcile(2, 0, 2));
        assert_eq!(cursor.cursor.segment, 5);
        assert!(cursor.is_degraded());
    }

    #[test]
    fn automatic_resize_and_detail_change_preserve_partial_record() {
        use super::super::segments::{Segment, SegmentContent};
        use crate::surfaces::layout::UiPresentationLevel;
        let mut segment = Segment::assistant_text();
        if let SegmentContent::AssistantText { text, thinking, .. } = &mut segment.content {
            *text = "answer ".repeat(30);
            *thinking = "thinking ".repeat(30);
        }
        let source = vec![segment];
        let mut cursor = AutomaticPublication::default();
        let first = cursor
            .prepare(
                0,
                &source,
                1,
                UiPresentationLevel::Active,
                40,
                inline_budget(),
            )
            .unwrap();
        let mut output = first.lines.concat();
        cursor.settle(first, DeliveryResult::Committed);
        for _ in 0..1000 {
            let Some(batch) = cursor.prepare(
                0,
                &source,
                1,
                UiPresentationLevel::Full,
                56,
                inline_budget(),
            ) else {
                break;
            };
            output.push_str(&batch.lines.concat());
            cursor.settle(batch, DeliveryResult::Committed);
        }
        assert_eq!(output, "answer ".repeat(30));
    }

    #[test]
    fn automatic_untrusted_control_text_cannot_select_a_terminal_buffer() {
        let source = vec![super::super::segments::Segment::system(
            "safe\x1b[?1049hBAD\x1b]52;c;secret\x07done",
        )];
        let cursor = AutomaticPublication::default();
        let batch = cursor
            .prepare(
                0,
                &source,
                1,
                crate::surfaces::layout::UiPresentationLevel::Full,
                80,
                PreparationBudget::default(),
            )
            .unwrap();
        assert!(
            batch
                .lines
                .iter()
                .all(|line| !line.chars().any(char::is_control))
        );
    }

    fn small_budget(max_bytes: usize) -> PreparationBudget {
        PreparationBudget {
            max_bytes,
            max_records: 10,
            max_rows: 10,
            max_elapsed: Duration::from_secs(1),
        }
    }

    #[test]
    fn successful_chunk_commits_only_contiguous_range() {
        let mut state = NativePublicationState::default();
        let first = state
            .prepare("abcdef", 1, small_budget(3))
            .unwrap()
            .unwrap();
        assert_eq!(first.range, 0..3);
        state.settle(&first, DeliveryResult::Committed).unwrap();
        let second = state
            .prepare("abcdef", 2, small_budget(3))
            .unwrap()
            .unwrap();
        assert_eq!(second.range, 3..6);
    }

    #[test]
    fn known_failure_preserves_cursor() {
        let mut state = NativePublicationState::default();
        let prepared = state
            .prepare("abcdef", 1, small_budget(3))
            .unwrap()
            .unwrap();
        state
            .settle(&prepared, DeliveryResult::KnownFailure)
            .unwrap();
        assert_eq!(state.committed_byte(), 0);
    }

    #[test]
    fn stale_attachment_and_noncontiguous_ranges_cannot_commit() {
        let mut state = NativePublicationState::default();
        let stale = state
            .prepare("abcdef", 1, small_budget(3))
            .unwrap()
            .unwrap();
        state.begin_attachment();
        assert_eq!(
            state.settle(&stale, DeliveryResult::Committed),
            Err(SettlementError::StaleAttachment)
        );

        let mut noncontiguous = state
            .prepare("abcdef", 1, small_budget(3))
            .unwrap()
            .unwrap();
        noncontiguous.range = 1..3;
        assert_eq!(
            state.settle(&noncontiguous, DeliveryResult::Committed),
            Err(SettlementError::NonContiguous)
        );
    }

    #[test]
    fn growing_canonical_transcript_resumes_after_committed_prefix() {
        let mut state = NativePublicationState::default();
        let first = state.prepare("abc", 10, small_budget(3)).unwrap().unwrap();
        state.settle(&first, DeliveryResult::Committed).unwrap();

        let appended = state
            .prepare("abcdef", 11, small_budget(3))
            .unwrap()
            .unwrap();
        assert_eq!(appended.range, 3..6);
        assert_eq!(appended.text, "def");
    }

    #[test]
    fn changed_canonical_prefix_requires_snapshot_rebuild() {
        let mut state = NativePublicationState::default();
        let first = state
            .prepare("abcdef", 1, small_budget(3))
            .unwrap()
            .unwrap();
        state.settle(&first, DeliveryResult::Committed).unwrap();

        assert_eq!(
            state.prepare("xyzdef", 2, small_budget(3)),
            Err(SettlementError::NonContiguous)
        );
    }

    #[test]
    fn ambiguous_delivery_disables_blind_retry() {
        let mut state = NativePublicationState::default();
        let prepared = state
            .prepare("abcdef", 1, small_budget(3))
            .unwrap()
            .unwrap();
        state.settle(&prepared, DeliveryResult::Ambiguous).unwrap();
        assert!(state.is_degraded());
        assert_eq!(
            state.prepare("abcdef", 1, small_budget(3)),
            Err(SettlementError::Degraded)
        );
    }

    #[test]
    fn oversized_unicode_chunk_ends_on_utf8_boundary_and_resumes() {
        let mut state = NativePublicationState::default();
        let canonical = "αβγδ";
        let first = state
            .prepare(canonical, 1, small_budget(5))
            .unwrap()
            .unwrap();
        assert_eq!(first.text, "αβ");
        assert!(canonical.is_char_boundary(first.range.end));
        state.settle(&first, DeliveryResult::Committed).unwrap();
        let second = state
            .prepare(canonical, 2, small_budget(5))
            .unwrap()
            .unwrap();
        assert_eq!(second.text, "γδ");
    }
}
