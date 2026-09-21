//! Snapshot-bound request passed from the tool owner to the runtime permission handler.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub(crate) struct ConfirmationRequest {
    pub candidate: omegon_memory::FactPrecondition,
    pub snapshot_hash: String,
    pub session_id: String,
    pub request_id: String,
    pub supersedes: Option<omegon_memory::FactPrecondition>,
}

#[derive(Debug)]
pub(crate) struct MemoryConfirmationRequired {
    pub request: ConfirmationRequest,
    pub prompt: String,
}
impl std::fmt::Display for MemoryConfirmationRequired {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("memory candidate requires operator confirmation")
    }
}
impl std::error::Error for MemoryConfirmationRequired {}

pub(super) fn readable(text: &str) -> String {
    text.chars()
        .map(|character| {
            if character.is_control() && character != '\n' {
                character.escape_default().to_string()
            } else {
                character.to_string()
            }
        })
        .collect()
}
