//! F* IDE protocol message types.
//!
//! These types mirror the F* IDE protocol as documented at:
//! https://github.com/FStarLang/FStar/wiki/Editor-support-for-F*

use serde::{Deserialize, Serialize};

/// Position in F* (1-based line, 0-based column)
pub type FStarPosition = (u32, u32);

/// A range in a source file
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct FStarRange {
    pub fname: String,
    pub beg: FStarPosition,
    pub end: FStarPosition,
}

// ============================================================================
// Protocol Info (sent by F* on startup)
// ============================================================================

#[derive(Debug, Clone, Deserialize)]
pub struct ProtocolInfo {
    #[allow(dead_code)]
    kind: String, // "protocol-info"
    pub version: i32,
    pub features: Vec<String>,
}

impl ProtocolInfo {
    pub fn is_protocol_info(value: &serde_json::Value) -> bool {
        value.get("kind").and_then(|k| k.as_str()) == Some("protocol-info")
    }

    pub fn supports_full_buffer(&self) -> bool {
        self.features.contains(&"full-buffer".to_string())
    }
}

// ============================================================================
// Response Types
// ============================================================================

/// Base response structure
#[derive(Debug, Clone, Deserialize)]
pub struct IdeResponseBase {
    #[serde(rename = "query-id")]
    pub query_id: String,
    #[allow(dead_code)]
    kind: String, // "response" or "message"
    #[serde(default)]
    pub status: Option<String>, // "success", "failure", "protocol-violation"
    #[serde(default)]
    pub response: Option<serde_json::Value>,
}

/// Diagnostic from F*
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IdeDiagnostic {
    pub message: String,
    pub number: i32,
    pub level: String, // "warning", "error", "info"
    pub ranges: Vec<FStarRange>,
}

/// Symbol lookup response
#[derive(Debug, Clone, Deserialize)]
pub struct IdeSymbol {
    pub name: String,
    #[serde(rename = "type")]
    pub type_info: Option<String>,
    pub documentation: Option<String>,
    #[serde(rename = "defined-at")]
    pub defined_at: Option<FStarRange>,
}

/// Module lookup response
#[derive(Debug, Clone, Deserialize)]
pub struct IdeModule {
    pub name: String,
    pub path: String,
}

/// Lookup response (either symbol or module)
#[derive(Debug, Clone, Deserialize)]
#[serde(untagged)]
pub enum IdeLookupResponse {
    Symbol(IdeSymbol),
    Module(IdeModule),
}

/// Proof state from tactics
#[derive(Debug, Clone, Deserialize, serde::Serialize)]
pub struct IdeProofState {
    pub label: String,
    pub depth: i32,
    pub urgency: i32,
    pub goals: Vec<IdeProofStateContextualGoal>,
    #[serde(rename = "smt-goals")]
    pub smt_goals: Vec<IdeProofStateContextualGoal>,
    pub location: FStarRange,
}

#[derive(Debug, Clone, Deserialize, serde::Serialize)]
pub struct IdeProofStateContextualGoal {
    pub hyps: Vec<IdeHypothesis>,
    pub goal: IdeProofStateGoal,
}

#[derive(Debug, Clone, Deserialize, serde::Serialize)]
pub struct IdeHypothesis {
    pub name: String,
    #[serde(rename = "type")]
    pub type_info: String,
}

#[derive(Debug, Clone, Deserialize, serde::Serialize)]
pub struct IdeProofStateGoal {
    pub witness: String,
    #[serde(rename = "type")]
    pub type_info: String,
    pub label: String,
}
