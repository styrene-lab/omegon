use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ClaimRecord {
    pub schema: String,
    pub id: String,
    pub kind: String,
    pub text: String,
    pub status: String,
    #[serde(default)]
    pub scope: Vec<String>,
    pub created_at_ms: u128,
    #[serde(default)]
    pub metadata: Value,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct EvidenceRecord {
    pub schema: String,
    pub id: String,
    pub provider: String,
    pub kind: String,
    pub status: String,
    #[serde(default)]
    pub subjects: Vec<String>,
    #[serde(default)]
    pub claims: Vec<String>,
    #[serde(default)]
    pub artifacts: Vec<String>,
    #[serde(default)]
    pub source_state: Value,
    pub created_at_ms: u128,
    #[serde(default)]
    pub metadata: Value,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct EvidenceEdge {
    pub schema: String,
    #[serde(rename = "from")]
    pub from_id: String,
    #[serde(rename = "to")]
    pub to_id: String,
    pub kind: String,
    pub created_at_ms: u128,
}
