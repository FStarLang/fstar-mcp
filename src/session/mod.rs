//! Session management for F* MCP server.

pub mod types;

use crate::fstar::{FStarConfig, FStarProcess, FullBufferResult, IdeProofState, ProcessError};
use chrono::{DateTime, Utc};
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use thiserror::Error;
use tokio::sync::RwLock;
use uuid::Uuid;

pub use types::*;

/// Default sweep period in seconds for cleaning up marked sessions
pub const DEFAULT_SWEEP_PERIOD_SECS: u64 = 300;

#[derive(Error, Debug)]
pub enum SessionError {
    #[error("Session not found: {0}")]
    NotFound(String),
    #[error("Failed to create session: {0}")]
    CreateError(#[from] ProcessError),
    #[error("Config error: {0}")]
    ConfigError(#[from] crate::fstar::ConfigError),
}

/// A single F* session
pub struct Session {
    pub id: String,
    pub file_path: PathBuf,
    pub process: FStarProcess,
    pub created_at: DateTime<Utc>,
    pub last_activity: DateTime<Utc>,
    /// Proof states collected from the last typecheck (from tactics)
    pub proof_states: Vec<IdeProofState>,
    /// The MCP client session that owns this F* session
    pub mcp_session_id: Option<String>,
    /// Whether this session is marked for deletion (will be cleaned up by sweeper)
    pub marked_for_deletion: bool,
}

impl Session {
    /// Create a new session
    pub async fn new(
        file_path: &Path,
        config: FStarConfig,
        lax: bool,
    ) -> Result<Self, SessionError> {
        let id = Uuid::new_v4().to_string();
        let process = FStarProcess::spawn(config, file_path, lax).await?;
        let now = Utc::now();

        Ok(Session {
            id,
            file_path: file_path.to_path_buf(),
            process,
            created_at: now,
            last_activity: now,
            proof_states: Vec::new(),
            mcp_session_id: None,
            marked_for_deletion: false,
        })
    }

    /// Update last activity timestamp
    pub fn touch(&mut self) {
        self.last_activity = Utc::now();
    }

    /// Run initial typecheck and store proof states
    pub async fn typecheck(&mut self, code: &str) -> Result<FullBufferResult, ProcessError> {
        self.touch();
        let result = self.process.full_buffer_query(code, "full", None).await?;
        self.proof_states = result.proof_states.clone();
        Ok(result)
    }

    /// Run typecheck with specified kind and store proof states
    pub async fn typecheck_with_kind(
        &mut self,
        code: &str,
        kind: &str,
        to_position: Option<(u32, u32)>,
    ) -> Result<FullBufferResult, ProcessError> {
        self.touch();
        let result = self.process.full_buffer_query(code, kind, to_position).await?;
        self.proof_states = result.proof_states.clone();
        Ok(result)
    }

    /// Find proof state at a given line
    pub fn find_proof_state_at_line(&self, line: u32) -> Option<&IdeProofState> {
        self.proof_states.iter().find(|ps| ps.location.beg.0 == line)
    }

    /// Get all proof states
    pub fn get_proof_states(&self) -> &[IdeProofState] {
        &self.proof_states
    }
}

/// Session info for list_sessions response
#[derive(Debug, Clone, serde::Serialize)]
pub struct SessionInfo {
    pub session_id: String,
    pub file_path: String,
    pub created_at: String,
    pub last_activity: String,
    pub idle_seconds: i64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mcp_session_id: Option<String>,
    pub marked_for_deletion: bool,
}

/// Manages multiple F* sessions
pub struct SessionManager {
    pub sessions: Arc<RwLock<HashMap<String, Session>>>,
    /// Maps file paths to session IDs for auto-replacement
    file_to_session: Arc<RwLock<HashMap<PathBuf, String>>>,
    /// Maps MCP session IDs to F* session IDs they own
    mcp_to_fstar_sessions: Arc<RwLock<HashMap<String, HashSet<String>>>>,
}

impl Default for SessionManager {
    fn default() -> Self {
        Self::new()
    }
}

impl SessionManager {
    pub fn new() -> Self {
        SessionManager {
            sessions: Arc::new(RwLock::new(HashMap::new())),
            file_to_session: Arc::new(RwLock::new(HashMap::new())),
            mcp_to_fstar_sessions: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// Create a new session, replacing any existing session for the same file
    pub async fn create_session(
        &self,
        file_path: &Path,
        config: FStarConfig,
    ) -> Result<String, SessionError> {
        self.create_session_with_mcp(file_path, config, None).await
    }

    /// Create a new session with MCP session tracking
    pub async fn create_session_with_mcp(
        &self,
        file_path: &Path,
        config: FStarConfig,
        mcp_session_id: Option<String>,
    ) -> Result<String, SessionError> {
        // Check for existing session for this file
        let existing_session_id = {
            let file_map = self.file_to_session.read().await;
            file_map.get(file_path).cloned()
        };

        // Close existing session if any
        if let Some(old_id) = existing_session_id {
            self.close_session(&old_id).await.ok();
        }

        // Create new session
        let mut session = Session::new(file_path, config, false).await?;
        session.mcp_session_id = mcp_session_id.clone();
        let session_id = session.id.clone();
        let file_path_owned = file_path.to_path_buf();

        // Store session
        {
            let mut sessions = self.sessions.write().await;
            sessions.insert(session_id.clone(), session);
        }

        // Update file mapping
        {
            let mut file_map = self.file_to_session.write().await;
            file_map.insert(file_path_owned, session_id.clone());
        }

        // Track MCP session ownership
        if let Some(mcp_id) = mcp_session_id {
            let mut mcp_map = self.mcp_to_fstar_sessions.write().await;
            mcp_map
                .entry(mcp_id)
                .or_insert_with(HashSet::new)
                .insert(session_id.clone());
        }

        Ok(session_id)
    }

    /// List all active sessions (excludes sessions marked for deletion)
    pub async fn list_sessions(&self) -> Vec<SessionInfo> {
        let sessions = self.sessions.read().await;
        let now = Utc::now();
        
        sessions.values()
            .filter(|s| !s.marked_for_deletion)
            .map(|s| {
                let idle_seconds = (now - s.last_activity).num_seconds();
                SessionInfo {
                    session_id: s.id.clone(),
                    file_path: s.file_path.to_string_lossy().to_string(),
                    created_at: s.created_at.to_rfc3339(),
                    last_activity: s.last_activity.to_rfc3339(),
                    idle_seconds,
                    mcp_session_id: s.mcp_session_id.clone(),
                    marked_for_deletion: s.marked_for_deletion,
                }
            }).collect()
    }

    /// Mark all sessions belonging to an MCP session for deletion
    pub async fn mark_sessions_for_deletion(&self, mcp_session_id: &str) {
        // Get F* session IDs owned by this MCP session
        let fstar_session_ids = {
            let mcp_map = self.mcp_to_fstar_sessions.read().await;
            mcp_map.get(mcp_session_id).cloned().unwrap_or_default()
        };

        if fstar_session_ids.is_empty() {
            return;
        }

        tracing::info!(
            mcp_session = %mcp_session_id,
            session_count = fstar_session_ids.len(),
            "Marking F* sessions for deletion"
        );

        // Mark each session
        let mut sessions = self.sessions.write().await;
        for session_id in &fstar_session_ids {
            if let Some(session) = sessions.get_mut(session_id) {
                session.marked_for_deletion = true;
            }
        }
    }

    /// Sweep and delete all sessions marked for deletion
    pub async fn sweep_marked_sessions(&self) -> usize {
        let sessions_to_delete: Vec<String> = {
            let sessions = self.sessions.read().await;
            sessions
                .values()
                .filter(|s| s.marked_for_deletion)
                .map(|s| s.id.clone())
                .collect()
        };

        let count = sessions_to_delete.len();
        if count > 0 {
            tracing::info!(count = count, "Sweeping marked sessions");
        }

        for session_id in sessions_to_delete {
            self.close_session(&session_id).await.ok();
        }

        count
    }

    /// Close a session
    pub async fn close_session(&self, session_id: &str) -> Result<(), SessionError> {
        let session = {
            let mut sessions = self.sessions.write().await;
            sessions.remove(session_id)
        };

        if let Some(mut session) = session {
            // Remove from file mapping
            {
                let mut file_map = self.file_to_session.write().await;
                file_map.remove(&session.file_path);
            }

            // Remove from MCP session mapping
            if let Some(mcp_id) = &session.mcp_session_id {
                let mut mcp_map = self.mcp_to_fstar_sessions.write().await;
                if let Some(fstar_ids) = mcp_map.get_mut(mcp_id) {
                    fstar_ids.remove(session_id);
                    if fstar_ids.is_empty() {
                        mcp_map.remove(mcp_id);
                    }
                }
            }

            // Kill the process
            session.process.kill().await.ok();
            Ok(())
        } else {
            Err(SessionError::NotFound(session_id.to_string()))
        }
    }
}
