//! JSON-RPC and application error codes aligned with `zion-core` (`V3/L1/core/src/rpc.rs`).
//! The SDK is standalone — it does not import `zion-core`, but the numbers must stay in sync.

/// JSON-RPC 2.0 — parse error.
pub const PARSE_ERROR: i64 = -32_700;
/// JSON-RPC 2.0 — invalid request.
pub const INVALID_REQUEST: i64 = -32_600;
/// JSON-RPC 2.0 — method not found.
pub const METHOD_NOT_FOUND: i64 = -32_601;
/// JSON-RPC 2.0 — invalid params.
pub const INVALID_PARAMS: i64 = -32_602;
/// JSON-RPC 2.0 — internal error.
pub const INTERNAL_ERROR: i64 = -32_603;

/// Aplikace — block not found.
pub const BLOCK_NOT_FOUND: i64 = -32_001;
/// Aplikace — transaction not found.
pub const TX_NOT_FOUND: i64 = -32_002;
/// Aplikace — invalid address.
pub const INVALID_ADDRESS: i64 = -32_003;
/// Aplikace — transaction rejected.
pub const TX_REJECTED: i64 = -32_004;
/// Aplikace — node not synced.
pub const NOT_SYNCED: i64 = -32_005;
