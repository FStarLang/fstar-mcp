//! F* IDE process management.

use crate::fstar::config::FStarConfig;
use crate::fstar::messages::*;
use crate::fstar::protocol::{parse_response, FStarResponse, JsonlInterface};
use crate::is_verbose;
use std::path::Path;
use std::process::Stdio;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use thiserror::Error;
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::{Child, Command};
use tokio::sync::mpsc;

#[derive(Error, Debug)]
pub enum ProcessError {
    #[error("Failed to spawn F* process: {0}")]
    SpawnError(#[from] std::io::Error),
    #[error("F* executable not found: {0}")]
    ExecutableNotFound(String),
    #[error("F* process exited unexpectedly with code {0:?}")]
    ProcessExited(Option<i32>),
    #[error("Failed to send message to F*: {0}")]
    SendError(String),
    #[error("F* does not support full-buffer mode")]
    NoFullBufferSupport,
    #[error("Query timed out")]
    Timeout,
}

/// Result of a full-buffer query
#[derive(Debug, Clone, Default)]
pub struct FullBufferResult {
    pub diagnostics: Vec<IdeDiagnostic>,
    pub fragments: Vec<FragmentResult>,
    pub proof_states: Vec<IdeProofState>,
    pub finished: bool,
}

/// Result for a single fragment
#[derive(Debug, Clone)]
pub struct FragmentResult {
    pub range: FStarRange,
    pub status: FragmentStatus,
}

#[derive(Debug, Clone, PartialEq)]
pub enum FragmentStatus {
    Ok,
    LaxOk,
    Failed,
    InProgress,
}

/// Manages a single F* IDE process
pub struct FStarProcess {
    child: Child,
    jsonl: JsonlInterface,
    query_id: AtomicU64,
    response_rx: mpsc::Receiver<FStarResponse>,
    pub supports_full_buffer: bool,
    pub ide_version: i32,
}

impl FStarProcess {
    /// Spawn a new F* IDE process
    pub async fn spawn(
        config: FStarConfig,
        file_path: &Path,
        lax: bool,
    ) -> Result<Self, ProcessError> {
        let fstar_exe = config.fstar_exe().to_string();
        let cwd = config.cwd_or(file_path.parent().unwrap_or(Path::new(".")));
        let args = config.build_args(&file_path.to_string_lossy(), lax);

        if is_verbose() {
            tracing::info!("[F* spawn] {} {} (cwd: {:?})", fstar_exe, args.join(" "), cwd);
        } else {
            tracing::debug!("Spawning F* with args: {:?} in {:?}", args, cwd);
        }

        let mut child = Command::new(&fstar_exe)
            .args(&args) // Pass all args including --ide
            .current_dir(&cwd)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|e| {
                if e.kind() == std::io::ErrorKind::NotFound {
                    ProcessError::ExecutableNotFound(fstar_exe.clone())
                } else {
                    ProcessError::SpawnError(e)
                }
            })?;

        let stdin = child.stdin.take().expect("stdin not captured");
        let stdout = child.stdout.take().expect("stdout not captured");
        let stderr = child.stderr.take().expect("stderr not captured");

        // Set up response channel
        let (tx, rx) = mpsc::channel(100);

        // Capture verbose flag for async tasks
        let verbose = Arc::new(AtomicBool::new(is_verbose()));

        // Spawn stdout reader task
        let tx_clone = tx.clone();
        let verbose_stdout = verbose.clone();
        tokio::spawn(async move {
            let mut reader = BufReader::new(stdout);
            let mut line = String::new();
            loop {
                line.clear();
                match reader.read_line(&mut line).await {
                    Ok(0) => {
                        if verbose_stdout.load(Ordering::Relaxed) {
                            tracing::info!("[F* → MCP] <EOF>");
                        }
                        break; // EOF
                    }
                    Ok(_) => {
                        let trimmed = line.trim();
                        if trimmed.is_empty() {
                            continue;
                        }
                        
                        if verbose_stdout.load(Ordering::Relaxed) {
                            tracing::info!("[F* → MCP] {}", trimmed);
                        }
                        
                        match parse_response(trimmed) {
                            Ok(response) => {
                                if tx_clone.send(response).await.is_err() {
                                    break;
                                }
                            }
                            Err(e) => {
                                tracing::warn!("Failed to parse F* response: {} | raw: {}", e, trimmed);
                            }
                        }
                    }
                    Err(e) => {
                        tracing::error!("Error reading F* stdout: {}", e);
                        break;
                    }
                }
            }
        });

        // Spawn stderr reader task (always log stderr)
        let verbose_stderr = verbose.clone();
        tokio::spawn(async move {
            let mut reader = BufReader::new(stderr);
            let mut line = String::new();
            loop {
                line.clear();
                match reader.read_line(&mut line).await {
                    Ok(0) => break,
                    Ok(_) => {
                        let trimmed = line.trim();
                        if !trimmed.is_empty() {
                            if verbose_stderr.load(Ordering::Relaxed) {
                                tracing::info!("[F* stderr] {}", trimmed);
                            } else {
                                tracing::warn!("F* stderr: {}", trimmed);
                            }
                        }
                    }
                    Err(_) => break,
                }
            }
        });

        let jsonl = JsonlInterface::new(stdin);

        let mut process = FStarProcess {
            child,
            jsonl,
            query_id: AtomicU64::new(0),
            response_rx: rx,
            supports_full_buffer: true, // Assume true until we get protocol-info
            ide_version: 3,
        };

        // Wait for protocol-info
        process.wait_for_protocol_info().await?;

        Ok(process)
    }

    /// Wait for the initial protocol-info message
    async fn wait_for_protocol_info(&mut self) -> Result<(), ProcessError> {
        // Give F* some time to start up
        let timeout = tokio::time::Duration::from_secs(30);
        let result = tokio::time::timeout(timeout, self.response_rx.recv()).await;

        match result {
            Ok(Some(FStarResponse::ProtocolInfo(info))) => {
                self.supports_full_buffer = info.supports_full_buffer();
                self.ide_version = info.version;
                tracing::info!(
                    "F* protocol version: {}, full-buffer: {}",
                    info.version,
                    self.supports_full_buffer
                );
                Ok(())
            }
            Ok(Some(other)) => {
                tracing::warn!("Expected protocol-info, got: {:?}", other);
                Ok(()) // Continue anyway
            }
            Ok(None) => Err(ProcessError::ProcessExited(None)),
            Err(_) => Err(ProcessError::Timeout),
        }
    }

    /// Get the next query ID
    fn next_query_id(&self) -> String {
        self.query_id.fetch_add(1, Ordering::SeqCst).to_string()
    }

    /// Send a query and return the query ID
    pub async fn send_query(&self, mut query: serde_json::Value) -> Result<String, ProcessError> {
        let qid = self.next_query_id();
        query["query-id"] = serde_json::Value::String(qid.clone());

        tracing::debug!("Sending query: {}", serde_json::to_string(&query).unwrap());

        self.jsonl
            .send_message(&query)
            .await
            .map_err(|e| ProcessError::SendError(e.to_string()))?;

        Ok(qid)
    }

    /// Send a full-buffer query and collect all responses until finished
    pub async fn full_buffer_query(
        &mut self,
        code: &str,
        kind: &str,
        to_position: Option<(u32, u32)>,
    ) -> Result<FullBufferResult, ProcessError> {
        if !self.supports_full_buffer {
            return Err(ProcessError::NoFullBufferSupport);
        }

        let mut query = serde_json::json!({
            "query": "full-buffer",
            "args": {
                "kind": kind,
                "with-symbols": false,
                "code": code,
                "line": 0,
                "column": 0
            }
        });

        if let Some((line, col)) = to_position {
            query["args"]["to-position"] = serde_json::json!({
                "line": line,
                "column": col
            });
        }

        let qid = self.send_query(query).await?;
        let base_qid = qid.clone();

        let mut result = FullBufferResult::default();

        // Collect responses until full-buffer-finished
        loop {
            match self.response_rx.recv().await {
                Some(FStarResponse::Progress {
                    query_id,
                    stage,
                    ranges,
                }) => {
                    if !query_id.starts_with(&base_qid) {
                        continue;
                    }

                    match stage.as_str() {
                        "full-buffer-started" => {
                            tracing::debug!("Full buffer started");
                        }
                        "full-buffer-finished" => {
                            result.finished = true;
                            break;
                        }
                        "full-buffer-fragment-started" => {
                            if let Some(r) = ranges {
                                result.fragments.push(FragmentResult {
                                    range: r,
                                    status: FragmentStatus::InProgress,
                                });
                            }
                        }
                        "full-buffer-fragment-ok" => {
                            if let Some(last) = result.fragments.last_mut() {
                                last.status = FragmentStatus::Ok;
                            }
                        }
                        "full-buffer-fragment-lax-ok" => {
                            if let Some(last) = result.fragments.last_mut() {
                                last.status = FragmentStatus::LaxOk;
                            }
                        }
                        "full-buffer-fragment-failed" => {
                            if let Some(last) = result.fragments.last_mut() {
                                last.status = FragmentStatus::Failed;
                            }
                        }
                        _ => {}
                    }
                }
                Some(FStarResponse::Response(resp)) => {
                    if !resp.query_id.starts_with(&base_qid) {
                        continue;
                    }
                    // Check for diagnostics in response
                    if let Some(response) = &resp.response {
                        if let Ok(diags) = serde_json::from_value::<Vec<IdeDiagnostic>>(response.clone()) {
                            result.diagnostics.extend(diags);
                        }
                    }
                }
                Some(FStarResponse::ProofState(ps)) => {
                    result.proof_states.push(ps);
                }
                Some(FStarResponse::StatusMessage { level, contents, .. }) => {
                    tracing::debug!("F* {}: {}", level, contents);
                }
                Some(FStarResponse::ProtocolInfo(_)) => {
                    // Ignore late protocol info
                }
                None => {
                    return Err(ProcessError::ProcessExited(None));
                }
            }
        }

        Ok(result)
    }

    /// Send a vfs-add query
    pub async fn vfs_add(&mut self, filename: Option<&str>, contents: &str) -> Result<(), ProcessError> {
        let query = serde_json::json!({
            "query": "vfs-add",
            "args": {
                "filename": filename,
                "contents": contents
            }
        });

        let qid = self.send_query(query).await?;

        // Wait for response
        while let Some(response) = self.response_rx.recv().await {
            if let FStarResponse::Response(resp) = response {
                if resp.query_id == qid {
                    return Ok(());
                }
            }
        }

        Err(ProcessError::ProcessExited(None))
    }

    /// Send a lookup query
    pub async fn lookup(
        &mut self,
        filename: &str,
        line: u32,
        column: u32,
        symbol: &str,
    ) -> Result<Option<IdeLookupResponse>, ProcessError> {
        let query = serde_json::json!({
            "query": "lookup",
            "args": {
                "context": "code",
                "symbol": symbol,
                "requested-info": ["type", "documentation", "defined-at"],
                "location": {
                    "filename": filename,
                    "line": line,
                    "column": column
                }
            }
        });

        let qid = self.send_query(query).await?;

        while let Some(response) = self.response_rx.recv().await {
            if let FStarResponse::Response(resp) = response {
                if resp.query_id == qid {
                    if resp.status == Some("success".to_string()) {
                        if let Some(r) = resp.response {
                            return Ok(serde_json::from_value(r).ok());
                        }
                    }
                    return Ok(None);
                }
            }
        }

        Err(ProcessError::ProcessExited(None))
    }

    /// Send restart-solver request
    pub async fn restart_solver(&mut self) -> Result<(), ProcessError> {
        let query = serde_json::json!({
            "query": "restart-solver",
            "args": {}
        });

        self.send_query(query).await?;
        // restart-solver doesn't send a response
        Ok(())
    }

    /// Kill the F* process
    pub async fn kill(&mut self) -> Result<(), ProcessError> {
        self.child.kill().await.map_err(ProcessError::SpawnError)
    }
}

impl Drop for FStarProcess {
    fn drop(&mut self) {
        // Try to kill the process when dropped
        let _ = self.child.start_kill();
    }
}
