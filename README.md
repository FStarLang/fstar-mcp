# F* MCP Server

An MCP (Model Context Protocol) server that provides a front-end for F*'s `--ide` stdio protocol. This enables AI assistants to interact with F* for typechecking, symbol lookup, and other IDE features via the MCP standard.

## Features

- **Session Management**: One session per file path with automatic replacement
- **Full F* IDE Protocol Support**: Typechecking, symbol lookup, and more
- **Proof Context**: Access proof obligations and goals during typechecking

## Installation

```bash
# Build the server
cargo build --release

# Run the server
./target/release/fstar-mcp
```

## MCP Tools

### `create_session`

Create a new F* session. All arguments are optional with sensible defaults.

**Parameters:**
- `file_path` (string, optional): Path to the F* file. If omitted, creates a temporary .fst file.
- `fstar_exe` (string, optional): Path to fstar.exe. Defaults to 'fstar.exe' in PATH.
- `cwd` (string, optional): Working directory for F*. Defaults to the file's directory.
- `include_dirs` (array of strings, optional): Include directories (--include paths).
- `options` (array of strings, optional): F* command-line options (e.g., `['--cache_dir', '.cache']`).

**Returns:**
```json
{
  "session_id": "uuid",
  "status": "ok" | "error",
  "diagnostics": [...],
  "fragments": [...],
  "created_at": "2024-01-01T00:00:00Z"
}
```

### `list_sessions`

List all active F* sessions with status information.

**Parameters:** None

**Returns:**
```json
{
  "sessions": [...],
  "count": 2
}
```

### `typecheck_buffer`

Typecheck code in an existing F* session.

**Parameters:**
- `session_id` (string): Session ID from `create_session`
- `code` (string): The F* code to typecheck
- `lax` (boolean, optional): If true, use lax mode (admits all SMT queries). Shortcut for kind='lax'
- `kind` (string, optional): Typecheck kind - `"full"`, `"lax"`, `"cache"`, `"reload-deps"`, `"verify-to-position"`, `"lax-to-position"`. Default: `"full"`. Overridden by lax=true
- `to_line` (integer, optional): Line to typecheck to (for position-based kinds)
- `to_column` (integer, optional): Column to typecheck to (for position-based kinds)

**Returns:**
```json
{
  "status": "ok" | "error",
  "diagnostics": [...],
  "fragments": [...]
}
```

### `update_buffer`

Add or update a file in F*'s virtual file system (vfs-add).

**Parameters:**
- `session_id` (string): Session ID from `create_session`
- `file_path` (string): Path to the file in the virtual file system
- `contents` (string): Contents of the file

**Returns:**
```json
{
  "status": "ok" | "error"
}
```

### `lookup_symbol`

Look up type information, documentation, and definition location for a symbol.

**Parameters:**
- `session_id` (string): Session ID from `create_session`
- `file_path` (string): Path to the file containing the symbol
- `line` (integer): Line number (1-based)
- `column` (integer): Column number (0-based)
- `symbol` (string): The symbol to look up

**Returns:**
```json
{
  "kind": "symbol" | "module" | "not_found",
  "name": "FStar.List.map",
  "type_info": "('a -> 'b) -> list 'a -> list 'b",
  "documentation": "...",
  "defined_at": {
    "file": "...",
    "start_line": 1,
    "start_column": 0,
    "end_line": 1,
    "end_column": 10
  }
}
```

### `get_proof_context`

Get proof obligations and goals at a position. Returns proof states collected during last typecheck.

**Parameters:**
- `session_id` (string): Session ID from `create_session`
- `line` (integer, optional): Line number to get proof state at. If omitted, returns all proof states.

**Returns:**
```json
{
  "found": true,
  "line": 10,
  "proof_state": {...}
}
```

Or when no line is specified:
```json
{
  "count": 3,
  "proof_states": [...]
}
```

### `restart_solver`

Restart the Z3 SMT solver for a session.

**Parameters:**
- `session_id` (string): Session ID from `create_session`

**Returns:**
```json
{
  "status": "ok"
}
```

### `close_session`

Close an F* session and clean up resources.

**Parameters:**
- `session_id` (string): Session ID from `create_session`

**Returns:**
```json
{
  "status": "ok"
}
```

## Development

```bash
# Run tests
cargo test

# Run with debug logging
RUST_LOG=fstar_mcp=debug cargo run
```

## License

MIT
