//! F* MCP Server - HTTP MCP front-end for F*'s IDE protocol.

mod fstar;
mod mcp;
mod session;

use mcp::{create_fstar_server, SESSION_MANAGER};
use pmcp::server::streamable_http_server::{StreamableHttpServer, StreamableHttpServerConfig};
use session::DEFAULT_SWEEP_PERIOD_SECS;
use std::net::{Ipv4Addr, SocketAddr};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use tokio::sync::Mutex;
use tracing::info;

/// Global verbose flag for detailed F* I/O logging
pub static VERBOSE: AtomicBool = AtomicBool::new(false);

/// Check if verbose mode is enabled
pub fn is_verbose() -> bool {
    VERBOSE.load(Ordering::Relaxed)
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Check for --verbose flag
    let args: Vec<String> = std::env::args().collect();
    let verbose = args.iter().any(|a| a == "--verbose" || a == "-v");
    VERBOSE.store(verbose, Ordering::Relaxed);

    // Get sweep period from environment or use default
    let sweep_period: u64 = std::env::var("FSTAR_MCP_SWEEP_PERIOD")
        .unwrap_or_else(|_| DEFAULT_SWEEP_PERIOD_SECS.to_string())
        .parse()
        .unwrap_or(DEFAULT_SWEEP_PERIOD_SECS);

    // Initialize logging - use debug level if verbose
    let default_filter = if verbose {
        "fstar_mcp=debug,pmcp=debug"
    } else {
        "fstar_mcp=info,pmcp=info"
    };
    
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| default_filter.into()),
        )
        .init();

    let port: u16 = std::env::var("FSTAR_MCP_PORT")
        .unwrap_or_else(|_| "3000".to_string())
        .parse()
        .unwrap_or(3000);

    let addr = SocketAddr::new(Ipv4Addr::LOCALHOST.into(), port);

    info!("Starting F* MCP server on {}", addr);
    if verbose {
        info!("Verbose mode enabled - logging all F* I/O");
    }
    info!("Session sweep period: {} seconds", sweep_period);

    // Create the MCP server with tools
    let server = create_fstar_server()?;

    // Wrap server in Arc<Mutex<>> for sharing
    let server = Arc::new(Mutex::new(server));

    // Create config with session lifecycle callbacks
    let config = StreamableHttpServerConfig {
        session_id_generator: None,
        enable_json_response: true,
        event_store: None,
        on_session_initialized: Some(Box::new(|session_id| {
            tracing::debug!(mcp_session = %session_id, "MCP session initialized");
        })),
        on_session_closed: Some(Box::new(|session_id| {
            tracing::info!(mcp_session = %session_id, "MCP session closed, marking F* sessions for deletion");
            // We need to spawn a task because the callback is sync
            let session_id = session_id.to_string();
            tokio::spawn(async move {
                SESSION_MANAGER.mark_sessions_for_deletion(&session_id).await;
            });
        })),
        http_middleware: None,
    };

    // Create the streamable HTTP server with config
    let http_server = StreamableHttpServer::with_config(addr, server, config);

    // Start the server
    let (bound_addr, server_handle) = http_server
        .start()
        .await
        .map_err(|e| Box::new(e) as Box<dyn std::error::Error>)?;

    // Start the session sweeper task
    let sweeper_handle = tokio::spawn(async move {
        let mut interval = tokio::time::interval(std::time::Duration::from_secs(sweep_period));
        loop {
            interval.tick().await;
            let count = SESSION_MANAGER.sweep_marked_sessions().await;
            if count > 0 {
                tracing::info!(count = count, "Swept marked sessions");
            }
        }
    });

    println!("╔════════════════════════════════════════════════════════════╗");
    println!("║              F* MCP SERVER RUNNING                        ║");
    println!("╠════════════════════════════════════════════════════════════╣");
    println!("║ Address: http://{:43} ║", bound_addr);
    println!("║ Mode:    Stateful (with session management)               ║");
    println!("║ Sweep:   Every {} seconds{:30} ║", sweep_period, "");
    if verbose {
    println!("║ Verbose: ON (logging all F* I/O)                          ║");
    }
    println!("╠════════════════════════════════════════════════════════════╣");
    println!("║ Available Tools:                                           ║");
    println!("║ • create_session    - Create F* session and typecheck      ║");
    println!("║ • list_sessions     - List active sessions with status     ║");
    println!("║ • typecheck_buffer  - Typecheck code (supports lax flag)   ║");
    println!("║ • update_buffer     - Add file to virtual file system      ║");
    println!("║ • lookup_symbol     - Get symbol info at position          ║");
    println!("║ • get_proof_context - Get proof goals from tactics         ║");
    println!("║ • restart_solver    - Restart Z3 SMT solver                ║");
    println!("║ • close_session     - Close F* session                     ║");
    println!("╚════════════════════════════════════════════════════════════╝");
    println!();
    println!("Press Ctrl+C to stop the server");

    // Keep the server running
    tokio::select! {
        result = server_handle => {
            result.map_err(|e| pmcp::Error::Internal(e.to_string()))?;
        }
        _ = sweeper_handle => {
            // Sweeper shouldn't exit, but handle it gracefully
        }
    }

    Ok(())
}
