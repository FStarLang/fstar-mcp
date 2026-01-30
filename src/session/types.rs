//! Session types for MCP responses.

use crate::fstar::{FragmentResult, FragmentStatus, FStarRange, IdeDiagnostic};
use serde::{Deserialize, Serialize};

/// Response from create_fstar tool
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateFStarResponse {
    pub session_id: String,
    pub status: String, // "ok" or "error"
    pub diagnostics: Vec<DiagnosticInfo>,
    pub fragments: Vec<FragmentInfo>,
    pub created_at: String,
}

/// Response from typecheck_buffer tool
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TypecheckResponse {
    pub status: String, // "ok" or "error"
    pub diagnostics: Vec<DiagnosticInfo>,
    pub fragments: Vec<FragmentInfo>,
}

/// Simplified diagnostic for MCP responses
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiagnosticInfo {
    pub level: String,
    pub message: String,
    pub file: String,
    pub start_line: u32,
    pub start_column: u32,
    pub end_line: u32,
    pub end_column: u32,
}

impl From<&IdeDiagnostic> for DiagnosticInfo {
    fn from(diag: &IdeDiagnostic) -> Self {
        let (file, start_line, start_column, end_line, end_column) = if let Some(range) = diag.ranges.first() {
            (
                range.fname.clone(),
                range.beg.0,
                range.beg.1,
                range.end.0,
                range.end.1,
            )
        } else {
            (String::new(), 0, 0, 0, 0)
        };

        DiagnosticInfo {
            level: diag.level.clone(),
            message: diag.message.clone(),
            file,
            start_line,
            start_column,
            end_line,
            end_column,
        }
    }
}

/// Fragment status for MCP responses
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FragmentInfo {
    pub start_line: u32,
    pub start_column: u32,
    pub end_line: u32,
    pub end_column: u32,
    pub status: String, // "ok", "lax-ok", "failed", "in-progress"
}

impl From<&FragmentResult> for FragmentInfo {
    fn from(frag: &FragmentResult) -> Self {
        FragmentInfo {
            start_line: frag.range.beg.0,
            start_column: frag.range.beg.1,
            end_line: frag.range.end.0,
            end_column: frag.range.end.1,
            status: match frag.status {
                FragmentStatus::Ok => "ok".to_string(),
                FragmentStatus::LaxOk => "lax-ok".to_string(),
                FragmentStatus::Failed => "failed".to_string(),
                FragmentStatus::InProgress => "in-progress".to_string(),
            },
        }
    }
}

/// Response from update_buffer tool
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UpdateBufferResponse {
    pub status: String,
}

/// Response from lookup_symbol tool
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LookupResponse {
    pub kind: String, // "symbol", "module", or "not_found"
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub type_info: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub documentation: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub defined_at: Option<RangeInfo>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RangeInfo {
    pub file: String,
    pub start_line: u32,
    pub start_column: u32,
    pub end_line: u32,
    pub end_column: u32,
}

impl From<&FStarRange> for RangeInfo {
    fn from(range: &FStarRange) -> Self {
        RangeInfo {
            file: range.fname.clone(),
            start_line: range.beg.0,
            start_column: range.beg.1,
            end_line: range.end.0,
            end_column: range.end.1,
        }
    }
}

/// Response from restart_solver tool
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RestartSolverResponse {
    pub status: String,
}

/// Response from close_session tool
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CloseSessionResponse {
    pub status: String,
}
