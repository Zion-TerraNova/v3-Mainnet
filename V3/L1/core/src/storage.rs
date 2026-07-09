// Phase 7a — LMDB persistent storage
//
// Audit references: P0-05 (atomic writes), P0-09 (crash recovery)
//
// Uses `heed` (typed LMDB wrapper). All block+UTXO updates happen inside a
// single LMDB write transaction — crash at any point leaves the database in
// the last committed state.
//
// 8 databases:
//   blocks          — hash → serialized block
//   utxos           — outpoint → serialized UTXO
//   tx_index        — tx_hash → (block_hash, index)
//   balance_cache   — address → balance (flowers)
//   undo_blocks     — height → serialized UndoBlock
//   height_to_hash  — height → block_hash
//   hash_to_height  — block_hash → height
//   meta            — chain metadata + known_peers (Phase 11 peer persistence)

use heed::types::{Bytes, Str};
use heed::{Database, Env, EnvOpenOptions};
use serde::{Deserialize, Serialize};
use std::path::Path;

use crate::chain::{Outpoint, UndoBlock};
use crate::tx;

// ── Constants ──────────────────────────────────────────────────────────

/// Default LMDB map size: 10 GB.
pub const DEFAULT_MAP_SIZE_BYTES: usize = 10 * 1024 * 1024 * 1024;

/// Maximum number of named databases.
const MAX_DBS: u32 = 12;

/// Current schema version (for future migrations).
pub const SCHEMA_VERSION: u32 = 1;

// ── Stored types ───────────────────────────────────────────────────────

/// A stored block — enough to reconstruct the full accepted block.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StoredBlock {
    pub hash: [u8; 32],
    pub prev_hash: [u8; 32],
    pub height: u64,
    pub timestamp: u64,
    pub difficulty: u64,
    pub nonce: u64,
    pub total_work: u128,
    pub transactions: Vec<tx::Transaction>,
    pub coinbase_amount: u64,
}

/// A stored UTXO.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StoredUtxo {
    pub amount: u64,
    pub address: String,
    pub created_height: u64,
    pub is_coinbase: bool,
}

/// Transaction location within a block.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TxLocation {
    pub block_hash: [u8; 32],
    pub index: u32,
}

/// Serialized UndoBlock for storage.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StoredUndoBlock {
    pub height: u64,
    pub hash: [u8; 32],
    pub spent_utxos: Vec<StoredRestoredUtxo>,
    pub created_outpoints: Vec<StoredOutpoint>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StoredRestoredUtxo {
    pub tx_hash: [u8; 32],
    pub output_index: u32,
    pub amount: u64,
    pub address: String,
    pub created_height: u64,
    pub is_coinbase: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StoredOutpoint {
    pub tx_hash: [u8; 32],
    pub output_index: u32,
}

/// Metadata stored in the `meta` database.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChainMeta {
    pub schema_version: u32,
    pub tip_hash: [u8; 32],
    pub tip_height: u64,
    pub total_work: u128,
}

// ── Errors ─────────────────────────────────────────────────────────────

#[derive(Debug)]
pub enum StorageError {
    Heed(heed::Error),
    Serde(String),
    NotFound(String),
    SchemaVersionMismatch { expected: u32, found: u32 },
}

impl std::fmt::Display for StorageError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Heed(e) => write!(f, "LMDB error: {e}"),
            Self::Serde(e) => write!(f, "serialization error: {e}"),
            Self::NotFound(k) => write!(f, "not found: {k}"),
            Self::SchemaVersionMismatch { expected, found } => write!(
                f,
                "schema version mismatch: expected {expected}, found {found}"
            ),
        }
    }
}

impl std::error::Error for StorageError {}

impl From<heed::Error> for StorageError {
    fn from(e: heed::Error) -> Self {
        Self::Heed(e)
    }
}

// ── Helper: encode/decode via serde_json ───────────────────────────────

fn encode<T: Serialize>(val: &T) -> Result<Vec<u8>, StorageError> {
    serde_json::to_vec(val).map_err(|e| StorageError::Serde(e.to_string()))
}

fn decode<T: for<'de> Deserialize<'de>>(bytes: &[u8]) -> Result<T, StorageError> {
    serde_json::from_slice(bytes).map_err(|e| StorageError::Serde(e.to_string()))
}

/// Encode an outpoint as 36-byte key: tx_hash(32) + output_index(4 LE).
fn outpoint_key(tx_hash: &[u8; 32], index: u32) -> [u8; 36] {
    let mut key = [0u8; 36];
    key[..32].copy_from_slice(tx_hash);
    key[32..].copy_from_slice(&index.to_le_bytes());
    key
}

/// Encode a u64 as 8-byte big-endian key (preserves sort order).
fn height_key(h: u64) -> [u8; 8] {
    h.to_be_bytes()
}

// ── ChainDb ────────────────────────────────────────────────────────────

/// LMDB-backed chain storage.
pub struct ChainDb {
    env: Env,
    blocks: Database<Bytes, Bytes>,
    utxos: Database<Bytes, Bytes>,
    tx_index: Database<Bytes, Bytes>,
    balance_cache: Database<Str, Bytes>,
    undo_blocks: Database<Bytes, Bytes>,
    height_to_hash: Database<Bytes, Bytes>,
    hash_to_height: Database<Bytes, Bytes>,
    meta: Database<Str, Bytes>,
}

impl ChainDb {
    /// Open (or create) the LMDB environment at `path`.
    pub fn open(path: &Path) -> Result<Self, StorageError> {
        Self::open_with_map_size(path, DEFAULT_MAP_SIZE_BYTES)
    }

    /// Open with a custom map size (useful for tests).
    pub fn open_with_map_size(path: &Path, map_size: usize) -> Result<Self, StorageError> {
        std::fs::create_dir_all(path).map_err(|e| StorageError::Serde(format!("mkdir: {e}")))?;

        let env = unsafe {
            EnvOpenOptions::new()
                .map_size(map_size)
                .max_dbs(MAX_DBS)
                .open(path)?
        };

        let mut wtxn = env.write_txn()?;
        let blocks = env.create_database(&mut wtxn, Some("blocks"))?;
        let utxos = env.create_database(&mut wtxn, Some("utxos"))?;
        let tx_index = env.create_database(&mut wtxn, Some("tx_index"))?;
        let balance_cache = env.create_database(&mut wtxn, Some("balance_cache"))?;
        let undo_blocks = env.create_database(&mut wtxn, Some("undo_blocks"))?;
        let height_to_hash = env.create_database(&mut wtxn, Some("height_to_hash"))?;
        let hash_to_height = env.create_database(&mut wtxn, Some("hash_to_height"))?;
        let meta = env.create_database(&mut wtxn, Some("meta"))?;
        wtxn.commit()?;

        let db = Self {
            env,
            blocks,
            utxos,
            tx_index,
            balance_cache,
            undo_blocks,
            height_to_hash,
            hash_to_height,
            meta,
        };

        // Check or initialize schema version
        db.init_schema()?;

        Ok(db)
    }

    fn init_schema(&self) -> Result<(), StorageError> {
        let rtxn = self.env.read_txn()?;
        match self.meta.get(&rtxn, "chain_meta")? {
            Some(bytes) => {
                let m: ChainMeta = decode(bytes)?;
                if m.schema_version != SCHEMA_VERSION {
                    return Err(StorageError::SchemaVersionMismatch {
                        expected: SCHEMA_VERSION,
                        found: m.schema_version,
                    });
                }
                Ok(())
            }
            None => {
                drop(rtxn);
                // Fresh database — write initial meta
                let m = ChainMeta {
                    schema_version: SCHEMA_VERSION,
                    tip_hash: [0u8; 32],
                    tip_height: 0,
                    total_work: 0,
                };
                let mut wtxn = self.env.write_txn()?;
                self.meta.put(&mut wtxn, "chain_meta", &encode(&m)?)?;
                wtxn.commit()?;
                Ok(())
            }
        }
    }

    // ── Single-transaction atomic block application ────────────────────

    /// Atomically store a block and apply UTXO changes.
    /// This is the primary write path — audit P0-05/P0-09 require atomicity.
    pub fn save_block_and_apply_utxos(
        &self,
        block: &StoredBlock,
        undo: &StoredUndoBlock,
        new_utxos: &[(Outpoint, StoredUtxo)],
        spent_outpoints: &[Outpoint],
        new_tip_meta: &ChainMeta,
    ) -> Result<(), StorageError> {
        let mut wtxn = self.env.write_txn()?;

        // 1. Store the block
        let block_bytes = encode(block)?;
        self.blocks.put(&mut wtxn, &block.hash, &block_bytes)?;

        // 2. Height ↔ hash indexes
        self.height_to_hash
            .put(&mut wtxn, &height_key(block.height), &block.hash)?;
        self.hash_to_height
            .put(&mut wtxn, &block.hash, &height_key(block.height))?;

        // 3. Remove spent UTXOs and update balance cache
        for op in spent_outpoints {
            let key = outpoint_key(&op.tx_hash, op.index);
            // Read existing UTXO for balance update
            if let Some(utxo_bytes) = self.utxos.get(&wtxn, &key)? {
                let utxo: StoredUtxo = decode(utxo_bytes)?;
                self.adjust_balance(&mut wtxn, &utxo.address, -(utxo.amount as i128))?;
            }
            self.utxos.delete(&mut wtxn, &key)?;
        }

        // 4. Insert new UTXOs and update balance cache
        for (op, utxo) in new_utxos {
            let key = outpoint_key(&op.tx_hash, op.index);
            let utxo_bytes = encode(utxo)?;
            self.utxos.put(&mut wtxn, &key, &utxo_bytes)?;
            self.adjust_balance(&mut wtxn, &utxo.address, utxo.amount as i128)?;
        }

        // 5. Index transactions
        for (idx, tx) in block.transactions.iter().enumerate() {
            let loc = TxLocation {
                block_hash: block.hash,
                index: idx as u32,
            };
            let loc_bytes = encode(&loc)?;
            self.tx_index.put(&mut wtxn, &tx.id, &loc_bytes)?;
        }

        // 6. Store undo block
        let undo_bytes = encode(undo)?;
        self.undo_blocks
            .put(&mut wtxn, &height_key(undo.height), &undo_bytes)?;

        // 7. Update chain tip metadata
        let meta_bytes = encode(new_tip_meta)?;
        self.meta.put(&mut wtxn, "chain_meta", &meta_bytes)?;

        // Atomically commit everything
        wtxn.commit()?;
        Ok(())
    }

    /// Roll back one block during a reorg. Inverse of `save_block_and_apply_utxos`.
    pub fn rollback_block(
        &self,
        height: u64,
        new_tip_meta: &ChainMeta,
    ) -> Result<StoredUndoBlock, StorageError> {
        let mut wtxn = self.env.write_txn()?;

        // 1. Get undo block
        let hk = height_key(height);
        let undo_bytes = self
            .undo_blocks
            .get(&wtxn, &hk)?
            .ok_or_else(|| StorageError::NotFound(format!("undo block at height {height}")))?;
        let undo: StoredUndoBlock = decode(undo_bytes)?;

        // 2. Get block hash at this height
        let block_hash_bytes = self
            .height_to_hash
            .get(&wtxn, &hk)?
            .ok_or_else(|| StorageError::NotFound(format!("hash at height {height}")))?;
        let mut block_hash = [0u8; 32];
        block_hash.copy_from_slice(block_hash_bytes);

        // 3. Remove created outpoints (undo created UTXOs)
        for op in &undo.created_outpoints {
            let key = outpoint_key(&op.tx_hash, op.output_index);
            if let Some(utxo_bytes) = self.utxos.get(&wtxn, &key)? {
                let utxo: StoredUtxo = decode(utxo_bytes)?;
                self.adjust_balance(&mut wtxn, &utxo.address, -(utxo.amount as i128))?;
            }
            self.utxos.delete(&mut wtxn, &key)?;
        }

        // 4. Restore spent UTXOs
        for su in &undo.spent_utxos {
            let key = outpoint_key(&su.tx_hash, su.output_index);
            let utxo = StoredUtxo {
                amount: su.amount,
                address: su.address.clone(),
                created_height: su.created_height,
                is_coinbase: su.is_coinbase,
            };
            let utxo_bytes = encode(&utxo)?;
            self.utxos.put(&mut wtxn, &key, &utxo_bytes)?;
            self.adjust_balance(&mut wtxn, &utxo.address, utxo.amount as i128)?;
        }

        // 5. Remove tx index entries
        let block_bytes = self.blocks.get(&wtxn, &block_hash)?;
        if let Some(bb) = block_bytes {
            let block: StoredBlock = decode(bb)?;
            for tx in &block.transactions {
                self.tx_index.delete(&mut wtxn, &tx.id)?;
            }
        }

        // 6. Remove height ↔ hash mappings
        self.height_to_hash.delete(&mut wtxn, &hk)?;
        self.hash_to_height.delete(&mut wtxn, &block_hash)?;

        // 7. Remove undo block
        self.undo_blocks.delete(&mut wtxn, &hk)?;

        // 8. Update chain tip
        let meta_bytes = encode(new_tip_meta)?;
        self.meta.put(&mut wtxn, "chain_meta", &meta_bytes)?;

        wtxn.commit()?;
        Ok(undo)
    }

    // ── Readers ────────────────────────────────────────────────────────

    /// Get chain metadata (tip hash, height, total_work).
    pub fn get_meta(&self) -> Result<ChainMeta, StorageError> {
        let rtxn = self.env.read_txn()?;
        let bytes = self
            .meta
            .get(&rtxn, "chain_meta")?
            .ok_or_else(|| StorageError::NotFound("chain_meta".to_string()))?;
        decode(bytes)
    }

    /// Get a block by hash.
    pub fn get_block(&self, hash: &[u8; 32]) -> Result<Option<StoredBlock>, StorageError> {
        let rtxn = self.env.read_txn()?;
        match self.blocks.get(&rtxn, hash)? {
            Some(bytes) => Ok(Some(decode(bytes)?)),
            None => Ok(None),
        }
    }

    /// Get a block hash by height.
    pub fn get_hash_by_height(&self, height: u64) -> Result<Option<[u8; 32]>, StorageError> {
        let rtxn = self.env.read_txn()?;
        match self.height_to_hash.get(&rtxn, &height_key(height))? {
            Some(bytes) if bytes.len() == 32 => {
                let mut hash = [0u8; 32];
                hash.copy_from_slice(bytes);
                Ok(Some(hash))
            }
            _ => Ok(None),
        }
    }

    /// Get a block by height.
    pub fn get_block_by_height(&self, height: u64) -> Result<Option<StoredBlock>, StorageError> {
        match self.get_hash_by_height(height)? {
            Some(hash) => self.get_block(&hash),
            None => Ok(None),
        }
    }

    /// Get a UTXO by outpoint.
    pub fn get_utxo(
        &self,
        tx_hash: &[u8; 32],
        index: u32,
    ) -> Result<Option<StoredUtxo>, StorageError> {
        let rtxn = self.env.read_txn()?;
        let key = outpoint_key(tx_hash, index);
        match self.utxos.get(&rtxn, &key)? {
            Some(bytes) => Ok(Some(decode(bytes)?)),
            None => Ok(None),
        }
    }

    /// Get the balance (in flowers) for an address.
    pub fn get_balance(&self, address: &str) -> Result<u64, StorageError> {
        let rtxn = self.env.read_txn()?;
        match self.balance_cache.get(&rtxn, address)? {
            Some(bytes) if bytes.len() == 16 => {
                let mut buf = [0u8; 16];
                buf.copy_from_slice(bytes);
                let val = i128::from_le_bytes(buf);
                Ok(val.max(0) as u64)
            }
            _ => Ok(0),
        }
    }

    /// Look up a transaction's block location.
    pub fn get_tx_location(&self, tx_hash: &[u8; 32]) -> Result<Option<TxLocation>, StorageError> {
        let rtxn = self.env.read_txn()?;
        match self.tx_index.get(&rtxn, tx_hash)? {
            Some(bytes) => Ok(Some(decode(bytes)?)),
            None => Ok(None),
        }
    }

    /// Get the undo block for a given height.
    pub fn get_undo_block(&self, height: u64) -> Result<Option<StoredUndoBlock>, StorageError> {
        let rtxn = self.env.read_txn()?;
        match self.undo_blocks.get(&rtxn, &height_key(height))? {
            Some(bytes) => Ok(Some(decode(bytes)?)),
            None => Ok(None),
        }
    }

    /// Get the current chain tip height.
    pub fn tip_height(&self) -> Result<u64, StorageError> {
        Ok(self.get_meta()?.tip_height)
    }

    // ── Export / Import (JSON snapshot compatibility) ───────────────────

    /// Export all blocks as a JSON-compatible vector (for debugging / migration).
    pub fn export_blocks(
        &self,
        start_height: u64,
        end_height: u64,
    ) -> Result<Vec<StoredBlock>, StorageError> {
        let rtxn = self.env.read_txn()?;
        let mut blocks = Vec::new();
        for h in start_height..=end_height {
            if let Some(hash_bytes) = self.height_to_hash.get(&rtxn, &height_key(h))? {
                if let Some(block_bytes) = self.blocks.get(&rtxn, hash_bytes)? {
                    blocks.push(decode(block_bytes)?);
                }
            }
        }
        Ok(blocks)
    }

    // ── Internal helpers ───────────────────────────────────────────────

    fn adjust_balance(
        &self,
        wtxn: &mut heed::RwTxn,
        address: &str,
        delta: i128,
    ) -> Result<(), StorageError> {
        let current = match self.balance_cache.get(wtxn, address)? {
            Some(bytes) if bytes.len() == 16 => {
                let mut buf = [0u8; 16];
                buf.copy_from_slice(bytes);
                i128::from_le_bytes(buf)
            }
            _ => 0i128,
        };
        let new_val = current + delta;
        self.balance_cache
            .put(wtxn, address, &new_val.to_le_bytes())?;
        Ok(())
    }

    // ── Peer persistence (Phase 11) ────────────────────────────────────

    /// Persist the known peer list to LMDB (meta database, key "known_peers").
    pub fn save_peers(&self, peers: &[crate::PeerEndpoint]) -> Result<(), StorageError> {
        let peer_vec: Vec<&crate::PeerEndpoint> = peers.iter().collect();
        let bytes = encode(&peer_vec)?;
        let mut wtxn = self.env.write_txn()?;
        self.meta.put(&mut wtxn, "known_peers", &bytes)?;
        wtxn.commit()?;
        Ok(())
    }

    /// Load persisted peer list from LMDB. Returns empty vec if none saved.
    pub fn load_peers(&self) -> Result<Vec<crate::PeerEndpoint>, StorageError> {
        let rtxn = self.env.read_txn()?;
        match self.meta.get(&rtxn, "known_peers")? {
            Some(bytes) => Ok(decode(bytes)?),
            None => Ok(Vec::new()),
        }
    }
}

// ── Conversion helpers ─────────────────────────────────────────────────

impl From<&UndoBlock> for StoredUndoBlock {
    fn from(ub: &UndoBlock) -> Self {
        Self {
            height: ub.height,
            hash: ub.hash,
            spent_utxos: ub
                .spent_utxos
                .iter()
                .map(|su| StoredRestoredUtxo {
                    tx_hash: su.outpoint.tx_hash,
                    output_index: su.outpoint.index,
                    amount: su.amount,
                    address: su.address.clone(),
                    created_height: su.created_height,
                    is_coinbase: su.is_coinbase,
                })
                .collect(),
            created_outpoints: ub
                .created_outpoints
                .iter()
                .map(|op| StoredOutpoint {
                    tx_hash: op.tx_hash,
                    output_index: op.index,
                })
                .collect(),
        }
    }
}

// ═══════════════════════════════════════════════════════════════════════
// Tests
// ═══════════════════════════════════════════════════════════════════════

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    fn make_test_db() -> (ChainDb, tempfile::TempDir) {
        let dir = tempdir().unwrap();
        let db = ChainDb::open_with_map_size(dir.path(), 10 * 1024 * 1024).unwrap();
        (db, dir)
    }

    fn make_test_block(height: u64, hash: [u8; 32], prev_hash: [u8; 32]) -> StoredBlock {
        StoredBlock {
            hash,
            prev_hash,
            height,
            timestamp: 1000 + height * 60,
            difficulty: 1000,
            nonce: height,
            total_work: (height + 1) as u128 * 1000,
            transactions: vec![],
            coinbase_amount: 5_400_067_000_000_000,
        }
    }

    fn make_test_undo(height: u64, hash: [u8; 32]) -> StoredUndoBlock {
        StoredUndoBlock {
            height,
            hash,
            spent_utxos: vec![],
            created_outpoints: vec![],
        }
    }

    #[test]
    fn open_fresh_database() {
        let (db, _dir) = make_test_db();
        let meta = db.get_meta().unwrap();
        assert_eq!(meta.schema_version, SCHEMA_VERSION);
        assert_eq!(meta.tip_height, 0);
        assert_eq!(meta.tip_hash, [0u8; 32]);
    }

    #[test]
    fn save_and_retrieve_block() {
        let (db, _dir) = make_test_db();
        let hash = [1u8; 32];
        let block = make_test_block(1, hash, [0u8; 32]);
        let undo = make_test_undo(1, hash);
        let meta = ChainMeta {
            schema_version: SCHEMA_VERSION,
            tip_hash: hash,
            tip_height: 1,
            total_work: 1000,
        };

        db.save_block_and_apply_utxos(&block, &undo, &[], &[], &meta)
            .unwrap();

        let got = db.get_block(&hash).unwrap().unwrap();
        assert_eq!(got.height, 1);
        assert_eq!(got.hash, hash);

        let got_meta = db.get_meta().unwrap();
        assert_eq!(got_meta.tip_height, 1);
        assert_eq!(got_meta.tip_hash, hash);
    }

    #[test]
    fn height_to_hash_lookup() {
        let (db, _dir) = make_test_db();
        let hash = [2u8; 32];
        let block = make_test_block(5, hash, [0u8; 32]);
        let undo = make_test_undo(5, hash);
        let meta = ChainMeta {
            schema_version: SCHEMA_VERSION,
            tip_hash: hash,
            tip_height: 5,
            total_work: 5000,
        };
        db.save_block_and_apply_utxos(&block, &undo, &[], &[], &meta)
            .unwrap();

        assert_eq!(db.get_hash_by_height(5).unwrap(), Some(hash));
        assert_eq!(db.get_hash_by_height(6).unwrap(), None);
    }

    #[test]
    fn utxo_insert_and_spend() {
        let (db, _dir) = make_test_db();
        let hash = [3u8; 32];
        let block = make_test_block(1, hash, [0u8; 32]);
        let undo = make_test_undo(1, hash);
        let meta = ChainMeta {
            schema_version: SCHEMA_VERSION,
            tip_hash: hash,
            tip_height: 1,
            total_work: 1000,
        };

        let tx_hash = [10u8; 32];
        let op = Outpoint { tx_hash, index: 0 };
        let utxo = StoredUtxo {
            amount: 100_000_000,
            address: "zion1test".to_string(),
            created_height: 1,
            is_coinbase: false,
        };

        db.save_block_and_apply_utxos(&block, &undo, &[(op.clone(), utxo.clone())], &[], &meta)
            .unwrap();

        let got = db.get_utxo(&tx_hash, 0).unwrap().unwrap();
        assert_eq!(got.amount, 100_000_000);
        assert_eq!(db.get_balance("zion1test").unwrap(), 100_000_000);

        // Spend it in the next block
        let hash2 = [4u8; 32];
        let block2 = make_test_block(2, hash2, hash);
        let undo2 = make_test_undo(2, hash2);
        let meta2 = ChainMeta {
            schema_version: SCHEMA_VERSION,
            tip_hash: hash2,
            tip_height: 2,
            total_work: 2000,
        };
        db.save_block_and_apply_utxos(&block2, &undo2, &[], &[op], &meta2)
            .unwrap();

        assert!(db.get_utxo(&tx_hash, 0).unwrap().is_none());
        assert_eq!(db.get_balance("zion1test").unwrap(), 0);
    }

    #[test]
    fn balance_tracks_multiple_utxos() {
        let (db, _dir) = make_test_db();
        let hash = [5u8; 32];
        let block = make_test_block(1, hash, [0u8; 32]);
        let undo = make_test_undo(1, hash);
        let meta = ChainMeta {
            schema_version: SCHEMA_VERSION,
            tip_hash: hash,
            tip_height: 1,
            total_work: 1000,
        };

        let addr = "zion1alice";
        let u1 = (
            Outpoint {
                tx_hash: [11u8; 32],
                index: 0,
            },
            StoredUtxo {
                amount: 500,
                address: addr.to_string(),
                created_height: 1,
                is_coinbase: false,
            },
        );
        let u2 = (
            Outpoint {
                tx_hash: [12u8; 32],
                index: 0,
            },
            StoredUtxo {
                amount: 300,
                address: addr.to_string(),
                created_height: 1,
                is_coinbase: false,
            },
        );

        db.save_block_and_apply_utxos(&block, &undo, &[u1, u2], &[], &meta)
            .unwrap();
        assert_eq!(db.get_balance(addr).unwrap(), 800);
    }

    #[test]
    fn tx_index_lookup() {
        let (db, _dir) = make_test_db();
        let hash = [6u8; 32];
        let tx_id = [99u8; 32];
        let tx = tx::Transaction {
            id: tx_id,
            version: crate::tx::TX_HASH_V2_VERSION,
            inputs: vec![],
            outputs: vec![],
            fee: 0,
            timestamp: 1000,
        };
        let block = StoredBlock {
            hash,
            prev_hash: [0u8; 32],
            height: 1,
            timestamp: 1000,
            difficulty: 1000,
            nonce: 0,
            total_work: 1000,
            transactions: vec![tx],
            coinbase_amount: 5_400_067_000_000_000,
        };
        let undo = make_test_undo(1, hash);
        let meta = ChainMeta {
            schema_version: SCHEMA_VERSION,
            tip_hash: hash,
            tip_height: 1,
            total_work: 1000,
        };

        db.save_block_and_apply_utxos(&block, &undo, &[], &[], &meta)
            .unwrap();

        let loc = db.get_tx_location(&tx_id).unwrap().unwrap();
        assert_eq!(loc.block_hash, hash);
        assert_eq!(loc.index, 0);
    }

    #[test]
    fn rollback_block_restores_state() {
        let (db, _dir) = make_test_db();

        // Block 1: create a UTXO
        let hash1 = [7u8; 32];
        let block1 = make_test_block(1, hash1, [0u8; 32]);
        let op = Outpoint {
            tx_hash: [20u8; 32],
            index: 0,
        };
        let utxo = StoredUtxo {
            amount: 1_000,
            address: "zion1bob".to_string(),
            created_height: 1,
            is_coinbase: false,
        };
        let undo1 = make_test_undo(1, hash1);
        let meta1 = ChainMeta {
            schema_version: SCHEMA_VERSION,
            tip_hash: hash1,
            tip_height: 1,
            total_work: 1000,
        };
        db.save_block_and_apply_utxos(&block1, &undo1, &[(op.clone(), utxo)], &[], &meta1)
            .unwrap();

        // Block 2: spend that UTXO
        let hash2 = [8u8; 32];
        let block2 = make_test_block(2, hash2, hash1);
        let undo2 = StoredUndoBlock {
            height: 2,
            hash: hash2,
            spent_utxos: vec![StoredRestoredUtxo {
                tx_hash: [20u8; 32],
                output_index: 0,
                amount: 1_000,
                address: "zion1bob".to_string(),
                created_height: 1,
                is_coinbase: false,
            }],
            created_outpoints: vec![],
        };
        let meta2 = ChainMeta {
            schema_version: SCHEMA_VERSION,
            tip_hash: hash2,
            tip_height: 2,
            total_work: 2000,
        };
        db.save_block_and_apply_utxos(&block2, &undo2, &[], std::slice::from_ref(&op), &meta2)
            .unwrap();

        // UTXO should be gone
        assert!(db.get_utxo(&[20u8; 32], 0).unwrap().is_none());
        assert_eq!(db.get_balance("zion1bob").unwrap(), 0);

        // Now rollback block 2
        let meta_after = ChainMeta {
            schema_version: SCHEMA_VERSION,
            tip_hash: hash1,
            tip_height: 1,
            total_work: 1000,
        };
        db.rollback_block(2, &meta_after).unwrap();

        // UTXO should be restored
        let restored = db.get_utxo(&[20u8; 32], 0).unwrap().unwrap();
        assert_eq!(restored.amount, 1_000);
        assert_eq!(db.get_balance("zion1bob").unwrap(), 1_000);
        assert_eq!(db.get_meta().unwrap().tip_height, 1);
    }

    #[test]
    fn get_block_by_height() {
        let (db, _dir) = make_test_db();
        let hash = [9u8; 32];
        let block = make_test_block(3, hash, [0u8; 32]);
        let undo = make_test_undo(3, hash);
        let meta = ChainMeta {
            schema_version: SCHEMA_VERSION,
            tip_hash: hash,
            tip_height: 3,
            total_work: 3000,
        };
        db.save_block_and_apply_utxos(&block, &undo, &[], &[], &meta)
            .unwrap();

        let got = db.get_block_by_height(3).unwrap().unwrap();
        assert_eq!(got.hash, hash);
        assert!(db.get_block_by_height(4).unwrap().is_none());
    }

    #[test]
    fn export_blocks_range() {
        let (db, _dir) = make_test_db();
        let mut prev = [0u8; 32];
        for h in 1..=5 {
            let mut hash = [0u8; 32];
            hash[0] = h as u8;
            let block = make_test_block(h, hash, prev);
            let undo = make_test_undo(h, hash);
            let meta = ChainMeta {
                schema_version: SCHEMA_VERSION,
                tip_hash: hash,
                tip_height: h,
                total_work: h as u128 * 1000,
            };
            db.save_block_and_apply_utxos(&block, &undo, &[], &[], &meta)
                .unwrap();
            prev = hash;
        }

        let exported = db.export_blocks(2, 4).unwrap();
        assert_eq!(exported.len(), 3);
        assert_eq!(exported[0].height, 2);
        assert_eq!(exported[2].height, 4);
    }

    #[test]
    fn schema_version_check() {
        let dir = tempdir().unwrap();
        // First open — OK
        let db = ChainDb::open_with_map_size(dir.path(), 10 * 1024 * 1024).unwrap();
        assert_eq!(db.get_meta().unwrap().schema_version, SCHEMA_VERSION);
        // Must drop before reopening (LMDB allows one env per path per process)
        drop(db);
        // Second open — still OK (same version)
        let db2 = ChainDb::open_with_map_size(dir.path(), 10 * 1024 * 1024).unwrap();
        assert_eq!(db2.get_meta().unwrap().schema_version, SCHEMA_VERSION);
    }

    #[test]
    fn nonexistent_lookups_return_none() {
        let (db, _dir) = make_test_db();
        assert!(db.get_block(&[255u8; 32]).unwrap().is_none());
        assert!(db.get_utxo(&[255u8; 32], 0).unwrap().is_none());
        assert!(db.get_tx_location(&[255u8; 32]).unwrap().is_none());
        assert!(db.get_hash_by_height(999).unwrap().is_none());
        assert!(db.get_undo_block(999).unwrap().is_none());
    }

    #[test]
    fn tip_height_helper() {
        let (db, _dir) = make_test_db();
        assert_eq!(db.tip_height().unwrap(), 0);

        let hash = [42u8; 32];
        let block = make_test_block(10, hash, [0u8; 32]);
        let undo = make_test_undo(10, hash);
        let meta = ChainMeta {
            schema_version: SCHEMA_VERSION,
            tip_hash: hash,
            tip_height: 10,
            total_work: 10_000,
        };
        db.save_block_and_apply_utxos(&block, &undo, &[], &[], &meta)
            .unwrap();
        assert_eq!(db.tip_height().unwrap(), 10);
    }

    #[test]
    fn peer_persistence_round_trip() {
        let (db, _dir) = make_test_db();

        // No peers initially
        let empty = db.load_peers().unwrap();
        assert!(empty.is_empty());

        // Save some peers
        let peers = vec![
            crate::PeerEndpoint::new("10.0.0.1", 8334),
            crate::PeerEndpoint::new("10.0.0.2", 8334),
            crate::PeerEndpoint::new("seed-eu1.example.com", 9000),
        ];
        db.save_peers(&peers).unwrap();

        // Load them back
        let loaded = db.load_peers().unwrap();
        assert_eq!(loaded.len(), 3);
        assert_eq!(loaded[0].address(), "10.0.0.1:8334");
        assert_eq!(loaded[2].address(), "seed-eu1.example.com:9000");

        // Overwrite with fewer peers
        let fewer = vec![crate::PeerEndpoint::new("10.0.0.99", 1234)];
        db.save_peers(&fewer).unwrap();
        let reloaded = db.load_peers().unwrap();
        assert_eq!(reloaded.len(), 1);
        assert_eq!(reloaded[0].address(), "10.0.0.99:1234");
    }
}
