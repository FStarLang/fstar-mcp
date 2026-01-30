//! MCP Client integration tests.
//!
//! These tests verify the MCP server tools work correctly by simulating
//! client interactions with mock F* processes.

mod mock_fstar;

use mock_fstar::{MockFStarConfig, MockFStarProcess};
use std::collections::HashMap;
use std::sync::{Arc, RwLock};

// ============================================================================
// Test Fixtures - These will be replaced with actual types once implemented
// ============================================================================

/// Placeholder for session ID type.
type SessionId = String;

/// Placeholder for the session manager that will be implemented.
/// This simulates the expected API.
#[derive(Default)]
struct MockSessionManager {
    sessions: Arc<RwLock<HashMap<String, MockSession>>>,
}

struct MockSession {
    file_path: String,
    fstar: MockFStarProcess,
}

impl MockSessionManager {
    fn new() -> Self {
        Self::default()
    }

    fn create_session(&self, file_path: &str, config: MockFStarConfig) -> SessionId {
        let session_id = uuid::Uuid::new_v4().to_string();
        let session = MockSession {
            file_path: file_path.to_string(),
            fstar: MockFStarProcess::new(config),
        };

        let mut sessions = self.sessions.write().unwrap();

        // Auto-replace: remove existing session for same file
        let existing_key = sessions
            .iter()
            .find(|(_, s)| s.file_path == file_path)
            .map(|(k, _)| k.clone());
        if let Some(key) = existing_key {
            sessions.remove(&key);
        }

        sessions.insert(session_id.clone(), session);
        session_id
    }

    fn get_session(&self, session_id: &str) -> Option<std::sync::RwLockReadGuard<HashMap<String, MockSession>>> {
        let sessions = self.sessions.read().unwrap();
        if sessions.contains_key(session_id) {
            Some(sessions)
        } else {
            None
        }
    }

    fn close_session(&self, session_id: &str) -> bool {
        let mut sessions = self.sessions.write().unwrap();
        sessions.remove(session_id).is_some()
    }

    fn session_count(&self) -> usize {
        self.sessions.read().unwrap().len()
    }
}

// ============================================================================
// Tool Response Types (matching planned MCP tool outputs)
// ============================================================================

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
struct CreateFStarResponse {
    session_id: String,
    status: String, // "ok" or "error"
    diagnostics: Vec<Diagnostic>,
    fragments: Vec<Fragment>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
struct TypecheckResponse {
    status: String,
    diagnostics: Vec<Diagnostic>,
    fragments: Vec<Fragment>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
struct Diagnostic {
    level: String,
    message: String,
    line: u32,
    column: u32,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
struct Fragment {
    start_line: u32,
    end_line: u32,
    status: String, // "ok", "lax-ok", "failed", "in-progress"
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
struct LookupResponse {
    kind: String, // "symbol" or "module"
    name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    type_info: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    documentation: Option<String>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
struct AutocompleteResponse {
    completions: Vec<CompletionItem>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
struct CompletionItem {
    match_length: u32,
    annotation: String,
    candidate: String,
}

// ============================================================================
// Tests for create_fstar tool
// ============================================================================

#[cfg(test)]
mod create_fstar_tests {
    use super::*;

    #[test]
    fn test_create_fstar_success() {
        let manager = MockSessionManager::new();
        let config = MockFStarConfig::new()
            .with_successful_typecheck()
            .with_vfs_add_success();

        let session_id = manager.create_session("/path/to/Test.fst", config);

        assert!(!session_id.is_empty());
        assert_eq!(manager.session_count(), 1);

        // Verify session exists
        assert!(manager.get_session(&session_id).is_some());
    }

    #[test]
    fn test_create_fstar_with_error() {
        let manager = MockSessionManager::new();
        let config = MockFStarConfig::new()
            .with_typecheck_error(5, "Expected type int, got bool")
            .with_vfs_add_success();

        let session_id = manager.create_session("/path/to/Bad.fst", config);

        // Session should still be created (we don't kill on error)
        assert!(!session_id.is_empty());
        assert_eq!(manager.session_count(), 1);
    }

    #[test]
    fn test_create_fstar_replaces_existing_session() {
        let manager = MockSessionManager::new();

        // Create first session for file
        let config1 = MockFStarConfig::new().with_successful_typecheck();
        let session1 = manager.create_session("/path/to/Test.fst", config1);
        assert_eq!(manager.session_count(), 1);

        // Create second session for same file - should replace
        let config2 = MockFStarConfig::new().with_successful_typecheck();
        let session2 = manager.create_session("/path/to/Test.fst", config2);

        assert_eq!(manager.session_count(), 1);
        assert_ne!(session1, session2);

        // Old session should be gone
        assert!(manager.get_session(&session1).is_none());
        // New session should exist
        assert!(manager.get_session(&session2).is_some());
    }

    #[test]
    fn test_create_multiple_sessions_different_files() {
        let manager = MockSessionManager::new();

        let config1 = MockFStarConfig::new().with_successful_typecheck();
        let session1 = manager.create_session("/path/to/File1.fst", config1);

        let config2 = MockFStarConfig::new().with_successful_typecheck();
        let session2 = manager.create_session("/path/to/File2.fst", config2);

        assert_eq!(manager.session_count(), 2);
        assert_ne!(session1, session2);
    }
}

// ============================================================================
// Tests for typecheck_buffer tool
// ============================================================================

#[cfg(test)]
mod typecheck_buffer_tests {
    use super::*;

    #[test]
    fn test_typecheck_full_success() {
        let manager = MockSessionManager::new();
        let config = MockFStarConfig::new().with_successful_typecheck();
        let session_id = manager.create_session("/path/to/Test.fst", config);

        // Simulate typecheck query
        let sessions = manager.get_session(&session_id).unwrap();
        let session = sessions.get(&session_id).unwrap();

        let query = serde_json::json!({
            "query-id": "1",
            "query": "full-buffer",
            "args": {
                "kind": "full",
                "code": "module Test\nlet x = 1",
                "line": 0,
                "column": 0,
                "with-symbols": false
            }
        });

        let mut fstar = MockFStarProcess::new(MockFStarConfig::new().with_successful_typecheck());
        let responses = fstar.process_query(&query);

        // Should have streaming responses ending with full-buffer-finished
        assert!(!responses.is_empty());
        let last = responses.last().unwrap();
        assert_eq!(last["contents"]["stage"], "full-buffer-finished");
    }

    #[test]
    fn test_typecheck_incremental_cache() {
        let config = MockFStarConfig::new().with_successful_typecheck();
        let mut fstar = MockFStarProcess::new(config);

        // Simulate cache query (incremental check)
        let query = serde_json::json!({
            "query-id": "2",
            "query": "full-buffer",
            "args": {
                "kind": "cache",
                "code": "module Test\nlet x = 1\nlet y = 2",
                "line": 0,
                "column": 0,
                "with-symbols": false
            }
        });

        let responses = fstar.process_query(&query);
        assert!(!responses.is_empty());
    }

    #[test]
    fn test_typecheck_to_position() {
        let config = MockFStarConfig::new().with_successful_typecheck();
        let mut fstar = MockFStarProcess::new(config);

        let query = serde_json::json!({
            "query-id": "3",
            "query": "full-buffer",
            "args": {
                "kind": "verify-to-position",
                "code": "module Test\nlet x = 1\nlet y = 2",
                "line": 0,
                "column": 0,
                "with-symbols": false,
                "to-position": { "line": 2, "column": 0 }
            }
        });

        let responses = fstar.process_query(&query);
        assert!(!responses.is_empty());
    }

    #[test]
    fn test_typecheck_lax_mode() {
        let config = MockFStarConfig::new().with_successful_typecheck();
        let mut fstar = MockFStarProcess::new(config);

        let query = serde_json::json!({
            "query-id": "4",
            "query": "full-buffer",
            "args": {
                "kind": "lax",
                "code": "module Test\nlet x = 1",
                "line": 0,
                "column": 0,
                "with-symbols": false
            }
        });

        let responses = fstar.process_query(&query);
        assert!(!responses.is_empty());
    }
}

// ============================================================================
// Tests for update_buffer (vfs-add) tool
// ============================================================================

#[cfg(test)]
mod update_buffer_tests {
    use super::*;

    #[test]
    fn test_vfs_add_success() {
        let config = MockFStarConfig::new().with_vfs_add_success();
        let mut fstar = MockFStarProcess::new(config);

        let query = serde_json::json!({
            "query-id": "1",
            "query": "vfs-add",
            "args": {
                "filename": "/path/to/Dep.fst",
                "contents": "module Dep\nlet helper = 42"
            }
        });

        let responses = fstar.process_query(&query);
        assert_eq!(responses.len(), 1);
        assert_eq!(responses[0]["status"], "success");
    }

    #[test]
    fn test_vfs_add_null_filename() {
        let config = MockFStarConfig::new().with_vfs_add_success();
        let mut fstar = MockFStarProcess::new(config);

        // Initial vfs-add with null filename (as done on document open)
        let query = serde_json::json!({
            "query-id": "1",
            "query": "vfs-add",
            "args": {
                "filename": null,
                "contents": "module Test"
            }
        });

        let responses = fstar.process_query(&query);
        assert_eq!(responses.len(), 1);
        assert_eq!(responses[0]["status"], "success");
    }
}

// ============================================================================
// Tests for lookup_symbol tool
// ============================================================================

#[cfg(test)]
mod lookup_symbol_tests {
    use super::*;

    #[test]
    fn test_lookup_symbol_success() {
        let config = MockFStarConfig::new()
            .with_symbol_lookup("FStar.List.map", "('a -> 'b) -> list 'a -> list 'b");
        let mut fstar = MockFStarProcess::new(config);

        let query = serde_json::json!({
            "query-id": "1",
            "query": "lookup",
            "args": {
                "context": "code",
                "symbol": "map",
                "requested-info": ["type", "documentation", "defined-at"],
                "location": {
                    "filename": "Test.fst",
                    "line": 5,
                    "column": 10
                }
            }
        });

        let responses = fstar.process_query(&query);
        assert_eq!(responses.len(), 1);
        assert_eq!(responses[0]["status"], "success");
        assert_eq!(responses[0]["response"]["kind"], "symbol");
        assert_eq!(responses[0]["response"]["name"], "FStar.List.map");
    }

    #[test]
    fn test_lookup_module() {
        // Would need to add module lookup support to mock
        let config = MockFStarConfig::new();
        let mut fstar = MockFStarProcess::new(config);

        let query = serde_json::json!({
            "query-id": "1",
            "query": "lookup",
            "args": {
                "context": "code",
                "symbol": "FStar.List",
                "requested-info": ["type", "documentation", "defined-at"],
                "location": {
                    "filename": "Test.fst",
                    "line": 1,
                    "column": 5
                }
            }
        });

        let responses = fstar.process_query(&query);
        // Default response when not configured
        assert_eq!(responses.len(), 1);
        assert_eq!(responses[0]["status"], "success");
    }
}

// ============================================================================
// Tests for autocomplete tool
// ============================================================================

#[cfg(test)]
mod autocomplete_tests {
    use super::*;

    #[test]
    fn test_autocomplete_success() {
        let config = MockFStarConfig::new().with_autocomplete(vec![
            ("val", "FStar.List.map"),
            ("val", "FStar.List.mapi"),
            ("val", "FStar.List.mapT"),
        ]);
        let mut fstar = MockFStarProcess::new(config);

        let query = serde_json::json!({
            "query-id": "1",
            "query": "autocomplete",
            "args": {
                "partial-symbol": "map",
                "context": "code"
            }
        });

        let responses = fstar.process_query(&query);
        assert_eq!(responses.len(), 1);
        assert_eq!(responses[0]["status"], "success");

        let completions = responses[0]["response"].as_array().unwrap();
        assert_eq!(completions.len(), 3);

        // Each completion is [match_length, annotation, candidate]
        assert_eq!(completions[0][2], "FStar.List.map");
    }

    #[test]
    fn test_autocomplete_empty() {
        let config = MockFStarConfig::new().with_autocomplete(vec![]);
        let mut fstar = MockFStarProcess::new(config);

        let query = serde_json::json!({
            "query-id": "1",
            "query": "autocomplete",
            "args": {
                "partial-symbol": "nonexistent",
                "context": "code"
            }
        });

        let responses = fstar.process_query(&query);
        assert_eq!(responses.len(), 1);
        assert_eq!(responses[0]["status"], "success");

        let completions = responses[0]["response"].as_array().unwrap();
        assert!(completions.is_empty());
    }
}

// ============================================================================
// Tests for restart_solver tool
// ============================================================================

#[cfg(test)]
mod restart_solver_tests {
    use super::*;

    #[test]
    fn test_restart_solver() {
        let config = MockFStarConfig::new();
        let mut fstar = MockFStarProcess::new(config);

        let query = serde_json::json!({
            "query-id": "1",
            "query": "restart-solver",
            "args": {}
        });

        let responses = fstar.process_query(&query);
        // restart-solver doesn't expect a response in the real protocol,
        // but our mock returns success for any unknown query
        assert_eq!(responses.len(), 1);
        assert_eq!(responses[0]["status"], "success");
    }
}

// ============================================================================
// Tests for close_session tool
// ============================================================================

#[cfg(test)]
mod close_session_tests {
    use super::*;

    #[test]
    fn test_close_session_success() {
        let manager = MockSessionManager::new();
        let config = MockFStarConfig::new().with_successful_typecheck();
        let session_id = manager.create_session("/path/to/Test.fst", config);

        assert_eq!(manager.session_count(), 1);

        let closed = manager.close_session(&session_id);
        assert!(closed);
        assert_eq!(manager.session_count(), 0);
    }

    #[test]
    fn test_close_nonexistent_session() {
        let manager = MockSessionManager::new();

        let closed = manager.close_session("nonexistent-session-id");
        assert!(!closed);
    }

    #[test]
    fn test_close_cleans_up_session() {
        let manager = MockSessionManager::new();
        let config = MockFStarConfig::new().with_successful_typecheck();
        let session_id = manager.create_session("/path/to/Test.fst", config);

        manager.close_session(&session_id);

        // Session should no longer exist
        assert!(manager.get_session(&session_id).is_none());
    }
}

// ============================================================================
// Tests for concurrent sessions
// ============================================================================

#[cfg(test)]
mod concurrent_session_tests {
    use super::*;
    use std::thread;

    #[test]
    fn test_concurrent_session_creation() {
        let manager = Arc::new(MockSessionManager::new());
        let mut handles = vec![];

        for i in 0..5 {
            let manager_clone = Arc::clone(&manager);
            let handle = thread::spawn(move || {
                let config = MockFStarConfig::new().with_successful_typecheck();
                manager_clone.create_session(&format!("/path/to/File{}.fst", i), config)
            });
            handles.push(handle);
        }

        let session_ids: Vec<String> = handles.into_iter().map(|h| h.join().unwrap()).collect();

        // All sessions should be unique
        let unique_count = session_ids.iter().collect::<std::collections::HashSet<_>>().len();
        assert_eq!(unique_count, 5);
        assert_eq!(manager.session_count(), 5);
    }

    #[test]
    fn test_concurrent_queries_same_session() {
        // This tests that queries can be issued to different mock processes concurrently
        let configs: Vec<MockFStarConfig> = (0..3)
            .map(|_| MockFStarConfig::new().with_successful_typecheck())
            .collect();

        let handles: Vec<_> = configs
            .into_iter()
            .enumerate()
            .map(|(i, config)| {
                thread::spawn(move || {
                    let mut fstar = MockFStarProcess::new(config);
                    let query = serde_json::json!({
                        "query-id": format!("{}", i),
                        "query": "full-buffer",
                        "args": { "kind": "full", "code": "let x = 1", "line": 0, "column": 0 }
                    });
                    fstar.process_query(&query)
                })
            })
            .collect();

        for handle in handles {
            let responses = handle.join().unwrap();
            assert!(!responses.is_empty());
        }
    }
}

// ============================================================================
// Tests for error handling
// ============================================================================

#[cfg(test)]
mod error_handling_tests {
    use super::*;

    #[test]
    fn test_invalid_session_id() {
        let manager = MockSessionManager::new();

        // Try to get non-existent session
        assert!(manager.get_session("invalid-uuid").is_none());
    }

    #[test]
    fn test_fstar_crash_handling() {
        let mut config = MockFStarConfig::new();
        config.simulate_crash = true;
        let mut fstar = MockFStarProcess::new(config);

        let query = serde_json::json!({
            "query-id": "1",
            "query": "full-buffer",
            "args": { "kind": "full", "code": "let x = 1", "line": 0, "column": 0 }
        });

        let responses = fstar.process_query(&query);
        assert!(responses.is_empty()); // Crash returns no responses
    }
}

// ============================================================================
// Tests for get_proof_context tool
// ============================================================================

#[cfg(test)]
mod get_proof_context_tests {
    use super::*;

    #[test]
    fn test_proof_states_collected() {
        // Test that proof states are included in full-buffer responses
        let config = MockFStarConfig::new().with_proof_states(vec![
            (5, "goal1", vec!["squash (x == 1)"]),
            (10, "goal2", vec!["squash (y == 2)", "squash (z == 3)"]),
        ]);
        let mut fstar = MockFStarProcess::new(config);

        let query = serde_json::json!({
            "query-id": "1",
            "query": "full-buffer",
            "args": {
                "kind": "full",
                "code": "module Test\nlet x = 1",
                "line": 0,
                "column": 0,
                "with-symbols": false
            }
        });

        let responses = fstar.process_query(&query);
        
        // Should have proof-state messages in the stream
        let proof_state_msgs: Vec<_> = responses
            .iter()
            .filter(|r| r["level"] == "proof-state")
            .collect();
        
        assert_eq!(proof_state_msgs.len(), 2);
        assert_eq!(proof_state_msgs[0]["contents"]["label"], "goal1");
        assert_eq!(proof_state_msgs[1]["contents"]["label"], "goal2");
    }

    #[test]
    fn test_proof_state_at_line() {
        let config = MockFStarConfig::new().with_proof_states(vec![
            (5, "goal_at_line_5", vec!["squash (x == 1)"]),
        ]);
        let mut fstar = MockFStarProcess::new(config);

        let query = serde_json::json!({
            "query-id": "1",
            "query": "full-buffer",
            "args": {
                "kind": "full",
                "code": "module Test\nlet x = 1",
                "line": 0,
                "column": 0,
                "with-symbols": false
            }
        });

        let responses = fstar.process_query(&query);
        
        // Find proof state at line 5
        let proof_state = responses
            .iter()
            .find(|r| {
                r["level"] == "proof-state" && 
                r["contents"]["location"]["beg"][0] == 5
            });
        
        assert!(proof_state.is_some());
        let ps = proof_state.unwrap();
        assert_eq!(ps["contents"]["label"], "goal_at_line_5");
        
        // Goals should be present
        let goals = ps["contents"]["goals"].as_array().unwrap();
        assert_eq!(goals.len(), 1);
        assert_eq!(goals[0]["goal"]["type"], "squash (x == 1)");
    }

    #[test]
    fn test_no_proof_states() {
        // Regular typecheck without tactics should have no proof states
        let config = MockFStarConfig::new().with_successful_typecheck();
        let mut fstar = MockFStarProcess::new(config);

        let query = serde_json::json!({
            "query-id": "1",
            "query": "full-buffer",
            "args": {
                "kind": "full",
                "code": "module Test\nlet x = 1",
                "line": 0,
                "column": 0,
                "with-symbols": false
            }
        });

        let responses = fstar.process_query(&query);
        
        // Should have no proof-state messages
        let proof_state_msgs: Vec<_> = responses
            .iter()
            .filter(|r| r["level"] == "proof-state")
            .collect();
        
        assert_eq!(proof_state_msgs.len(), 0);
    }

    #[test]
    fn test_multiple_goals_in_proof_state() {
        let config = MockFStarConfig::new().with_proof_states(vec![
            (7, "multi_goal", vec!["goal1: nat", "goal2: bool", "goal3: unit"]),
        ]);
        let mut fstar = MockFStarProcess::new(config);

        let query = serde_json::json!({
            "query-id": "1",
            "query": "full-buffer",
            "args": {
                "kind": "full",
                "code": "module Test",
                "line": 0,
                "column": 0,
                "with-symbols": false
            }
        });

        let responses = fstar.process_query(&query);
        
        let proof_state = responses
            .iter()
            .find(|r| r["level"] == "proof-state")
            .unwrap();
        
        let goals = proof_state["contents"]["goals"].as_array().unwrap();
        assert_eq!(goals.len(), 3);
        assert_eq!(goals[0]["goal"]["type"], "goal1: nat");
        assert_eq!(goals[1]["goal"]["type"], "goal2: bool");
        assert_eq!(goals[2]["goal"]["type"], "goal3: unit");
    }
}

// ============================================================================
// Tests for session cleanup (mark for deletion and sweep)
// ============================================================================

#[cfg(test)]
mod session_cleanup_tests {
    use super::*;

    #[test]
    fn test_mark_sessions_for_deletion() {
        let manager = MockSessionManager::new();
        
        // Create sessions
        let config1 = MockFStarConfig::new().with_successful_typecheck();
        let config2 = MockFStarConfig::new().with_successful_typecheck();
        let session1 = manager.create_session("/path/to/File1.fst", config1);
        let session2 = manager.create_session("/path/to/File2.fst", config2);
        
        assert_eq!(manager.session_count(), 2);
        
        // Sessions should exist
        assert!(manager.get_session(&session1).is_some());
        assert!(manager.get_session(&session2).is_some());
        
        // Close one session
        manager.close_session(&session1);
        assert_eq!(manager.session_count(), 1);
        assert!(manager.get_session(&session1).is_none());
        assert!(manager.get_session(&session2).is_some());
    }

    #[test]
    fn test_session_auto_replacement() {
        let manager = MockSessionManager::new();
        
        // Create session for a file
        let config1 = MockFStarConfig::new().with_successful_typecheck();
        let session1 = manager.create_session("/path/to/Test.fst", config1);
        assert_eq!(manager.session_count(), 1);
        
        // Create another session for the same file - should replace
        let config2 = MockFStarConfig::new().with_successful_typecheck();
        let session2 = manager.create_session("/path/to/Test.fst", config2);
        assert_eq!(manager.session_count(), 1);
        
        // Old session should be gone
        assert!(manager.get_session(&session1).is_none());
        // New session should exist
        assert!(manager.get_session(&session2).is_some());
    }
}
