use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::{BTreeMap, HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};
use zion_cosmic_harmony::{
    account_tx_memo_v1_active, body_root_v2_active,
    cosmic_harmony_ekam_deeksha, cosmic_harmony_with_height, profile_name, profile_name_for_height,
    tx_hash_v2_active, NclStats, RevenueCollector, RevenueEvent, RevenueStats,
    CHV_EKAM_FORK_HEIGHT, EKAM_FUSION_ROUNDS, FIRE_FORK_HEIGHT, TX_HASH_V2_ACTIVATION_HEIGHT,
};

pub use zion_cosmic_harmony::ExternalCoin;
pub use zion_cosmic_harmony::NclStats as NclSnapshot;
pub use zion_cosmic_harmony::RevenueSource;

pub mod admin;
pub mod chain;
pub mod checkpoint;
pub mod crypto;
pub mod difficulty;
pub mod discovery;
pub mod emission;
pub mod fee;
pub mod genesis;
pub mod ibd;
pub mod launch;
pub mod mempool_v2;
pub mod metrics;
pub mod migration;
pub mod node_builder;
pub mod orphan;
pub mod p2p_security;
pub mod peer_manager;
pub mod propagation;
pub mod rpc;
pub mod storage;
pub mod tx;
pub mod validation;
pub mod wallet;
pub mod websocket;

mod peer_block_validation;

pub const HEADER_SIZE: usize = 80;
pub const NODE_PROTOCOL_VERSION: &str = "zion-v3-node/3.0.6";
/// Numeric protocol version — bumped to 2 at 3.0.3 decimal fork (block H+1).
/// Pre-3.0.3 blocks (0..H) use legacy 12-decimal flowers; post-3.0.3 uses 6-decimal.
pub const PROTOCOL_VERSION: u32 = 2;
pub const LEGACY_PROTOCOL_VERSION: u32 = 1;
pub const MAX_TEMPLATE_TRANSACTIONS: usize = 16;
pub const MAX_MEMPOOL_TRANSACTIONS: usize = 4_096;
pub const MAX_TEMPLATE_UTXO_TRANSACTIONS: usize = 16;

/// Default block retention window: 0 = unlimited (keep all blocks in memory).
/// Set via `ZION_BLOCK_RETENTION` env var to cap memory usage.
/// When set, old blocks are pruned from in-memory caches but remain in LMDB.
pub const DEFAULT_BLOCK_RETENTION: usize = 0;

pub mod bridge;
pub use bridge::{
    bridge_operation_message, BridgeUnlockRequest, BridgeValidatorProof,
    BRIDGE_MIN_VALIDATOR_PROOFS,
};
#[allow(unused_imports)]
pub(crate) use bridge::{
    bridge_unlock_memo, bridge_unlock_memo_with_proofs, bridge_unlock_replay_key,
    bridge_unlock_replay_key_from_transaction, load_bridge_validator_pubkey_allowlist,
    parse_bridge_proofs, parse_bridge_unlock_memo, required_bridge_validator_threshold,
    validate_bridge_unlock_transaction_shape_with_utxos, verify_bridge_proofs, BRIDGE_MAX_MEMO_LEN,
    BRIDGE_MAX_VALIDATOR_PROOFS,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum NetworkId {
    Mainnet,
    Testnet,
    Devnet,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PeerEndpoint {
    pub host: String,
    pub port: u16,
}

impl PeerEndpoint {
    pub fn new(host: impl Into<String>, port: u16) -> Self {
        Self {
            host: host.into(),
            port,
        }
    }

    pub fn address(&self) -> String {
        format!("{}:{}", self.host, self.port)
    }

    pub fn parse(address: &str) -> Result<Self, String> {
        let (host, port) = address
            .rsplit_once(':')
            .ok_or_else(|| format!("invalid endpoint address: {address}"))?;
        let port = port
            .parse::<u16>()
            .map_err(|_| format!("invalid endpoint port in {address}"))?;
        Ok(Self::new(host, port))
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NodeConfig {
    pub network: NetworkId,
    pub p2p_bind: PeerEndpoint,
    pub rpc_bind: PeerEndpoint,
    pub pool_bind: PeerEndpoint,
    pub websocket_bind: PeerEndpoint,
    pub seed_peers: Vec<PeerEndpoint>,
}

impl NodeConfig {
    pub fn mainnet() -> Self {
        Self {
            network: NetworkId::Mainnet,
            p2p_bind: PeerEndpoint::new("0.0.0.0", 8333),
            rpc_bind: PeerEndpoint::new("0.0.0.0", 8443),
            pool_bind: PeerEndpoint::new("0.0.0.0", 8444),
            websocket_bind: PeerEndpoint::new("0.0.0.0", 8445),
            seed_peers: vec![
                // 3.0.4 canonical mainnet server (public P2P entrypoint).
                // Old Edge (<LEGACY_EDGE>) decommissioned 2026-07-07 hard reset.
                PeerEndpoint::new("<ZION_SEED_PEER>", 8333),
                // Local fallback for same-machine or LAN bootstrap
                PeerEndpoint::new("127.0.0.1", 8333),
            ],
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct MiningHeader {
    pub version: u32,
    pub previous_hash: [u8; 32],
    pub merkle_root: [u8; 32],
    pub timestamp: u64,
    pub difficulty_bits: u32,
}

impl MiningHeader {
    pub fn to_bytes(self) -> [u8; HEADER_SIZE] {
        let mut bytes = [0u8; HEADER_SIZE];
        bytes[0..4].copy_from_slice(&self.version.to_le_bytes());
        bytes[4..36].copy_from_slice(&self.previous_hash);
        bytes[36..68].copy_from_slice(&self.merkle_root);
        bytes[68..76].copy_from_slice(&self.timestamp.to_le_bytes());
        bytes[76..80].copy_from_slice(&self.difficulty_bits.to_le_bytes());
        bytes
    }

    pub fn from_bytes(bytes: [u8; HEADER_SIZE]) -> Self {
        Self {
            version: u32::from_le_bytes(bytes[0..4].try_into().expect("header version slice")),
            previous_hash: bytes[4..36].try_into().expect("previous hash slice"),
            merkle_root: bytes[36..68].try_into().expect("merkle root slice"),
            timestamp: u64::from_le_bytes(bytes[68..76].try_into().expect("timestamp slice")),
            difficulty_bits: u32::from_le_bytes(
                bytes[76..80].try_into().expect("difficulty bits slice"),
            ),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct BlockCandidate {
    pub header: MiningHeader,
    pub nonce: u64,
    pub height: u64,
}

impl BlockCandidate {
    pub fn hash(self) -> [u8; 32] {
        zion_cosmic_harmony::deeksha_lite::deeksha_lite(&self.header.to_bytes(), self.nonce)
    }

    pub fn seal(self) -> SealedBlock {
        SealedBlock {
            header: self.header,
            nonce: self.nonce,
            hash: self.hash(),
        }
    }

    /// Dual-algo hash — select algorithm by name.
    /// "deeksha_lite_v1" uses the simplified GCN-friendly algorithm.
    pub fn hash_with_algorithm(&self, algorithm: &str) -> [u8; 32] {
        match algorithm {
            "deeksha_lite_v1" => {
                zion_cosmic_harmony::deeksha_lite::deeksha_lite(&self.header.to_bytes(), self.nonce)
            }
            "deeksha_lite_fire" => zion_cosmic_harmony::deeksha_lite_fire::deeksha_lite_fire(
                &self.header.to_bytes(),
                self.nonce,
            ),
            _ => cosmic_harmony_with_height(&self.header.to_bytes(), self.nonce, self.height).data,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct SealedBlock {
    pub header: MiningHeader,
    pub nonce: u64,
    pub hash: [u8; 32],
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct MiningJob {
    pub job_id: u64,
    pub header: MiningHeader,
    pub target: DifficultyTarget,
    pub start_nonce: u64,
    pub nonce_count: u64,
    pub height: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct MiningSolution {
    pub job_id: u64,
    pub candidate: BlockCandidate,
    pub hash: [u8; 32],
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct DifficultyTarget {
    pub bytes: [u8; 32],
}

impl DifficultyTarget {
    pub const MAX: Self = Self { bytes: [0xFF; 32] };

    pub fn allows(&self, hash: &[u8; 32]) -> bool {
        hash <= &self.bytes
    }

    pub fn to_hex(self) -> String {
        hex(&self.bytes)
    }

    pub fn from_hex(raw: &str) -> Result<Self, String> {
        Ok(Self {
            bytes: parse_fixed_hex::<32>(raw, "difficulty target")?,
        })
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ConsensusConfig {
    pub profile: &'static str,
    pub ekam_fork_height: u64,
    pub fire_fork_height: u64,
    pub fusion_rounds: usize,
    pub default_target: DifficultyTarget,
}

impl Default for ConsensusConfig {
    fn default() -> Self {
        Self {
            profile: profile_name(),
            ekam_fork_height: CHV_EKAM_FORK_HEIGHT,
            fire_fork_height: FIRE_FORK_HEIGHT,
            fusion_rounds: EKAM_FUSION_ROUNDS,
            default_target: DifficultyTarget::MAX,
        }
    }
}

impl ConsensusConfig {
    pub fn profile_for_height(&self, height: u64) -> &'static str {
        profile_name_for_height(height)
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RevenueSnapshot {
    pub total_earnings_usd: f64,
    pub zion_fees_usd: f64,
    pub miner_payout_usd: f64,
    // ZION-denominated fields (flowers).
    pub total_zion: u64,
    pub zion_fees_zion: u64,
    pub humanitarian_zion: u64,
    pub issobella_zion: u64,
    pub miner_payout_zion: u64,
    pub blocks_found: u64,
    // Audit fields.
    pub last_block_height: u64,
    pub last_block_ts: Option<String>,
}

impl From<RevenueStats> for RevenueSnapshot {
    fn from(value: RevenueStats) -> Self {
        Self {
            total_earnings_usd: value.total_earnings_usd,
            zion_fees_usd: value.zion_fees_usd,
            miner_payout_usd: value.miner_payout_usd,
            total_zion: value.total_zion,
            zion_fees_zion: value.zion_fees_zion,
            humanitarian_zion: value.humanitarian_zion,
            issobella_zion: value.issobella_zion,
            miner_payout_zion: value.miner_payout_zion,
            blocks_found: value.blocks_found,
            last_block_height: value.last_block_height,
            last_block_ts: value.last_block_ts,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct NodeStatus {
    pub node_id: String,
    pub network: NetworkId,
    pub protocol_version: String,
    pub consensus_profile: String,
    pub chain_height: u64,
    pub tip_hash_hex: String,
    pub active_template_id: u64,
    pub active_template_height: u64,
    pub accepted_blocks: usize,
    pub mempool_transactions: usize,
    pub active_template_transactions: usize,
    pub active_template_total_fees_zion: u64,
    pub p2p_bind: PeerEndpoint,
    pub rpc_bind: PeerEndpoint,
    pub pool_bind: PeerEndpoint,
    pub known_peers: Vec<PeerEndpoint>,
    pub revenue: RevenueSnapshot,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BlockTemplate {
    pub template_id: u64,
    pub height: u64,
    pub header_hex: String,
    pub target_hex: String,
    pub reward_zion: u64,
    pub transaction_ids: Vec<String>,
    pub transaction_count: usize,
    pub total_fees_zion: u64,
    pub body_hash_hex: String,
    pub estimated_miner_reward_zion: u64,
    pub utxo_transaction_ids: Vec<String>,
    pub utxo_transaction_count: usize,
    pub total_utxo_fees: u64,
}

/// Custom serde for u128: serializes as string, deserializes from string or number.
/// Required because serde_json does not natively support u128 without `arbitrary_precision`.
mod serde_u128 {
    use serde::{self, Deserialize, Deserializer, Serializer};
    pub fn serialize<S>(value: &u128, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(&value.to_string())
    }
    pub fn deserialize<'de, D>(deserializer: D) -> Result<u128, D::Error>
    where
        D: Deserializer<'de>,
    {
        #[derive(Deserialize)]
        #[serde(untagged)]
        enum StringOrNum {
            Str(String),
            Num(u64),
        }
        match StringOrNum::deserialize(deserializer)? {
            StringOrNum::Str(s) => s.parse::<u128>().map_err(serde::de::Error::custom),
            StringOrNum::Num(n) => Ok(n as u128),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Transaction {
    pub tx_id: String,
    pub from: String,
    pub to: String,
    #[serde(with = "serde_u128")]
    pub amount_zion: u128,
    pub fee_zion: u64,
    pub nonce: u64,
    /// Ed25519 signature (hex, 128 chars). Required for non-coinbase transactions.
    #[serde(default)]
    pub signature: String,
    /// Ed25519 public key (hex, 64 chars). Required for non-coinbase transactions.
    #[serde(default)]
    pub public_key: String,
    /// Optional protocol memo (account-model). When present, included in the
    /// signed tx_id preimage. ASCII-only, max 256 bytes. Activated by
    /// `ACCOUNT_TX_MEMO_V1_ACTIVATION_HEIGHT`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub memo: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "model", content = "data", rename_all = "snake_case")]
pub enum SubmittedTransaction {
    Account(Transaction),
    Utxo(tx::Transaction),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum RuntimeTransaction {
    Account(Transaction),
    Utxo(tx::Transaction),
}

impl SubmittedTransaction {
    pub fn parse_value(value: Value) -> Result<Self, String> {
        if value.is_string() {
            return Err(
                "raw hex transactions are not supported by the current runtime; submit a transaction object"
                    .into(),
            );
        }

        if let Ok(account_tx) = serde_json::from_value::<Transaction>(value.clone()) {
            return Ok(Self::Account(account_tx));
        }

        if let Ok(utxo_tx) = serde_json::from_value::<tx::Transaction>(value) {
            return Ok(Self::Utxo(utxo_tx));
        }

        Err("invalid transaction payload for both account and UTXO models".into())
    }

    pub fn model(&self) -> &'static str {
        match self {
            Self::Account(_) => "account",
            Self::Utxo(_) => "utxo",
        }
    }

    pub fn tx_id(&self) -> String {
        match self {
            Self::Account(tx) => tx.tx_id.clone(),
            Self::Utxo(tx) => hex(&tx.id),
        }
    }
}

impl RuntimeTransaction {
    fn tx_id(&self) -> String {
        match self {
            Self::Account(tx) => tx.tx_id.clone(),
            Self::Utxo(tx) => hex(&tx.id),
        }
    }

    fn as_account(&self) -> Option<&Transaction> {
        match self {
            Self::Account(tx) => Some(tx),
            Self::Utxo(_) => None,
        }
    }

    #[cfg(test)]
    fn as_account_mut(&mut self) -> Option<&mut Transaction> {
        match self {
            Self::Account(tx) => Some(tx),
            Self::Utxo(_) => None,
        }
    }

    fn into_account(self) -> Option<Transaction> {
        match self {
            Self::Account(tx) => Some(tx),
            Self::Utxo(_) => None,
        }
    }

    fn as_utxo(&self) -> Option<&tx::Transaction> {
        match self {
            Self::Utxo(tx) => Some(tx),
            Self::Account(_) => None,
        }
    }
}

impl From<Transaction> for RuntimeTransaction {
    fn from(value: Transaction) -> Self {
        Self::Account(value)
    }
}

impl From<tx::Transaction> for RuntimeTransaction {
    fn from(value: tx::Transaction) -> Self {
        Self::Utxo(value)
    }
}

fn default_deeksha_lite_v1() -> String {
    "deeksha_lite_v1".to_string()
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AcceptedBlock {
    pub template_id: u64,
    pub height: u64,
    pub timestamp: u64,
    pub difficulty: u64,
    pub nonce: u64,
    pub hash_hex: String,
    /// Serialized 80-byte MiningHeader as hex. Enables PoW verification for
    /// peer-imported blocks.  Empty for legacy persisted blocks (pre-Phase 12).
    #[serde(default)]
    pub header_hex: String,
    /// Hash of the parent block (hex). Enables chain-linkage verification.
    /// Empty for legacy persisted blocks (pre-Phase 13). All-zeros hex for genesis.
    #[serde(default)]
    pub previous_hash_hex: String,
    /// Mining algorithm used for this block (e.g. "deeksha_lite_v1", "deeksha_lite_fire").
    #[serde(default = "default_deeksha_lite_v1")]
    pub algorithm: String,
    pub transaction_ids: Vec<String>,
    pub transactions: Vec<Transaction>,
    pub total_fees_zion: u64,
    pub body_hash_hex: String,
    pub subsidy_zion: u64,
    pub miner_reward_zion: u64,
    /// Address credited by the coinbase transaction. Empty for legacy blocks
    /// (pre-Phase 14) and for blocks mined without a configured miner address.
    #[serde(default)]
    pub miner_address: String,
    /// Humanitarian fund address (5% coinbase). Empty for legacy/single-output blocks.
    #[serde(default)]
    pub humanitarian_address: String,
    /// Issobella fund address (5% coinbase). Empty for legacy/single-output blocks.
    #[serde(default)]
    pub issobella_address: String,
    /// Pool fee address (1% coinbase). Empty for legacy/single-output blocks.
    #[serde(default)]
    pub pool_fee_address: String,
    /// UTXO transaction IDs included in this block (Phase 16). Empty for
    /// account-only blocks or legacy blocks.
    #[serde(default)]
    pub utxo_transaction_ids: Vec<String>,
    /// Full UTXO transactions included in this block.
    #[serde(default)]
    pub utxo_transactions: Vec<tx::Transaction>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum P2pMessage {
    Hello {
        node_id: String,
        protocol_version: String,
        network: NetworkId,
        listen_addr: String,
    },
    Welcome {
        node_id: String,
        protocol_version: String,
        profile: String,
        peers: Vec<PeerEndpoint>,
    },
    Ping {
        nonce: u64,
    },
    Pong {
        nonce: u64,
    },
    GetPeers,
    Peers {
        peers: Vec<PeerEndpoint>,
    },
    GetStatus,
    Status {
        status: NodeStatus,
    },
    GetBlocksSince {
        from_height: u64,
        limit: u16,
    },
    Blocks {
        blocks: Vec<AcceptedBlock>,
    },
    AnnounceBlock {
        block: AcceptedBlock,
    },
    AnnounceTx {
        tx_id: String,
        transaction: SubmittedTransaction,
    },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "method", rename_all = "snake_case")]
pub enum RpcRequest {
    GetStatus,
    GetPeers,
    GetRevenue,
    GetMempool,
    GetTemplate,
    SubmitTransaction {
        transaction: Transaction,
    },
    SubmitCandidate {
        template_id: u64,
        header_hex: String,
        nonce: u64,
        target_hex: String,
        algorithm: String,
    },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum RpcResponse {
    Status {
        status: NodeStatus,
    },
    Peers {
        peers: Vec<PeerEndpoint>,
    },
    Revenue {
        revenue: RevenueSnapshot,
    },
    Mempool {
        transactions: Vec<Transaction>,
    },
    Template {
        template: BlockTemplate,
    },
    TransactionResult {
        accepted: bool,
        tx_id: String,
        reason: Option<String>,
    },
    SubmitResult {
        accepted: bool,
        template_id: u64,
        block_height: Option<u64>,
        hash_hex: String,
        reason: Option<String>,
    },
}

#[derive(Debug, Clone)]
struct TemplateState {
    template_id: u64,
    height: u64,
    header: MiningHeader,
    target: DifficultyTarget,
    difficulty: u64,
    reward_zion: u64,
    transactions: Vec<RuntimeTransaction>,
    total_fees_zion: u64,
}

#[derive(Debug, Clone)]
struct ChainState {
    height: u64,
    tip_hash: [u8; 32],
    next_template_id: u64,
    active_template: TemplateState,
    accepted_blocks: Vec<AcceptedBlock>,
    accepted_by_height: BTreeMap<u64, AcceptedBlock>,
    accepted_by_template_id: HashMap<u64, AcceptedBlock>,
    mempool: Vec<RuntimeTransaction>,
    mempool_by_id: HashMap<String, RuntimeTransaction>,
    /// Address to credit in coinbase transactions. Empty = no coinbase generated.
    miner_address: String,
    /// Humanitarian fund address (5% of coinbase). Empty = portion goes to miner.
    humanitarian_address: String,
    /// Issobella fund address (5% of coinbase). Empty = portion goes to miner.
    issobella_address: String,
    /// Pool fee address (1% of coinbase). Empty = portion goes to miner.
    pool_fee_address: String,
    bridge_unlock_replay_keys: HashSet<String>,
    /// In-memory address → block indices map for O(1) transaction history lookup.
    /// Built incrementally as blocks are accepted. Key = address, Value = indices
    /// into `accepted_blocks` where that address appears (as sender, recipient, or miner).
    address_tx_index: HashMap<String, Vec<usize>>,
    /// Maximum number of blocks to keep in memory. 0 = unlimited.
    /// Old blocks are pruned from in-memory caches but remain in LMDB.
    block_retention: usize,
    /// Per-instance F5 balance-check activation height. Default `u64::MAX`
    /// (disabled). Tests can set this to 0 to enable from genesis without
    /// affecting parallel test runtimes.
    balance_check_height: u64,
    /// Per-instance F4.7 max-tx-amount cap activation height. Default `u64::MAX`
    /// (disabled). When active, rejects any non-genesis, non-coinbase TX whose
    /// amount exceeds `emission::TOTAL_SUPPLY`.
    max_tx_amount_height: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct ChainStateSnapshot {
    height: u64,
    tip_hash_hex: String,
    next_template_id: u64,
    active_template: BlockTemplate,
    accepted_blocks: Vec<AcceptedBlock>,
    mempool: Vec<Transaction>,
    #[serde(default)]
    utxo_mempool: Vec<tx::Transaction>,
    #[serde(default)]
    bridge_unlock_replay_keys: Vec<String>,
}

#[derive(Debug, Clone)]
struct ChainStore {
    path: PathBuf,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
#[allow(clippy::large_enum_variant)] // boxing would change journal (de)serialization shape
enum ChainJournalEntry {
    TransactionAccepted { transaction: RuntimeTransaction },
    BlockAccepted { block: AcceptedBlock },
}

pub struct CoreRuntime {
    consensus: ConsensusConfig,
    revenue: RevenueCollector,
}

pub struct NodeRuntime {
    node_id: String,
    config: NodeConfig,
    core: CoreRuntime,
    known_peers: Vec<PeerEndpoint>,
    chain_state: ChainState,
    chain_store: Option<ChainStore>,
    miner_address: String,
    humanitarian_address: String,
    issobella_address: String,
    pool_fee_address: String,
    ws_notifier: Option<std::sync::Arc<websocket::WebSocketServer>>,
}

impl Default for CoreRuntime {
    fn default() -> Self {
        Self::new(ConsensusConfig::default())
    }
}

impl CoreRuntime {
    pub fn new(consensus: ConsensusConfig) -> Self {
        Self {
            consensus,
            revenue: RevenueCollector::new(),
        }
    }

    /// Create a CoreRuntime with an env-configured RevenueJournal and replay
    /// all persisted events so the revenue collector resumes from where it
    /// left off after a restart.
    pub fn new_with_journal_replay(consensus: ConsensusConfig) -> Self {
        let collector = RevenueCollector::with_env_journal();
        collector.replay();
        Self {
            consensus,
            revenue: collector,
        }
    }

    pub fn consensus(&self) -> &ConsensusConfig {
        &self.consensus
    }

    pub fn consensus_profile(&self) -> &'static str {
        self.consensus.profile
    }

    pub fn consensus_profile_for_height(&self, height: u64) -> &'static str {
        self.consensus.profile_for_height(height)
    }

    pub fn hash_candidate(&self, candidate: BlockCandidate) -> [u8; 32] {
        candidate.hash()
    }

    pub fn hash_candidate_with_algorithm(
        &self,
        candidate: BlockCandidate,
        algorithm: &str,
    ) -> [u8; 32] {
        candidate.hash_with_algorithm(algorithm)
    }

    pub fn validate_candidate(
        &self,
        candidate: BlockCandidate,
        target: DifficultyTarget,
    ) -> Option<SealedBlock> {
        let sealed = candidate.seal();
        if target.allows(&sealed.hash) {
            Some(sealed)
        } else {
            None
        }
    }

    pub fn validate_candidate_with_algorithm(
        &self,
        candidate: BlockCandidate,
        target: DifficultyTarget,
        algorithm: &str,
    ) -> Option<SealedBlock> {
        let hash = candidate.hash_with_algorithm(algorithm);
        if target.allows(&hash) {
            Some(SealedBlock {
                header: candidate.header,
                nonce: candidate.nonce,
                hash,
            })
        } else {
            None
        }
    }

    pub fn validate_candidate_for_height(
        &self,
        candidate: BlockCandidate,
        target: DifficultyTarget,
        height: u64,
    ) -> Option<SealedBlock> {
        let algorithm = self.consensus_profile_for_height(height);
        self.validate_candidate_with_algorithm(candidate, target, algorithm)
    }

    pub fn scan_nonce_range(&self, job: MiningJob) -> Option<MiningSolution> {
        for offset in 0..job.nonce_count {
            let candidate = BlockCandidate {
                header: job.header,
                nonce: job.start_nonce.wrapping_add(offset),
                height: job.height,
            };
            let hash = self.hash_candidate(candidate);
            if job.target.allows(&hash) {
                return Some(MiningSolution {
                    job_id: job.job_id,
                    candidate,
                    hash,
                });
            }
        }
        None
    }

    pub fn validate_solution(
        &self,
        job: MiningJob,
        solution: MiningSolution,
    ) -> Option<SealedBlock> {
        if solution.job_id != job.job_id || solution.candidate.header != job.header {
            return None;
        }

        let sealed = self.validate_candidate(solution.candidate, job.target)?;
        if sealed.hash == solution.hash {
            Some(sealed)
        } else {
            None
        }
    }

    pub fn record_revenue(&self, source: RevenueSource, value_usd: f64, qualifies: bool) {
        self.revenue
            .track_event(RevenueEvent::new(source, value_usd, qualifies));
    }

    /// Record revenue from an external pool (e.g. 2miners, NiceHash).
    /// `external_coin` is the ticker of the mined coin (e.g. "DCR", "KAS", "ETC").
    /// This is used for multi-algo revenue tracking with BTC payout.
    pub fn record_external_revenue(
        &self,
        source: RevenueSource,
        value_usd: f64,
        external_coin: Option<&str>,
    ) {
        let mut event = RevenueEvent::new(source, value_usd, true);
        if let Some(coin) = external_coin {
            event = event.with_external_coin(coin);
        }
        self.revenue.track_event(event);
    }

    /// Record a canonical ZION Deeksha block reward in the revenue collector.
    /// `tx_hash` may be `None` if the on-chain tx hash is not yet known.
    pub fn record_zion_block_revenue(
        &self,
        height: u64,
        subsidy: u64,
        pool_fee_pct: u64,
        tx_hash: Option<String>,
    ) {
        self.revenue
            .track_zion_block(height, subsidy, pool_fee_pct, tx_hash);
    }

    pub fn revenue_snapshot(&self) -> RevenueSnapshot {
        self.revenue.get_stats().into()
    }

    /// Record a Neural Compute Layer (NCL) inference task that produced
    /// revenue.  `value_usd` is the gross customer payment for the task;
    /// the standard NCL fee rate (`NCL_FEE = 10 %`) is applied internally.
    /// Set `success = false` to record a failed task — this still bumps
    /// the failure counter (and trips the circuit breaker after enough
    /// consecutive failures) but contributes zero revenue.
    pub fn record_ncl_task_revenue(
        &self,
        value_usd: f64,
        tokens_in: u64,
        tokens_out: u64,
        latency_ms: u64,
        success: bool,
    ) {
        self.revenue
            .track_ncl_task_detailed(value_usd, tokens_in, tokens_out, latency_ms, success);
    }

    /// Snapshot of the NCL task / token / latency telemetry counters.
    pub fn ncl_stats(&self) -> NclStats {
        self.revenue.ncl_stats()
    }

    /// Clone of the underlying revenue collector handle.  Inexpensive
    /// (`RevenueCollector` is `Arc`-internal) and intended for async
    /// subsystems — e.g. the pool's NCL gateway dispatcher — that need
    /// to push events into the same accounting state as the main
    /// `CoreRuntime` without coupling to its full lifecycle.
    pub fn revenue_handle(&self) -> RevenueCollector {
        self.revenue.clone()
    }
}

pub fn consensus_profile() -> &'static str {
    profile_name()
}

pub fn node_protocol_version() -> &'static str {
    NODE_PROTOCOL_VERSION
}

pub fn encode_p2p_message(message: &P2pMessage) -> Result<String, serde_json::Error> {
    encode_json_line(message)
}

pub fn decode_p2p_message(line: &str) -> Result<P2pMessage, serde_json::Error> {
    serde_json::from_str(line.trim())
}

pub fn encode_rpc_request(message: &RpcRequest) -> Result<String, serde_json::Error> {
    encode_json_line(message)
}

pub fn decode_rpc_request(line: &str) -> Result<RpcRequest, serde_json::Error> {
    serde_json::from_str(line.trim())
}

pub fn encode_rpc_response(message: &RpcResponse) -> Result<String, serde_json::Error> {
    encode_json_line(message)
}

pub fn decode_rpc_response(line: &str) -> Result<RpcResponse, serde_json::Error> {
    serde_json::from_str(line.trim())
}

impl NodeRuntime {
    fn uses_strict_mainnet_seed_peers(&self) -> bool {
        self.config.network == NetworkId::Mainnet && !self.config.seed_peers.is_empty()
    }

    fn is_allowed_peer(&self, peer: &PeerEndpoint) -> bool {
        if !self.uses_strict_mainnet_seed_peers() {
            return true;
        }

        self.config
            .seed_peers
            .iter()
            .any(|allowed| allowed.address().eq_ignore_ascii_case(&peer.address()))
    }

    fn prune_known_peers(&mut self) {
        if !self.uses_strict_mainnet_seed_peers() {
            return;
        }

        self.known_peers = dedup_peers(
            self.known_peers
                .iter()
                .filter(|peer| self.is_allowed_peer(peer))
                .cloned()
                .collect(),
        );
    }

    pub fn new(node_id: impl Into<String>, config: NodeConfig) -> Self {
        let node_id = node_id.into();
        let known_peers = dedup_peers(config.seed_peers.clone());
        let core = CoreRuntime::default();
        let chain_state = ChainState::new(&node_id, &core);
        Self {
            node_id,
            config,
            core,
            known_peers,
            chain_state,
            chain_store: None,
            miner_address: String::new(),
            humanitarian_address: String::new(),
            issobella_address: String::new(),
            pool_fee_address: String::new(),
            ws_notifier: None,
        }
    }

    /// Set the in-memory block retention window.
    /// 0 = unlimited (default), N = keep only last N blocks in memory.
    /// Old blocks are pruned from RAM but remain in LMDB persistent storage.
    pub fn set_block_retention(&mut self, retention: usize) {
        self.chain_state.block_retention = retention;
        // Immediately prune if we're already over the limit
        self.chain_state.prune_old_blocks();
    }

    pub fn with_websocket_notifier(
        node_id: impl Into<String>,
        config: NodeConfig,
        ws_notifier: std::sync::Arc<websocket::WebSocketServer>,
    ) -> Self {
        let node_id = node_id.into();
        let known_peers = dedup_peers(config.seed_peers.clone());
        let core = CoreRuntime::default();
        let chain_state = ChainState::new(&node_id, &core);
        Self {
            node_id,
            config,
            core,
            known_peers,
            chain_state,
            chain_store: None,
            miner_address: String::new(),
            humanitarian_address: String::new(),
            issobella_address: String::new(),
            pool_fee_address: String::new(),
            ws_notifier: Some(ws_notifier),
        }
    }

    pub fn with_chain_store(
        node_id: impl Into<String>,
        config: NodeConfig,
        state_path: impl Into<PathBuf>,
    ) -> Result<Self, String> {
        let node_id = node_id.into();
        let known_peers = dedup_peers(config.seed_peers.clone());
        let core = CoreRuntime::default();
        let chain_store = ChainStore {
            path: state_path.into(),
        };
        let snapshot = match chain_store.load_snapshot() {
            Ok(snapshot) => snapshot,
            Err(error) => {
                if chain_store.journal_exists() {
                    eprintln!("node_state_snapshot_recovery_fallback={error}");
                    None
                } else {
                    return Err(error);
                }
            }
        };
        let mut chain_state = match snapshot {
            Some(snapshot) => ChainState::from_snapshot(&node_id, &core, snapshot)?,
            None => ChainState::new(&node_id, &core),
        };
        chain_store.replay_journal(&node_id, &core, &mut chain_state)?;

        let mut runtime = Self {
            node_id,
            config,
            core,
            known_peers,
            chain_state,
            chain_store: Some(chain_store),
            miner_address: String::new(),
            humanitarian_address: String::new(),
            issobella_address: String::new(),
            pool_fee_address: String::new(),
            ws_notifier: None,
        };
        runtime.persist_chain_state()?;
        runtime.load_persisted_peers();
        Ok(runtime)
    }

    pub fn with_chain_store_and_websocket_notifier(
        node_id: impl Into<String>,
        config: NodeConfig,
        state_path: impl Into<PathBuf>,
        ws_notifier: std::sync::Arc<websocket::WebSocketServer>,
    ) -> Result<Self, String> {
        let node_id = node_id.into();
        let known_peers = dedup_peers(config.seed_peers.clone());
        let core = CoreRuntime::default();
        let chain_store = ChainStore {
            path: state_path.into(),
        };
        let snapshot = match chain_store.load_snapshot() {
            Ok(snapshot) => snapshot,
            Err(error) => {
                if chain_store.journal_exists() {
                    eprintln!("node_state_snapshot_recovery_fallback={error}");
                    None
                } else {
                    return Err(error);
                }
            }
        };
        let mut chain_state = match snapshot {
            Some(snapshot) => ChainState::from_snapshot(&node_id, &core, snapshot)?,
            None => ChainState::new(&node_id, &core),
        };
        chain_store.replay_journal(&node_id, &core, &mut chain_state)?;

        let mut runtime = Self {
            node_id,
            config,
            core,
            known_peers,
            chain_state,
            chain_store: Some(chain_store),
            miner_address: String::new(),
            humanitarian_address: String::new(),
            issobella_address: String::new(),
            pool_fee_address: String::new(),
            ws_notifier: Some(ws_notifier),
        };
        runtime.persist_chain_state()?;
        runtime.load_persisted_peers();
        Ok(runtime)
    }

    /// Set the wallet id that receives coinbase rewards for mined blocks.
    /// When set, every new block template will include a coinbase transaction
    /// crediting the block subsidy to this wallet id.
    pub fn set_miner_address(&mut self, addr: String) {
        self.miner_address = addr.clone();
        self.chain_state.miner_address = addr;
        self.rebuild_active_template();
    }

    /// Set the per-instance F5 balance-check activation height for the
    /// underlying chain state. Use 0 to enable from genesis, or `u64::MAX`
    /// to disable (default).
    pub fn set_balance_check_height(&mut self, height: u64) {
        self.chain_state.balance_check_height = height;
    }

    /// Set the per-instance F4.7 max-tx-amount cap activation height for the
    /// underlying chain state. Use 0 to enable from genesis, or `u64::MAX`
    /// to disable (default).
    pub fn set_max_tx_amount_height(&mut self, height: u64) {
        self.chain_state.max_tx_amount_height = height;
    }

    /// Set the WebSocket notifier for real-time event broadcasting.
    pub fn set_websocket_notifier(
        &mut self,
        ws_notifier: std::sync::Arc<websocket::WebSocketServer>,
    ) {
        self.ws_notifier = Some(ws_notifier);
    }

    /// Set the fee split destination addresses.
    /// When set, coinbase is split: 89% miner, 5% humanitarian, 5% issobella, 1% pool fee.
    pub fn set_fee_addresses(&mut self, humanitarian: String, issobella: String, pool_fee: String) {
        self.humanitarian_address = humanitarian.clone();
        self.issobella_address = issobella.clone();
        self.pool_fee_address = pool_fee.clone();
        self.chain_state.humanitarian_address = humanitarian;
        self.chain_state.issobella_address = issobella;
        self.chain_state.pool_fee_address = pool_fee;
        self.rebuild_active_template();
    }

    fn rebuild_active_template(&mut self) {
        let next_id = self.chain_state.next_template_id.saturating_sub(1);
        self.chain_state.active_template = ChainState::build_template(
            &self.node_id,
            &self.core,
            self.chain_state.height,
            self.chain_state.tip_hash,
            next_id,
            &self.chain_state.mempool,
            &self.chain_state.accepted_blocks,
            &self.chain_state.miner_address,
            &self.chain_state.humanitarian_address,
            &self.chain_state.issobella_address,
            &self.chain_state.pool_fee_address,
        );
    }

    pub fn miner_address(&self) -> &str {
        &self.miner_address
    }

    pub fn config(&self) -> &NodeConfig {
        &self.config
    }

    pub fn node_id(&self) -> &str {
        &self.node_id
    }

    pub fn known_peers(&self) -> &[PeerEndpoint] {
        &self.known_peers
    }

    pub fn register_peer(&mut self, peer: PeerEndpoint) {
        if peer.address() == self.config.p2p_bind.address() {
            return;
        }
        if !self.is_allowed_peer(&peer) {
            return;
        }
        if self
            .known_peers
            .iter()
            .all(|known| known.address() != peer.address())
        {
            self.known_peers.push(peer);
        }
    }

    pub fn register_peers(&mut self, peers: impl IntoIterator<Item = PeerEndpoint>) {
        for peer in peers {
            self.register_peer(peer);
        }
    }

    // ── Peer persistence (Phase 11) ────────────────────────────────────

    /// Return the path for persisting known peers (sibling of chain state file).
    fn peers_path(&self) -> Option<PathBuf> {
        self.chain_store.as_ref().map(|cs| {
            let mut p = cs.path.clone();
            p.set_file_name("peers.json");
            p
        })
    }

    /// Save known_peers to disk as JSON. No-op if no state_path configured.
    pub fn persist_peers(&self) -> Result<(), String> {
        let path = match self.peers_path() {
            Some(p) => p,
            None => return Ok(()),
        };
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).map_err(|e| format!("peers mkdir: {e}"))?;
        }
        let json = serde_json::to_string_pretty(&self.known_peers)
            .map_err(|e| format!("peers encode: {e}"))?;
        fs::write(&path, json.as_bytes()).map_err(|e| format!("peers write: {e}"))?;
        Ok(())
    }

    /// Load persisted peers from disk and merge into known_peers.
    /// Called once at startup after `with_chain_store`.
    pub fn load_persisted_peers(&mut self) {
        let path = match self.peers_path() {
            Some(p) => p,
            None => return,
        };
        let raw = match fs::read_to_string(&path) {
            Ok(r) => r,
            Err(_) => return, // no file yet — first run
        };
        let peers: Vec<PeerEndpoint> = match serde_json::from_str(&raw) {
            Ok(p) => p,
            Err(e) => {
                eprintln!("peers_load_err path={} err={e}", path.display());
                return;
            }
        };
        let count = peers.len();
        self.register_peers(peers);
        self.prune_known_peers();
        println!(
            "peers_loaded count={count} total={}",
            self.known_peers.len()
        );
    }

    /// Number of known peers (for diagnostics).
    pub fn peer_count(&self) -> usize {
        self.known_peers.len()
    }

    pub fn status(&self) -> NodeStatus {
        NodeStatus {
            node_id: self.node_id.clone(),
            network: self.config.network,
            protocol_version: node_protocol_version().to_string(),
            consensus_profile: self.core.consensus_profile().to_string(),
            chain_height: self.chain_state.height,
            tip_hash_hex: hex(&self.chain_state.tip_hash),
            active_template_id: self.chain_state.active_template.template_id,
            active_template_height: self.chain_state.active_template.height,
            accepted_blocks: self.chain_state.accepted_blocks.len(),
            mempool_transactions: self.chain_state.mempool.len(),
            active_template_transactions: self.chain_state.active_template.transactions.len(),
            active_template_total_fees_zion: self.chain_state.active_template.total_fees_zion,
            p2p_bind: self.config.p2p_bind.clone(),
            rpc_bind: self.config.rpc_bind.clone(),
            pool_bind: self.config.pool_bind.clone(),
            known_peers: self.known_peers.clone(),
            revenue: self.core.revenue_snapshot(),
        }
    }

    pub fn active_template(&self) -> BlockTemplate {
        self.chain_state.active_template.as_public()
    }

    pub fn accepted_blocks(&self) -> &[AcceptedBlock] {
        &self.chain_state.accepted_blocks
    }

    /// Returns indices of blocks that contain transactions for the given address.
    /// Uses the in-memory address_tx_index for O(1) lookup instead of scanning
    /// all blocks. Returns an empty vec if the address has no transactions.
    pub fn block_indices_for_address(&self, address: &str) -> Vec<usize> {
        self.chain_state
            .block_indices_for_address(address)
            .cloned()
            .unwrap_or_default()
    }

    pub fn accepted_blocks_since(&self, from_height: u64, limit: usize) -> Vec<AcceptedBlock> {
        self.chain_state
            .accepted_blocks
            .iter()
            .filter(|block| block.height > from_height)
            .take(limit)
            .cloned()
            .collect()
    }

    pub fn accepted_block_by_height(&self, height: u64) -> Option<&AcceptedBlock> {
        self.chain_state.accepted_by_height.get(&height)
    }

    pub fn accepted_block_by_template_id(&self, template_id: u64) -> Option<&AcceptedBlock> {
        self.chain_state.accepted_by_template_id.get(&template_id)
    }

    pub fn chain_height(&self) -> u64 {
        self.chain_state.height
    }

    pub fn utxo_balance(&self, address: &str) -> u128 {
        self.chain_state.utxo_balance(address)
    }

    pub fn spendable_utxos(&self, address: &str) -> Vec<SpendableUtxo> {
        self.chain_state.spendable_utxos(address)
    }

    pub fn needs_blocks_from(&self, peer_height: u64) -> bool {
        peer_height > self.chain_state.height
    }

    /// Return the current tip hash as hex string.
    pub fn tip_hash_hex(&self) -> String {
        hex(&self.chain_state.tip_hash)
    }

    /// Wipe chain state back to genesis and persist the clean slate.
    /// Used for fork recovery when the local chain has diverged beyond
    /// MAX_REORG_DEPTH and automatic reorg is impossible.
    pub fn reset_to_genesis(&mut self) -> Result<(), String> {
        eprintln!(
            "fork_recovery_reset height={} tip={}",
            self.chain_state.height,
            hex(&self.chain_state.tip_hash),
        );
        self.chain_state = ChainState::new(&self.node_id, &self.core);
        // Restore wallet addresses so post-IBD mining works
        self.chain_state.miner_address = self.miner_address.clone();
        self.chain_state.humanitarian_address = self.humanitarian_address.clone();
        self.chain_state.issobella_address = self.issobella_address.clone();
        self.chain_state.pool_fee_address = self.pool_fee_address.clone();
        self.persist_chain_state()?;
        if let Some(ref chain_store) = self.chain_store {
            chain_store.clear_journal()?;
        }
        eprintln!("fork_recovery_reset_complete new_height=0");
        Ok(())
    }

    pub fn persist_chain_state(&self) -> Result<(), String> {
        match &self.chain_store {
            Some(chain_store) => chain_store.save_snapshot(&self.chain_state.snapshot())?,
            None => return Ok(()),
        }
        if let Some(chain_store) = &self.chain_store {
            chain_store.clear_journal()?;
        }
        Ok(())
    }

    fn persist_chain_update(&self, entry: &ChainJournalEntry) -> Result<(), String> {
        match &self.chain_store {
            Some(chain_store) => {
                chain_store.append_journal_entry(entry)?;
                chain_store.save_snapshot(&self.chain_state.snapshot())?;
                chain_store.clear_journal()
            }
            None => Ok(()),
        }
    }

    pub fn p2p_hello(&self) -> P2pMessage {
        P2pMessage::Hello {
            node_id: self.node_id.clone(),
            protocol_version: node_protocol_version().to_string(),
            network: self.config.network,
            listen_addr: self.config.p2p_bind.address(),
        }
    }

    pub fn handle_p2p_message(&mut self, message: P2pMessage) -> Result<P2pMessage, String> {
        match message {
            P2pMessage::Hello {
                network,
                listen_addr,
                ..
            } => {
                if network != self.config.network {
                    return Err(format!(
                        "network mismatch: expected {:?}, got {:?}",
                        self.config.network, network
                    ));
                }
                self.register_peer(PeerEndpoint::parse(&listen_addr)?);
                Ok(P2pMessage::Welcome {
                    node_id: self.node_id.clone(),
                    protocol_version: node_protocol_version().to_string(),
                    profile: self.core.consensus_profile().to_string(),
                    peers: self.known_peers.clone(),
                })
            }
            P2pMessage::Ping { nonce } => Ok(P2pMessage::Pong { nonce }),
            P2pMessage::GetPeers => Ok(P2pMessage::Peers {
                peers: self.known_peers.clone(),
            }),
            P2pMessage::GetStatus => Ok(P2pMessage::Status {
                status: self.status(),
            }),
            P2pMessage::GetBlocksSince { from_height, limit } => Ok(P2pMessage::Blocks {
                blocks: self.accepted_blocks_since(from_height, limit.max(1) as usize),
            }),
            P2pMessage::AnnounceBlock { block } => {
                let _newly_accepted = self.import_peer_block(block)?;
                Ok(P2pMessage::Status {
                    status: self.status(),
                })
            }
            P2pMessage::AnnounceTx { tx_id, transaction } => {
                let response = self.submit_submitted_transaction(transaction);
                match &response {
                    RpcResponse::TransactionResult { accepted, .. } if *accepted => {
                        Ok(P2pMessage::Status {
                            status: self.status(),
                        })
                    }
                    RpcResponse::TransactionResult { reason, .. } => Err(format!(
                        "tx {} rejected: {}",
                        tx_id,
                        reason.as_deref().unwrap_or("unknown")
                    )),
                    _ => Err("unexpected response from submit_submitted_transaction".into()),
                }
            }
            other => Err(format!("unsupported inbound p2p message: {other:?}")),
        }
    }

    /// Handle an `AnnounceBlock` from a peer. Returns the newly accepted
    /// block if it was new (for relay), or `None` if it was a duplicate.
    /// The caller is responsible for relaying to other peers.
    pub fn handle_announce_block(
        &mut self,
        block: AcceptedBlock,
    ) -> Result<Option<AcceptedBlock>, String> {
        self.import_peer_block(block)
    }

    /// Handle an `AnnounceTx` from a peer. Returns `true` if the
    /// transaction was accepted into the mempool (and should be relayed).
    pub fn handle_announce_tx(&mut self, _tx_id: &str, transaction: SubmittedTransaction) -> bool {
        let response = self.submit_submitted_transaction(transaction);
        matches!(&response, RpcResponse::TransactionResult { accepted, .. } if *accepted)
    }

    /// Return the last accepted block, if any. Useful after RPC
    /// `submit_candidate` to relay the newly mined block.
    pub fn last_accepted_block(&self) -> Option<&AcceptedBlock> {
        self.chain_state.accepted_blocks.last()
    }

    /// Import a single peer block. Returns `Ok(Some(block))` if the block
    /// was newly accepted (and should be relayed), `Ok(None)` if it was a
    /// duplicate, or `Err` on validation failure.
    fn import_peer_block(&mut self, block: AcceptedBlock) -> Result<Option<AcceptedBlock>, String> {
        let height_before = self.chain_state.height;
        self.chain_state
            .import_peer_block(&self.node_id, &self.core, block)?;
        self.persist_chain_state()?;
        if self.chain_state.height > height_before {
            let accepted_block = self.chain_state.accepted_blocks.last().cloned();

            // Notify WebSocket subscribers about new block
            if let (Some(ws_notifier), Some(block)) = (&self.ws_notifier, &accepted_block) {
                ws_notifier.notify_new_block(block);
            }

            Ok(accepted_block)
        } else {
            Ok(None)
        }
    }

    pub fn import_peer_blocks(&mut self, blocks: Vec<AcceptedBlock>) -> Result<usize, String> {
        let imported = self
            .chain_state
            .import_peer_blocks(&self.node_id, &self.core, blocks)?;
        if imported > 0 {
            self.persist_chain_state()?;

            // Notify WebSocket subscribers about newly accepted blocks
            if let Some(ws_notifier) = &self.ws_notifier {
                for block in self.chain_state.accepted_blocks.iter().rev().take(imported) {
                    ws_notifier.notify_new_block(block);
                }
            }
        }
        Ok(imported)
    }

    pub fn handle_rpc_request(&mut self, request: RpcRequest) -> RpcResponse {
        match request {
            RpcRequest::GetStatus => RpcResponse::Status {
                status: self.status(),
            },
            RpcRequest::GetPeers => RpcResponse::Peers {
                peers: self.known_peers.clone(),
            },
            RpcRequest::GetRevenue => RpcResponse::Revenue {
                revenue: self.core.revenue_snapshot(),
            },
            RpcRequest::GetMempool => RpcResponse::Mempool {
                transactions: self.chain_state.account_mempool_transactions(),
            },
            RpcRequest::GetTemplate => RpcResponse::Template {
                template: self.active_template(),
            },
            RpcRequest::SubmitTransaction { transaction } => {
                self.submit_submitted_transaction(SubmittedTransaction::Account(transaction))
            }
            RpcRequest::SubmitCandidate {
                template_id,
                header_hex,
                nonce,
                target_hex,
                algorithm,
            } => {
                self.submit_candidate_rpc(template_id, &header_hex, nonce, &target_hex, &algorithm)
            }
        }
    }

    pub fn submit_submitted_transaction(
        &mut self,
        transaction: SubmittedTransaction,
    ) -> RpcResponse {
        match transaction {
            SubmittedTransaction::Account(transaction) => self.submit_transaction_rpc(transaction),
            SubmittedTransaction::Utxo(transaction) => {
                self.submit_utxo_transaction_rpc(transaction)
            }
        }
    }

    fn submit_utxo_transaction_rpc(&mut self, transaction: tx::Transaction) -> RpcResponse {
        let tx_id = hex(&transaction.id);
        match self
            .chain_state
            .insert_utxo_transaction(&self.node_id, &self.core, transaction)
        {
            Ok(()) => {
                // The transaction was accepted into the mempool, so it
                // *should* be present in mempool_by_id. If it isn't (e.g.
                // a future state-machine bug, or an in-flight eviction),
                // log the inconsistency and skip journaling rather than
                // panicking — the tx is already accepted, the caller
                // deserves a successful response. (Audit finding F5.)
                match self.chain_state.mempool_by_id.get(&tx_id).cloned() {
                    Some(transaction) => {
                        if let Err(error) =
                            self.persist_chain_update(&ChainJournalEntry::TransactionAccepted {
                                transaction: transaction.clone(),
                            })
                        {
                            eprintln!("node_state_persist_error={error}");
                        }

                        // Notify WebSocket subscribers about pending transaction
                        if let Some(ws_notifier) = &self.ws_notifier {
                            ws_notifier.notify_pending_transaction(&transaction);
                        }
                    }
                    None => {
                        eprintln!(
                            "node_state_persist_skipped: accepted UTXO transaction {tx_id} \
                             missing from mempool_by_id index"
                        );
                    }
                }
                RpcResponse::TransactionResult {
                    accepted: true,
                    tx_id,
                    reason: None,
                }
            }
            Err(reason) => RpcResponse::TransactionResult {
                accepted: false,
                tx_id,
                reason: Some(reason),
            },
        }
    }

    pub fn submit_bridge_unlock(
        &mut self,
        request: BridgeUnlockRequest,
        proofs: Vec<BridgeValidatorProof>,
    ) -> RpcResponse {
        match self
            .chain_state
            .build_bridge_unlock_transaction(&request, &proofs)
        {
            Ok(transaction) => self.submit_utxo_transaction_rpc(transaction),
            Err(reason) => RpcResponse::TransactionResult {
                accepted: false,
                tx_id: String::new(),
                reason: Some(reason),
            },
        }
    }

    fn submit_candidate_rpc(
        &mut self,
        template_id: u64,
        header_hex: &str,
        nonce: u64,
        target_hex: &str,
        algorithm: &str,
    ) -> RpcResponse {
        let header = match parse_fixed_hex::<HEADER_SIZE>(header_hex, "rpc header") {
            Ok(bytes) => MiningHeader::from_bytes(bytes),
            Err(reason) => {
                return RpcResponse::SubmitResult {
                    accepted: false,
                    template_id,
                    block_height: None,
                    hash_hex: String::new(),
                    reason: Some(reason),
                }
            }
        };

        let target = match DifficultyTarget::from_hex(target_hex) {
            Ok(target) => target,
            Err(reason) => {
                return RpcResponse::SubmitResult {
                    accepted: false,
                    template_id,
                    block_height: None,
                    hash_hex: String::new(),
                    reason: Some(reason),
                }
            }
        };

        let active_template = &self.chain_state.active_template;
        if template_id != active_template.template_id {
            return RpcResponse::SubmitResult {
                accepted: false,
                template_id,
                block_height: None,
                hash_hex: String::new(),
                reason: Some(format!(
                    "stale template: expected {}, got {}",
                    active_template.template_id, template_id
                )),
            };
        }

        if header != active_template.header {
            return RpcResponse::SubmitResult {
                accepted: false,
                template_id,
                block_height: None,
                hash_hex: String::new(),
                reason: Some("candidate header does not match active template".to_string()),
            };
        }

        if target != active_template.target {
            return RpcResponse::SubmitResult {
                accepted: false,
                template_id,
                block_height: None,
                hash_hex: String::new(),
                reason: Some("candidate target does not match active template".to_string()),
            };
        }

        let candidate = BlockCandidate {
            header,
            nonce,
            height: active_template.height,
        };
        let hash = self
            .core
            .hash_candidate_with_algorithm(candidate, algorithm);
        let sealed = self
            .core
            .validate_candidate_with_algorithm(candidate, target, algorithm);
        let accepted = sealed.is_some();

        if let Some(sealed_block) = sealed {
            let template_transactions = active_template.account_transactions();
            let template_utxo_transactions = active_template.utxo_transactions();
            let miner_reward_zion = template_transactions
                .first()
                .filter(|transaction| transaction.from == "coinbase")
                .map(|transaction| transaction.amount_zion)
                .map(|amount| u64::try_from(amount).unwrap_or(active_template.reward_zion))
                .unwrap_or(active_template.reward_zion);
            let accepted_block = AcceptedBlock {
                template_id,
                height: active_template.height,
                timestamp: active_template.header.timestamp,
                difficulty: active_template.difficulty,
                nonce: sealed_block.nonce,
                hash_hex: hex(&sealed_block.hash),
                header_hex: hex(&active_template.header.to_bytes()),
                previous_hash_hex: hex(&active_template.header.previous_hash),
                algorithm: algorithm.to_string(),
                transaction_ids: active_template
                    .account_transactions()
                    .iter()
                    .map(|transaction| transaction.tx_id.clone())
                    .collect(),
                transactions: template_transactions.clone(),
                total_fees_zion: active_template.total_fees_zion,
                body_hash_hex: body_hash_hex(&template_transactions),
                subsidy_zion: active_template.reward_zion,
                miner_reward_zion,
                miner_address: self.miner_address.clone(),
                humanitarian_address: self.humanitarian_address.clone(),
                issobella_address: self.issobella_address.clone(),
                pool_fee_address: self.pool_fee_address.clone(),
                utxo_transaction_ids: template_utxo_transactions
                    .iter()
                    .map(|tx| hex(&tx.id))
                    .collect(),
                utxo_transactions: template_utxo_transactions,
            };
            if let Err(reason) = self.chain_state.accept_block(
                &self.node_id,
                &self.core,
                accepted_block,
                sealed_block,
            ) {
                return RpcResponse::SubmitResult {
                    accepted: false,
                    template_id,
                    block_height: None,
                    hash_hex: String::new(),
                    reason: Some(format!("locally mined block failed validation: {reason}")),
                };
            }
            // Soft-fail rather than panic if the just-accepted block
            // somehow isn't at the back of accepted_blocks (e.g. a future
            // pruning interaction). The block was already accepted; we
            // owe the caller a successful response. (Audit finding F5.)
            match self.chain_state.accepted_blocks.last().cloned() {
                Some(block) => {
                    if let Err(error) =
                        self.persist_chain_update(&ChainJournalEntry::BlockAccepted { block })
                    {
                        eprintln!("node_state_persist_error={error}");
                    }
                }
                None => {
                    eprintln!(
                        "node_state_persist_skipped: locally accepted block missing from \
                         accepted_blocks tail"
                    );
                }
            }
        }

        RpcResponse::SubmitResult {
            accepted,
            template_id,
            block_height: accepted.then_some(self.chain_state.height),
            hash_hex: hex(&hash),
            reason: if accepted {
                None
            } else {
                Some("low difficulty".to_string())
            },
        }
    }

    fn submit_transaction_rpc(&mut self, transaction: Transaction) -> RpcResponse {
        let tx_id = transaction.tx_id.clone();
        match self
            .chain_state
            .insert_transaction(&self.node_id, &self.core, transaction)
        {
            Ok(()) => {
                // Same soft-fail pattern as submit_utxo_transaction_rpc:
                // the tx was accepted into the mempool, so log + skip
                // journaling rather than panic if the index lookup
                // misses. (Audit finding F5.)
                match self.chain_state.mempool_by_id.get(&tx_id).cloned() {
                    Some(transaction) => {
                        if let Err(error) =
                            self.persist_chain_update(&ChainJournalEntry::TransactionAccepted {
                                transaction: transaction.clone(),
                            })
                        {
                            eprintln!("node_state_persist_error={error}");
                        }

                        // Notify WebSocket subscribers about pending transaction
                        if let Some(ws_notifier) = &self.ws_notifier {
                            ws_notifier.notify_pending_transaction(&transaction);
                        }
                    }
                    None => {
                        eprintln!(
                            "node_state_persist_skipped: accepted account transaction {tx_id} \
                             missing from mempool_by_id index"
                        );
                    }
                }
                RpcResponse::TransactionResult {
                    accepted: true,
                    tx_id,
                    reason: None,
                }
            }
            Err(reason) => RpcResponse::TransactionResult {
                accepted: false,
                tx_id,
                reason: Some(reason),
            },
        }
    }
}

impl TemplateState {
    fn account_transactions(&self) -> Vec<Transaction> {
        self.transactions
            .iter()
            .filter_map(|transaction| transaction.as_account().cloned())
            .collect()
    }

    fn utxo_transactions(&self) -> Vec<tx::Transaction> {
        self.transactions
            .iter()
            .filter_map(|transaction| transaction.as_utxo().cloned())
            .collect()
    }

    fn as_public(&self) -> BlockTemplate {
        let account_transactions = self.account_transactions();
        let utxo_transactions = self.utxo_transactions();
        let estimated_miner_reward_zion = account_transactions
            .first()
            .filter(|transaction| transaction.from == "coinbase")
            .map(|transaction| transaction.amount_zion)
            .map(|amount| u64::try_from(amount).unwrap_or(self.reward_zion))
            .unwrap_or(self.reward_zion);
        BlockTemplate {
            template_id: self.template_id,
            height: self.height,
            header_hex: hex(&self.header.to_bytes()),
            target_hex: self.target.to_hex(),
            reward_zion: self.reward_zion,
            transaction_ids: account_transactions
                .iter()
                .map(|transaction| transaction.tx_id.clone())
                .collect(),
            transaction_count: account_transactions.len(),
            total_fees_zion: self.total_fees_zion,
            body_hash_hex: body_hash_hex(&account_transactions),
            estimated_miner_reward_zion,
            utxo_transaction_ids: utxo_transactions.iter().map(|tx| hex(&tx.id)).collect(),
            utxo_transaction_count: utxo_transactions.len(),
            total_utxo_fees: utxo_transactions.iter().map(|tx| tx.fee).sum(),
        }
    }
}

impl Transaction {
    fn validate(&self) -> Result<(), String> {
        if self.tx_id.trim().is_empty() {
            return Err("transaction id must not be empty".to_string());
        }
        if self.tx_id.len() != 64 || !self.tx_id.chars().all(|ch| ch.is_ascii_hexdigit()) {
            return Err("transaction id must be exactly 64 hex chars".to_string());
        }
        if self.from.trim().is_empty() || self.to.trim().is_empty() {
            return Err("transaction endpoints must not be empty".to_string());
        }
        if !is_valid_account_id(&self.from) || !is_valid_account_id(&self.to) {
            return Err("transaction endpoints must use 3-64 ascii wallet ids".to_string());
        }
        if self.from == self.to {
            return Err("transaction sender and recipient must differ".to_string());
        }
        if self.amount_zion == 0 {
            return Err("transaction amount must be greater than zero".to_string());
        }
        if self.fee_zion == 0 {
            return Err("transaction fee must be greater than zero".to_string());
        }
        if (self.fee_zion as u128) > self.amount_zion {
            return Err("transaction fee must not exceed transaction amount".to_string());
        }
        if let Some(ref memo) = self.memo {
            if memo.len() > 256 {
                return Err("transaction memo must not exceed 256 bytes".to_string());
            }
            if !memo.is_ascii() {
                return Err("transaction memo must be ASCII only".to_string());
            }
        }
        Ok(())
    }

    /// Verify Ed25519 signature for non-coinbase transactions.
    /// Coinbase transactions (from == "coinbase") are always valid.
    /// Returns true if the signature is valid or if the transaction is coinbase.
    pub fn verify_signature(&self) -> bool {
        if self.from == "coinbase" {
            return true;
        }
        if self.signature.len() != 128 || self.public_key.len() != 64 {
            return false;
        }
        let pk_bytes = match hex::decode(&self.public_key) {
            Ok(v) if v.len() == 32 => v,
            _ => return false,
        };
        let sig_bytes = match hex::decode(&self.signature) {
            Ok(v) if v.len() == 64 => v,
            _ => return false,
        };
        // CRITICAL: the public key must derive to the sender address. Without
        // this check, any account balance can be spent by signing with an
        // unrelated key.
        let derived_from = crypto::derive_address(&pk_bytes);
        if derived_from != self.from {
            return false;
        }
        crypto::verify(&pk_bytes, self.tx_id.as_bytes(), &sig_bytes)
    }
}

/// A spendable UTXO: output that has not been consumed by any accepted block.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SpendableUtxo {
    pub tx_hash: String,
    pub output_index: u32,
    pub amount: u64,
    pub address: String,
    pub height: u64,
}

impl ChainState {
    fn account_mempool_transactions(&self) -> Vec<Transaction> {
        self.mempool
            .iter()
            .filter_map(|transaction| transaction.as_account().cloned())
            .collect()
    }

    fn utxo_mempool_transactions(&self) -> Vec<tx::Transaction> {
        self.mempool
            .iter()
            .filter_map(|transaction| transaction.as_utxo().cloned())
            .collect()
    }

    /// Returns `true` if F5 balance validation is active at the given height
    /// for this chain state instance.
    fn balance_check_active_at(&self, height: u64) -> bool {
        height >= self.balance_check_height
    }

    /// Returns `true` if the F4.7 max-tx-amount cap is active at the given
    /// height for this chain state instance.
    fn max_tx_amount_active_at(&self, height: u64) -> bool {
        height >= self.max_tx_amount_height
    }

    /// Build the full UTXO set from accepted blocks. Returns a map from
    /// (tx_hash_hex, output_index) → SpendableUtxo for all unspent outputs.
    fn utxo_set(&self) -> HashMap<(String, u32), SpendableUtxo> {
        let mut utxos: HashMap<(String, u32), SpendableUtxo> = HashMap::new();
        for block in &self.accepted_blocks {
            // Consume spent inputs
            for utxo_tx in &block.utxo_transactions {
                for input in &utxo_tx.inputs {
                    utxos.remove(&(hex(&input.prev_tx_hash), input.output_index));
                }
            }
            // Create new outputs
            for utxo_tx in &block.utxo_transactions {
                let tx_hash = hex(&utxo_tx.id);
                for (idx, output) in utxo_tx.outputs.iter().enumerate() {
                    utxos.insert(
                        (tx_hash.clone(), idx as u32),
                        SpendableUtxo {
                            tx_hash: tx_hash.clone(),
                            output_index: idx as u32,
                            amount: output.amount,
                            address: output.address.clone(),
                            height: block.height,
                        },
                    );
                }
            }
        }
        utxos
    }

    /// Compute balance for a `zion1...` address by summing unspent UTXO outputs.
    fn utxo_balance(&self, address: &str) -> u128 {
        self.utxo_set()
            .values()
            .filter(|u| u.address == address)
            .map(|u| u.amount as u128)
            .sum()
    }

    /// Compute the confirmed account-model balance for a `zion1...` address
    /// by walking all accepted blocks and summing credits (to) minus debits
    /// (from + fee). Returns 0 for unknown addresses. This mirrors the RPC
    /// `getBalance` computation and is used by the F5 balance-check guard.
    fn account_balance_for(&self, address: &str) -> u128 {
        let mut balance: i128 = 0;
        for block in &self.accepted_blocks {
            for tx in &block.transactions {
                if tx.from == "coinbase" {
                    if tx.to == address {
                        balance = balance.saturating_add(tx.amount_zion as i128);
                    }
                    continue;
                }
                if tx.to == address {
                    balance = balance.saturating_add(tx.amount_zion as i128);
                }
                if tx.from == address {
                    balance =
                        balance.saturating_sub((tx.amount_zion + tx.fee_zion as u128) as i128);
                }
            }
        }
        // Also include pending mempool debits so a sender cannot double-spend
        // the same balance via two rapid RPC submissions.
        for entry in &self.mempool {
            if let Some(tx) = entry.as_account() {
                if tx.from == address {
                    balance =
                        balance.saturating_sub((tx.amount_zion + tx.fee_zion as u128) as i128);
                }
            }
        }
        balance.max(0) as u128
    }

    /// Return all spendable (unspent) UTXOs for a `zion1...` address.
    fn spendable_utxos(&self, address: &str) -> Vec<SpendableUtxo> {
        self.utxo_set()
            .into_values()
            .filter(|u| u.address == address)
            .collect()
    }

    fn accepted_bridge_unlock_replay_keys(&self) -> HashSet<String> {
        self.accepted_blocks
            .iter()
            .flat_map(|block| block.utxo_transactions.iter())
            .filter_map(bridge_unlock_replay_key_from_transaction)
            .collect()
    }

    fn rebuild_bridge_unlock_replay_keys(&mut self) {
        self.bridge_unlock_replay_keys = self.accepted_bridge_unlock_replay_keys();
        self.bridge_unlock_replay_keys.extend(
            self.mempool
                .iter()
                .filter_map(|transaction| transaction.as_utxo())
                .filter_map(bridge_unlock_replay_key_from_transaction),
        );
    }

    fn validate_bridge_unlock_transaction_shape(
        &self,
        transaction: &tx::Transaction,
    ) -> Result<Option<String>, String> {
        let utxos = self.utxo_set();
        validate_bridge_unlock_transaction_shape_with_utxos(transaction, &utxos)
    }

    /// Check whether a specific outpoint exists as an unspent UTXO on chain.
    fn utxo_exists(&self, tx_hash: &[u8; 32], output_index: u32) -> bool {
        self.utxo_set().contains_key(&(hex(tx_hash), output_index))
    }

    fn new(node_id: &str, core: &CoreRuntime) -> Self {
        let genesis = genesis::genesis_block();
        let genesis_hash = parse_fixed_hex::<32>(&genesis.hash_hex, "genesis hash")
            .expect("genesis hash must be valid hex");
        let mempool = Vec::new();
        let template = Self::build_template(
            node_id,
            core,
            0,
            genesis_hash,
            1,
            &mempool,
            std::slice::from_ref(&genesis),
            "",
            "",
            "",
            "",
        );
        let mut accepted_by_height = BTreeMap::new();
        accepted_by_height.insert(0, genesis.clone());
        let mut state = Self {
            height: 0,
            tip_hash: genesis_hash,
            next_template_id: 2,
            active_template: template,
            accepted_blocks: vec![genesis],
            accepted_by_height,
            accepted_by_template_id: HashMap::new(),
            mempool,
            mempool_by_id: HashMap::new(),
            miner_address: String::new(),
            humanitarian_address: String::new(),
            issobella_address: String::new(),
            pool_fee_address: String::new(),
            bridge_unlock_replay_keys: HashSet::new(),
            address_tx_index: HashMap::new(),
            block_retention: DEFAULT_BLOCK_RETENTION,
            balance_check_height: zion_cosmic_harmony::balance_check_activation_height(),
            max_tx_amount_height: zion_cosmic_harmony::max_tx_amount_activation_height(),
        };
        // Index genesis block (height 0) for address lookups.
        state.index_block_addresses(0);
        state
    }

    fn from_snapshot(
        node_id: &str,
        core: &CoreRuntime,
        snapshot: ChainStateSnapshot,
    ) -> Result<Self, String> {
        let persisted_transaction_ids = snapshot.active_template.transaction_ids.clone();
        let tip_hash = parse_fixed_hex::<32>(&snapshot.tip_hash_hex, "persisted tip hash")?;
        let header = MiningHeader::from_bytes(parse_fixed_hex::<HEADER_SIZE>(
            &snapshot.active_template.header_hex,
            "persisted active template header",
        )?);
        let target = DifficultyTarget::from_hex(&snapshot.active_template.target_hex)?;
        // Recover difficulty from accepted blocks via LWMA for the persisted template.
        let recovered_difficulty = if snapshot.accepted_blocks.is_empty() {
            difficulty::GENESIS_DIFFICULTY
        } else {
            let ab = &snapshot.accepted_blocks;
            let start = ab.len().saturating_sub(difficulty::LWMA_WINDOW + 1);
            let window: Vec<difficulty::BlockInfo> = ab[start..]
                .iter()
                .map(|b| difficulty::BlockInfo {
                    timestamp: b.timestamp,
                    difficulty: b.difficulty,
                })
                .collect();
            difficulty::lwma_next_difficulty(&window)
        };
        let mut chain_state = Self {
            height: snapshot.height,
            tip_hash,
            next_template_id: snapshot.next_template_id,
            active_template: TemplateState {
                template_id: snapshot.active_template.template_id,
                height: snapshot.active_template.height,
                header,
                target,
                difficulty: recovered_difficulty,
                reward_zion: snapshot.active_template.reward_zion,
                transactions: Vec::new(),
                total_fees_zion: snapshot.active_template.total_fees_zion,
            },
            accepted_blocks: {
                // Ensure genesis block (height 0) is always present in memory.
                // It may be missing if the snapshot was saved with block retention
                // pruning enabled. Genesis is needed for premine wallet discovery
                // via getBlockByHeight(0) RPC.
                let mut blocks = snapshot.accepted_blocks;
                if blocks.first().map(|b| b.height) != Some(0) {
                    blocks.insert(0, genesis::genesis_block());
                }
                blocks
            },
            accepted_by_height: BTreeMap::new(),
            accepted_by_template_id: HashMap::new(),
            mempool: snapshot
                .mempool
                .into_iter()
                .map(RuntimeTransaction::from)
                .chain(
                    snapshot
                        .utxo_mempool
                        .into_iter()
                        .map(RuntimeTransaction::from),
                )
                .collect(),
            mempool_by_id: HashMap::new(),
            miner_address: String::new(),
            humanitarian_address: String::new(),
            issobella_address: String::new(),
            pool_fee_address: String::new(),
            bridge_unlock_replay_keys: snapshot.bridge_unlock_replay_keys.into_iter().collect(),
            address_tx_index: HashMap::new(),
            block_retention: DEFAULT_BLOCK_RETENTION,
            balance_check_height: zion_cosmic_harmony::balance_check_activation_height(),
            max_tx_amount_height: zion_cosmic_harmony::max_tx_amount_activation_height(),
        };
        chain_state.rebuild_mempool_index();
        chain_state.rebuild_address_tx_index();
        chain_state.active_template.transactions = persisted_transaction_ids
            .iter()
            .filter_map(|tx_id| chain_state.mempool_by_id.get(tx_id).cloned())
            .collect();
        chain_state.sanitize_recovered_state(node_id, core)?;
        Ok(chain_state)
    }

    fn build_bridge_unlock_transaction(
        &self,
        request: &BridgeUnlockRequest,
        proofs: &[BridgeValidatorProof],
    ) -> Result<tx::Transaction, String> {
        if request.amount_flowers == 0 {
            return Err("bridge unlock amount must be greater than zero".to_string());
        }
        if !crypto::is_valid_address(&request.recipient) {
            return Err("bridge unlock recipient must be a valid zion1 address".to_string());
        }

        // Validate that the proofs sign the canonical operation message
        // *before* persisting anything. The same checks run again in
        // `validate_bridge_unlock_transaction_shape_with_utxos` (peer block /
        // mempool path), so this is defence in depth — and a clearer error
        // surface for the JSON-RPC submitter.
        let operation_message = bridge_operation_message(
            &request.recipient,
            request.amount_flowers,
            &request.source_chain,
            &request.burn_id,
            &request.evm_tx_hash,
        );
        let allowed_pubkeys = load_bridge_validator_pubkey_allowlist();
        let threshold = required_bridge_validator_threshold();
        verify_bridge_proofs(proofs, &operation_message, &allowed_pubkeys, threshold)?;

        let replay_key = bridge_unlock_replay_key(
            &request.source_chain,
            &request.burn_id,
            &request.evm_tx_hash,
        );
        if self.bridge_unlock_replay_keys.contains(&replay_key) {
            return Err(format!(
                "bridge unlock replay key already used: {replay_key}"
            ));
        }

        let mut spendable = self.spendable_utxos(fee::BRIDGE_VAULT_ADDRESS);
        spendable.sort_by(|left, right| {
            left.height
                .cmp(&right.height)
                .then(left.tx_hash.cmp(&right.tx_hash))
                .then(left.output_index.cmp(&right.output_index))
        });

        let mut selected = Vec::new();
        let mut total_input = 0u64;
        let mut required_fee = fee::minimum_fee_for_size(fee::estimate_tx_size(1, 2));
        for utxo in spendable {
            total_input = total_input
                .checked_add(utxo.amount)
                .ok_or_else(|| "bridge unlock input sum overflowed".to_string())?;
            selected.push(utxo);
            required_fee = fee::minimum_fee_for_size(fee::estimate_tx_size(selected.len(), 2));
            let required_total = request
                .amount_flowers
                .checked_add(required_fee)
                .ok_or_else(|| "bridge unlock amount plus fee overflowed".to_string())?;
            if total_input >= required_total {
                break;
            }
        }

        let required_total = request
            .amount_flowers
            .checked_add(required_fee)
            .ok_or_else(|| "bridge unlock amount plus fee overflowed".to_string())?;
        if total_input < required_total {
            return Err(format!(
                "bridge vault balance {} is insufficient for unlock amount {} plus fee {}",
                total_input, request.amount_flowers, required_fee,
            ));
        }

        let mut outputs = vec![tx::TxOutput {
            amount: request.amount_flowers,
            address: request.recipient.clone(),
            memo: Some(bridge_unlock_memo_with_proofs(
                &request.source_chain,
                &request.burn_id,
                &request.evm_tx_hash,
                proofs,
            )),
        }];

        let change = total_input - required_total;
        if change > 0 {
            outputs.push(tx::TxOutput {
                amount: change,
                address: fee::BRIDGE_VAULT_ADDRESS.to_string(),
                memo: None,
            });
        }

        let pending_height = self.height.saturating_add(1);
        let bridge_utxo_ver = if tx_hash_v2_active(pending_height) {
            tx::TX_HASH_V2_VERSION
        } else {
            1
        };
        let mut transaction = tx::Transaction {
            id: [0u8; 32],
            version: bridge_utxo_ver,
            inputs: selected
                .into_iter()
                .map(|utxo| tx::TxInput {
                    prev_tx_hash: parse_fixed_hex::<32>(&utxo.tx_hash, "bridge vault UTXO hash")
                        .expect("spendable_utxos must contain valid tx hashes"),
                    output_index: utxo.output_index,
                    signature: Vec::new(),
                    public_key: Vec::new(),
                })
                .collect(),
            outputs,
            fee: required_fee,
            timestamp: now_secs(),
        };
        transaction.finalize_id();
        Ok(transaction)
    }

    fn accept_block(
        &mut self,
        node_id: &str,
        core: &CoreRuntime,
        accepted_block: AcceptedBlock,
        sealed_block: SealedBlock,
    ) -> Result<(), String> {
        self.validate_peer_block(&accepted_block)?;
        self.accept_block_record(node_id, core, accepted_block, sealed_block.hash);
        Ok(())
    }

    fn accept_block_record(
        &mut self,
        node_id: &str,
        core: &CoreRuntime,
        accepted_block: AcceptedBlock,
        tip_hash: [u8; 32],
    ) {
        self.height = accepted_block.height;
        self.tip_hash = tip_hash;
        let mined_ids: HashSet<&str> = accepted_block
            .transaction_ids
            .iter()
            .chain(accepted_block.utxo_transaction_ids.iter())
            .map(String::as_str)
            .collect();
        self.mempool
            .retain(|transaction| !mined_ids.contains(transaction.tx_id().as_str()));
        self.rebuild_mempool_index();
        self.accepted_by_height
            .insert(accepted_block.height, accepted_block.clone());
        self.accepted_by_template_id
            .insert(accepted_block.template_id, accepted_block.clone());
        self.accepted_blocks.push(accepted_block);
        // Index the newly accepted block by all involved addresses.
        let new_idx = self.accepted_blocks.len() - 1;
        self.index_block_addresses(new_idx);
        self.rebuild_bridge_unlock_replay_keys();
        // Prune old blocks from memory if retention window is set.
        self.prune_old_blocks();
        let next_template_id = self.next_template_id;
        let miner_addr = self.miner_address.clone();
        let humanitarian_addr = self.humanitarian_address.clone();
        let issobella_addr = self.issobella_address.clone();
        let pool_fee_addr = self.pool_fee_address.clone();
        self.active_template = Self::build_template(
            node_id,
            core,
            self.height,
            self.tip_hash,
            next_template_id,
            &self.mempool,
            &self.accepted_blocks,
            &miner_addr,
            &humanitarian_addr,
            &issobella_addr,
            &pool_fee_addr,
        );
        self.next_template_id = self.next_template_id.wrapping_add(1);
    }

    fn apply_journal_entry(
        &mut self,
        node_id: &str,
        core: &CoreRuntime,
        entry: ChainJournalEntry,
    ) -> Result<(), String> {
        match entry {
            ChainJournalEntry::TransactionAccepted { transaction } => {
                if self.mempool_by_id.contains_key(&transaction.tx_id())
                    || self.accepted_blocks.iter().any(|block| {
                        block
                            .transaction_ids
                            .iter()
                            .chain(block.utxo_transaction_ids.iter())
                            .any(|tx_id| tx_id == &transaction.tx_id())
                    })
                {
                    return Ok(());
                }
                match transaction {
                    RuntimeTransaction::Account(transaction) => {
                        self.insert_transaction(node_id, core, transaction)
                    }
                    RuntimeTransaction::Utxo(transaction) => {
                        self.insert_utxo_transaction(node_id, core, transaction)
                    }
                }
            }
            ChainJournalEntry::BlockAccepted { block } => {
                if let Some(existing) = self.accepted_by_template_id.get(&block.template_id) {
                    if existing.hash_hex == block.hash_hex {
                        return Ok(());
                    }
                    return Err(format!(
                        "journal block {} conflicts with existing accepted block",
                        block.template_id
                    ));
                }

                let tip_hash = parse_fixed_hex::<32>(&block.hash_hex, "journal block hash")?;
                self.accept_block_record(node_id, core, block, tip_hash);
                Ok(())
            }
        }
    }

    fn import_peer_block(
        &mut self,
        node_id: &str,
        core: &CoreRuntime,
        block: AcceptedBlock,
    ) -> Result<(), String> {
        // Early-return for duplicate blocks before expensive validation
        // (avoids LWMA difficulty mismatch when seeds re-announce blocks
        // that this node already accepted).
        if let Some(existing) = self.accepted_by_height.get(&block.height) {
            if existing.hash_hex == block.hash_hex {
                return Ok(());
            }
            return Err(format!("conflicting peer block at height {}", block.height));
        }

        self.validate_peer_block(&block)?;

        if block.height != self.height.saturating_add(1) {
            return Err(format!(
                "peer block height {} is not contiguous with local height {}",
                block.height, self.height
            ));
        }

        // Chain linkage: block must reference our current tip as parent.
        let tip_hex = hex(&self.tip_hash);
        if !block.previous_hash_hex.is_empty() {
            if block.previous_hash_hex != tip_hex {
                return Err(format!(
                    "peer block previous_hash {} does not link to local tip {}",
                    block.previous_hash_hex, tip_hex
                ));
            }
        } else if !block.header_hex.is_empty() {
            // Fall back to extracting previous_hash from header.
            let header_bytes = parse_fixed_hex::<HEADER_SIZE>(
                &block.header_hex,
                "peer block header for chain linkage",
            )?;
            let header = MiningHeader::from_bytes(header_bytes);
            if hex(&header.previous_hash) != tip_hex {
                return Err(format!(
                    "peer block header previous_hash does not link to local tip {}",
                    tip_hex
                ));
            }
        }

        let tip_hash = parse_fixed_hex::<32>(&block.hash_hex, "peer block hash")?;
        self.accept_block_record(node_id, core, block, tip_hash);
        Ok(())
    }

    fn import_peer_blocks(
        &mut self,
        node_id: &str,
        core: &CoreRuntime,
        blocks: Vec<AcceptedBlock>,
    ) -> Result<usize, String> {
        if blocks.is_empty() {
            return Ok(0);
        }

        // Skip any leading blocks we already have (e.g. genesis).
        let skip_count = blocks
            .iter()
            .take_while(|block| {
                self.accepted_by_height
                    .get(&block.height)
                    .is_some_and(|existing| existing.hash_hex == block.hash_hex)
            })
            .count();
        let blocks: Vec<AcceptedBlock> = blocks.into_iter().skip(skip_count).collect();
        if blocks.is_empty() {
            return Ok(0);
        }

        // ── Structural pre-checks (no chain-state dependency) ──────────
        let mut expected_height = self.height.saturating_add(1);
        let mut seen_heights = HashSet::new();
        let mut seen_template_ids = HashSet::new();
        let mut expected_parent_hex = hex(&self.tip_hash);
        for block in &blocks {
            if !seen_heights.insert(block.height) {
                return Err(format!(
                    "duplicate peer block height {} in batch",
                    block.height
                ));
            }
            if !seen_template_ids.insert(block.template_id) {
                return Err(format!(
                    "duplicate peer template id {} in batch",
                    block.template_id
                ));
            }
            if let Some(existing) = self.accepted_by_height.get(&block.height) {
                if existing.hash_hex != block.hash_hex {
                    return Err(format!("conflicting peer block at height {}", block.height));
                }
                return Err(format!(
                    "peer batch starts at already imported height {}",
                    block.height
                ));
            }
            if block.height != expected_height {
                return Err(format!(
                    "peer batch is not contiguous: expected height {}, got {}",
                    expected_height, block.height
                ));
            }
            // Chain linkage: every block must reference the previous one.
            let parent_hex = Self::extract_previous_hash_hex(block);
            if let Some(ref parent) = parent_hex {
                if parent != &expected_parent_hex {
                    return Err(format!(
                        "peer batch block at height {} does not link to expected parent {}",
                        block.height, expected_parent_hex
                    ));
                }
            }
            expected_parent_hex = block.hash_hex.clone();
            expected_height = expected_height.saturating_add(1);
        }

        // ── Validate-and-accept one block at a time so that each
        //    subsequent block sees the updated accepted_blocks window
        //    (required for correct LWMA difficulty validation). ─────────
        let mut imported = 0usize;
        for block in blocks {
            self.validate_peer_block(&block)?;
            let tip_hash = parse_fixed_hex::<32>(&block.hash_hex, "peer block hash")?;
            self.accept_block_record(node_id, core, block, tip_hash);
            imported += 1;
        }
        Ok(imported)
    }

    /// Extract previous_hash_hex from a peer block, preferring the explicit
    /// field and falling back to header_hex extraction.  Returns `None` for
    /// legacy blocks that carry neither.
    fn extract_previous_hash_hex(block: &AcceptedBlock) -> Option<String> {
        if !block.previous_hash_hex.is_empty() {
            return Some(block.previous_hash_hex.clone());
        }
        if !block.header_hex.is_empty() {
            if let Ok(bytes) = parse_fixed_hex::<HEADER_SIZE>(&block.header_hex, "header") {
                let header = MiningHeader::from_bytes(bytes);
                return Some(hex(&header.previous_hash));
            }
        }
        None
    }

    fn validate_peer_block(&self, block: &AcceptedBlock) -> Result<(), String> {
        // Genesis block is hard-coded — only verify hash match.
        if block.height == 0 {
            let expected = genesis::genesis_block();
            if block.hash_hex != expected.hash_hex {
                return Err("genesis block hash does not match canonical genesis".to_string());
            }
            return Ok(());
        }

        // ── Checkpoint verification ────────────────────────────────────
        launch::verify_checkpoint(block.height, &block.hash_hex)?;

        // ── PoW verification (when header is available) ────────────────
        let block_hash = parse_fixed_hex::<32>(&block.hash_hex, "peer block hash")?;
        if !block.header_hex.is_empty() {
            let header_bytes =
                parse_fixed_hex::<HEADER_SIZE>(&block.header_hex, "peer block header")?;
            let header = MiningHeader::from_bytes(header_bytes);

            // Header fields must be consistent with block metadata
            if header.timestamp != block.timestamp {
                return Err(
                    "peer block header timestamp does not match block timestamp".to_string()
                );
            }
            let expected_target = difficulty::difficulty_to_target(block.difficulty);
            let expected_bits = difficulty::target_to_compact(&expected_target);
            if header.difficulty_bits != expected_bits {
                return Err(format!(
                    "peer block header difficulty_bits {} does not match expected {}",
                    header.difficulty_bits, expected_bits
                ));
            }

            // Reject inconsistent parent metadata before doing expensive PoW work.
            if !block.previous_hash_hex.is_empty() {
                let header_prev = hex(&header.previous_hash);
                if block.previous_hash_hex != header_prev {
                    return Err(
                        "peer block previous_hash_hex does not match header previous_hash"
                            .to_string(),
                    );
                }
            }

            // Verify PoW: recompute hash from header + nonce
            let candidate = BlockCandidate {
                header,
                nonce: block.nonce,
                height: block.height,
            };
            let computed_hash = candidate.hash();
            if computed_hash != block_hash {
                return Err(
                    "peer block hash does not match PoW computation from header and nonce"
                        .to_string(),
                );
            }

            // Verify hash meets difficulty target
            let target = difficulty::difficulty_to_target(block.difficulty);
            if !target.allows(&computed_hash) {
                return Err("peer block PoW hash does not meet difficulty target".to_string());
            }
        }

        // ── Timestamp sanity ───────────────────────────────────────────
        let current_time = now_secs();
        let median_time_past = if self.accepted_blocks.is_empty() {
            0
        } else {
            let start = self.accepted_blocks.len().saturating_sub(11);
            let mut timestamps: Vec<u64> = self.accepted_blocks[start..]
                .iter()
                .map(|b| b.timestamp)
                .collect();
            timestamps.sort_unstable();
            timestamps[timestamps.len() / 2]
        };
        validation::validate_timestamp(block.timestamp, median_time_past, current_time)
            .map_err(|e| format!("peer block timestamp invalid: {e}"))?;

        // ── Transaction structure ──────────────────────────────────────
        if block.transaction_ids.len() != block.transactions.len() {
            return Err("peer block transaction ids do not match block body length".to_string());
        }
        let expected_ids = block
            .transactions
            .iter()
            .map(|transaction| transaction.tx_id.clone())
            .collect::<Vec<_>>();
        if expected_ids != block.transaction_ids {
            return Err(
                "peer block transaction ids do not match serialized transactions".to_string(),
            );
        }
        let mut seen_tx_ids = HashSet::new();
        let mut seen_sender_nonces = HashSet::new();
        let mut coinbase_count = 0usize;
        let mut total_coinbase_zion = 0u64;
        // Fee split is active when humanitarian + issobella funds are present.
        // The pool-fee 1% slot is burned (never minted), so it has no address
        // and no coinbase output.
        let has_fee_addresses =
            !block.humanitarian_address.is_empty() || !block.issobella_address.is_empty();
        let has_all_fee_addresses =
            !block.humanitarian_address.is_empty() && !block.issobella_address.is_empty();
        if has_fee_addresses && !has_all_fee_addresses {
            return Err("peer block fee split metadata must provide all fee addresses".to_string());
        }
        let (
            expected_miner_reward,
            expected_humanitarian_reward,
            expected_issobella_reward,
            expected_burned_pool_fee,
        ) = emission::fee_split(block.subsidy_zion);
        // Total newly-minted coinbase = subsidy minus the burned pool fee.
        let expected_minted = block.subsidy_zion - expected_burned_pool_fee;
        let total_fees_zion = block
            .transactions
            .iter()
            .enumerate()
            .map(|(index, transaction)| {
                if !seen_tx_ids.insert(transaction.tx_id.clone()) {
                    return Err(format!(
                        "peer block contains duplicate transaction id {}",
                        transaction.tx_id
                    ));
                }
                if transaction.from == "coinbase" {
                    coinbase_count = coinbase_count.saturating_add(1);
                    let coinbase_amt = u64::try_from(transaction.amount_zion).map_err(|_| {
                        "peer block coinbase amount exceeds u64".to_string()
                    })?;
                    total_coinbase_zion = total_coinbase_zion.saturating_add(coinbase_amt);
                    if transaction.tx_id.len() != 64
                        || !transaction.tx_id.chars().all(|ch| ch.is_ascii_hexdigit())
                    {
                        return Err("peer block coinbase transaction id must be exactly 64 hex chars"
                            .to_string());
                    }
                    if transaction.to.trim().is_empty() {
                        return Err(
                            "peer block coinbase recipient must not be empty".to_string(),
                        );
                    }
                    if !is_valid_account_id(&transaction.to) {
                        return Err(
                            "peer block coinbase recipient must use a 3-64 ascii wallet id"
                                .to_string(),
                        );
                    }
                    if index != coinbase_count.saturating_sub(1) {
                        return Err(
                            "peer block coinbase transactions must be contiguous at the start"
                                .to_string(),
                        );
                    }
                    if transaction.fee_zion != 0 {
                        return Err("peer block coinbase transaction must have zero fee".to_string());
                    }
                    if transaction.nonce != block.height {
                        return Err(format!(
                            "peer block coinbase nonce {} does not match block height {}",
                            transaction.nonce, block.height
                        ));
                    }
                    if block.miner_address.is_empty() {
                        return Err(
                            "peer block coinbase transaction requires miner_address metadata"
                                .to_string(),
                        );
                    }
                    let (expected_to, expected_amount, expected_label) = if has_all_fee_addresses {
                        match index {
                            0 => (
                                block.miner_address.as_str(),
                                expected_miner_reward,
                                format!("coinbase:{}:{}", block.height, block.miner_address),
                            ),
                            1 => (
                                block.humanitarian_address.as_str(),
                                expected_humanitarian_reward,
                                format!(
                                    "coinbase_humanitarian:{}:{}",
                                    block.height, block.humanitarian_address
                                ),
                            ),
                            2 => (
                                block.issobella_address.as_str(),
                                expected_issobella_reward,
                                format!(
                                    "coinbase_issobella:{}:{}",
                                    block.height, block.issobella_address
                                ),
                            ),
                            _ => {
                                return Err(
                                    "peer block contains too many split coinbase transactions"
                                        .to_string(),
                                )
                            }
                        }
                    } else {
                        (
                            block.miner_address.as_str(),
                            block.subsidy_zion,
                            format!("coinbase:{}:{}", block.height, block.miner_address),
                        )
                    };
                    if transaction.to != expected_to {
                        return Err(
                            "peer block coinbase recipient does not match expected payout address"
                                .to_string(),
                        );
                    }
                    if transaction.amount_zion != u128::from(expected_amount) {
                        return Err(format!(
                            "peer block coinbase amount {} does not match expected {}",
                            transaction.amount_zion, expected_amount
                        ));
                    }
                    let expected_coinbase_hash =
                        cosmic_harmony_ekam_deeksha(expected_label.as_bytes(), block.height);
                    let expected_coinbase_id = hex(&expected_coinbase_hash.data);
                    if transaction.tx_id != expected_coinbase_id {
                        return Err(
                            "peer block coinbase tx_id is not deterministic for the expected payout slot"
                                .to_string(),
                        );
                    }
                } else {
                    transaction.validate()?;
                    // Height-gate the from-address signature verification: only
                    // enforce for blocks at or after the account-model memo v1
                    // hard fork. Historical blocks (pre-activation) may contain
                    // account TXs where the public key does not derive to the
                    // sender address, because that check was not enforced when
                    // they were created.
                    if account_tx_memo_v1_active(block.height)
                        && !transaction.verify_signature()
                    {
                        return Err("account transaction signature verification failed".to_string());
                    }
                    if !seen_sender_nonces.insert((transaction.from.clone(), transaction.nonce)) {
                        return Err(format!(
                            "peer block reuses sender nonce {} for {}",
                            transaction.nonce, transaction.from
                        ));
                    }
                    // Cross-block replay guard: reject an account nonce that was
                    // already mined in a prior accepted block. Mirrors the RPC
                    // `insert_transaction` "already mined" check so the peer-block
                    // path is not weaker than the RPC path (F1-class parity).
                    // Blocks are accepted one at a time in `accept_peer_blocks`,
                    // so `self.accepted_blocks` reflects every prior block in the
                    // same sync batch as well as the persisted chain.
                    if self.accepted_blocks.iter().any(|prior| {
                        prior.transactions.iter().any(|known| {
                            known.from == transaction.from
                                && known.nonce == transaction.nonce
                        })
                    }) {
                        return Err(format!(
                            "peer block reuses already-mined sender nonce {} for {}",
                            transaction.nonce, transaction.from
                        ));
                    }
                    // F4.7: Max-tx-amount sanity cap. No single non-genesis,
                    // non-coinbase TX may move more than the entire money supply
                    // (`emission::TOTAL_SUPPLY`). Defense-in-depth on top of the
                    // F5 balance check: bounds damage from any inflation bug that
                    // fabricates an absurd amount. Height-gated so historical
                    // blocks are never retroactively rejected; genesis (height 0)
                    // is below any activation height and also guarded explicitly.
                    if self.max_tx_amount_active_at(block.height)
                        && transaction.from != "genesis"
                        && transaction.from != "coinbase"
                        && transaction.amount_zion > emission::TOTAL_SUPPLY
                    {
                        return Err(format!(
                            "peer block TX from {} exceeds max allowed amount: {} > TOTAL_SUPPLY {}",
                            transaction.from, transaction.amount_zion, emission::TOTAL_SUPPLY
                        ));
                    }
                    // F5: Balance check — reject if sender has insufficient
                    // confirmed balance. We compute the running balance from
                    // prior accepted blocks plus credits/debits from earlier
                    // transactions in THIS block (so multiple TXs from the
                    // same sender in one block are handled correctly).
                    if self.balance_check_active_at(block.height) {
                        let mut sender_balance: i128 =
                            self.account_balance_for(&transaction.from) as i128;
                        // Apply credits/debits from earlier TXs in this block
                        for prior_tx in block.transactions.iter().take(index) {
                            if prior_tx.from == "coinbase" {
                                if prior_tx.to == transaction.from {
                                    sender_balance =
                                        sender_balance.saturating_add(prior_tx.amount_zion as i128);
                                }
                                continue;
                            }
                            if prior_tx.to == transaction.from {
                                sender_balance =
                                    sender_balance.saturating_add(prior_tx.amount_zion as i128);
                            }
                            if prior_tx.from == transaction.from {
                                sender_balance = sender_balance.saturating_sub(
                                    (prior_tx.amount_zion + prior_tx.fee_zion as u128) as i128,
                                );
                            }
                        }
                        let needed =
                            transaction.amount_zion + transaction.fee_zion as u128;
                        if (sender_balance as u128) < needed {
                            return Err(format!(
                                "peer block TX from {} has insufficient balance: {} < {} (amount {} + fee {})",
                                transaction.from, sender_balance.max(0), needed,
                                transaction.amount_zion, transaction.fee_zion
                            ));
                        }
                    }
                }
                Ok(transaction.fee_zion)
            })
            .collect::<Result<Vec<_>, String>>()?
            .into_iter()
            .sum::<u64>();
        if coinbase_count > 3 {
            return Err("peer block contains more than three coinbase transactions".to_string());
        }
        if !block.miner_address.is_empty() && coinbase_count == 0 {
            return Err(
                "peer block miner_address is set but coinbase transaction is missing".to_string(),
            );
        }
        if has_all_fee_addresses && coinbase_count != 3 {
            return Err(
                "peer block with fee split metadata must contain three coinbase transactions \
                 (miner/humanitarian/issobella; the 1% pool fee is burned)"
                    .to_string(),
            );
        }
        if !has_all_fee_addresses && coinbase_count > 1 {
            return Err(
                "peer block without fee split metadata must contain at most one coinbase transaction"
                    .to_string(),
            );
        }
        // With fee split, the coinbase mints 99% (89/5/5) and burns the 1% pool
        // fee; without it, the single coinbase mints the full subsidy.
        let expected_coinbase_total = if has_all_fee_addresses {
            expected_minted
        } else {
            block.subsidy_zion
        };
        if total_coinbase_zion != 0 && total_coinbase_zion != expected_coinbase_total {
            return Err(format!(
                "peer block coinbase total {} does not match expected {}",
                total_coinbase_zion, expected_coinbase_total
            ));
        }
        if total_fees_zion != block.total_fees_zion {
            return Err("peer block fee total does not match serialized transactions".to_string());
        }
        if block.body_hash_hex != body_hash_hex(&block.transactions) {
            return Err("peer block body hash does not match serialized transactions".to_string());
        }
        let expected_block_miner_reward = if has_all_fee_addresses && coinbase_count == 3 {
            expected_miner_reward
        } else {
            block.subsidy_zion
        };
        if block.miner_reward_zion != expected_block_miner_reward {
            return Err(format!(
                "peer block miner reward {} does not match expected {}",
                block.miner_reward_zion, expected_block_miner_reward
            ));
        }
        let expected_subsidy = emission::block_subsidy(block.height);
        if block.subsidy_zion != expected_subsidy {
            return Err(format!(
                "peer block subsidy {} does not match emission schedule {} at height {}",
                block.subsidy_zion, expected_subsidy, block.height
            ));
        }
        // Validate difficulty against LWMA
        let expected_difficulty = if self.accepted_blocks.is_empty() {
            difficulty::GENESIS_DIFFICULTY
        } else {
            let start = self
                .accepted_blocks
                .len()
                .saturating_sub(difficulty::LWMA_WINDOW + 1);
            let window: Vec<difficulty::BlockInfo> = self.accepted_blocks[start..]
                .iter()
                .map(|b| difficulty::BlockInfo {
                    timestamp: b.timestamp,
                    difficulty: b.difficulty,
                })
                .collect();
            difficulty::lwma_next_difficulty(&window)
        };
        if block.difficulty != expected_difficulty {
            return Err(format!(
                "peer block difficulty {} does not match expected {} at height {}",
                block.difficulty, expected_difficulty, block.height
            ));
        }
        // ── UTXO transaction structure ─────────────────────────────────
        let utxo_expected_ids: Vec<String> = block
            .utxo_transactions
            .iter()
            .map(|utxo_tx| hex(&utxo_tx.id))
            .collect();
        if utxo_expected_ids != block.utxo_transaction_ids {
            return Err(
                "peer block UTXO transaction ids do not match serialized UTXO transactions"
                    .to_string(),
            );
        }
        let mut seen_utxo_inputs: HashSet<([u8; 32], u32)> = HashSet::new();
        let mut seen_bridge_unlock_replay_keys = self.accepted_bridge_unlock_replay_keys();
        for utxo_tx in &block.utxo_transactions {
            if utxo_tx.id != utxo_tx.calculate_hash() {
                return Err(format!(
                    "peer block UTXO transaction {} has invalid id",
                    hex(&utxo_tx.id)
                ));
            }
            match self.validate_bridge_unlock_transaction_shape(utxo_tx)? {
                Some(replay_key) => {
                    if !seen_bridge_unlock_replay_keys.insert(replay_key.clone()) {
                        return Err(format!(
                            "peer block bridge unlock replay key already used: {}",
                            replay_key,
                        ));
                    }
                }
                None => {
                    if !utxo_tx.verify_signatures() {
                        return Err(format!(
                            "peer block UTXO transaction {} has invalid signatures",
                            hex(&utxo_tx.id)
                        ));
                    }
                }
            }
            let utxo_id_hex = hex(&utxo_tx.id);
            if !seen_tx_ids.insert(utxo_id_hex) {
                return Err(format!(
                    "peer block contains duplicate UTXO transaction id {}",
                    hex(&utxo_tx.id)
                ));
            }
            for input in &utxo_tx.inputs {
                if !seen_utxo_inputs.insert((input.prev_tx_hash, input.output_index)) {
                    return Err(format!(
                        "peer block contains double-spend of UTXO input {}:{}",
                        hex(&input.prev_tx_hash),
                        input.output_index,
                    ));
                }
            }
        }

        // ── UTXO input existence + value conservation (F1) ─────────────────
        //
        // These checks must apply both to peer-imported blocks and to locally
        // mined candidates accepted via SubmitCandidate, otherwise a miner
        // could accidentally (or maliciously) mint value by submitting a block
        // whose UTXO tx outputs+fee exceed its referenced inputs.
        let utxos = self.utxo_set();
        let coinbase_outpoints: HashSet<(String, u32)> = self
            .accepted_blocks
            .iter()
            .flat_map(|b| {
                b.utxo_transactions
                    .iter()
                    .filter(|tx| tx.is_coinbase())
                    .map(move |tx| (b.height, tx))
            })
            .flat_map(|(height, tx)| {
                let id_hex = hex(&tx.id);
                tx.outputs
                    .iter()
                    .enumerate()
                    .map(move |(idx, _)| (id_hex.clone(), idx as u32, height))
            })
            .map(|(id_hex, idx, _height)| (id_hex, idx))
            .collect();

        let utxo_lookup = |tx_hash: &[u8; 32], output_index: u32| -> Option<validation::UtxoInfo> {
            let key = (hex(tx_hash), output_index);
            utxos.get(&key).map(|u| validation::UtxoInfo {
                amount: u.amount,
                address: u.address.clone(),
                created_height: u.height,
                is_coinbase: coinbase_outpoints.contains(&key),
            })
        };
        let is_bridge_unlock =
            |tx: &tx::Transaction| bridge_unlock_replay_key_from_transaction(tx).is_some();

        validation::validate_inputs_exist(
            &block.utxo_transactions,
            &utxo_lookup,
            &is_bridge_unlock,
        )
        .map_err(|err| format!("peer block UTXO input existence failed: {err}"))?;
        validation::validate_value_conservation(
            &block.utxo_transactions,
            &utxo_lookup,
            &is_bridge_unlock,
        )
        .map_err(|err| format!("peer block UTXO value conservation failed: {err}"))?;
        Ok(())
    }

    fn insert_transaction(
        &mut self,
        node_id: &str,
        core: &CoreRuntime,
        transaction: Transaction,
    ) -> Result<(), String> {
        transaction.validate()?;
        if !transaction.verify_signature() {
            return Err("account transaction signature verification failed".to_string());
        }
        if self.mempool.len() >= MAX_MEMPOOL_TRANSACTIONS {
            return Err(format!(
                "mempool capacity reached: {MAX_MEMPOOL_TRANSACTIONS}"
            ));
        }
        if self.mempool_by_id.contains_key(&transaction.tx_id) {
            return Err(format!("duplicate transaction id: {}", transaction.tx_id));
        }
        if self.accepted_blocks.iter().any(|block| {
            block
                .transaction_ids
                .iter()
                .any(|tx_id| tx_id == &transaction.tx_id)
        }) {
            return Err(format!("transaction {} already mined", transaction.tx_id));
        }
        if self
            .mempool
            .iter()
            .filter_map(RuntimeTransaction::as_account)
            .any(|known| known.from == transaction.from && known.nonce == transaction.nonce)
        {
            return Err(format!(
                "transaction nonce {} for sender {} is already pending",
                transaction.nonce, transaction.from
            ));
        }
        if self.accepted_blocks.iter().any(|block| {
            block
                .transactions
                .iter()
                .any(|known| known.from == transaction.from && known.nonce == transaction.nonce)
        }) {
            return Err(format!(
                "transaction nonce {} for sender {} is already mined",
                transaction.nonce, transaction.from
            ));
        }
        // F4.7: Max-tx-amount sanity cap. No single non-genesis, non-coinbase
        // TX may move more than the entire money supply (`emission::TOTAL_SUPPLY`).
        // Defense-in-depth on top of F5: bounds damage from any inflation bug.
        // Height-gated; genesis/coinbase are guarded explicitly.
        if self.max_tx_amount_active_at(self.height)
            && transaction.from != "genesis"
            && transaction.from != "coinbase"
            && transaction.amount_zion > emission::TOTAL_SUPPLY
        {
            return Err(format!(
                "transaction from {} exceeds max allowed amount: {} > TOTAL_SUPPLY {}",
                transaction.from,
                transaction.amount_zion,
                emission::TOTAL_SUPPLY
            ));
        }
        // F5: Reject transactions where the sender does not have sufficient
        // confirmed account-model balance to cover amount + fee. Without this
        // check, any Ed25519 key holder can create ZION from nothing by
        // submitting a TX from an empty address. Height-gated so historical
        // blocks (pre-fix) are not rejected on IBD.
        if self.balance_check_active_at(self.height) {
            let sender_balance = self.account_balance_for(&transaction.from);
            let needed = transaction.amount_zion + transaction.fee_zion as u128;
            if sender_balance < needed {
                return Err(format!(
                    "insufficient balance: sender {} has {} flowers but needs {} (amount {} + fee {})",
                    transaction.from, sender_balance, needed,
                    transaction.amount_zion, transaction.fee_zion
                ));
            }
        }

        self.mempool
            .push(RuntimeTransaction::from(transaction.clone()));
        self.mempool_by_id.insert(
            transaction.tx_id.clone(),
            RuntimeTransaction::from(transaction.clone()),
        );
        let miner_addr = self.miner_address.clone();
        let humanitarian_addr = self.humanitarian_address.clone();
        let issobella_addr = self.issobella_address.clone();
        let pool_fee_addr = self.pool_fee_address.clone();
        self.active_template = Self::build_template(
            node_id,
            core,
            self.height,
            self.tip_hash,
            self.active_template.template_id,
            &self.mempool,
            &self.accepted_blocks,
            &miner_addr,
            &humanitarian_addr,
            &issobella_addr,
            &pool_fee_addr,
        );
        Ok(())
    }

    fn insert_utxo_transaction(
        &mut self,
        node_id: &str,
        core: &CoreRuntime,
        transaction: tx::Transaction,
    ) -> Result<(), String> {
        let pending_height = self.height.saturating_add(1);
        if tx_hash_v2_active(pending_height) && transaction.version < tx::TX_HASH_V2_VERSION {
            return Err(format!(
                "UTXO mempool rejects tx.version {} — pending block height {} requires tx.version >= {} (TX_HASH_V2 activation {})",
                transaction.version,
                pending_height,
                tx::TX_HASH_V2_VERSION,
                TX_HASH_V2_ACTIVATION_HEIGHT
            ));
        }
        if transaction.id != transaction.calculate_hash() {
            return Err("UTXO transaction id does not match calculated hash".to_string());
        }
        let bridge_unlock_replay_key =
            match self.validate_bridge_unlock_transaction_shape(&transaction)? {
                Some(replay_key) => Some(replay_key),
                None => {
                    if !transaction.verify_signatures() {
                        return Err("UTXO transaction signature verification failed".to_string());
                    }
                    None
                }
            };
        if self.mempool.len() >= MAX_MEMPOOL_TRANSACTIONS {
            return Err(format!(
                "mempool capacity reached: {MAX_MEMPOOL_TRANSACTIONS}"
            ));
        }
        let tx_id = hex(&transaction.id);
        if self.mempool_by_id.contains_key(&tx_id) {
            return Err(format!("duplicate transaction id: {tx_id}"));
        }
        if let Some(replay_key) = &bridge_unlock_replay_key {
            if self.bridge_unlock_replay_keys.contains(replay_key) {
                return Err(format!(
                    "bridge unlock replay key already used: {replay_key}"
                ));
            }
        }
        if self
            .accepted_blocks
            .iter()
            .any(|block| block.utxo_transaction_ids.iter().any(|id| id == &tx_id))
        {
            return Err(format!("UTXO transaction {} already mined", tx_id));
        }
        for input in &transaction.inputs {
            // Verify the referenced UTXO output actually exists on chain and
            // has not already been spent.
            if !self.utxo_exists(&input.prev_tx_hash, input.output_index) {
                return Err(format!(
                    "UTXO input {}:{} does not exist or is already spent",
                    hex(&input.prev_tx_hash),
                    input.output_index,
                ));
            }
            let already_in_mempool = self.mempool.iter().any(|known| {
                known.as_utxo().is_some_and(|utxo| {
                    utxo.inputs.iter().any(|ki| {
                        ki.prev_tx_hash == input.prev_tx_hash
                            && ki.output_index == input.output_index
                    })
                })
            });
            if already_in_mempool {
                return Err(format!(
                    "UTXO input {}:{} is already being spent in mempool",
                    hex(&input.prev_tx_hash),
                    input.output_index,
                ));
            }
        }
        if let Some(replay_key) = bridge_unlock_replay_key {
            self.bridge_unlock_replay_keys.insert(replay_key);
        }
        self.mempool
            .push(RuntimeTransaction::Utxo(transaction.clone()));
        self.mempool_by_id
            .insert(tx_id, RuntimeTransaction::Utxo(transaction));
        let miner_addr = self.miner_address.clone();
        let humanitarian_addr = self.humanitarian_address.clone();
        let issobella_addr = self.issobella_address.clone();
        let pool_fee_addr = self.pool_fee_address.clone();
        self.active_template = Self::build_template(
            node_id,
            core,
            self.height,
            self.tip_hash,
            self.active_template.template_id,
            &self.mempool,
            &self.accepted_blocks,
            &miner_addr,
            &humanitarian_addr,
            &issobella_addr,
            &pool_fee_addr,
        );
        Ok(())
    }

    fn rebuild_indexes(&mut self) {
        self.accepted_by_height.clear();
        self.accepted_by_template_id.clear();
        for block in &self.accepted_blocks {
            self.accepted_by_height.insert(block.height, block.clone());
            self.accepted_by_template_id
                .insert(block.template_id, block.clone());
        }
    }

    /// Prune old blocks from in-memory caches if `block_retention > 0`.
    ///
    /// Removes the oldest block from `accepted_blocks`, `accepted_by_height`,
    /// and `accepted_by_template_id`. Adjusts `address_tx_index` by removing
    /// the pruned index and decrementing all higher indices.
    ///
    /// The genesis block (height 0) is NEVER pruned — it's needed for
    /// premine wallet discovery via `getBlockByHeight(0)` RPC.
    ///
    /// Blocks remain in LMDB persistent storage — this only affects in-memory
    /// caches for RPC queries and consensus validation of recent blocks.
    fn prune_old_blocks(&mut self) {
        if self.block_retention == 0 {
            return;
        }
        // Genesis (height 0) is always at index 0 and must never be pruned.
        // We prune blocks starting from index 1 (the oldest non-genesis block).
        while self.accepted_blocks.len() > self.block_retention {
            // Determine which index to prune: skip genesis if it's at index 0
            let genesis_at_start = self.accepted_blocks.first().map(|b| b.height) == Some(0);
            let prune_idx = if genesis_at_start { 1 } else { 0 };
            if prune_idx >= self.accepted_blocks.len() {
                break;
            }
            let removed = self.accepted_blocks.remove(prune_idx);

            // Remove from height and template_id indexes
            self.accepted_by_height.remove(&removed.height);
            self.accepted_by_template_id.remove(&removed.template_id);

            // Adjust address_tx_index: remove the pruned index from all entries,
            // then decrement all indices greater than it.
            let mut empty_keys = Vec::new();
            for (addr, indices) in self.address_tx_index.iter_mut() {
                // Remove the pruned block index if present
                indices.retain(|&idx| idx != prune_idx);
                // Decrement all indices greater than prune_idx by 1
                for idx in indices.iter_mut() {
                    if *idx > prune_idx {
                        *idx -= 1;
                    }
                }
                if indices.is_empty() {
                    empty_keys.push(addr.clone());
                }
            }
            for key in empty_keys {
                self.address_tx_index.remove(&key);
            }
        }
    }

    /// Rebuild the address→block-index map from scratch by scanning all
    /// accepted blocks. Called on startup after loading from disk.
    fn rebuild_address_tx_index(&mut self) {
        self.address_tx_index.clear();
        for idx in 0..self.accepted_blocks.len() {
            self.index_block_addresses(idx);
        }
    }

    /// Index a single block by all addresses involved in its transactions.
    /// For each address that appears as a sender, recipient, or miner in the
    /// block, add this block's index to the address's lookup vector.
    fn index_block_addresses(&mut self, block_idx: usize) {
        let block = match self.accepted_blocks.get(block_idx) {
            Some(b) => b,
            None => return,
        };
        let mut addresses = std::collections::HashSet::new();

        // Account-model transactions (from/to)
        for tx in &block.transactions {
            if !tx.from.is_empty() {
                addresses.insert(tx.from.clone());
            }
            if !tx.to.is_empty() {
                addresses.insert(tx.to.clone());
            }
        }

        // UTXO transactions (inputs/outputs)
        for utxo_tx in &block.utxo_transactions {
            for output in &utxo_tx.outputs {
                if !output.address.is_empty() {
                    addresses.insert(output.address.clone());
                }
            }
            for input in &utxo_tx.inputs {
                let addr = crate::crypto::derive_address(&input.public_key);
                if !addr.is_empty() {
                    addresses.insert(addr);
                }
            }
        }

        // Coinbase addresses (miner, humanitarian, issobella)
        if !block.miner_address.is_empty() {
            addresses.insert(block.miner_address.clone());
        }
        if !block.humanitarian_address.is_empty() {
            addresses.insert(block.humanitarian_address.clone());
        }
        if !block.issobella_address.is_empty() {
            addresses.insert(block.issobella_address.clone());
        }

        for addr in addresses {
            self.address_tx_index
                .entry(addr)
                .or_default()
                .push(block_idx);
        }
    }

    /// Get the indices of blocks that contain transactions for the given address.
    /// Returns `None` if the address has no transactions (not in index).
    fn block_indices_for_address(&self, address: &str) -> Option<&Vec<usize>> {
        self.address_tx_index.get(address)
    }

    fn rebuild_mempool_index(&mut self) {
        self.mempool_by_id.clear();
        for transaction in &self.mempool {
            self.mempool_by_id
                .insert(transaction.tx_id(), transaction.clone());
        }
    }

    fn sanitize_recovered_state(
        &mut self,
        node_id: &str,
        core: &CoreRuntime,
    ) -> Result<(), String> {
        self.rebuild_indexes();

        let mined_ids: HashSet<&str> = self
            .accepted_blocks
            .iter()
            .flat_map(|block| {
                block
                    .transaction_ids
                    .iter()
                    .chain(block.utxo_transaction_ids.iter())
                    .map(String::as_str)
            })
            .collect();
        let mut sender_nonces = HashSet::new();
        let mut seen = HashSet::new();
        let mut seen_utxo_inputs: HashSet<([u8; 32], u32)> = HashSet::new();
        let mut seen_bridge_unlock_replay_keys = self.accepted_bridge_unlock_replay_keys();
        let utxos = self.utxo_set();
        self.mempool.retain(|transaction| match transaction {
            RuntimeTransaction::Account(tx) => {
                tx.validate().is_ok()
                    && !mined_ids.contains(tx.tx_id.as_str())
                    && seen.insert(tx.tx_id.clone())
                    && sender_nonces.insert((tx.from.clone(), tx.nonce))
                    && !self.accepted_blocks.iter().any(|block| {
                        block
                            .transactions
                            .iter()
                            .any(|known| known.from == tx.from && known.nonce == tx.nonce)
                    })
            }
            RuntimeTransaction::Utxo(utxo) => {
                let id_hex = hex(&utxo.id);
                if utxo.id != utxo.calculate_hash()
                    || mined_ids.contains(id_hex.as_str())
                    || !seen.insert(id_hex)
                    || !utxo.inputs.iter().all(|input| {
                        seen_utxo_inputs.insert((input.prev_tx_hash, input.output_index))
                    })
                {
                    return false;
                }

                match validate_bridge_unlock_transaction_shape_with_utxos(utxo, &utxos) {
                    Ok(Some(replay_key)) => seen_bridge_unlock_replay_keys.insert(replay_key),
                    Ok(None) => utxo.verify_signatures(),
                    Err(_) => false,
                }
            }
        });
        self.rebuild_mempool_index();
        self.bridge_unlock_replay_keys = seen_bridge_unlock_replay_keys;

        let mut template_transactions = Vec::new();
        for tx_id in &self.active_template.as_public().transaction_ids {
            let Some(transaction) = self
                .mempool_by_id
                .get(tx_id)
                .cloned()
                .and_then(RuntimeTransaction::into_account)
            else {
                let miner_addr = self.miner_address.clone();
                let humanitarian_addr = self.humanitarian_address.clone();
                let issobella_addr = self.issobella_address.clone();
                let pool_fee_addr = self.pool_fee_address.clone();
                self.active_template = Self::build_template(
                    node_id,
                    core,
                    self.height,
                    self.tip_hash,
                    self.next_template_id.saturating_sub(1),
                    &self.mempool,
                    &self.accepted_blocks,
                    &miner_addr,
                    &humanitarian_addr,
                    &issobella_addr,
                    &pool_fee_addr,
                );
                return Ok(());
            };
            template_transactions.push(RuntimeTransaction::from(transaction));
        }

        self.active_template.transactions = template_transactions;
        self.active_template.total_fees_zion = self
            .active_template
            .transactions
            .iter()
            .filter_map(|transaction| {
                transaction
                    .as_account()
                    .map(|transaction| transaction.fee_zion)
            })
            .sum();

        if self.active_template.height != self.height.saturating_add(1) {
            let miner_addr = self.miner_address.clone();
            let humanitarian_addr = self.humanitarian_address.clone();
            let issobella_addr = self.issobella_address.clone();
            let pool_fee_addr = self.pool_fee_address.clone();
            self.active_template = Self::build_template(
                node_id,
                core,
                self.height,
                self.tip_hash,
                self.next_template_id.saturating_sub(1),
                &self.mempool,
                &self.accepted_blocks,
                &miner_addr,
                &humanitarian_addr,
                &issobella_addr,
                &pool_fee_addr,
            );
        }

        Ok(())
    }

    fn snapshot(&self) -> ChainStateSnapshot {
        ChainStateSnapshot {
            height: self.height,
            tip_hash_hex: hex(&self.tip_hash),
            next_template_id: self.next_template_id,
            active_template: self.active_template.as_public(),
            accepted_blocks: self.accepted_blocks.clone(),
            mempool: self.account_mempool_transactions(),
            utxo_mempool: self.utxo_mempool_transactions(),
            bridge_unlock_replay_keys: self.bridge_unlock_replay_keys.iter().cloned().collect(),
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn build_template(
        node_id: &str,
        _core: &CoreRuntime,
        current_height: u64,
        previous_hash: [u8; 32],
        template_id: u64,
        mempool: &[RuntimeTransaction],
        accepted_blocks: &[AcceptedBlock],
        miner_address: &str,
        humanitarian_address: &str,
        issobella_address: &str,
        // The pool-fee 1% slot is burned (never minted), so no address is used.
        _pool_fee_address: &str,
    ) -> TemplateState {
        let next_height = current_height.saturating_add(1);
        let mut selected_transactions = select_template_transactions(mempool);
        let total_fees_zion: u64 = selected_transactions
            .iter()
            .map(|transaction| transaction.fee_zion)
            .sum();

        let selected_utxo_transactions = select_template_utxo_transactions(mempool);

        // Phase 14: Generate coinbase transaction(s) when miner_address is configured.
        if !miner_address.is_empty() {
            let subsidy = emission::block_subsidy(next_height);
            // Fee split is active when the humanitarian + issobella funds are
            // configured. The pool-fee 1% slot is BURNED (never minted), so it
            // requires no address and produces no coinbase output.
            let has_fee_addresses =
                !humanitarian_address.is_empty() && !issobella_address.is_empty();

            if has_fee_addresses {
                // Multi-output coinbase: mint 89/5/5 (miner/humanitarian/issobella).
                // The remaining 1% (pool_fee) is burned — no output is created.
                let (miner_amt, humanitarian_amt, issobella_amt, _burned_pool_fee) =
                    emission::fee_split(subsidy);

                let mk_coinbase = |label_prefix: &str, addr: &str, amount: u64| {
                    let label = format!("{}:{}:{}", label_prefix, next_height, addr);
                    let hash = cosmic_harmony_ekam_deeksha(label.as_bytes(), next_height);
                    Transaction {
                        tx_id: hex(&hash.data),
                        from: "coinbase".to_string(),
                        to: addr.to_string(),
                        amount_zion: u128::from(amount),
                        fee_zion: 0,
                        nonce: next_height,
                        signature: String::new(),
                        public_key: String::new(),
                        memo: None,
                    }
                };

                // Insert in reverse order so positions are: 0=miner, 1=humanitarian, 2=issobella
                selected_transactions.insert(
                    0,
                    mk_coinbase("coinbase_issobella", issobella_address, issobella_amt),
                );
                selected_transactions.insert(
                    0,
                    mk_coinbase(
                        "coinbase_humanitarian",
                        humanitarian_address,
                        humanitarian_amt,
                    ),
                );
                selected_transactions.insert(0, mk_coinbase("coinbase", miner_address, miner_amt));
            } else {
                // Legacy single coinbase: 100% to miner
                let coinbase_label = format!("coinbase:{}:{}", next_height, miner_address);
                let coinbase_hash =
                    cosmic_harmony_ekam_deeksha(coinbase_label.as_bytes(), next_height);
                let coinbase_tx = Transaction {
                    tx_id: hex(&coinbase_hash.data),
                    from: "coinbase".to_string(),
                    to: miner_address.to_string(),
                    amount_zion: u128::from(subsidy),
                    fee_zion: 0,
                    nonce: next_height,
                    signature: String::new(),
                    public_key: String::new(),
                    memo: None,
                };
                selected_transactions.insert(0, coinbase_tx);
            }
        }

        let mut transactions: Vec<RuntimeTransaction> = selected_transactions
            .iter()
            .cloned()
            .map(RuntimeTransaction::from)
            .collect();
        for utxo_tx in &selected_utxo_transactions {
            transactions.push(RuntimeTransaction::from(utxo_tx.clone()));
        }

        let merkle_root = derive_template_merkle_root(
            node_id,
            next_height,
            template_id,
            previous_hash,
            &selected_transactions,
            &selected_utxo_transactions,
        );

        let next_difficulty = if accepted_blocks.is_empty() {
            difficulty::GENESIS_DIFFICULTY
        } else {
            let start = accepted_blocks
                .len()
                .saturating_sub(difficulty::LWMA_WINDOW + 1);
            let window: Vec<difficulty::BlockInfo> = accepted_blocks[start..]
                .iter()
                .map(|b| difficulty::BlockInfo {
                    timestamp: b.timestamp,
                    difficulty: b.difficulty,
                })
                .collect();
            difficulty::lwma_next_difficulty(&window)
        };
        let target = difficulty::difficulty_to_target(next_difficulty);
        let bits = difficulty::target_to_compact(&target);

        TemplateState {
            template_id,
            height: next_height,
            header: MiningHeader {
                version: 3,
                previous_hash,
                merkle_root,
                timestamp: now_secs(),
                difficulty_bits: bits,
            },
            target,
            difficulty: next_difficulty,
            reward_zion: emission::block_subsidy(next_height),
            transactions,
            total_fees_zion,
        }
    }
}

impl ChainStore {
    fn load_snapshot(&self) -> Result<Option<ChainStateSnapshot>, String> {
        if !self.path.exists() {
            return Ok(None);
        }

        let raw = fs::read_to_string(&self.path).map_err(|error| {
            format!(
                "failed to read chain state {}: {error}",
                self.path.display()
            )
        })?;
        let snapshot = serde_json::from_str::<ChainStateSnapshot>(&raw).map_err(|error| {
            format!(
                "failed to decode chain state {}: {error}",
                self.path.display()
            )
        })?;
        Ok(Some(snapshot))
    }

    fn journal_exists(&self) -> bool {
        journal_path(&self.path).exists()
    }

    fn load_journal_entries(&self) -> Result<Vec<ChainJournalEntry>, String> {
        let path = journal_path(&self.path);
        if !path.exists() {
            return Ok(Vec::new());
        }

        let raw = fs::read_to_string(&path)
            .map_err(|error| format!("failed to read chain journal {}: {error}", path.display()))?;
        let mut entries = Vec::new();
        for (index, line) in raw.lines().enumerate() {
            if line.trim().is_empty() {
                continue;
            }
            let entry = serde_json::from_str::<ChainJournalEntry>(line).map_err(|error| {
                format!(
                    "failed to decode chain journal {} at line {}: {error}",
                    path.display(),
                    index + 1
                )
            })?;
            entries.push(entry);
        }
        Ok(entries)
    }

    fn append_journal_entry(&self, entry: &ChainJournalEntry) -> Result<(), String> {
        if let Some(parent) = self.path.parent() {
            fs::create_dir_all(parent).map_err(|error| {
                format!(
                    "failed to create chain state dir {}: {error}",
                    parent.display()
                )
            })?;
        }
        let path = journal_path(&self.path);
        let line = encode_json_line(entry).map_err(|error| {
            format!(
                "failed to encode chain journal entry {}: {error}",
                path.display()
            )
        })?;
        use std::io::Write as _;
        let mut file = fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&path)
            .map_err(|error| format!("failed to open chain journal {}: {error}", path.display()))?;
        file.write_all(line.as_bytes()).map_err(|error| {
            format!("failed to append chain journal {}: {error}", path.display())
        })?;
        file.flush().map_err(|error| {
            format!("failed to flush chain journal {}: {error}", path.display())
        })?;
        Ok(())
    }

    fn clear_journal(&self) -> Result<(), String> {
        let path = journal_path(&self.path);
        if path.exists() {
            fs::remove_file(&path).map_err(|error| {
                format!("failed to remove chain journal {}: {error}", path.display())
            })?;
        }
        Ok(())
    }

    fn replay_journal(
        &self,
        node_id: &str,
        core: &CoreRuntime,
        chain_state: &mut ChainState,
    ) -> Result<(), String> {
        for entry in self.load_journal_entries()? {
            chain_state.apply_journal_entry(node_id, core, entry)?;
        }
        Ok(())
    }

    fn save_snapshot(&self, snapshot: &ChainStateSnapshot) -> Result<(), String> {
        if let Some(parent) = self.path.parent() {
            fs::create_dir_all(parent).map_err(|error| {
                format!(
                    "failed to create chain state dir {}: {error}",
                    parent.display()
                )
            })?;
        }

        let encoded = serde_json::to_string_pretty(snapshot).map_err(|error| {
            format!(
                "failed to encode chain state {}: {error}",
                self.path.display()
            )
        })?;
        let temp_path = snapshot_temp_path(&self.path);
        fs::write(&temp_path, encoded).map_err(|error| {
            format!(
                "failed to write temp chain state {}: {error}",
                temp_path.display()
            )
        })?;
        fs::rename(&temp_path, &self.path).map_err(|error| {
            format!(
                "failed to move chain state {} into place: {error}",
                self.path.display()
            )
        })?;
        Ok(())
    }
}

fn encode_json_line<T: Serialize>(message: &T) -> Result<String, serde_json::Error> {
    let mut line = serde_json::to_string(message)?;
    line.push('\n');
    Ok(line)
}

fn dedup_peers(peers: Vec<PeerEndpoint>) -> Vec<PeerEndpoint> {
    let mut seen = HashSet::new();
    let mut deduped = Vec::new();
    for peer in peers {
        if seen.insert(peer.address()) {
            deduped.push(peer);
        }
    }
    deduped
}

/// Top-level dispatcher for the block body's Merkle root.
///
/// At/above [`BODY_ROOT_V2_ACTIVATION_HEIGHT`] this commits via a Bitcoin-style
/// BLAKE3 binary Merkle tree (audit §F2 / `AUDIT_COMPLETION.md` §2). Below that
/// height the legacy XOR aggregate is preserved bit-for-bit so historical
/// blocks continue to validate against their stored body roots.
///
/// # Why two paths
/// The XOR aggregate is birthday-resistant only at 2^64 and uses
/// `cosmic_harmony_ekam_deeksha` (256 KiB scratchpad) as a per-tx hash, which
/// is a misuse of a PoW function as a data-structure hash. The v2 path
/// replaces both: leaf hash drops to BLAKE3 via
/// `Transaction::calculate_hash()` (cheap, ASIC-irrelevant), aggregation
/// becomes a proper tree (collision-bounded at 2^256/2 with BLAKE3).
fn derive_template_merkle_root(
    node_id: &str,
    height: u64,
    template_id: u64,
    previous_hash: [u8; 32],
    transactions: &[Transaction],
    utxo_transactions: &[tx::Transaction],
) -> [u8; 32] {
    if body_root_v2_active(height) {
        derive_template_merkle_root_v2_blake3(transactions, utxo_transactions)
    } else {
        derive_template_merkle_root_v1_xor(
            node_id,
            height,
            template_id,
            previous_hash,
            transactions,
            utxo_transactions,
        )
    }
}

/// Legacy XOR aggregate body root (audit §F2 documents this as a misuse —
/// kept for pre-fork blocks so historical hashes don't change).
///
/// **Do not call directly** outside the dispatcher above and the regression
/// tests that pin v1 ↔ v2 distinction.
fn derive_template_merkle_root_v1_xor(
    node_id: &str,
    height: u64,
    template_id: u64,
    previous_hash: [u8; 32],
    transactions: &[Transaction],
    utxo_transactions: &[tx::Transaction],
) -> [u8; 32] {
    let mut seed = [0u8; HEADER_SIZE];
    seed[0..32].copy_from_slice(&previous_hash);
    seed[32..40].copy_from_slice(&height.to_le_bytes());
    seed[40..48].copy_from_slice(&template_id.to_le_bytes());
    let node_bytes = node_id.as_bytes();
    let copy_len = node_bytes.len().min(HEADER_SIZE - 48);
    seed[48..48 + copy_len].copy_from_slice(&node_bytes[..copy_len]);
    for transaction in transactions {
        let tx_hash = cosmic_harmony_ekam_deeksha(
            transaction.tx_id.as_bytes(),
            transaction.nonce ^ transaction.fee_zion ^ (transaction.amount_zion as u64),
        )
        .data;
        for (slot, value) in seed.iter_mut().zip(tx_hash.iter().cycle()) {
            *slot ^= *value;
        }
    }
    for utxo_tx in utxo_transactions {
        let tx_hash = cosmic_harmony_ekam_deeksha(
            &utxo_tx.id,
            utxo_tx.fee ^ utxo_tx.timestamp ^ utxo_tx.total_output(),
        )
        .data;
        for (slot, value) in seed.iter_mut().zip(tx_hash.iter().cycle()) {
            *slot ^= *value;
        }
    }
    cosmic_harmony_ekam_deeksha(
        &seed,
        template_id ^ height ^ (transactions.len() + utxo_transactions.len()) as u64,
    )
    .data
}

/// F2 BLAKE3 Merkle body root — the post-fork rule.
///
/// Builds leaves from BLAKE3 over the account-model `tx_id` (which already
/// commits to all consensus-relevant fields) and from `tx::Transaction::id`
/// for UTXO-model txs (which is itself already BLAKE3-derived via
/// [`tx::Transaction::calculate_hash`] — that dispatches to v2 once
/// [`TX_HASH_V2_ACTIVATION_HEIGHT`] is met, so the body root inherits the v2
/// malleability fix automatically). Aggregation uses
/// [`validation::merkle_root`] (Bitcoin-style pair-duplicate-on-odd-count
/// binary tree, BLAKE3 hash pairs).
///
/// Order of leaves: account-model txs first (in `transactions` order), then
/// UTXO txs (in `utxo_transactions` order). This matches the order in which
/// they are serialized into the block body, so peer validators can re-derive
/// the same root deterministically.
fn derive_template_merkle_root_v2_blake3(
    transactions: &[Transaction],
    utxo_transactions: &[tx::Transaction],
) -> [u8; 32] {
    let mut leaves: Vec<[u8; 32]> =
        Vec::with_capacity(transactions.len() + utxo_transactions.len());
    for transaction in transactions {
        leaves.push(crypto::blake3_hash(transaction.tx_id.as_bytes()));
    }
    for utxo_tx in utxo_transactions {
        leaves.push(utxo_tx.id);
    }
    validation::merkle_root(&leaves)
}

fn select_template_transactions(mempool: &[RuntimeTransaction]) -> Vec<Transaction> {
    let mut selected: Vec<Transaction> = mempool
        .iter()
        .filter_map(|transaction| transaction.as_account().cloned())
        .collect();
    selected.sort_by(|left, right| {
        right
            .fee_zion
            .cmp(&left.fee_zion)
            .then(left.nonce.cmp(&right.nonce))
            .then(left.tx_id.cmp(&right.tx_id))
    });
    selected.truncate(MAX_TEMPLATE_TRANSACTIONS);
    selected
}

fn select_template_utxo_transactions(mempool: &[RuntimeTransaction]) -> Vec<tx::Transaction> {
    let mut selected: Vec<tx::Transaction> = mempool
        .iter()
        .filter_map(|transaction| transaction.as_utxo().cloned())
        .collect();
    selected.sort_by(|left, right| right.fee.cmp(&left.fee));
    selected.truncate(MAX_TEMPLATE_UTXO_TRANSACTIONS);
    selected
}

pub(crate) fn body_hash_hex(transactions: &[Transaction]) -> String {
    let hash = derive_block_body_hash(transactions);
    hex(&hash)
}

fn derive_block_body_hash(transactions: &[Transaction]) -> [u8; 32] {
    let mut seed = [0u8; HEADER_SIZE];
    seed[0..8].copy_from_slice(&(transactions.len() as u64).to_le_bytes());
    for transaction in transactions {
        let tx_hash = cosmic_harmony_ekam_deeksha(
            transaction.tx_id.as_bytes(),
            transaction.nonce ^ (transaction.amount_zion as u64) ^ transaction.fee_zion,
        )
        .data;
        for (slot, value) in seed.iter_mut().zip(tx_hash.iter().cycle()) {
            *slot ^= *value;
        }
    }
    cosmic_harmony_ekam_deeksha(&seed, transactions.len() as u64).data
}

pub(crate) fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

fn snapshot_temp_path(path: &Path) -> PathBuf {
    let file_name = path
        .file_name()
        .and_then(|value| value.to_str())
        .unwrap_or("chain-state.json");
    path.with_file_name(format!("{file_name}.tmp"))
}

fn journal_path(path: &Path) -> PathBuf {
    let file_name = path
        .file_name()
        .and_then(|value| value.to_str())
        .unwrap_or("chain-state.json");
    path.with_file_name(format!("{file_name}.journal"))
}

pub(crate) fn is_valid_account_id(value: &str) -> bool {
    let len = value.len();
    (3..=64).contains(&len)
        && value
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_' | '.'))
}

#[allow(dead_code)]
fn looks_like_utxo_address(value: &str) -> bool {
    value.starts_with("zion1")
}

pub(crate) fn parse_fixed_hex<const N: usize>(raw: &str, label: &str) -> Result<[u8; N], String> {
    let normalized = raw.trim().trim_start_matches("0x");
    if normalized.len() != N * 2 {
        return Err(format!("{label} must be exactly {} hex chars", N * 2));
    }

    let mut bytes = [0u8; N];
    for (index, chunk) in normalized.as_bytes().chunks(2).enumerate() {
        let pair =
            std::str::from_utf8(chunk).map_err(|_| format!("{label} contains non-utf8 hex"))?;
        bytes[index] = u8::from_str_radix(pair, 16)
            .map_err(|_| format!("invalid hex byte '{pair}' in {label}"))?;
    }
    Ok(bytes)
}

pub(crate) fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{:02x}", byte)).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    /// Deterministic test keypair for signing account transactions in tests.
    fn test_keypair() -> (ed25519_dalek::SigningKey, ed25519_dalek::VerifyingKey) {
        crypto::keypair_from_canonical_label("__test_dummy_signer_v1__")
    }

    /// Sign a test transaction with the dummy test keypair.
    fn sign_test_tx(tx: &mut Transaction) {
        let (sk, vk) = test_keypair();
        let sig = crypto::sign(&sk, tx.tx_id.as_bytes());
        tx.signature = hex(&sig);
        tx.public_key = hex(vk.as_bytes());
    }

    /// Build a valid account transaction whose `from` address matches the signing key.
    fn build_valid_account_tx(
        from_label: &str,
        to: &str,
        amount: u64,
        fee: u64,
        nonce: u64,
    ) -> Transaction {
        let (sk, vk) = crypto::keypair_from_canonical_label(from_label);
        let from = crypto::derive_address(vk.as_bytes());
        let tx_id = crate::wallet::generate_account_tx_id(&from, to, amount, nonce, None, 1);
        let sig = crypto::sign(&sk, tx_id.as_bytes());
        Transaction {
            tx_id,
            from,
            to: to.to_string(),
            amount_zion: amount as u128,
            fee_zion: fee,
            nonce,
            signature: hex(&sig),
            public_key: hex(vk.as_bytes()),
            memo: None,
        }
    }
    use zion_cosmic_harmony::{
        generate_ekam_test_vector, BODY_ROOT_V2_ACTIVATION_HEIGHT, EKAM_CANONICAL_TEST_VECTOR_HEX,
    };

    fn sample_header() -> MiningHeader {
        MiningHeader {
            version: 3,
            previous_hash: [0x11; 32],
            merkle_root: [0x22; 32],
            timestamp: 1_762_000_000,
            difficulty_bits: 0x1f00ffff,
        }
    }

    fn sample_transaction(tx_id: &str, fee_zion: u64, nonce: u64) -> Transaction {
        let mut tx_hex = tx_id
            .as_bytes()
            .iter()
            .map(|byte| format!("{:02x}", byte))
            .collect::<String>();
        while tx_hex.len() < 64 {
            tx_hex.push('0');
        }
        tx_hex.truncate(64);
        // Derive the sender address from the test keypair so that
        // verify_signature() (which checks derive_address(public_key) == from)
        // passes. Using a bare label like "wallet.alpha" would fail the
        // signature check enforced since the F1 hardening fix.
        let (_, vk) = test_keypair();
        let from = crypto::derive_address(vk.as_bytes());
        let mut tx = Transaction {
            tx_id: tx_hex,
            from: from.clone(),
            to: "wallet.beta".to_string(),
            amount_zion: 25,
            fee_zion,
            nonce,
            signature: String::new(),
            public_key: String::new(),
            memo: None,
        };
        sign_test_tx(&mut tx);
        tx
    }

    #[test]
    fn core_uses_canonical_profile() {
        assert_eq!(consensus_profile(), "deeksha_lite_v1");
    }

    #[test]
    fn mining_header_serializes_to_80_bytes() {
        let bytes = sample_header().to_bytes();
        assert_eq!(bytes.len(), HEADER_SIZE);
        assert_eq!(&bytes[0..4], &3u32.to_le_bytes());
    }

    #[test]
    fn mining_header_roundtrip_from_bytes() {
        let header = sample_header();
        assert_eq!(MiningHeader::from_bytes(header.to_bytes()), header);
    }

    #[test]
    fn runtime_hashes_candidate_with_ekam() {
        let runtime = CoreRuntime::default();
        let candidate = BlockCandidate {
            header: sample_header(),
            nonce: 42,
            height: 0,
        };

        let direct = zion_cosmic_harmony::deeksha_lite::deeksha_lite(
            &candidate.header.to_bytes(),
            candidate.nonce,
        );
        assert_eq!(runtime.hash_candidate(candidate), direct);
    }

    #[test]
    fn hash_with_algorithm_fire_matches_direct() {
        let candidate = BlockCandidate {
            header: sample_header(),
            nonce: 42,
            height: 0,
        };
        let direct = zion_cosmic_harmony::deeksha_lite_fire::deeksha_lite_fire(
            &candidate.header.to_bytes(),
            candidate.nonce,
        );
        assert_eq!(candidate.hash_with_algorithm("deeksha_lite_fire"), direct);
    }

    #[test]
    fn hash_with_algorithm_fire_is_deterministic() {
        let candidate = BlockCandidate {
            header: sample_header(),
            nonce: 99,
            height: 0,
        };
        let h1 = candidate.hash_with_algorithm("deeksha_lite_fire");
        let h2 = candidate.hash_with_algorithm("deeksha_lite_fire");
        assert_eq!(h1, h2);
        assert_ne!(h1, [0u8; 32]);
    }

    #[test]
    fn runtime_validates_target() {
        let runtime = CoreRuntime::default();
        let candidate = BlockCandidate {
            header: sample_header(),
            nonce: 7,
            height: 0,
        };

        let sealed = runtime
            .validate_candidate(candidate, DifficultyTarget::MAX)
            .expect("max target should accept any hash");
        assert_eq!(sealed.hash, candidate.hash());
    }

    #[test]
    fn runtime_tracks_revenue() {
        let runtime = CoreRuntime::default();
        runtime.record_revenue(RevenueSource::ProfitSwitch, 100.0, true);

        let snapshot = runtime.revenue_snapshot();
        assert_eq!(snapshot.total_earnings_usd, 100.0);
        assert!((snapshot.zion_fees_usd - 2.0).abs() < 0.001);
    }

    #[test]
    fn runtime_tracks_zion_block_revenue() {
        let runtime = CoreRuntime::default();
        let subsidy = 5_400_067_000_000_000_u64;
        runtime.record_zion_block_revenue(42, subsidy, 1, None);

        let snapshot = runtime.revenue_snapshot();
        let expected_pool = subsidy * zion_cosmic_harmony::ZION_POOL_PCT / 100;
        let expected_humanitarian = subsidy * zion_cosmic_harmony::ZION_HUMANITARIAN_PCT / 100;
        let expected_issobella = subsidy * zion_cosmic_harmony::ZION_ISSOBELLA_PCT / 100;
        let expected_miner = subsidy - expected_pool - expected_humanitarian - expected_issobella;

        assert_eq!(snapshot.total_zion, subsidy);
        assert_eq!(snapshot.zion_fees_zion, expected_pool);
        assert_eq!(snapshot.humanitarian_zion, expected_humanitarian);
        assert_eq!(snapshot.issobella_zion, expected_issobella);
        assert_eq!(snapshot.miner_payout_zion, expected_miner);
        assert_eq!(snapshot.blocks_found, 1);
        assert_eq!(snapshot.last_block_height, 42);
        assert_eq!(snapshot.total_earnings_usd, 0.0); // USD untouched
    }

    #[test]
    fn canonical_vector_is_exposed_from_consensus_crate() {
        assert_eq!(generate_ekam_test_vector(), EKAM_CANONICAL_TEST_VECTOR_HEX);
    }

    #[test]
    fn node_config_mainnet_defaults_are_stable() {
        let config = NodeConfig::mainnet();
        assert_eq!(config.network, NetworkId::Mainnet);
        assert_eq!(config.p2p_bind.address(), "0.0.0.0:8333");
        assert_eq!(config.seed_peers[0].address(), "<ZION_SEED_PEER>:8333");
    }

    #[test]
    fn runtime_scans_nonce_range() {
        let runtime = CoreRuntime::default();
        let job = MiningJob {
            job_id: 1,
            header: sample_header(),
            target: DifficultyTarget::MAX,
            start_nonce: 100,
            nonce_count: 8,
            height: 0,
        };

        let solution = runtime
            .scan_nonce_range(job)
            .expect("max target must find a solution");
        assert_eq!(solution.job_id, 1);
        assert_eq!(solution.candidate.nonce, 100);
        assert_eq!(solution.hash, solution.candidate.hash());
    }

    #[test]
    fn runtime_validates_job_bound_solution() {
        let runtime = CoreRuntime::default();
        let job = MiningJob {
            job_id: 7,
            header: sample_header(),
            target: DifficultyTarget::MAX,
            start_nonce: 55,
            nonce_count: 4,
            height: 0,
        };

        let solution = runtime
            .scan_nonce_range(job)
            .expect("solution should exist");
        let sealed = runtime
            .validate_solution(job, solution)
            .expect("matching job solution must validate");
        assert_eq!(sealed.nonce, 55);
    }

    #[test]
    fn p2p_message_roundtrip_is_stable() {
        let status = NodeRuntime::new("node-a", NodeConfig::mainnet()).status();
        let message = P2pMessage::Status { status };
        let encoded = encode_p2p_message(&message).expect("encode p2p");
        let decoded = decode_p2p_message(&encoded).expect("decode p2p");
        assert_eq!(decoded, message);
    }

    /// Scan nonces to find one that meets the template target.
    fn find_valid_nonce(template: &BlockTemplate) -> u64 {
        let header = MiningHeader::from_bytes(
            parse_fixed_hex::<HEADER_SIZE>(&template.header_hex, "test header").unwrap(),
        );
        let target = DifficultyTarget::from_hex(&template.target_hex).unwrap();
        for nonce in 0..10_000_000 {
            let candidate = BlockCandidate {
                header,
                nonce,
                height: template.height,
            };
            if target.allows(&candidate.hash()) {
                return nonce;
            }
        }
        panic!("no valid nonce found in 10M attempts");
    }

    #[test]
    fn rpc_submit_candidate_accepts_active_template() {
        let mut runtime = NodeRuntime::new("node-rpc", NodeConfig::mainnet());
        let template = runtime.active_template();
        let nonce = find_valid_nonce(&template);
        let header = MiningHeader::from_bytes(
            parse_fixed_hex::<HEADER_SIZE>(&template.header_hex, "template header")
                .expect("template header bytes"),
        );
        let candidate = BlockCandidate {
            header,
            nonce,
            height: template.height,
        };

        let response = runtime.handle_rpc_request(RpcRequest::SubmitCandidate {
            template_id: template.template_id,
            header_hex: template.header_hex.clone(),
            nonce,
            target_hex: template.target_hex.clone(),
            algorithm: "deeksha_lite_v1".to_string(),
        });

        match response {
            RpcResponse::SubmitResult {
                accepted,
                template_id,
                block_height,
                hash_hex,
                reason,
            } => {
                assert!(accepted);
                assert_eq!(template_id, template.template_id);
                assert_eq!(block_height, Some(1));
                assert_eq!(hash_hex, hex(&candidate.hash()));
                assert_eq!(reason, None);
            }
            other => panic!("unexpected rpc response: {other:?}"),
        }
    }

    #[test]
    fn rpc_get_template_returns_active_template() {
        let mut runtime = NodeRuntime::new("node-template", NodeConfig::mainnet());
        let expected = runtime.active_template();
        let response = runtime.handle_rpc_request(RpcRequest::GetTemplate);

        match response {
            RpcResponse::Template { template } => assert_eq!(template, expected),
            other => panic!("unexpected template response: {other:?}"),
        }
    }

    #[test]
    fn rpc_submit_transaction_updates_mempool_and_template() {
        let mut runtime = NodeRuntime::new("node-mempool", NodeConfig::mainnet());
        let transaction = sample_transaction("tx-a", 9, 1);
        let response = runtime.handle_rpc_request(RpcRequest::SubmitTransaction {
            transaction: transaction.clone(),
        });

        assert!(matches!(
            response,
            RpcResponse::TransactionResult {
                accepted: true,
                reason: None,
                ..
            }
        ));
        assert!(matches!(
            runtime.chain_state.mempool.as_slice(),
            [RuntimeTransaction::Account(stored)] if stored.tx_id == transaction.tx_id
        ));

        match runtime.handle_rpc_request(RpcRequest::GetMempool) {
            RpcResponse::Mempool { transactions } => {
                assert_eq!(transactions.len(), 1);
                assert_eq!(transactions[0].tx_id, transaction.tx_id);
            }
            other => panic!("unexpected mempool response: {other:?}"),
        }

        let template = runtime.active_template();
        assert_eq!(template.transaction_count, 1);
        assert_eq!(template.transaction_ids, vec![transaction.tx_id]);
        assert_eq!(template.total_fees_zion, 9);
        assert_eq!(
            template.body_hash_hex,
            body_hash_hex(&[sample_transaction("tx-a", 9, 1)])
        );
        assert_eq!(
            template.estimated_miner_reward_zion,
            emission::block_subsidy(1)
        );

        let status = runtime.status();
        assert_eq!(status.mempool_transactions, 1);
        assert_eq!(status.active_template_transactions, 1);
        assert_eq!(status.active_template_total_fees_zion, 9);
    }

    #[test]
    fn transaction_validation_rejects_bad_ids_and_sender_nonce_reuse() {
        let mut runtime = NodeRuntime::new("node-validate", NodeConfig::mainnet());

        let invalid = runtime.handle_rpc_request(RpcRequest::SubmitTransaction {
            transaction: Transaction {
                tx_id: "bad-id".to_string(),
                from: "wallet.alpha".to_string(),
                to: "wallet.beta".to_string(),
                amount_zion: 10,
                fee_zion: 1,
                nonce: 1,
                signature: String::new(),
                public_key: String::new(),
                memo: None,
            },
        });
        assert!(matches!(
            invalid,
            RpcResponse::TransactionResult {
                accepted: false,
                reason: Some(ref reason),
                ..
            } if reason.contains("64 hex chars")
        ));

        let utxo_like_endpoints = runtime.handle_rpc_request(RpcRequest::SubmitTransaction {
            transaction: Transaction {
                tx_id: sample_transaction("tx-utxo-like", 3, 1).tx_id,
                from: "zion1qqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqq".to_string(),
                to: "wallet.beta".to_string(),
                amount_zion: 10,
                fee_zion: 1,
                nonce: 2,
                signature: String::new(),
                public_key: String::new(),
                memo: None,
            },
        });
        assert!(matches!(
            utxo_like_endpoints,
            RpcResponse::TransactionResult {
                accepted: false,
                reason: Some(ref reason),
                ..
            } if reason.contains("signature verification failed")
        ));

        let first = sample_transaction("tx-nonce-a", 2, 9);
        let mut reused_nonce = Transaction {
            tx_id: sample_transaction("tx-nonce-b", 4, 9).tx_id,
            from: first.from.clone(),
            to: first.to.clone(),
            amount_zion: 22,
            fee_zion: 4,
            nonce: first.nonce,
            signature: String::new(),
            public_key: String::new(),
            memo: None,
        };
        sign_test_tx(&mut reused_nonce);
        assert!(matches!(
            runtime.handle_rpc_request(RpcRequest::SubmitTransaction {
                transaction: first.clone()
            }),
            RpcResponse::TransactionResult { accepted: true, .. }
        ));
        assert!(matches!(
            runtime.handle_rpc_request(RpcRequest::SubmitTransaction {
                transaction: reused_nonce
            }),
            RpcResponse::TransactionResult {
                accepted: false,
                reason: Some(ref reason),
                ..
            } if reason.contains("already pending")
        ));
    }

    #[test]
    fn template_prioritizes_high_fee_transactions() {
        let mut runtime = NodeRuntime::new("node-priority", NodeConfig::mainnet());
        let tx_low = sample_transaction("tx-low", 1, 2);
        let tx_high = sample_transaction("tx-high", 7, 1);
        let _ = runtime.handle_rpc_request(RpcRequest::SubmitTransaction {
            transaction: tx_low.clone(),
        });
        let _ = runtime.handle_rpc_request(RpcRequest::SubmitTransaction {
            transaction: tx_high.clone(),
        });

        let template = runtime.active_template();
        assert_eq!(template.transaction_ids, vec![tx_high.tx_id, tx_low.tx_id]);
        assert_eq!(template.total_fees_zion, 8);
    }

    /// Slow: `find_valid_nonce` + template rotation in debug build.
    /// Run via: `cargo test --release -- --include-ignored`
    #[test]
    #[ignore = "slow PoW in debug build; run with --release --ignored"]
    fn accepted_submission_rotates_template_and_updates_tip() {
        let mut runtime = NodeRuntime::new("node-rotate", NodeConfig::mainnet());
        let mined_transaction = sample_transaction("tx-mined", 3, 1);
        let _ = runtime.handle_rpc_request(RpcRequest::SubmitTransaction {
            transaction: mined_transaction.clone(),
        });
        let first_template = runtime.active_template();
        assert_eq!(
            first_template.transaction_ids,
            vec![mined_transaction.tx_id.clone()]
        );
        let nonce = find_valid_nonce(&first_template);

        let response = runtime.handle_rpc_request(RpcRequest::SubmitCandidate {
            template_id: first_template.template_id,
            header_hex: first_template.header_hex.clone(),
            nonce,
            target_hex: first_template.target_hex.clone(),
            algorithm: "deeksha_lite_v1".to_string(),
        });

        assert!(matches!(
            response,
            RpcResponse::SubmitResult { accepted: true, .. }
        ));
        assert_eq!(runtime.status().chain_height, 1);
        assert_eq!(runtime.accepted_blocks().len(), 2); // genesis + mined
        assert_ne!(
            runtime.active_template().template_id,
            first_template.template_id
        );
        assert_eq!(runtime.active_template().height, 2);
        assert!(runtime.active_template().transaction_ids.is_empty());
        assert_eq!(
            runtime.accepted_blocks()[1].transaction_ids,
            vec![mined_transaction.tx_id]
        );
        assert_eq!(
            runtime.accepted_blocks()[1].subsidy_zion,
            emission::block_subsidy(1)
        );
        assert_eq!(
            runtime.accepted_blocks()[1].miner_reward_zion,
            emission::block_subsidy(1)
        );
    }

    #[test]
    fn stale_template_submission_is_rejected() {
        let mut runtime = NodeRuntime::new("node-stale", NodeConfig::mainnet());
        let template = runtime.active_template();
        let nonce = find_valid_nonce(&template);
        let _ = runtime.handle_rpc_request(RpcRequest::SubmitCandidate {
            template_id: template.template_id,
            header_hex: template.header_hex.clone(),
            nonce,
            target_hex: template.target_hex.clone(),
            algorithm: "deeksha_lite_v1".to_string(),
        });

        let stale_response = runtime.handle_rpc_request(RpcRequest::SubmitCandidate {
            template_id: template.template_id,
            header_hex: template.header_hex,
            nonce: 19,
            target_hex: template.target_hex,
            algorithm: "deeksha_lite_v1".to_string(),
        });

        match stale_response {
            RpcResponse::SubmitResult {
                accepted, reason, ..
            } => {
                assert!(!accepted);
                assert!(reason.expect("stale reason").contains("stale template"));
            }
            other => panic!("unexpected stale response: {other:?}"),
        }
    }

    #[test]
    fn node_runtime_registers_peer_on_hello() {
        let mut runtime = NodeRuntime::new("node-core", NodeConfig::mainnet());
        let allowed_peer = runtime.config().seed_peers[0].address();

        let response = runtime
            .handle_p2p_message(P2pMessage::Hello {
                node_id: "peer-1".to_string(),
                protocol_version: node_protocol_version().to_string(),
                network: NetworkId::Mainnet,
                listen_addr: allowed_peer.clone(),
            })
            .expect("hello response");

        assert!(runtime
            .known_peers()
            .iter()
            .any(|peer| peer.address() == allowed_peer));
        assert!(matches!(response, P2pMessage::Welcome { .. }));
    }

    #[test]
    fn register_peer_ignores_own_bind_address() {
        let mut runtime = NodeRuntime::new("node-self", NodeConfig::mainnet());
        let before = runtime.known_peers().len();

        runtime.register_peer(runtime.config().p2p_bind.clone());

        assert_eq!(runtime.known_peers().len(), before);
        assert_eq!(
            runtime
                .known_peers()
                .iter()
                .filter(|peer| peer.address() == runtime.config().p2p_bind.address())
                .count(),
            0
        );
    }

    #[test]
    fn p2p_get_blocks_since_returns_accepted_blocks() {
        let mut runtime = NodeRuntime::new("node-sync-source", NodeConfig::mainnet());
        let first_tx = sample_transaction("tx-sync-1", 2, 1);
        let _ = runtime.handle_rpc_request(RpcRequest::SubmitTransaction {
            transaction: first_tx.clone(),
        });
        let first_template = runtime.active_template();
        let nonce1 = find_valid_nonce(&first_template);
        let _ = runtime.handle_rpc_request(RpcRequest::SubmitCandidate {
            template_id: first_template.template_id,
            header_hex: first_template.header_hex,
            nonce: nonce1,
            target_hex: first_template.target_hex,
            algorithm: "deeksha_lite_v1".to_string(),
        });

        let second_template = runtime.active_template();
        let nonce2 = find_valid_nonce(&second_template);
        let _ = runtime.handle_rpc_request(RpcRequest::SubmitCandidate {
            template_id: second_template.template_id,
            header_hex: second_template.header_hex,
            nonce: nonce2,
            target_hex: second_template.target_hex,
            algorithm: "deeksha_lite_v1".to_string(),
        });

        let response = runtime
            .handle_p2p_message(P2pMessage::GetBlocksSince {
                from_height: 0,
                limit: 8,
            })
            .expect("blocks since response");

        match response {
            P2pMessage::Blocks { blocks } => {
                assert_eq!(blocks.len(), 2);
                assert_eq!(blocks[0].height, 1);
                assert_eq!(blocks[0].transaction_ids, vec![first_tx.tx_id]);
                assert_eq!(blocks[1].height, 2);
            }
            other => panic!("unexpected blocks response: {other:?}"),
        }
    }

    #[test]
    fn p2p_announce_block_imports_contiguous_peer_block() {
        let mut source = NodeRuntime::new("node-source", NodeConfig::mainnet());
        let propagated_tx = sample_transaction("tx-peer-import", 4, 1);
        let _ = source.handle_rpc_request(RpcRequest::SubmitTransaction {
            transaction: propagated_tx,
        });
        let template = source.active_template();
        let nonce = find_valid_nonce(&template);
        let _ = source.handle_rpc_request(RpcRequest::SubmitCandidate {
            template_id: template.template_id,
            header_hex: template.header_hex,
            nonce,
            target_hex: template.target_hex,
            algorithm: "deeksha_lite_v1".to_string(),
        });

        let block = source.accepted_blocks()[1].clone(); // skip genesis
        let mut target = NodeRuntime::new("node-target", NodeConfig::mainnet());
        let response = target
            .handle_p2p_message(P2pMessage::AnnounceBlock {
                block: block.clone(),
            })
            .expect("announce block response");

        match response {
            P2pMessage::Status { status } => {
                assert_eq!(status.chain_height, 1);
                assert_eq!(status.accepted_blocks, 2); // genesis + imported
            }
            other => panic!("unexpected announce response: {other:?}"),
        }
        assert_eq!(target.accepted_blocks()[1], block);
        assert_eq!(target.active_template().height, 2);
    }

    #[test]
    fn p2p_announce_block_rejects_conflicting_height() {
        let mut left = NodeRuntime::new("node-left", NodeConfig::mainnet());
        let left_template = left.active_template();
        let left_nonce = find_valid_nonce(&left_template);
        let _ = left.handle_rpc_request(RpcRequest::SubmitCandidate {
            template_id: left_template.template_id,
            header_hex: left_template.header_hex,
            nonce: left_nonce,
            target_hex: left_template.target_hex,
            algorithm: "deeksha_lite_v1".to_string(),
        });

        let mut right = NodeRuntime::new("node-right", NodeConfig::mainnet());
        let right_template = right.active_template();
        let right_nonce = find_valid_nonce(&right_template);
        let _ = right.handle_rpc_request(RpcRequest::SubmitCandidate {
            template_id: right_template.template_id,
            header_hex: right_template.header_hex,
            nonce: right_nonce,
            target_hex: right_template.target_hex,
            algorithm: "deeksha_lite_v1".to_string(),
        });

        let error = right
            .handle_p2p_message(P2pMessage::AnnounceBlock {
                block: left.accepted_blocks()[1].clone(), // skip genesis
            })
            .expect_err("conflicting height should fail");
        assert!(error.contains("conflicting peer block"));
    }

    /// Regression test for 3.0.4 CRITICAL Finding 1: a peer block containing an
    /// account transaction whose public key does not derive to the `from` address
    /// must be rejected, even if the Ed25519 signature is otherwise valid.
    #[test]
    fn validate_peer_block_rejects_forged_account_transaction() {
        let mut source = NodeRuntime::new("node-forge-src", NodeConfig::mainnet());
        let valid_tx =
            build_valid_account_tx("__peer_block_test_victim__", "wallet.dest", 25, 1, 1);
        let submit_response = source.handle_rpc_request(RpcRequest::SubmitTransaction {
            transaction: valid_tx.clone(),
        });
        match submit_response {
            RpcResponse::TransactionResult { accepted: true, .. } => {}
            other => panic!("valid tx must be accepted by mempool: {other:?}"),
        }

        let template = source.active_template();
        let nonce = find_valid_nonce(&template);
        let candidate_response = source.handle_rpc_request(RpcRequest::SubmitCandidate {
            template_id: template.template_id,
            header_hex: template.header_hex,
            nonce,
            target_hex: template.target_hex,
            algorithm: "deeksha_lite_v1".to_string(),
        });
        match candidate_response {
            RpcResponse::SubmitResult { accepted: true, .. } => {}
            other => panic!("block candidate must be accepted: {other:?}"),
        }

        let mut block = source.accepted_blocks()[1].clone(); // skip genesis
        let tx_index = block
            .transactions
            .iter()
            .position(|tx| tx.tx_id == valid_tx.tx_id)
            .expect("valid tx must be in mined block");

        // Forge the transaction: keep the same tx_id and content, but sign with
        // an unrelated attacker key. The signature is valid for (attacker_pk, tx_id),
        // but the key does not derive to the victim `from` address.
        let mut forged_tx = valid_tx.clone();
        let (attacker_sk, attacker_vk) = crypto::generate_keypair();
        let sig = crypto::sign(&attacker_sk, forged_tx.tx_id.as_bytes());
        forged_tx.signature = hex(&sig);
        forged_tx.public_key = hex(attacker_vk.as_bytes());
        block.transactions[tx_index] = forged_tx;

        let mut target = NodeRuntime::new("node-forge-tgt", NodeConfig::mainnet());
        let err = target
            .import_peer_blocks(vec![block])
            .expect_err("forged account tx must be rejected");
        assert!(
            err.contains("signature verification failed"),
            "unexpected error: {err}"
        );
    }

    /// F5 regression: an account-model transaction from an address with
    /// insufficient balance must be rejected by the RPC path
    /// (`insert_transaction`). Without the F5 fix this would be accepted,
    /// creating ZION from nothing.
    #[test]
    fn rpc_rejects_account_tx_with_insufficient_balance() {
        let mut runtime = NodeRuntime::new("node-f5-rpc", NodeConfig::mainnet());
        // Enable F5 balance check from genesis (height 0) for this runtime only.
        runtime.set_balance_check_height(0);
        // Build a valid (signed) TX from a brand-new address with 0 balance.
        let tx = build_valid_account_tx("__f5_empty_sender__", "wallet.dest", 100, 1, 1);
        let resp = runtime.handle_rpc_request(RpcRequest::SubmitTransaction { transaction: tx });
        match resp {
            RpcResponse::TransactionResult {
                accepted: false,
                reason: Some(ref reason),
                ..
            } if reason.contains("insufficient balance") => {}
            other => panic!(
                "F5: TX from empty address must be rejected with 'insufficient balance', got: {other:?}"
            ),
        }
    }

    /// F5 regression: an account-model transaction from an address with
    /// insufficient balance must be rejected by the peer-block path
    /// (`validate_peer_block`).
    #[test]
    #[ignore = "slow PoW in debug build; run with --release --ignored"]
    fn peer_block_rejects_account_tx_with_insufficient_balance() {
        let mut source = NodeRuntime::new("node-f5-peer-src", NodeConfig::mainnet());
        source.set_balance_check_height(0);
        // Build a TX from an empty address and mine it into a block.
        let tx = build_valid_account_tx("__f5_empty_peer__", "wallet.dest", 100, 1, 1);
        let submit_resp = source.handle_rpc_request(RpcRequest::SubmitTransaction {
            transaction: tx.clone(),
        });
        // The RPC path should reject it first (F5 RPC guard).
        if matches!(
            submit_resp,
            RpcResponse::TransactionResult {
                accepted: false,
                ..
            }
        ) {
            return;
        }
        // If RPC somehow accepted it, mine and verify peer-block rejection.
        let template = source.active_template();
        let nonce = find_valid_nonce(&template);
        let _ = source.handle_rpc_request(RpcRequest::SubmitCandidate {
            template_id: template.template_id,
            header_hex: template.header_hex,
            nonce,
            target_hex: template.target_hex,
            algorithm: "deeksha_lite_v1".to_string(),
        });
        let block = source.accepted_blocks()[1].clone();
        let mut target = NodeRuntime::new("node-f5-peer-tgt", NodeConfig::mainnet());
        target.set_balance_check_height(0);
        let result = target.import_peer_blocks(vec![block]);
        let err = result.expect_err("F5: peer block with TX from empty address must be rejected");
        assert!(
            err.contains("insufficient balance"),
            "F5: unexpected peer-block error: {err}"
        );
    }

    /// F5 positive: a TX from an address that DOES have sufficient balance
    /// (funded by a coinbase output) must still be accepted.
    #[test]
    fn rpc_accepts_account_tx_with_sufficient_balance() {
        let mut runtime = NodeRuntime::new("node-f5-pos", NodeConfig::mainnet());
        runtime.set_balance_check_height(0);
        // Derive the zion1 address for the miner label so the coinbase
        // output goes to the same address that build_valid_account_tx will
        // use as the `from` field.
        let (_miner_sk, miner_vk) = crypto::keypair_from_canonical_label("wallet.f5_miner");
        let miner_addr = crypto::derive_address(miner_vk.as_bytes());
        runtime.set_miner_address(miner_addr.clone());
        // Mine a block to fund the miner address via coinbase.
        let template = runtime.active_template();
        let nonce = find_valid_nonce(&template);
        let _ = runtime.handle_rpc_request(RpcRequest::SubmitCandidate {
            template_id: template.template_id,
            header_hex: template.header_hex,
            nonce,
            target_hex: template.target_hex,
            algorithm: "deeksha_lite_v1".to_string(),
        });
        // Now build a TX from the funded miner address. The coinbase
        // output went to miner_addr (a zion1 address), and
        // build_valid_account_tx derives the same keypair from the same
        // label, so the from address matches.
        let tx = build_valid_account_tx("wallet.f5_miner", "wallet.dest", 10, 1, 1);
        let resp = runtime.handle_rpc_request(RpcRequest::SubmitTransaction { transaction: tx });
        match resp {
            RpcResponse::TransactionResult { accepted: true, .. } => {}
            other => panic!("F5 positive: TX from funded address must be accepted, got: {other:?}"),
        }
    }

    // ═══════════════════════════════════════════════════════════════════════
    // F5 FUZZ TESTS — randomized stress tests for balance validation
    // ═══════════════════════════════════════════════════════════════════════

    /// F5 fuzz: submit N random TXs from random unfunded addresses with
    /// random amounts. Every single one must be rejected.
    #[test]
    fn fuzz_rpc_rejects_random_unfunded_senders() {
        let mut runtime = NodeRuntime::new("node-f5-fuzz-rpc", NodeConfig::mainnet());
        runtime.set_balance_check_height(0);
        let mut rejections = 0u32;
        // 100 random TXs from 100 different unfunded addresses
        for i in 0..100u32 {
            let label = format!("__f5_fuzz_sender_{i}__");
            let amount = (i as u64 * 1_000_000) + 1; // 1 to ~100M flowers
            let tx = build_valid_account_tx(&label, "wallet.dest", amount, 1, 1);
            let resp =
                runtime.handle_rpc_request(RpcRequest::SubmitTransaction { transaction: tx });
            if matches!(
                resp,
                RpcResponse::TransactionResult {
                    accepted: false,
                    reason: Some(ref r),
                    ..
                } if r.contains("insufficient balance")
            ) {
                rejections += 1;
            }
        }
        assert_eq!(
            rejections, 100,
            "F5 fuzz: all 100 TXs from unfunded addresses must be rejected, only {rejections} were"
        );
    }

    /// F5 fuzz: double-spend attempt — submit 2 TXs from the same funded
    /// address where the second exceeds remaining balance.
    #[test]
    fn fuzz_rpc_rejects_double_spend_exceeding_balance() {
        let mut runtime = NodeRuntime::new("node-f5-fuzz-ds", NodeConfig::mainnet());
        runtime.set_balance_check_height(0);
        // Fund the miner address via coinbase
        let (_miner_sk, miner_vk) = crypto::keypair_from_canonical_label("wallet.f5_fuzz_ds");
        let miner_addr = crypto::derive_address(miner_vk.as_bytes());
        runtime.set_miner_address(miner_addr.clone());
        let template = runtime.active_template();
        let nonce = find_valid_nonce(&template);
        let _ = runtime.handle_rpc_request(RpcRequest::SubmitCandidate {
            template_id: template.template_id,
            header_hex: template.header_hex,
            nonce,
            target_hex: template.target_hex,
            algorithm: "deeksha_lite_v1".to_string(),
        });
        // Submit TX for most of the balance.
        // Coinbase reward = 5,400,067,000 flowers. Miner gets 89% = ~4,806,059,630 flowers.
        // TX1 sends 4,000,000,000 flowers (~4000 ZION) + 1 fee.
        let tx1 = build_valid_account_tx("wallet.f5_fuzz_ds", "wallet.dest1", 4_000_000_000, 1, 1);
        let resp1 = runtime.handle_rpc_request(RpcRequest::SubmitTransaction { transaction: tx1 });
        assert!(
            matches!(resp1, RpcResponse::TransactionResult { accepted: true, .. }),
            "F5 fuzz: first TX within balance must be accepted"
        );
        // Submit second TX that exceeds remaining balance (double-spend).
        // Remaining: ~806,059,630 flowers. TX2 asks for 4,000,000,000 → must be rejected.
        let tx2 = build_valid_account_tx("wallet.f5_fuzz_ds", "wallet.dest2", 4_000_000_000, 1, 2);
        let resp2 = runtime.handle_rpc_request(RpcRequest::SubmitTransaction { transaction: tx2 });
        assert!(
            matches!(
                resp2,
                RpcResponse::TransactionResult {
                    accepted: false,
                    reason: Some(ref r),
                    ..
                } if r.contains("insufficient balance")
            ),
            "F5 fuzz: second TX exceeding remaining balance must be rejected (double-spend), got: {resp2:?}"
        );
    }

    /// F5 fuzz: max u128 amount overflow — submit TX with amount near u128::MAX.
    /// Must be rejected (sender has 0 balance, and amount is absurd).
    #[test]
    fn fuzz_rpc_rejects_max_u128_amount() {
        let mut runtime = NodeRuntime::new("node-f5-fuzz-max", NodeConfig::mainnet());
        runtime.set_balance_check_height(0);
        let tx = build_valid_account_tx("__f5_fuzz_max__", "wallet.dest", u64::MAX, u64::MAX, 1);
        let resp = runtime.handle_rpc_request(RpcRequest::SubmitTransaction { transaction: tx });
        assert!(
            matches!(
                resp,
                RpcResponse::TransactionResult {
                    accepted: false,
                    reason: Some(ref r),
                    ..
                } if r.contains("insufficient balance")
            ),
            "F5 fuzz: TX with u64::MAX amount + fee must be rejected, got: {resp:?}"
        );
    }

    /// F5 fuzz: many small TXs from different unfunded addresses in rapid
    /// succession — verify no state corruption or panic.
    #[test]
    fn fuzz_rpc_rapid_fire_no_panic() {
        let mut runtime = NodeRuntime::new("node-f5-fuzz-rapid", NodeConfig::mainnet());
        runtime.set_balance_check_height(0);
        for i in 0..200u32 {
            let label = format!("__f5_rapid_{i}__");
            let tx = build_valid_account_tx(&label, &format!("wallet.dest_{i}"), 1, 1, 1);
            let _ = runtime.handle_rpc_request(RpcRequest::SubmitTransaction { transaction: tx });
        }
        // If we reach here without panic, the test passes.
    }

    /// F5 fuzz: TX from self to self (same address) with 0 balance.
    /// Must be rejected — either by the "sender and recipient must differ"
    /// guard or by the balance check. Either rejection is acceptable.
    #[test]
    fn fuzz_rpc_rejects_self_send_from_empty() {
        let mut runtime = NodeRuntime::new("node-f5-fuzz-self", NodeConfig::mainnet());
        runtime.set_balance_check_height(0);
        let (sk, vk) = crypto::keypair_from_canonical_label("__f5_self_send__");
        let addr = crypto::derive_address(vk.as_bytes());
        let tx_id = crate::wallet::generate_account_tx_id(&addr, &addr, 1, 1, None, 1);
        let sig = crypto::sign(&sk, tx_id.as_bytes());
        let tx = Transaction {
            tx_id,
            from: addr.clone(),
            to: addr,
            amount_zion: 1,
            fee_zion: 1,
            nonce: 1,
            signature: hex(&sig),
            public_key: hex(vk.as_bytes()),
            memo: None,
        };
        let resp = runtime.handle_rpc_request(RpcRequest::SubmitTransaction { transaction: tx });
        assert!(
            matches!(
                resp,
                RpcResponse::TransactionResult {
                    accepted: false,
                    ..
                }
            ),
            "F5 fuzz: self-send from empty address must be rejected, got: {resp:?}"
        );
    }

    // ── F4.7: max-tx-amount cap ────────────────────────────────────────────

    /// F4.7: a TX whose amount exceeds TOTAL_SUPPLY must be rejected once the
    /// cap is active — the check runs before the F5 balance check, so any
    /// sender (funded or not) is caught.
    #[test]
    fn f4_7_rejects_tx_above_total_supply() {
        let mut runtime = NodeRuntime::new("node-f47-over", NodeConfig::mainnet());
        runtime.set_max_tx_amount_height(0);
        let over = (emission::TOTAL_SUPPLY as u64).saturating_add(1);
        let tx = build_valid_account_tx("__f47_over__", "wallet.dest", over, 1, 1);
        let resp = runtime.handle_rpc_request(RpcRequest::SubmitTransaction { transaction: tx });
        assert!(
            matches!(
                resp,
                RpcResponse::TransactionResult {
                    accepted: false,
                    reason: Some(ref r),
                    ..
                } if r.contains("exceeds max allowed amount")
            ),
            "F4.7: TX above TOTAL_SUPPLY must be rejected by the cap, got: {resp:?}"
        );
    }

    /// F4.7: a premine-sized TX (2.5B ZION, the largest genesis slot) is far
    /// below TOTAL_SUPPLY and must NOT be rejected by the cap. F5 is left
    /// disabled here so we isolate the F4.7 behaviour — the TX is accepted.
    #[test]
    fn f4_7_allows_premine_sized_tx() {
        let mut runtime = NodeRuntime::new("node-f47-premine", NodeConfig::mainnet());
        runtime.set_max_tx_amount_height(0);
        // 2.5B ZION in 6-decimal flowers = 2_500_000_000 * 1_000_000.
        let premine_sized: u64 = 2_500_000_000 * emission::FLOWERS_PER_ZION;
        assert!(
            (premine_sized as u128) < emission::TOTAL_SUPPLY,
            "test premise: premine slot must be below TOTAL_SUPPLY"
        );
        let tx = build_valid_account_tx("__f47_premine__", "wallet.dest", premine_sized, 1, 1);
        let resp = runtime.handle_rpc_request(RpcRequest::SubmitTransaction { transaction: tx });
        assert!(
            matches!(resp, RpcResponse::TransactionResult { accepted: true, .. }),
            "F4.7: premine-sized TX below TOTAL_SUPPLY must pass the cap, got: {resp:?}"
        );
    }

    /// F4.7: a TX with amount exactly equal to TOTAL_SUPPLY is on the boundary
    /// (cap rejects only `> TOTAL_SUPPLY`) and must NOT be rejected by the cap.
    #[test]
    fn f4_7_boundary_exactly_total_supply_passes_cap() {
        let mut runtime = NodeRuntime::new("node-f47-boundary", NodeConfig::mainnet());
        runtime.set_max_tx_amount_height(0);
        let exact = emission::TOTAL_SUPPLY as u64;
        let tx = build_valid_account_tx("__f47_boundary__", "wallet.dest", exact, 1, 1);
        let resp = runtime.handle_rpc_request(RpcRequest::SubmitTransaction { transaction: tx });
        assert!(
            matches!(resp, RpcResponse::TransactionResult { accepted: true, .. }),
            "F4.7: TX at exactly TOTAL_SUPPLY must pass the cap (cap is strict >), got: {resp:?}"
        );
    }

    /// F4.7: disabled by default (u64::MAX activation height) — a huge TX is
    /// not rejected by the cap on a runtime that never enabled it. Guarantees
    /// backward compatibility with existing chains/tests.
    #[test]
    fn f4_7_disabled_by_default() {
        let mut runtime = NodeRuntime::new("node-f47-default", NodeConfig::mainnet());
        // Explicitly ensure both gates are off for this instance.
        runtime.set_max_tx_amount_height(u64::MAX);
        runtime.set_balance_check_height(u64::MAX);
        let over = (emission::TOTAL_SUPPLY as u64).saturating_add(1);
        let tx = build_valid_account_tx("__f47_default__", "wallet.dest", over, 1, 1);
        let resp = runtime.handle_rpc_request(RpcRequest::SubmitTransaction { transaction: tx });
        assert!(
            matches!(resp, RpcResponse::TransactionResult { accepted: true, .. }),
            "F4.7: cap disabled by default must accept (backward compat), got: {resp:?}"
        );
    }

    /// Slow: multiple `find_valid_nonce` rounds in debug build. Run via:
    ///   `cargo test --release -- --include-ignored`
    #[test]
    #[ignore = "slow PoW in debug build; run with --release --ignored"]
    fn import_peer_blocks_accepts_contiguous_batch() {
        let mut source = NodeRuntime::new("node-batch-source", NodeConfig::mainnet());
        let first_tx = sample_transaction("tx-batch-1", 3, 1);
        let second_tx = sample_transaction("tx-batch-2", 5, 2);
        let _ = source.handle_rpc_request(RpcRequest::SubmitTransaction {
            transaction: first_tx.clone(),
        });
        let first_template = source.active_template();
        let nonce1 = find_valid_nonce(&first_template);
        let _ = source.handle_rpc_request(RpcRequest::SubmitCandidate {
            template_id: first_template.template_id,
            header_hex: first_template.header_hex,
            nonce: nonce1,
            target_hex: first_template.target_hex,
            algorithm: "deeksha_lite_v1".to_string(),
        });
        let _ = source.handle_rpc_request(RpcRequest::SubmitTransaction {
            transaction: second_tx.clone(),
        });
        let second_template = source.active_template();
        let nonce2 = find_valid_nonce(&second_template);
        let _ = source.handle_rpc_request(RpcRequest::SubmitCandidate {
            template_id: second_template.template_id,
            header_hex: second_template.header_hex,
            nonce: nonce2,
            target_hex: second_template.target_hex,
            algorithm: "deeksha_lite_v1".to_string(),
        });

        let mut target = NodeRuntime::new("node-batch-target", NodeConfig::mainnet());
        let imported = target
            .import_peer_blocks(source.accepted_blocks().to_vec())
            .expect("batch import should succeed");

        assert_eq!(imported, 2); // genesis skipped, 2 new blocks imported
        assert_eq!(target.chain_height(), 2);
        assert_eq!(target.accepted_blocks().len(), 3); // genesis + 2
        assert_eq!(
            target.accepted_blocks()[1].transaction_ids,
            vec![first_tx.tx_id]
        );
        assert_eq!(
            target.accepted_blocks()[2].transaction_ids,
            vec![second_tx.tx_id]
        );
        assert_eq!(target.active_template().height, 3);
    }

    /// Slow: 2× `find_valid_nonce` in debug build.
    /// Run via: `cargo test --release -- --include-ignored`
    #[test]
    #[ignore = "slow PoW in debug build; run with --release --ignored"]
    fn import_peer_blocks_rejects_non_contiguous_batch() {
        let mut source = NodeRuntime::new("node-gap-source", NodeConfig::mainnet());
        let first_template = source.active_template();
        let nonce1 = find_valid_nonce(&first_template);
        let _ = source.handle_rpc_request(RpcRequest::SubmitCandidate {
            template_id: first_template.template_id,
            header_hex: first_template.header_hex,
            nonce: nonce1,
            target_hex: first_template.target_hex,
            algorithm: "deeksha_lite_v1".to_string(),
        });
        let second_template = source.active_template();
        let nonce2 = find_valid_nonce(&second_template);
        let _ = source.handle_rpc_request(RpcRequest::SubmitCandidate {
            template_id: second_template.template_id,
            header_hex: second_template.header_hex,
            nonce: nonce2,
            target_hex: second_template.target_hex,
            algorithm: "deeksha_lite_v1".to_string(),
        });

        let mut target = NodeRuntime::new("node-gap-target", NodeConfig::mainnet());
        let error = target
            .import_peer_blocks(vec![source.accepted_blocks()[2].clone()]) // height 2, skip 1
            .expect_err("non-contiguous batch should fail");

        assert!(error.contains("not contiguous"));
        assert_eq!(target.accepted_blocks().len(), 1); // only genesis
        assert_eq!(target.chain_height(), 0);
    }

    /// Slow: `find_valid_nonce` in debug build (~60s on M1).
    /// Run via: `cargo test --release -- --include-ignored`
    #[test]
    #[ignore = "slow PoW in debug build; run with --release --ignored"]
    fn accepted_block_indexes_are_available_after_submit() {
        let mut runtime = NodeRuntime::new("node-index", NodeConfig::mainnet());
        let template = runtime.active_template();
        let nonce = find_valid_nonce(&template);

        let response = runtime.handle_rpc_request(RpcRequest::SubmitCandidate {
            template_id: template.template_id,
            header_hex: template.header_hex,
            nonce,
            target_hex: template.target_hex,
            algorithm: "deeksha_lite_v1".to_string(),
        });

        assert!(matches!(
            response,
            RpcResponse::SubmitResult { accepted: true, .. }
        ));
        let by_height = runtime
            .accepted_block_by_height(1)
            .expect("accepted block should be indexed by height");
        let by_template = runtime
            .accepted_block_by_template_id(template.template_id)
            .expect("accepted block should be indexed by template id");
        assert_eq!(by_height, by_template);
        assert_eq!(by_height.height, 1);
        assert_eq!(by_height.template_id, template.template_id);
    }

    #[test]
    fn node_runtime_persists_and_restores_chain_state() {
        let state_path = std::env::temp_dir().join(format!(
            "zion-v3-core-state-{}-{}.json",
            std::process::id(),
            now_secs()
        ));
        let mut runtime =
            NodeRuntime::with_chain_store("node-persist", NodeConfig::mainnet(), &state_path)
                .expect("runtime with chain store");
        let persisted_transaction = sample_transaction("tx-persist", 5, 1);
        let _ = runtime.handle_rpc_request(RpcRequest::SubmitTransaction {
            transaction: persisted_transaction.clone(),
        });
        let template = runtime.active_template();
        let nonce = find_valid_nonce(&template);

        let response = runtime.handle_rpc_request(RpcRequest::SubmitCandidate {
            template_id: template.template_id,
            header_hex: template.header_hex,
            nonce,
            target_hex: template.target_hex,
            algorithm: "deeksha_lite_v1".to_string(),
        });
        assert!(matches!(
            response,
            RpcResponse::SubmitResult { accepted: true, .. }
        ));

        let mut restored =
            NodeRuntime::with_chain_store("node-persist", NodeConfig::mainnet(), &state_path)
                .expect("restored runtime with chain store");

        assert_eq!(restored.status().chain_height, 1);
        assert_eq!(restored.accepted_blocks().len(), 2); // genesis + 1
        assert!(restored.accepted_block_by_height(1).is_some());
        assert!(restored
            .accepted_block_by_template_id(template.template_id)
            .is_some());
        assert_eq!(
            restored.accepted_blocks()[1].transaction_ids,
            vec![persisted_transaction.tx_id]
        );
        assert!(matches!(
            restored.handle_rpc_request(RpcRequest::GetMempool),
            RpcResponse::Mempool { ref transactions } if transactions.is_empty()
        ));

        fs::remove_file(&state_path).ok();
    }

    #[test]
    fn restored_state_sanitizes_duplicate_and_mined_mempool_entries() {
        let state_path = std::env::temp_dir().join(format!(
            "zion-v3-core-recovery-{}-{}.json",
            std::process::id(),
            now_secs()
        ));
        let tx_dup = sample_transaction("tx-dup", 6, 1);
        let tx_mined = sample_transaction("tx-mined", 2, 2);
        let snapshot = ChainStateSnapshot {
            height: 1,
            tip_hash_hex: hex(&[0x44; 32]),
            next_template_id: 3,
            active_template: BlockTemplate {
                template_id: 2,
                height: 2,
                header_hex: hex(&sample_header().to_bytes()),
                target_hex: DifficultyTarget::MAX.to_hex(),
                reward_zion: emission::block_subsidy(2),
                transaction_ids: vec![tx_dup.tx_id.clone(), tx_mined.tx_id.clone()],
                transaction_count: 2,
                total_fees_zion: 8,
                body_hash_hex: body_hash_hex(&[tx_dup.clone(), tx_mined.clone()]),
                estimated_miner_reward_zion: emission::block_subsidy(2),
                utxo_transaction_ids: vec![],
                utxo_transaction_count: 0,
                total_utxo_fees: 0,
            },
            accepted_blocks: vec![AcceptedBlock {
                template_id: 1,
                height: 1,
                timestamp: 1_700_000_000,
                difficulty: difficulty::GENESIS_DIFFICULTY,
                nonce: 77,
                hash_hex: hex(&[0x55; 32]),
                header_hex: String::new(),
                previous_hash_hex: String::new(),
                algorithm: "deeksha_lite_v1".to_string(),
                transaction_ids: vec![tx_mined.tx_id.clone()],
                transactions: vec![tx_mined.clone()],
                total_fees_zion: 2,
                body_hash_hex: body_hash_hex(std::slice::from_ref(&tx_mined)),
                subsidy_zion: emission::block_subsidy(1),
                miner_reward_zion: emission::block_subsidy(1),
                miner_address: String::new(),
                humanitarian_address: String::new(),
                issobella_address: String::new(),
                pool_fee_address: String::new(),
                utxo_transaction_ids: vec![],
                utxo_transactions: vec![],
            }],
            mempool: vec![tx_dup.clone(), tx_dup.clone(), tx_mined.clone()],
            utxo_mempool: vec![],
            bridge_unlock_replay_keys: vec![],
        };
        fs::write(
            &state_path,
            serde_json::to_string_pretty(&snapshot).expect("encode recovery snapshot"),
        )
        .expect("write recovery snapshot");

        let restored =
            NodeRuntime::with_chain_store("node-recovery", NodeConfig::mainnet(), &state_path)
                .expect("restored runtime with sanitized state");

        let BlockTemplate {
            transaction_ids,
            transaction_count,
            ..
        } = restored.active_template();
        assert_eq!(transaction_ids, vec![tx_dup.tx_id]);
        assert_eq!(transaction_count, 1);

        fs::remove_file(&state_path).ok();
    }

    #[test]
    fn runtime_recovers_from_journal_when_snapshot_is_missing() {
        let state_path = std::env::temp_dir().join(format!(
            "zion-v3-core-journal-{}-{}.json",
            std::process::id(),
            now_secs()
        ));
        let tx = sample_transaction("tx-journal", 6, 1);
        let accepted_block = AcceptedBlock {
            template_id: 1,
            height: 1,
            timestamp: 1_700_000_060,
            difficulty: difficulty::GENESIS_DIFFICULTY,
            nonce: 42,
            hash_hex: hex(&[0x66; 32]),
            header_hex: String::new(),
            previous_hash_hex: String::new(),
            algorithm: "deeksha_lite_v1".to_string(),
            transaction_ids: vec![tx.tx_id.clone()],
            transactions: vec![tx.clone()],
            total_fees_zion: 6,
            body_hash_hex: body_hash_hex(std::slice::from_ref(&tx)),
            subsidy_zion: emission::block_subsidy(1),
            miner_reward_zion: emission::block_subsidy(1),
            miner_address: String::new(),
            humanitarian_address: String::new(),
            issobella_address: String::new(),
            pool_fee_address: String::new(),
            utxo_transaction_ids: vec![],
            utxo_transactions: vec![],
        };
        let journal = [
            ChainJournalEntry::TransactionAccepted {
                transaction: RuntimeTransaction::from(tx.clone()),
            },
            ChainJournalEntry::BlockAccepted {
                block: accepted_block.clone(),
            },
        ];
        let journal_body = journal
            .iter()
            .map(|entry| encode_json_line(entry).expect("encode journal entry"))
            .collect::<String>();
        fs::write(journal_path(&state_path), journal_body).expect("write journal file");

        let restored =
            NodeRuntime::with_chain_store("node-journal", NodeConfig::mainnet(), &state_path)
                .expect("restore from journal");

        assert_eq!(restored.status().chain_height, 1);
        assert_eq!(restored.accepted_blocks().len(), 2); // genesis + journal block
        assert_eq!(restored.accepted_blocks()[1], accepted_block);
        assert!(restored.active_template().transaction_ids.is_empty());
        assert!(!journal_path(&state_path).exists());

        fs::remove_file(&state_path).ok();
    }

    #[test]
    fn peer_persistence_round_trip() {
        let dir = tempfile::tempdir().expect("tempdir");
        let state_path = dir.path().join("state.json");
        let config = test_config_without_seed_allowlist();

        // Create runtime with chain store, add some peers
        let mut runtime = NodeRuntime::with_chain_store("node-peers", config.clone(), &state_path)
            .expect("create runtime");

        // Register new peers beyond the seeds
        runtime.register_peer(PeerEndpoint::new("10.0.0.1", 8333));
        runtime.register_peer(PeerEndpoint::new("10.0.0.2", 8333));
        runtime.register_peer(PeerEndpoint::new("10.0.0.3", 8333));

        let saved_count = runtime.known_peers().len();
        runtime.persist_peers().expect("persist peers");

        // Verify peers.json was created
        let peers_path = dir.path().join("peers.json");
        assert!(peers_path.exists(), "peers.json should exist");

        // Create a new runtime from the same state path — peers should be loaded
        let restored = NodeRuntime::with_chain_store("node-peers-2", config, &state_path)
            .expect("restore runtime");

        assert_eq!(restored.known_peers().len(), saved_count);
        assert!(
            restored
                .known_peers()
                .iter()
                .any(|p| p.address() == "10.0.0.1:8333"),
            "should contain persisted peer 10.0.0.1"
        );
        assert!(
            restored
                .known_peers()
                .iter()
                .any(|p| p.address() == "10.0.0.3:8333"),
            "should contain persisted peer 10.0.0.3"
        );
    }

    #[test]
    fn peer_persistence_no_state_path_is_noop() {
        let runtime = NodeRuntime::new("node-no-store", NodeConfig::mainnet());
        // Should succeed (no-op) without state path
        runtime.persist_peers().expect("persist should be no-op");
    }

    fn test_config_without_seed_allowlist() -> NodeConfig {
        NodeConfig {
            network: NetworkId::Devnet,
            p2p_bind: PeerEndpoint::new("0.0.0.0", 8333),
            rpc_bind: PeerEndpoint::new("127.0.0.1", 8443),
            pool_bind: PeerEndpoint::new("0.0.0.0", 8444),
            websocket_bind: PeerEndpoint::new("127.0.0.1", 8445),
            seed_peers: Vec::new(),
        }
    }

    #[test]
    fn register_peers_deduplicates() {
        let mut runtime = NodeRuntime::new("node-dedup", test_config_without_seed_allowlist());
        let before = runtime.known_peers().len();

        runtime.register_peer(PeerEndpoint::new("192.168.1.1", 8333));
        assert_eq!(runtime.known_peers().len(), before + 1);

        // Duplicate should be ignored
        runtime.register_peer(PeerEndpoint::new("192.168.1.1", 8333));
        assert_eq!(runtime.known_peers().len(), before + 1);

        // Different port = different peer
        runtime.register_peer(PeerEndpoint::new("192.168.1.1", 9999));
        assert_eq!(runtime.known_peers().len(), before + 2);
    }

    #[test]
    fn get_peers_returns_known_list() {
        let mut runtime = NodeRuntime::new("node-getpeers", test_config_without_seed_allowlist());
        runtime.register_peer(PeerEndpoint::new("10.1.1.1", 8333));
        runtime.register_peer(PeerEndpoint::new("10.1.1.2", 8333));

        let response = runtime.handle_p2p_message(P2pMessage::GetPeers).unwrap();
        match response {
            P2pMessage::Peers { peers } => {
                assert!(peers.iter().any(|p| p.address() == "10.1.1.1:8333"));
                assert!(peers.iter().any(|p| p.address() == "10.1.1.2:8333"));
            }
            other => panic!("expected Peers, got {other:?}"),
        }
    }

    #[test]
    fn mainnet_runtime_rejects_unknown_peers_outside_seed_list() {
        let mut runtime = NodeRuntime::new("node-mainnet", NodeConfig::mainnet());
        let before = runtime.known_peers().len();

        runtime.register_peer(PeerEndpoint::new("157.180.41.213", 8333));

        assert_eq!(runtime.known_peers().len(), before);
        assert!(runtime
            .known_peers()
            .iter()
            .all(|peer| peer.address() != "157.180.41.213:8333"));
    }

    // ── Phase 12: Block Validation Hardening tests ─────────────────────

    /// Helper: mine a block on `source` and return its accepted blocks.
    fn mine_one_block(runtime: &mut NodeRuntime) {
        let template = runtime.active_template();
        let nonce = find_valid_nonce(&template);
        let response = runtime.handle_rpc_request(RpcRequest::SubmitCandidate {
            template_id: template.template_id,
            header_hex: template.header_hex.clone(),
            nonce,
            target_hex: template.target_hex.clone(),
            algorithm: "deeksha_lite_v1".to_string(),
        });
        assert!(
            matches!(response, RpcResponse::SubmitResult { accepted: true, .. }),
            "unexpected submit response: {response:?}"
        );
    }

    fn build_peer_block_with_header_previous_hash_mismatch(runtime: &NodeRuntime) -> AcceptedBlock {
        let template = runtime.chain_state.active_template.clone();
        let transactions = template.account_transactions();
        let mut header = template.header;
        header.previous_hash = [0xEE; 32];
        let body_hash_hex = body_hash_hex(&transactions);

        AcceptedBlock {
            template_id: template.template_id,
            height: template.height,
            timestamp: header.timestamp,
            difficulty: template.difficulty,
            nonce: 0,
            hash_hex: hex(&[0x11; 32]),
            header_hex: hex(&header.to_bytes()),
            previous_hash_hex: hex(&runtime.chain_state.tip_hash),
            algorithm: "deeksha_lite_v1".to_string(),
            transaction_ids: transactions
                .iter()
                .map(|transaction| transaction.tx_id.clone())
                .collect(),
            transactions,
            total_fees_zion: template.total_fees_zion,
            body_hash_hex,
            subsidy_zion: template.reward_zion,
            miner_reward_zion: template.reward_zion,
            miner_address: runtime.miner_address.clone(),
            humanitarian_address: runtime.humanitarian_address.clone(),
            issobella_address: runtime.issobella_address.clone(),
            pool_fee_address: runtime.pool_fee_address.clone(),
            utxo_transaction_ids: vec![],
            utxo_transactions: vec![],
        }
    }

    #[test]
    fn peer_block_has_header_hex_after_mining() {
        let mut runtime = NodeRuntime::new("node-hdr", NodeConfig::mainnet());
        mine_one_block(&mut runtime);
        let block = &runtime.accepted_blocks()[1];
        assert!(
            !block.header_hex.is_empty(),
            "mined block must have header_hex"
        );
        assert_eq!(block.header_hex.len(), HEADER_SIZE * 2); // 80 bytes = 160 hex chars
    }

    #[test]
    fn peer_import_verifies_pow_via_header_hex() {
        let mut source = NodeRuntime::new("node-pow-src", NodeConfig::mainnet());
        mine_one_block(&mut source);

        // Import with valid header_hex should succeed
        let mut target = NodeRuntime::new("node-pow-tgt", NodeConfig::mainnet());
        let imported = target
            .import_peer_blocks(source.accepted_blocks().to_vec())
            .expect("import with valid PoW should succeed");
        assert_eq!(imported, 1);
    }

    #[test]
    fn peer_import_rejects_bad_pow_hash() {
        let mut source = NodeRuntime::new("node-badpow-src", NodeConfig::mainnet());
        mine_one_block(&mut source);

        let mut block = source.accepted_blocks()[1].clone();
        // Tamper with hash_hex while keeping header_hex intact
        block.hash_hex = hex(&[0xAA; 32]);

        let mut target = NodeRuntime::new("node-badpow-tgt", NodeConfig::mainnet());
        let err = target
            .import_peer_blocks(vec![block])
            .expect_err("tampered hash should be rejected");
        assert!(
            err.contains("PoW computation"),
            "expected PoW computation error, got: {err}"
        );
    }

    #[test]
    fn peer_import_rejects_bad_header_timestamp() {
        let mut source = NodeRuntime::new("node-badhdr-src", NodeConfig::mainnet());
        mine_one_block(&mut source);

        let mut block = source.accepted_blocks()[1].clone();
        // Tamper with header timestamp field (make it inconsistent with block.timestamp)
        let mut header_bytes =
            parse_fixed_hex::<HEADER_SIZE>(&block.header_hex, "test header").unwrap();
        // Overwrite timestamp bytes (offset 68..76) with a different value
        header_bytes[68..76].copy_from_slice(&(block.timestamp + 999).to_le_bytes());
        block.header_hex = hex(&header_bytes);

        let mut target = NodeRuntime::new("node-badhdr-tgt", NodeConfig::mainnet());
        let err = target
            .import_peer_blocks(vec![block])
            .expect_err("header with wrong timestamp should be rejected");
        assert!(
            err.contains("header timestamp"),
            "expected header timestamp error, got: {err}"
        );
    }

    #[test]
    fn peer_import_rejects_future_timestamp() {
        let mut source = NodeRuntime::new("node-future-src", NodeConfig::mainnet());
        mine_one_block(&mut source);

        let mut block = source.accepted_blocks()[1].clone();
        let far_future = now_secs() + validation::MAX_TIMESTAMP_DRIFT + 3_600;
        block.timestamp = far_future;
        // Rebuild header with the far-future timestamp so header consistency passes
        let mut header_bytes =
            parse_fixed_hex::<HEADER_SIZE>(&block.header_hex, "test header").unwrap();
        header_bytes[68..76].copy_from_slice(&far_future.to_le_bytes());
        let header = MiningHeader::from_bytes(header_bytes);
        // Re-mine to get a valid hash for the tampered header
        let target_val = difficulty::difficulty_to_target(block.difficulty);
        let mut found = false;
        for nonce in 0..10_000_000u64 {
            let candidate = BlockCandidate {
                header,
                nonce,
                height: block.height,
            };
            let h = candidate.hash();
            if target_val.allows(&h) {
                block.nonce = nonce;
                block.hash_hex = hex(&h);
                block.header_hex = hex(&header.to_bytes());
                found = true;
                break;
            }
        }
        assert!(found, "should find valid nonce for tampered header");

        let mut target = NodeRuntime::new("node-future-tgt", NodeConfig::mainnet());
        let err = target
            .import_peer_blocks(vec![block])
            .expect_err("far-future timestamp should be rejected");
        assert!(
            err.contains("timestamp"),
            "expected timestamp error, got: {err}"
        );
    }

    #[test]
    fn peer_import_rejects_wrong_subsidy() {
        let mut source = NodeRuntime::new("node-subsidy-src", NodeConfig::mainnet());
        mine_one_block(&mut source);

        let mut block = source.accepted_blocks()[1].clone();
        block.subsidy_zion += 1; // inflate subsidy by 1

        let mut target = NodeRuntime::new("node-subsidy-tgt", NodeConfig::mainnet());
        let err = target
            .import_peer_blocks(vec![block])
            .expect_err("wrong subsidy should be rejected");
        assert!(
            err.contains("subsidy") || err.contains("reward"),
            "expected subsidy error, got: {err}"
        );
    }

    #[test]
    fn genesis_block_has_header_hex() {
        let genesis = genesis::genesis_block();
        assert!(
            !genesis.header_hex.is_empty(),
            "genesis must have header_hex"
        );
        assert_eq!(genesis.header_hex.len(), HEADER_SIZE * 2);
    }

    #[test]
    fn peer_import_legacy_blocks_without_header_hex_still_accepted() {
        let mut source = NodeRuntime::new("node-legacy-src", NodeConfig::mainnet());
        mine_one_block(&mut source);

        let mut block = source.accepted_blocks()[1].clone();
        // Simulate legacy block without header_hex
        block.header_hex = String::new();

        let mut target = NodeRuntime::new("node-legacy-tgt", NodeConfig::mainnet());
        let imported = target
            .import_peer_blocks(vec![block])
            .expect("legacy block without header_hex should still be accepted");
        assert_eq!(imported, 1);
    }

    #[test]
    fn checkpoint_violation_rejects_peer_block() {
        // This test verifies the checkpoint check is wired in by importing
        // genesis with a wrong hash.
        let mut target = NodeRuntime::new("node-cp", NodeConfig::mainnet());
        let mut bad_genesis = genesis::genesis_block();
        bad_genesis.hash_hex = hex(&[0xBB; 32]);

        let err = target
            .import_peer_blocks(vec![bad_genesis])
            .expect_err("checkpoint violation should be rejected");
        assert!(
            err.contains("does not match canonical genesis")
                || err.contains("checkpoint")
                || err.contains("conflicting peer block at height 0"),
            "expected checkpoint, genesis hash, or conflict error, got: {err}"
        );
    }

    // ── Phase 13: Chain Linkage Verification tests ─────────────────────

    #[test]
    fn mined_block_has_previous_hash_hex() {
        let mut runtime = NodeRuntime::new("node-prevh", NodeConfig::mainnet());
        mine_one_block(&mut runtime);
        let block = &runtime.accepted_blocks()[1]; // height 1
        assert!(
            !block.previous_hash_hex.is_empty(),
            "mined block must have previous_hash_hex"
        );
        // previous_hash should be genesis hash
        let genesis = genesis::genesis_block();
        assert_eq!(block.previous_hash_hex, genesis.hash_hex);
    }

    #[test]
    fn genesis_block_has_zero_previous_hash() {
        let genesis = genesis::genesis_block();
        assert_eq!(genesis.previous_hash_hex, hex(&[0u8; 32]));
    }

    /// Slow: mines two blocks via `find_valid_nonce` (Cosmic Harmony Ekam Deeksha v2,
    /// 256 KiB scratchpad) in debug build. Run via:
    ///   `cargo test --manifest-path V3/Cargo.toml -p zion-core --release -- --ignored`
    /// or `cargo test --release -- --include-ignored`.
    #[test]
    #[ignore = "slow PoW in debug build; run with --release --ignored"]
    fn peer_import_verifies_chain_linkage() {
        let mut source = NodeRuntime::new("node-link-src", NodeConfig::mainnet());
        mine_one_block(&mut source);
        mine_one_block(&mut source);

        // Valid import: blocks link correctly genesis → h1 → h2
        let mut target = NodeRuntime::new("node-link-tgt", NodeConfig::mainnet());
        let imported = target
            .import_peer_blocks(source.accepted_blocks().to_vec())
            .expect("valid chain linkage should succeed");
        assert_eq!(imported, 2);
        assert_eq!(target.chain_height(), 2);
    }

    #[test]
    fn peer_import_rejects_broken_chain_linkage() {
        let mut source = NodeRuntime::new("node-break-src", NodeConfig::mainnet());
        mine_one_block(&mut source);

        let mut block = source.accepted_blocks()[1].clone();
        // Tamper with previous_hash_hex — make it point to wrong parent
        block.previous_hash_hex = hex(&[0xDD; 32]);

        let mut target = NodeRuntime::new("node-break-tgt", NodeConfig::mainnet());
        let err = target
            .import_peer_blocks(vec![block])
            .expect_err("broken chain linkage should be rejected");
        assert!(
            err.contains("does not link to") || err.contains("previous_hash"),
            "expected chain linkage error, got: {err}"
        );
    }

    #[test]
    fn peer_import_rejects_mismatched_previous_hash_in_header() {
        let source = NodeRuntime::new("node-hdr-mismatch-src", NodeConfig::mainnet());
        let block = build_peer_block_with_header_previous_hash_mismatch(&source);

        let mut target = NodeRuntime::new("node-hdr-mismatch-tgt", NodeConfig::mainnet());
        let err = target
            .import_peer_blocks(vec![block])
            .expect_err("header/previous_hash mismatch should be rejected");
        assert!(
            err.contains("previous_hash"),
            "expected previous_hash error, got: {err}"
        );
    }

    /// Slow: mines two blocks via `find_valid_nonce` (Cosmic Harmony Ekam Deeksha v2,
    /// 256 KiB scratchpad) in debug build. Run via:
    ///   `cargo test --manifest-path V3/Cargo.toml -p zion-core --release -- --ignored`
    /// or `cargo test --release -- --include-ignored`.
    #[test]
    #[ignore = "slow PoW in debug build; run with --release --ignored"]
    fn batch_import_verifies_intra_batch_chain_linkage() {
        let mut source = NodeRuntime::new("node-batch-link-src", NodeConfig::mainnet());
        mine_one_block(&mut source);
        mine_one_block(&mut source);

        let mut blocks = source.accepted_blocks().to_vec();
        // Tamper with block at height 2: make its previous_hash_hex point to wrong block
        blocks[2].previous_hash_hex = hex(&[0xCC; 32]);

        let mut target = NodeRuntime::new("node-batch-link-tgt", NodeConfig::mainnet());
        let err = target
            .import_peer_blocks(blocks)
            .expect_err("intra-batch broken linkage should be rejected");
        assert!(
            err.contains("does not link to") || err.contains("previous_hash"),
            "expected chain linkage error, got: {err}"
        );
    }

    #[test]
    fn peer_import_previous_hash_consistent_between_header_and_field() {
        let mut source = NodeRuntime::new("node-consist-src", NodeConfig::mainnet());
        mine_one_block(&mut source);
        let block = &source.accepted_blocks()[1];

        // Extract previous_hash from header_hex
        let header_bytes =
            parse_fixed_hex::<HEADER_SIZE>(&block.header_hex, "test header").unwrap();
        let header = MiningHeader::from_bytes(header_bytes);
        let header_prev = hex(&header.previous_hash);

        // Both should match
        assert_eq!(block.previous_hash_hex, header_prev);
    }

    #[test]
    fn legacy_block_without_previous_hash_still_accepted() {
        let mut source = NodeRuntime::new("node-legacy-prev-src", NodeConfig::mainnet());
        mine_one_block(&mut source);

        let mut block = source.accepted_blocks()[1].clone();
        // Simulate legacy block: no previous_hash_hex, no header_hex
        block.previous_hash_hex = String::new();
        block.header_hex = String::new();

        let mut target = NodeRuntime::new("node-legacy-prev-tgt", NodeConfig::mainnet());
        let imported = target
            .import_peer_blocks(vec![block])
            .expect("legacy block without previous_hash should still be accepted");
        assert_eq!(imported, 1);
    }

    // ── Phase 14: Coinbase transaction tests ──────────────────────────

    #[test]
    fn template_without_miner_address_has_no_coinbase() {
        let runtime = NodeRuntime::new("node-no-cb", NodeConfig::mainnet());
        let template = runtime.active_template();
        // With no miner_address configured, template should have no transactions
        assert!(
            template.transaction_ids.is_empty(),
            "template without miner_address must have no coinbase tx"
        );
    }

    #[test]
    fn template_with_miner_address_has_coinbase_tx() {
        let mut runtime = NodeRuntime::new("node-cb", NodeConfig::mainnet());
        runtime.set_miner_address("test-miner-wallet".to_string());
        let template = runtime.active_template();

        // Template should have exactly one transaction: the coinbase
        assert_eq!(
            template.transaction_count, 1,
            "template should have coinbase tx"
        );
        assert_eq!(template.transaction_ids.len(), 1);
    }

    /// Slow: `mine_one_block` in debug build (~140s on M1).
    /// Run via: `cargo test --release -- --include-ignored`
    #[test]
    #[ignore = "slow PoW in debug build; run with --release --ignored"]
    fn coinbase_tx_credits_correct_address_and_amount() {
        let mut runtime = NodeRuntime::new("node-cb-addr", NodeConfig::mainnet());
        runtime.set_miner_address("alice-wallet".to_string());

        // The active template is for height 1 (genesis is height 0).
        let height = runtime.active_template().height;
        assert_eq!(height, 1);

        // Mine a block to accept it.
        mine_one_block(&mut runtime);

        let block = &runtime.accepted_blocks()[1]; // genesis=0, mined=1
        assert_eq!(block.height, 1);
        assert_eq!(block.miner_address, "alice-wallet");

        // First transaction should be the coinbase.
        let coinbase = &block.transactions[0];
        assert_eq!(coinbase.from, "coinbase");
        assert_eq!(coinbase.to, "alice-wallet");
        assert_eq!(coinbase.fee_zion, 0);
        assert_eq!(coinbase.nonce, 1); // nonce = height

        // Amount = subsidy + fees (no user txs, so just subsidy).
        let expected_subsidy = emission::block_subsidy(1);
        assert_eq!(coinbase.amount_zion, expected_subsidy as u128);
        assert_eq!(block.miner_reward_zion, expected_subsidy);
    }

    #[test]
    fn template_with_fee_split_has_three_coinbase_outputs() {
        let mut runtime = NodeRuntime::new("node-cb-split-template", NodeConfig::mainnet());
        runtime.set_miner_address("alice-wallet".to_string());
        runtime.set_fee_addresses(
            "human-wallet".to_string(),
            "issobella-wallet".to_string(),
            // pool-fee slot is burned — address is ignored.
            String::new(),
        );

        let template = runtime.active_template();
        let expected_subsidy = emission::block_subsidy(template.height);
        let (miner_amount, humanitarian_amount, issobella_amount, _burned_pool_fee) =
            emission::fee_split(expected_subsidy);
        let transactions = runtime.chain_state.active_template.account_transactions();

        // Only 3 coinbase outputs (miner/humanitarian/issobella); the 1% pool
        // fee is burned and produces no output.
        assert_eq!(template.transaction_count, 3);
        assert_eq!(transactions.len(), 3);
        assert_eq!(template.estimated_miner_reward_zion, miner_amount);

        assert_eq!(transactions[0].from, "coinbase");
        assert_eq!(transactions[0].to, "alice-wallet");
        assert_eq!(transactions[0].amount_zion, u128::from(miner_amount));

        assert_eq!(transactions[1].from, "coinbase");
        assert_eq!(transactions[1].to, "human-wallet");
        assert_eq!(transactions[1].amount_zion, u128::from(humanitarian_amount));

        assert_eq!(transactions[2].from, "coinbase");
        assert_eq!(transactions[2].to, "issobella-wallet");
        assert_eq!(transactions[2].amount_zion, u128::from(issobella_amount));

        // Minted total = 99% of subsidy; the 1% pool fee is burned.
        let minted: u128 = transactions.iter().map(|t| t.amount_zion).sum();
        assert_eq!(
            minted,
            u128::from(emission::minted_subsidy(expected_subsidy))
        );
    }

    #[test]
    fn mined_block_with_fee_split_has_three_coinbase_outputs() {
        let mut source = NodeRuntime::new("node-cb-split-src", NodeConfig::mainnet());
        source.set_miner_address("alice-wallet".to_string());
        source.set_fee_addresses(
            "human-wallet".to_string(),
            "issobella-wallet".to_string(),
            String::new(),
        );

        let template = source.chain_state.active_template.clone();
        let transactions = template.account_transactions();
        let miner_reward_zion = u64::try_from(transactions[0].amount_zion)
            .expect("miner coinbase amount must fit u64 for block metadata");
        let block = AcceptedBlock {
            template_id: template.template_id,
            height: template.height,
            timestamp: template.header.timestamp,
            difficulty: template.difficulty,
            nonce: 0,
            hash_hex: hex(&[0x11; 32]),
            header_hex: String::new(),
            previous_hash_hex: hex(&source.chain_state.tip_hash),
            algorithm: "deeksha_lite_v1".to_string(),
            transaction_ids: transactions
                .iter()
                .map(|transaction| transaction.tx_id.clone())
                .collect(),
            transactions: transactions.clone(),
            total_fees_zion: template.total_fees_zion,
            body_hash_hex: body_hash_hex(&transactions),
            subsidy_zion: template.reward_zion,
            miner_reward_zion,
            miner_address: source.miner_address.clone(),
            humanitarian_address: source.humanitarian_address.clone(),
            issobella_address: source.issobella_address.clone(),
            pool_fee_address: source.pool_fee_address.clone(),
            utxo_transaction_ids: Vec::new(),
            utxo_transactions: Vec::new(),
        };

        let mut target = NodeRuntime::new("node-cb-split-target", NodeConfig::mainnet());
        target
            .import_peer_blocks(vec![block])
            .expect("split coinbase peer block should validate and import");

        let block = &target.accepted_blocks()[1];
        let (miner_amount, humanitarian_amount, issobella_amount, _burned_pool_fee) =
            emission::fee_split(block.subsidy_zion);

        assert_eq!(block.miner_address, "alice-wallet");
        assert_eq!(block.humanitarian_address, "human-wallet");
        assert_eq!(block.issobella_address, "issobella-wallet");
        // pool_fee slot is burned — no address, no output.
        assert_eq!(block.transactions.len(), 3);
        assert_eq!(block.miner_reward_zion, miner_amount);

        assert_eq!(block.transactions[0].to, "alice-wallet");
        assert_eq!(block.transactions[0].amount_zion, u128::from(miner_amount));
        assert_eq!(block.transactions[1].to, "human-wallet");
        assert_eq!(
            block.transactions[1].amount_zion,
            u128::from(humanitarian_amount)
        );
        assert_eq!(block.transactions[2].to, "issobella-wallet");
        assert_eq!(
            block.transactions[2].amount_zion,
            u128::from(issobella_amount)
        );

        // Minted total = 99% of subsidy; the 1% pool fee is burned.
        assert_eq!(
            block
                .transactions
                .iter()
                .map(|transaction| transaction.amount_zion)
                .sum::<u128>(),
            u128::from(emission::minted_subsidy(block.subsidy_zion))
        );
    }

    #[test]
    fn submit_candidate_rejects_locally_invalid_coinbase() {
        let mut runtime = NodeRuntime::new("node-local-validate", NodeConfig::mainnet());
        runtime.set_miner_address("local-wallet".to_string());
        runtime.chain_state.active_template.transactions[0]
            .as_account_mut()
            .expect("coinbase must stay account-based in current runtime")
            .amount_zion += 1;

        let template = runtime.active_template();
        let nonce = find_valid_nonce(&template);
        let response = runtime.handle_rpc_request(RpcRequest::SubmitCandidate {
            template_id: template.template_id,
            header_hex: template.header_hex,
            nonce,
            target_hex: template.target_hex,
            algorithm: "deeksha_lite_v1".to_string(),
        });

        assert!(
            matches!(
                response,
                RpcResponse::SubmitResult {
                    accepted: false,
                    reason: Some(ref reason),
                    ..
                } if reason.contains("failed validation") && reason.contains("coinbase amount")
            ),
            "unexpected submit response: {response:?}"
        );
    }

    #[test]
    fn coinbase_tx_id_is_deterministic() {
        let mut r1 = NodeRuntime::new("node-det-1", NodeConfig::mainnet());
        r1.set_miner_address("det-wallet".to_string());

        let mut r2 = NodeRuntime::new("node-det-2", NodeConfig::mainnet());
        r2.set_miner_address("det-wallet".to_string());

        // Both nodes at same height with same miner_address should produce
        // the same coinbase tx_id (deterministic from height + address).
        let t1 = r1.active_template();
        let t2 = r2.active_template();
        assert_eq!(t1.transaction_ids[0], t2.transaction_ids[0]);
    }

    #[test]
    fn coinbase_tx_id_differs_for_different_addresses() {
        let mut r1 = NodeRuntime::new("node-diff-1", NodeConfig::mainnet());
        r1.set_miner_address("wallet-a".to_string());

        let mut r2 = NodeRuntime::new("node-diff-2", NodeConfig::mainnet());
        r2.set_miner_address("wallet-b".to_string());

        let t1 = r1.active_template();
        let t2 = r2.active_template();
        assert_ne!(
            t1.transaction_ids[0], t2.transaction_ids[0],
            "different miner addresses must produce different coinbase tx_ids"
        );
    }

    #[test]
    fn mined_block_with_coinbase_flows_to_next_template() {
        let mut runtime = NodeRuntime::new("node-cb-flow", NodeConfig::mainnet());
        runtime.set_miner_address("flow-wallet".to_string());

        // Mine block 1.
        mine_one_block(&mut runtime);
        assert_eq!(runtime.accepted_blocks().len(), 2); // genesis + 1

        // Block 1 should have coinbase.
        let b1 = &runtime.accepted_blocks()[1];
        assert_eq!(b1.transactions[0].from, "coinbase");
        assert_eq!(b1.transactions[0].to, "flow-wallet");

        // Next template (height 2) should also have coinbase for height 2.
        let t2 = runtime.active_template();
        assert_eq!(t2.height, 2);
        assert_eq!(t2.transaction_count, 1); // coinbase only
                                             // Coinbase nonce should be the height.
                                             // We can verify by checking the tx_id is different from block 1's coinbase.
        assert_ne!(t2.transaction_ids[0], b1.transactions[0].tx_id);
    }

    #[test]
    fn genesis_block_has_no_coinbase() {
        let runtime = NodeRuntime::new("node-genesis-cb", NodeConfig::mainnet());
        let genesis = &runtime.accepted_blocks()[0];
        assert_eq!(genesis.height, 0);
        assert!(genesis.miner_address.is_empty());
        // Genesis should have premine transactions, none from "coinbase".
        assert!(!genesis.transactions.iter().any(|tx| tx.from == "coinbase"));
    }

    #[test]
    fn set_miner_address_rebuilds_active_template() {
        let mut runtime = NodeRuntime::new("node-rebuild", NodeConfig::mainnet());
        assert!(runtime.active_template().transaction_ids.is_empty());

        runtime.set_miner_address("rebuild-wallet".to_string());
        assert_eq!(runtime.miner_address(), "rebuild-wallet");
        assert_eq!(runtime.active_template().transaction_count, 1);
        assert_eq!(runtime.active_template().transaction_ids.len(), 1);
    }

    // ── Phase 16+17: UTXO bridge acceptance tests ──────────────────────

    /// Create a "funding" UTXO transaction (coinbase-like, no real inputs)
    /// and mine it into a block so subsequent tests can spend its outputs.
    /// Returns (funding_tx_id, address, verifying_key_bytes, signing_key).
    fn seed_utxo_funding(
        runtime: &mut NodeRuntime,
        amount: u64,
    ) -> ([u8; 32], String, Vec<u8>, ed25519_dalek::SigningKey) {
        let (sk, vk) = crypto::generate_keypair();
        let addr = crypto::derive_address(vk.as_bytes());
        let vk_bytes = vk.as_bytes().to_vec();

        let funding = build_synthetic_funding_tx(amount, &addr);
        inject_funding_into_genesis(runtime, &funding);

        (funding.id, addr, vk_bytes, sk)
    }

    /// Build a coinbase-like funding tx (empty inputs → the UTXO set builder
    /// treats outputs as new UTXOs regardless of inputs). Pure constructor so
    /// that both source and target test runtimes can be seeded with the
    /// **same** funding UTXO when peer-import flows are exercised.
    fn build_synthetic_funding_tx(amount: u64, address: &str) -> tx::Transaction {
        let mut funding = tx::Transaction {
            id: [0u8; 32],
            version: tx::TX_HASH_V2_VERSION,
            inputs: vec![],
            outputs: vec![tx::TxOutput {
                amount,
                address: address.to_string(),
                memo: None,
            }],
            fee: 0,
            timestamp: now_secs(),
        };
        funding.finalize_id();
        funding
    }

    /// Inject a synthetic funding tx into the runtime's genesis block so the
    /// UTXO set sees it. Idempotent: re-seeding the same tx is a no-op.
    fn inject_funding_into_genesis(runtime: &mut NodeRuntime, funding: &tx::Transaction) {
        let block = &mut runtime.chain_state.accepted_blocks[0];
        let id_hex = hex(&funding.id);
        if block
            .utxo_transaction_ids
            .iter()
            .any(|existing| existing == &id_hex)
        {
            return;
        }
        block.utxo_transactions.push(funding.clone());
        block.utxo_transaction_ids.push(id_hex);
    }

    fn make_signed_utxo_tx_spending(
        prev_hash: [u8; 32],
        output_index: u32,
        amount: u64,
        sk: &ed25519_dalek::SigningKey,
        vk_bytes: &[u8],
        dest_address: &str,
    ) -> tx::Transaction {
        let fee = 1_000u64;
        let mut utxo = tx::Transaction {
            id: [0u8; 32],
            version: tx::TX_HASH_V2_VERSION,
            inputs: vec![tx::TxInput {
                prev_tx_hash: prev_hash,
                output_index,
                signature: vec![],
                public_key: vk_bytes.to_vec(),
            }],
            outputs: vec![tx::TxOutput {
                amount: amount - fee,
                address: dest_address.to_string(),
                memo: None,
            }],
            fee,
            timestamp: now_secs(),
        };
        utxo.finalize_id();
        let sig = crypto::sign(sk, &utxo.id);
        utxo.inputs[0].signature = sig.to_vec();
        utxo
    }

    /// Legacy helper: creates a valid-looking UTXO tx whose inputs don't
    /// reference any on-chain output. Only usable for hash/signature tests
    /// that fail before the existence check.
    fn make_signed_utxo_tx() -> tx::Transaction {
        let (sk, vk) = crypto::generate_keypair();
        let addr = crypto::derive_address(vk.as_bytes());

        let mut utxo = tx::Transaction {
            id: [0u8; 32],
            version: tx::TX_HASH_V2_VERSION,
            inputs: vec![tx::TxInput {
                prev_tx_hash: [0xAA; 32],
                output_index: 0,
                signature: vec![],
                public_key: vk.as_bytes().to_vec(),
            }],
            outputs: vec![tx::TxOutput {
                amount: 1_000_000,
                address: addr,
                memo: None,
            }],
            fee: 1,
            timestamp: 1_700_000_000,
        };
        utxo.finalize_id();
        let sig = crypto::sign(&sk, &utxo.id);
        utxo.inputs[0].signature = sig.to_vec();
        utxo
    }

    #[test]
    fn utxo_transaction_submits_to_mempool() {
        let mut runtime = NodeRuntime::new("node-utxo-mempool", NodeConfig::mainnet());
        let (fund_id, _addr, vk, sk) = seed_utxo_funding(&mut runtime, 1_000_000);
        let utxo =
            make_signed_utxo_tx_spending(fund_id, 0, 1_000_000, &sk, &vk, "zion1destmempool");
        let tx_id = hex(&utxo.id);

        let resp = runtime.submit_submitted_transaction(SubmittedTransaction::Utxo(utxo));
        assert!(matches!(
            resp,
            RpcResponse::TransactionResult { accepted: true, .. }
        ));
        assert_eq!(runtime.status().mempool_transactions, 1);
        assert!(runtime.chain_state.mempool_by_id.contains_key(&tx_id));
    }

    #[test]
    fn utxo_transaction_appears_in_template() {
        let mut runtime = NodeRuntime::new("node-utxo-tmpl", NodeConfig::mainnet());
        let (fund_id, _addr, vk, sk) = seed_utxo_funding(&mut runtime, 1_000_000);
        let utxo = make_signed_utxo_tx_spending(fund_id, 0, 1_000_000, &sk, &vk, "zion1desttmpl");
        let tx_id = hex(&utxo.id);

        runtime.submit_submitted_transaction(SubmittedTransaction::Utxo(utxo));

        let template = runtime.active_template();
        assert_eq!(template.utxo_transaction_count, 1);
        assert_eq!(template.utxo_transaction_ids, vec![tx_id]);
        assert_eq!(template.total_utxo_fees, 1_000);
    }

    #[test]
    fn utxo_transaction_mined_in_block() {
        let mut runtime = NodeRuntime::new("node-utxo-mine", NodeConfig::mainnet());
        let (fund_id, _addr, vk, sk) = seed_utxo_funding(&mut runtime, 1_000_000);
        let utxo = make_signed_utxo_tx_spending(fund_id, 0, 1_000_000, &sk, &vk, "zion1destmine");
        let tx_id = hex(&utxo.id);

        runtime.submit_submitted_transaction(SubmittedTransaction::Utxo(utxo.clone()));
        mine_one_block(&mut runtime);

        let block = &runtime.accepted_blocks()[1];
        assert_eq!(block.utxo_transaction_ids, vec![tx_id]);
        assert_eq!(block.utxo_transactions.len(), 1);
        assert_eq!(block.utxo_transactions[0], utxo);
        // UTXO should be cleared from mempool after mining
        assert_eq!(runtime.status().mempool_transactions, 0);
    }

    #[test]
    fn utxo_transaction_rejects_invalid_hash() {
        let mut runtime = NodeRuntime::new("node-utxo-badhash", NodeConfig::mainnet());
        let mut utxo = make_signed_utxo_tx();
        utxo.id = [0xBB; 32]; // tamper with ID

        let resp = runtime.submit_submitted_transaction(SubmittedTransaction::Utxo(utxo));
        assert!(matches!(
            resp,
            RpcResponse::TransactionResult {
                accepted: false,
                reason: Some(ref reason),
                ..
            } if reason.contains("does not match calculated hash")
        ));
    }

    #[test]
    fn utxo_transaction_rejects_invalid_signature() {
        let mut runtime = NodeRuntime::new("node-utxo-badsig", NodeConfig::mainnet());
        let mut utxo = make_signed_utxo_tx();
        utxo.inputs[0].signature = vec![0u8; 64]; // zero out signature
        utxo.finalize_id(); // re-hash so ID matches

        let resp = runtime.submit_submitted_transaction(SubmittedTransaction::Utxo(utxo));
        assert!(matches!(
            resp,
            RpcResponse::TransactionResult {
                accepted: false,
                reason: Some(ref reason),
                ..
            } if reason.contains("signature verification failed")
        ));
    }

    #[test]
    fn utxo_transaction_rejects_duplicate_id() {
        let mut runtime = NodeRuntime::new("node-utxo-dup", NodeConfig::mainnet());
        let (fund_id, _addr, vk, sk) = seed_utxo_funding(&mut runtime, 1_000_000);
        let utxo = make_signed_utxo_tx_spending(fund_id, 0, 1_000_000, &sk, &vk, "zion1destdup");

        let first = runtime.submit_submitted_transaction(SubmittedTransaction::Utxo(utxo.clone()));
        assert!(matches!(
            first,
            RpcResponse::TransactionResult { accepted: true, .. }
        ));

        let second = runtime.submit_submitted_transaction(SubmittedTransaction::Utxo(utxo));
        assert!(matches!(
            second,
            RpcResponse::TransactionResult {
                accepted: false,
                reason: Some(ref reason),
                ..
            } if reason.contains("duplicate")
        ));
    }

    #[test]
    fn utxo_transaction_rejects_double_spend() {
        let mut runtime = NodeRuntime::new("node-utxo-dblspend", NodeConfig::mainnet());
        let (fund_id, _addr, vk, sk) = seed_utxo_funding(&mut runtime, 1_000_000);
        let tx1 = make_signed_utxo_tx_spending(fund_id, 0, 1_000_000, &sk, &vk, "zion1destdbl1");
        let tx2 = make_signed_utxo_tx_spending(fund_id, 0, 1_000_000, &sk, &vk, "zion1destdbl2"); // same input

        let first = runtime.submit_submitted_transaction(SubmittedTransaction::Utxo(tx1));
        assert!(matches!(
            first,
            RpcResponse::TransactionResult { accepted: true, .. }
        ));

        let second = runtime.submit_submitted_transaction(SubmittedTransaction::Utxo(tx2));
        assert!(matches!(
            second,
            RpcResponse::TransactionResult {
                accepted: false,
                reason: Some(ref reason),
                ..
            } if reason.contains("already being spent")
        ));
    }

    #[test]
    fn utxo_and_account_transactions_coexist_in_template() {
        let mut runtime = NodeRuntime::new("node-utxo-coexist", NodeConfig::mainnet());
        let account_tx = sample_transaction("tx-coexist", 5, 1);
        let (fund_id, _addr, vk, sk) = seed_utxo_funding(&mut runtime, 1_000_000);
        let utxo =
            make_signed_utxo_tx_spending(fund_id, 0, 1_000_000, &sk, &vk, "zion1destcoexist");

        runtime.handle_rpc_request(RpcRequest::SubmitTransaction {
            transaction: account_tx.clone(),
        });
        runtime.submit_submitted_transaction(SubmittedTransaction::Utxo(utxo.clone()));

        let template = runtime.active_template();
        assert_eq!(template.transaction_count, 1);
        assert_eq!(template.utxo_transaction_count, 1);
        assert_eq!(template.total_fees_zion, 5);
        assert_eq!(template.total_utxo_fees, 1_000);
        assert_eq!(runtime.status().mempool_transactions, 2);
    }

    /// Look up the funding tx that a previous `seed_utxo_funding` call
    /// injected into the runtime's genesis. Returns the cloned tx so callers
    /// can replicate the same UTXO into a fresh target runtime when peer
    /// import flows are exercised.
    fn extract_funding_tx_from_genesis(
        runtime: &NodeRuntime,
        fund_id: &[u8; 32],
    ) -> tx::Transaction {
        let block = &runtime.chain_state.accepted_blocks[0];
        block
            .utxo_transactions
            .iter()
            .find(|tx| &tx.id == fund_id)
            .expect("funding tx must already be seeded into genesis")
            .clone()
    }

    #[test]
    fn utxo_mined_block_passes_peer_import() {
        let mut source = NodeRuntime::new("node-utxo-src", NodeConfig::mainnet());
        let (fund_id, _addr, vk, sk) = seed_utxo_funding(&mut source, 1_000_000);
        let utxo = make_signed_utxo_tx_spending(fund_id, 0, 1_000_000, &sk, &vk, "zion1destpeer");

        source.submit_submitted_transaction(SubmittedTransaction::Utxo(utxo.clone()));
        mine_one_block(&mut source);

        // The target node must see the same funding UTXO that the source
        // spent, otherwise the F1 input-existence check correctly rejects
        // the imported block. Replicate the deterministic funding tx so
        // both runtimes share the genesis UTXO set.
        let mut target = NodeRuntime::new("node-utxo-tgt", NodeConfig::mainnet());
        let funding = extract_funding_tx_from_genesis(&source, &fund_id);
        inject_funding_into_genesis(&mut target, &funding);

        let imported = target
            .import_peer_blocks(source.accepted_blocks().to_vec())
            .expect("peer import with UTXO should succeed");
        assert_eq!(imported, 1);
        assert_eq!(target.accepted_blocks()[1].utxo_transactions.len(), 1);
    }

    /// F1 regression: a locally minted block whose UTXO transaction outputs
    /// (plus fee) exceed the value of its referenced inputs MUST be rejected
    /// by `validate_peer_block`. Before F1 the conservation-of-value check
    /// was dead code, so the same candidate would be accepted and the
    /// supply silently inflated by the difference.
    #[test]
    fn submit_candidate_rejects_utxo_inflating_supply() {
        let mut runtime = NodeRuntime::new("node-f1-inflate", NodeConfig::mainnet());
        let funding_amount: u64 = 1_000_000;
        let (fund_id, _addr, vk, sk) = seed_utxo_funding(&mut runtime, funding_amount);

        // Forge a UTXO transaction whose outputs are exactly the input value
        // but the fee is positive — outputs+fee > inputs, the simplest
        // possible "money printing" case. The signature is valid (we sign
        // the inflated tx with the genuine key) so only conservation can
        // catch this.
        let fee: u64 = 1_000;
        let mut utxo = tx::Transaction {
            id: [0u8; 32],
            version: tx::TX_HASH_V2_VERSION,
            inputs: vec![tx::TxInput {
                prev_tx_hash: fund_id,
                output_index: 0,
                signature: vec![],
                public_key: vk.clone(),
            }],
            outputs: vec![tx::TxOutput {
                amount: funding_amount,
                address: "zion1destinflated".to_string(),
                memo: None,
            }],
            fee,
            timestamp: now_secs(),
        };
        utxo.finalize_id();
        let sig = crypto::sign(&sk, &utxo.id);
        utxo.inputs[0].signature = sig.to_vec();

        runtime.submit_submitted_transaction(SubmittedTransaction::Utxo(utxo));

        // Mine via SubmitCandidate so we can inspect the validation
        // failure without relying on the helper's panic assertion.
        let template = runtime.active_template();
        let nonce = find_valid_nonce(&template);
        let response = runtime.handle_rpc_request(RpcRequest::SubmitCandidate {
            template_id: template.template_id,
            header_hex: template.header_hex.clone(),
            nonce,
            target_hex: template.target_hex.clone(),
            algorithm: "deeksha_lite_v1".to_string(),
        });
        match response {
            RpcResponse::SubmitResult {
                accepted: false,
                reason: Some(reason),
                ..
            } => {
                assert!(
                    reason.contains("value conservation") || reason.contains("ValueNotConserved"),
                    "expected value-conservation rejection, got: {reason}",
                );
            }
            RpcResponse::SubmitResult { accepted: true, .. } => {
                panic!(
                    "F1 regression: candidate with inflating UTXO transaction was accepted (response: {response:?})",
                );
            }
            other => panic!("unexpected submit response: {other:?}"),
        }
    }

    #[test]
    fn peer_import_rejects_utxo_with_bad_signature() {
        let mut source = NodeRuntime::new("node-utxo-badsig-src", NodeConfig::mainnet());
        let (fund_id, _addr, vk, sk) = seed_utxo_funding(&mut source, 1_000_000);
        let utxo = make_signed_utxo_tx_spending(fund_id, 0, 1_000_000, &sk, &vk, "zion1destbadsig");
        source.submit_submitted_transaction(SubmittedTransaction::Utxo(utxo));
        mine_one_block(&mut source);

        let mut block = source.accepted_blocks()[1].clone();
        // Tamper with UTXO signature
        block.utxo_transactions[0].inputs[0].signature = vec![0u8; 64];

        let mut target = NodeRuntime::new("node-utxo-badsig-tgt", NodeConfig::mainnet());
        // Replicate the same funding UTXO on the target so the
        // input-existence check passes and the validator reaches the
        // signature step.
        let funding = extract_funding_tx_from_genesis(&source, &fund_id);
        inject_funding_into_genesis(&mut target, &funding);

        let err = target
            .import_peer_blocks(vec![block])
            .expect_err("bad UTXO signature should be rejected");
        assert!(
            err.contains("invalid signatures"),
            "expected signature error, got: {err}"
        );
    }

    #[test]
    fn utxo_template_fields_default_empty() {
        let runtime = NodeRuntime::new("node-utxo-empty", NodeConfig::mainnet());
        let template = runtime.active_template();
        assert!(template.utxo_transaction_ids.is_empty());
        assert_eq!(template.utxo_transaction_count, 0);
        assert_eq!(template.total_utxo_fees, 0);
    }

    #[test]
    fn utxo_accepted_block_fields_default_empty_for_account_only_blocks() {
        let mut runtime = NodeRuntime::new("node-utxo-acct-only", NodeConfig::mainnet());
        let tx = sample_transaction("tx-acct-only", 3, 1);
        runtime.handle_rpc_request(RpcRequest::SubmitTransaction { transaction: tx });
        mine_one_block(&mut runtime);

        let block = &runtime.accepted_blocks()[1];
        assert!(block.utxo_transaction_ids.is_empty());
        assert!(block.utxo_transactions.is_empty());
    }

    #[test]
    fn utxo_transaction_rejects_nonexistent_input() {
        let mut runtime = NodeRuntime::new("node-utxo-noexist", NodeConfig::mainnet());
        let utxo = make_signed_utxo_tx(); // uses fake [0xAA; 32] input

        let resp = runtime.submit_submitted_transaction(SubmittedTransaction::Utxo(utxo));
        assert!(matches!(
            resp,
            RpcResponse::TransactionResult {
                accepted: false,
                reason: Some(ref reason),
                ..
            } if reason.contains("does not exist or is already spent")
        ));
    }

    #[test]
    fn utxo_balance_reflects_funded_and_spent() {
        let mut runtime = NodeRuntime::new("node-utxo-bal", NodeConfig::mainnet());
        let (fund_id, addr, vk, sk) = seed_utxo_funding(&mut runtime, 5_000_000);

        // Before spending, balance should be the funded amount
        assert_eq!(runtime.utxo_balance(&addr), 5_000_000);
        assert_eq!(runtime.spendable_utxos(&addr).len(), 1);

        // Submit a spending tx
        let utxo = make_signed_utxo_tx_spending(fund_id, 0, 5_000_000, &sk, &vk, "zion1destbal");
        runtime.submit_submitted_transaction(SubmittedTransaction::Utxo(utxo));
        mine_one_block(&mut runtime);

        // After mining the spend, the original address balance should be zero
        assert_eq!(runtime.utxo_balance(&addr), 0);
        assert!(runtime.spendable_utxos(&addr).is_empty());
        // Destination got the output (amount - fee = 5_000_000 - 1_000)
        assert_eq!(runtime.utxo_balance("zion1destbal"), 4_999_000);
    }

    // ═══════════════════════════════════════════════════════════════════
    // E2E multi-node tests
    // ═══════════════════════════════════════════════════════════════════

    /// Slow: 3× `mine_one_block` in debug build. Run via:
    ///   `cargo test --release -- --include-ignored`
    #[test]
    #[ignore = "slow PoW in debug build; run with --release --ignored"]
    fn e2e_block_relay_between_two_nodes() {
        let mut node_a = NodeRuntime::new("node-a", NodeConfig::mainnet());
        let mut node_b = NodeRuntime::new("node-b", NodeConfig::mainnet());

        // Mine 3 blocks on node A
        for _ in 0..3 {
            mine_one_block(&mut node_a);
        }
        assert_eq!(node_a.chain_height(), 3);
        assert_eq!(node_b.chain_height(), 0);

        // Relay each mined block from A to B via AnnounceBlock
        for block in node_a.accepted_blocks()[1..].iter().cloned() {
            let msg = P2pMessage::AnnounceBlock { block };
            let result = node_b.handle_p2p_message(msg);
            assert!(result.is_ok(), "block relay failed: {:?}", result.err());
        }

        assert_eq!(node_b.chain_height(), 3);
        assert_eq!(node_a.status().tip_hash_hex, node_b.status().tip_hash_hex,);
    }

    /// Slow / hangs in debug build: 5× `mine_one_block` (LWMA difficulty
    /// ramps each iteration). Empirically does not complete within 5 minutes
    /// in `cargo test` debug build. Run via:
    ///   `cargo test --release -- --include-ignored`
    #[test]
    #[ignore = "slow PoW in debug build; run with --release --ignored"]
    fn e2e_get_blocks_since_sync() {
        let mut node_a = NodeRuntime::new("node-src", NodeConfig::mainnet());
        let mut node_b = NodeRuntime::new("node-dst", NodeConfig::mainnet());

        // Mine 5 blocks on node A
        for _ in 0..5 {
            mine_one_block(&mut node_a);
        }

        // Simulate GetBlocksSince sync protocol
        let msg = P2pMessage::GetBlocksSince {
            from_height: 0,
            limit: 100,
        };
        let response = node_a.handle_p2p_message(msg).expect("GetBlocksSince");
        let blocks = match response {
            P2pMessage::Blocks { blocks } => blocks,
            other => panic!("expected Blocks, got {other:?}"),
        };

        // Import via sequential AnnounceBlock (respects per-block difficulty)
        for block in blocks {
            node_b
                .handle_p2p_message(P2pMessage::AnnounceBlock { block })
                .expect("announce");
        }
        assert_eq!(node_b.chain_height(), 5);
    }

    #[test]
    fn e2e_transaction_relay_between_nodes() {
        let mut node_a = NodeRuntime::new("node-tx-a", NodeConfig::mainnet());
        let mut node_b = NodeRuntime::new("node-tx-b", NodeConfig::mainnet());

        // Submit transaction on node A via RPC
        let tx = sample_transaction("relay-tx-1", 5, 1);
        let response = node_a.handle_rpc_request(RpcRequest::SubmitTransaction {
            transaction: tx.clone(),
        });
        assert!(
            matches!(
                &response,
                RpcResponse::TransactionResult { accepted: true, .. }
            ),
            "tx submit failed: {response:?}"
        );

        // Relay via AnnounceTx P2P message
        let msg = P2pMessage::AnnounceTx {
            tx_id: tx.tx_id.clone(),
            transaction: SubmittedTransaction::Account(tx.clone()),
        };
        let result = node_b.handle_p2p_message(msg);
        assert!(result.is_ok(), "tx relay failed: {:?}", result.err());

        // Node B should have the transaction in its mempool
        assert_eq!(node_b.status().mempool_transactions, 1);
    }

    #[test]
    fn e2e_announce_tx_roundtrip_serialization() {
        let tx = sample_transaction("serde-tx", 3, 1);
        let msg = P2pMessage::AnnounceTx {
            tx_id: tx.tx_id.clone(),
            transaction: SubmittedTransaction::Account(tx),
        };
        let encoded = encode_p2p_message(&msg).expect("encode AnnounceTx");
        let decoded = decode_p2p_message(&encoded).expect("decode AnnounceTx");
        assert_eq!(decoded, msg);
    }

    /// Slow: 2× `mine_one_block` in debug build. Run via:
    ///   `cargo test --release -- --include-ignored`
    #[test]
    #[ignore = "slow PoW in debug build; run with --release --ignored"]
    fn e2e_three_node_chain_sync() {
        let mut miner = NodeRuntime::new("miner", NodeConfig::mainnet());
        let mut relay = NodeRuntime::new("relay", NodeConfig::mainnet());
        let mut edge = NodeRuntime::new("edge", NodeConfig::mainnet());

        // Miner mines 2 blocks
        mine_one_block(&mut miner);
        mine_one_block(&mut miner);

        // Miner -> Relay (AnnounceBlock)
        for block in miner.accepted_blocks()[1..].iter().cloned() {
            let msg = P2pMessage::AnnounceBlock { block };
            relay.handle_p2p_message(msg).expect("relay import");
        }
        assert_eq!(relay.chain_height(), 2);

        // Relay -> Edge (GetBlocksSince sync)
        let resp = relay
            .handle_p2p_message(P2pMessage::GetBlocksSince {
                from_height: 0,
                limit: 100,
            })
            .expect("GetBlocksSince");
        if let P2pMessage::Blocks { blocks } = resp {
            edge.import_peer_blocks(blocks).expect("edge import");
        }
        assert_eq!(edge.chain_height(), 2);

        // All three nodes at same height with same tip
        assert_eq!(miner.status().tip_hash_hex, relay.status().tip_hash_hex);
        assert_eq!(relay.status().tip_hash_hex, edge.status().tip_hash_hex);
    }

    /// Slow: `mine_one_block` in debug build (~50s on M1).
    /// Run via: `cargo test --release -- --include-ignored`
    #[test]
    #[ignore = "slow PoW in debug build; run with --release --ignored"]
    fn e2e_duplicate_block_announce_is_harmless() {
        let mut node_a = NodeRuntime::new("dup-src", NodeConfig::mainnet());
        let mut node_b = NodeRuntime::new("dup-dst", NodeConfig::mainnet());

        mine_one_block(&mut node_a);
        let block = node_a.accepted_blocks()[1].clone();

        // First announce — should succeed
        let r1 = node_b.handle_p2p_message(P2pMessage::AnnounceBlock {
            block: block.clone(),
        });
        assert!(r1.is_ok());
        assert_eq!(node_b.chain_height(), 1);

        // Second announce of same block — should not error or change height
        let r2 = node_b.handle_p2p_message(P2pMessage::AnnounceBlock { block });
        assert!(r2.is_ok());
        assert_eq!(node_b.chain_height(), 1);
    }

    /// Slow: `mine_one_block` + sync in debug build.
    /// Run via: `cargo test --release -- --include-ignored`
    #[test]
    #[ignore = "slow PoW in debug build; run with --release --ignored"]
    fn e2e_transaction_then_mine_then_sync() {
        let mut node_a = NodeRuntime::new("txmine-a", NodeConfig::mainnet());
        let mut node_b = NodeRuntime::new("txmine-b", NodeConfig::mainnet());

        // Submit a transaction, then mine a block that includes it
        let tx = sample_transaction("mined-tx", 5, 1);
        node_a.handle_rpc_request(RpcRequest::SubmitTransaction {
            transaction: tx.clone(),
        });
        assert_eq!(node_a.status().mempool_transactions, 1);
        mine_one_block(&mut node_a);
        // After mining, mempool should be drained
        assert_eq!(node_a.status().mempool_transactions, 0);

        // Sync block to node B
        let block = node_a.accepted_blocks()[1].clone();
        node_b
            .handle_p2p_message(P2pMessage::AnnounceBlock { block })
            .expect("sync");
        assert_eq!(node_b.chain_height(), 1);
        assert_eq!(node_a.status().tip_hash_hex, node_b.status().tip_hash_hex,);
    }

    /// Slow: 2× `mine_one_block` in debug build. Run via:
    ///   `cargo test --release -- --include-ignored`
    #[test]
    #[ignore = "slow PoW in debug build; run with --release --ignored"]
    fn e2e_status_exchange() {
        let mut node_a = NodeRuntime::new("status-a", NodeConfig::mainnet());
        mine_one_block(&mut node_a);
        mine_one_block(&mut node_a);

        let resp = node_a
            .handle_p2p_message(P2pMessage::GetStatus)
            .expect("GetStatus");
        match resp {
            P2pMessage::Status { status } => {
                assert_eq!(status.chain_height, 2);
                assert!(!status.tip_hash_hex.is_empty());
            }
            other => panic!("expected Status, got {other:?}"),
        }
    }

    #[test]
    fn e2e_hello_handshake_network_mismatch_rejected() {
        let mut node = NodeRuntime::new("net-check", NodeConfig::mainnet());
        let msg = P2pMessage::Hello {
            node_id: "remote".to_string(),
            network: NetworkId::Testnet,
            protocol_version: NODE_PROTOCOL_VERSION.to_string(),
            listen_addr: "127.0.0.1:8334".to_string(),
        };
        let result = node.handle_p2p_message(msg);
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("network mismatch"));
    }

    // ───────────────────────────────────────────────────────────────────
    // F4 — bridge unlock multisig L1 enforcement (audit ref: V3 audit F4)
    //
    // These tests assert the protocol-level invariant that a bridge unlock
    // transaction can only be accepted if its memo carries ≥
    // BRIDGE_MIN_VALIDATOR_PROOFS distinct, allow-listed secp256k1
    // signatures over the canonical operation message reconstructed from
    // the transaction itself. The previous design verified signatures only
    // at the JSON-RPC entrypoint and discarded them, leaving peer-block
    // import unable to distinguish a real unlock from a forged one.
    // ───────────────────────────────────────────────────────────────────

    use k256::ecdsa::signature::Signer as _;
    use k256::ecdsa::{Signature as F4Signature, SigningKey};

    /// Generate `count` deterministic secp256k1 keypairs, plus the
    /// `ZION_BRIDGE_VALIDATOR_PUBKEYS` allowlist string they imply.
    fn f4_make_signing_keys(count: u8) -> (Vec<SigningKey>, String) {
        let mut keys = Vec::with_capacity(usize::from(count));
        let mut pubkeys_hex = Vec::with_capacity(usize::from(count));
        for i in 0..count {
            let bytes = [i + 0x21; 32];
            let key = SigningKey::from_slice(&bytes).expect("valid signing key");
            let sec1 = key.verifying_key().to_encoded_point(true);
            pubkeys_hex.push(hex::encode(sec1.as_bytes()));
            keys.push(key);
        }
        (keys, pubkeys_hex.join(","))
    }

    /// Build `count` proofs over `operation_message` from the supplied keys.
    fn f4_make_proofs(keys: &[SigningKey], operation_message: &str) -> Vec<BridgeValidatorProof> {
        keys.iter()
            .enumerate()
            .map(|(i, key)| {
                let sig: F4Signature = key.sign(operation_message.as_bytes());
                let sec1 = key.verifying_key().to_encoded_point(true);
                BridgeValidatorProof::new(
                    format!("v{}", i + 1),
                    hex::encode(sec1.as_bytes()),
                    hex::encode(sig.to_bytes()),
                )
                .expect("proof shape valid")
            })
            .collect()
    }

    #[test]
    fn f4_memo_with_proofs_round_trips_through_parser() {
        let (keys, _allowlist) = f4_make_signing_keys(3);
        let recipient = crypto::derive_address(&[0xAA; 32]);
        let amount: u64 = 1_000_000;
        let op = bridge_operation_message(&recipient, amount, "base", "burn-7", "0xfeed");
        let proofs = f4_make_proofs(&keys, &op);
        let memo = bridge_unlock_memo_with_proofs("base", "burn-7", "0xfeed", &proofs);

        let (src, burn, evm, raw) = parse_bridge_unlock_memo(&memo).expect("memo parses");
        assert_eq!(src, "base");
        assert_eq!(burn, "burn-7");
        assert_eq!(evm, "0xfeed");
        let parsed = parse_bridge_proofs(raw.expect("proofs present")).expect("proofs parse");
        assert_eq!(parsed, proofs);
        assert_eq!(
            bridge_unlock_replay_key_from_transaction_with_memo(&memo),
            Some("base:burn-7:0xfeed".to_string())
        );
    }

    /// Adapter: extract replay key from a memo string (used by the test
    /// above without constructing a full Transaction).
    fn bridge_unlock_replay_key_from_transaction_with_memo(memo: &str) -> Option<String> {
        let (s, b, e, _) = parse_bridge_unlock_memo(memo)?;
        Some(bridge_unlock_replay_key(s, b, e))
    }

    #[test]
    fn f4_verify_proofs_accepts_threshold_valid_signatures() {
        let (keys, allowlist_csv) = f4_make_signing_keys(3);
        let allowlist: HashSet<String> = allowlist_csv.split(',').map(str::to_string).collect();
        let op = "unlock|recipient=zion1xxx|amount=42|chain=base|burn_id=b|evm_tx=0x1";
        let proofs = f4_make_proofs(&keys, op);
        verify_bridge_proofs(&proofs, op, &allowlist, 3).expect("3/3 valid signatures accepted");
    }

    #[test]
    fn f4_verify_proofs_rejects_below_threshold() {
        let (keys, allowlist_csv) = f4_make_signing_keys(3);
        let allowlist: HashSet<String> = allowlist_csv.split(',').map(str::to_string).collect();
        let op = "unlock|recipient=zion1xxx|amount=42|chain=base|burn_id=b|evm_tx=0x1";
        let proofs = f4_make_proofs(&keys[..2], op); // only 2 signatures
        let err = verify_bridge_proofs(&proofs, op, &allowlist, 3).unwrap_err();
        assert!(
            err.contains("need at least 3"),
            "expected threshold error, got: {err}"
        );
    }

    #[test]
    fn f4_verify_proofs_rejects_pubkey_not_in_allowlist() {
        let (keys, _) = f4_make_signing_keys(3);
        // Allowlist contains different pubkeys than the signers.
        let (other_keys, other_csv) = f4_make_signing_keys(3);
        let _ = other_keys; // keep them in scope so seeds differ
        let mut allowlist: HashSet<String> = other_csv.split(',').map(str::to_string).collect();
        // Tweak last byte to force a non-overlapping allowlist.
        let any = allowlist.iter().next().cloned().unwrap_or_default();
        allowlist.remove(&any);
        let op = "unlock|recipient=zion1xxx|amount=42|chain=base|burn_id=b|evm_tx=0x1";
        let proofs = f4_make_proofs(&keys, op);
        let err = verify_bridge_proofs(&proofs, op, &allowlist, 3).unwrap_err();
        assert!(
            err.contains("not in core allowlist"),
            "expected allowlist error, got: {err}"
        );
    }

    #[test]
    fn f4_verify_proofs_rejects_duplicate_signers() {
        let (keys, allowlist_csv) = f4_make_signing_keys(2);
        let allowlist: HashSet<String> = allowlist_csv.split(',').map(str::to_string).collect();
        let op = "unlock|recipient=zion1xxx|amount=42|chain=base|burn_id=b|evm_tx=0x1";
        // Replicate one signer to fake a 3rd proof under a different
        // validator_id; the verifier must catch the pubkey reuse.
        let mut proofs = f4_make_proofs(&keys, op);
        let dup = proofs[0].clone();
        proofs.push(BridgeValidatorProof {
            validator_id: "v3-impostor".to_string(),
            ..dup
        });
        let err = verify_bridge_proofs(&proofs, op, &allowlist, 3).unwrap_err();
        assert!(
            err.contains("duplicate pubkey"),
            "expected duplicate-pubkey error, got: {err}"
        );
    }

    #[test]
    fn f4_verify_proofs_rejects_signature_for_wrong_message() {
        let (keys, allowlist_csv) = f4_make_signing_keys(3);
        let allowlist: HashSet<String> = allowlist_csv.split(',').map(str::to_string).collect();
        let signed_op = "unlock|recipient=zion1xxx|amount=42|chain=base|burn_id=b|evm_tx=0x1";
        let tampered_op = "unlock|recipient=zion1xxx|amount=999999|chain=base|burn_id=b|evm_tx=0x1";
        let proofs = f4_make_proofs(&keys, signed_op);
        let err = verify_bridge_proofs(&proofs, tampered_op, &allowlist, 3).unwrap_err();
        assert!(
            err.contains("failed secp256k1 signature verification"),
            "expected signature failure, got: {err}"
        );
    }

    #[test]
    fn f4_verify_proofs_rejects_empty_allowlist() {
        let (keys, _) = f4_make_signing_keys(3);
        let op = "unlock|recipient=zion1xxx|amount=42|chain=base|burn_id=b|evm_tx=0x1";
        let proofs = f4_make_proofs(&keys, op);
        let err =
            verify_bridge_proofs(&proofs, op, &HashSet::new(), 3).expect_err("empty allowlist");
        assert!(
            err.contains("allowlist is empty"),
            "expected empty-allowlist error, got: {err}"
        );
    }

    #[test]
    fn f4_parse_proofs_rejects_too_many_proofs() {
        let mut payload = String::new();
        for i in 0..(BRIDGE_MAX_VALIDATOR_PROOFS + 1) {
            if i > 0 {
                payload.push(',');
            }
            payload.push_str(&format!("v{i}:{}:{}", "0".repeat(66), "0".repeat(128)));
        }
        let err = parse_bridge_proofs(&payload).unwrap_err();
        assert!(err.contains("exceeding limit"), "got: {err}");
    }

    #[test]
    fn f4_parse_proofs_rejects_bad_pubkey_length() {
        let payload = format!("v1:{}:{}", "0".repeat(64), "0".repeat(128));
        let err = parse_bridge_proofs(&payload).unwrap_err();
        assert!(err.contains("66"), "got: {err}");
    }

    #[test]
    fn f4_parse_proofs_rejects_bad_signature_length() {
        let payload = format!("v1:{}:{}", "0".repeat(66), "0".repeat(64));
        let err = parse_bridge_proofs(&payload).unwrap_err();
        assert!(err.contains("128"), "got: {err}");
    }

    #[test]
    fn f4_parse_memo_rejects_oversize_input() {
        let blob = "x".repeat(BRIDGE_MAX_MEMO_LEN + 1);
        let memo = format!("BRIDGE_UNLOCK:base:burn:0xa|PROOFS={blob}");
        assert!(parse_bridge_unlock_memo(&memo).is_none());
    }

    #[test]
    fn f4_parse_memo_supports_legacy_body_only_form_for_replay_key() {
        // Body-only form must still parse so a node restoring journal data
        // tagged before this commit can extract the replay key. (Such a
        // memo will fail later at the proofs check, but replay-key
        // tracking must not silently lose entries.)
        let memo = "BRIDGE_UNLOCK:base:burn-1:0xdeadbeef";
        let (s, b, e, raw) = parse_bridge_unlock_memo(memo).expect("body-only memo parses");
        assert_eq!((s, b, e), ("base", "burn-1", "0xdeadbeef"));
        assert!(raw.is_none());
    }

    #[test]
    fn f4_validate_bridge_unlock_rejects_tx_without_proofs_payload() {
        let recipient = crypto::derive_address(&[0xCC; 32]);
        let utxos = HashMap::new();
        // Build a minimal bridge-unlock tx with the legacy body-only memo
        // (no |PROOFS=...). Because we have no UTXO entry, the function
        // would normally fail later on input lookup — but we're asserting
        // here that the proofs-missing check fires *first*, which is what
        // protects peers from accepting unsigned unlocks.
        let tx = tx::Transaction {
            id: [0u8; 32],
            version: tx::TX_HASH_V2_VERSION,
            inputs: vec![tx::TxInput {
                prev_tx_hash: [0u8; 32],
                output_index: 0,
                signature: Vec::new(),
                public_key: Vec::new(),
            }],
            outputs: vec![tx::TxOutput {
                amount: 1_000,
                address: recipient.clone(),
                memo: Some(bridge_unlock_memo("base", "burn-x", "0xnope")),
            }],
            fee: 100,
            timestamp: 0,
        };
        let err = validate_bridge_unlock_transaction_shape_with_utxos(&tx, &utxos).unwrap_err();
        assert!(
            err.contains("missing required validator proofs"),
            "expected proofs-missing rejection, got: {err}",
        );
    }

    // ════════════════════════════════════════════════════════════════════
    // F2 BLAKE3 Merkle body root — hard-fork dispatcher tests
    // (audit `AUDIT_COMPLETION.md` §2)
    // ════════════════════════════════════════════════════════════════════

    fn merkle_test_account_txs() -> Vec<Transaction> {
        vec![
            sample_transaction("merkle-tx-1", 5, 1),
            sample_transaction("merkle-tx-2", 7, 2),
            sample_transaction("merkle-tx-3", 11, 3),
        ]
    }

    /// Production activates BLAKE3 Merkle at genesis — dispatcher must match
    /// v2 for typical template heights (legacy v1 XOR remains testable via
    /// `derive_template_merkle_root_v1_xor` directly).
    #[cfg(not(feature = "testnet_fork_rehearsal"))]
    #[test]
    fn merkle_dispatcher_routes_v2_from_genesis_heights() {
        let txs = merkle_test_account_txs();
        let utxos: Vec<tx::Transaction> = Vec::new();
        let prev = [0x11u8; 32];
        let v2 = derive_template_merkle_root_v2_blake3(&txs, &utxos);

        for &h in &[0u64, 1, 100, 1_000_000] {
            let dispatched = derive_template_merkle_root("node-test", h, 42, prev, &txs, &utxos);
            assert_eq!(
                dispatched, v2,
                "height {h}: dispatcher must use v2 BLAKE3 Merkle"
            );
        }
    }

    /// Rehearsal build: XOR below `BODY_ROOT_V2_ACTIVATION_HEIGHT`, BLAKE3 Merkle at/above.
    #[cfg(feature = "testnet_fork_rehearsal")]
    #[test]
    fn merkle_dispatcher_routes_xor_then_v2_for_rehearsal_heights() {
        let txs = merkle_test_account_txs();
        let utxos: Vec<tx::Transaction> = Vec::new();
        let prev = [0x11u8; 32];
        let v2 = derive_template_merkle_root_v2_blake3(&txs, &utxos);
        let gate = BODY_ROOT_V2_ACTIVATION_HEIGHT;

        for h in 0..gate {
            let dispatched = derive_template_merkle_root("node-test", h, 42, prev, &txs, &utxos);
            let v1 = derive_template_merkle_root_v1_xor("node-test", h, 42, prev, &txs, &utxos);
            assert_eq!(
                dispatched, v1,
                "height {h}: rehearsal dispatcher must use legacy v1 XOR below gate"
            );
        }
        for &h in &[gate, gate + 1, 100, 1_000_000] {
            let dispatched = derive_template_merkle_root("node-test", h, 42, prev, &txs, &utxos);
            assert_eq!(
                dispatched, v2,
                "height {h}: rehearsal dispatcher must use v2 BLAKE3 Merkle at/above gate"
            );
        }
    }

    /// At [`BODY_ROOT_V2_ACTIVATION_HEIGHT`], dispatcher must route to v2 BLAKE3 Merkle.
    #[test]
    fn merkle_dispatcher_uses_v2_blake3_at_activation() {
        let txs = merkle_test_account_txs();
        let utxos: Vec<tx::Transaction> = Vec::new();
        let prev = [0x11u8; 32];

        let dispatched = derive_template_merkle_root(
            "node-test",
            BODY_ROOT_V2_ACTIVATION_HEIGHT,
            42,
            prev,
            &txs,
            &utxos,
        );
        let v2 = derive_template_merkle_root_v2_blake3(&txs, &utxos);
        assert_eq!(
            dispatched, v2,
            "at activation gate, dispatcher must equal v2 BLAKE3 Merkle"
        );
    }

    /// v1 XOR and v2 BLAKE3 Merkle MUST produce different roots for the
    /// same input set. If this ever returns equal, the fork is silent —
    /// peers wouldn't see a body-root mismatch and the migration would
    /// be invisible to consensus.
    #[test]
    fn merkle_v1_xor_and_v2_blake3_differ() {
        let txs = merkle_test_account_txs();
        let utxos: Vec<tx::Transaction> = Vec::new();
        let prev = [0x11u8; 32];

        let v1 = derive_template_merkle_root_v1_xor("node-test", 100, 42, prev, &txs, &utxos);
        let v2 = derive_template_merkle_root_v2_blake3(&txs, &utxos);
        assert_ne!(
            v1, v2,
            "v1 XOR and v2 BLAKE3 Merkle MUST produce different roots — silent fork detected"
        );
    }

    /// v2 BLAKE3 Merkle is deterministic — same inputs always produce the
    /// same root. This is the basic invariant peers rely on to agree on a
    /// block's body commitment.
    #[test]
    fn merkle_v2_blake3_is_deterministic() {
        let txs = merkle_test_account_txs();
        let utxos: Vec<tx::Transaction> = Vec::new();
        let a = derive_template_merkle_root_v2_blake3(&txs, &utxos);
        let b = derive_template_merkle_root_v2_blake3(&txs, &utxos);
        assert_eq!(a, b, "v2 BLAKE3 Merkle must be deterministic");
    }

    /// v2 BLAKE3 Merkle has avalanche behaviour — a single-byte change
    /// in any input tx propagates to the full root.
    #[test]
    fn merkle_v2_blake3_avalanche_on_tx_change() {
        let mut txs = merkle_test_account_txs();
        let utxos: Vec<tx::Transaction> = Vec::new();
        let baseline = derive_template_merkle_root_v2_blake3(&txs, &utxos);

        let original_id = txs[1].tx_id.clone();
        txs[1].tx_id = format!("{}f", &original_id[..63]);
        let perturbed = derive_template_merkle_root_v2_blake3(&txs, &utxos);
        assert_ne!(
            baseline, perturbed,
            "v2 BLAKE3 Merkle must change when any tx_id changes"
        );
    }

    /// v2 BLAKE3 Merkle commits to leaf order — swapping two txs in the
    /// list produces a different root. This pins that the body root is
    /// position-sensitive (Merkle property), so peers cannot reorder
    /// transactions in a block body without invalidating the root.
    #[test]
    fn merkle_v2_blake3_is_order_sensitive() {
        let txs = merkle_test_account_txs();
        let utxos: Vec<tx::Transaction> = Vec::new();
        let baseline = derive_template_merkle_root_v2_blake3(&txs, &utxos);

        let mut swapped = txs.clone();
        swapped.swap(0, 2);
        let swapped_root = derive_template_merkle_root_v2_blake3(&swapped, &utxos);
        assert_ne!(
            baseline, swapped_root,
            "v2 BLAKE3 Merkle must be order-sensitive (Merkle leaf-order property)"
        );
    }

    /// Empty tx list — v2 must collapse to all-zeros (matches
    /// `validation::merkle_root` empty contract). Pinning this so
    /// future contributors don't accidentally feed empty lists into
    /// a panic-prone branch.
    #[test]
    fn merkle_v2_blake3_empty_lists_yield_zero_root() {
        let v2 = derive_template_merkle_root_v2_blake3(&[], &[]);
        assert_eq!(v2, [0u8; 32], "empty tx + empty utxo => all-zeros root");
    }

    /// Pins `TESTNET_REHEARSAL_COORDINATED_HEIGHT` from `zion-cosmic-harmony`
    /// when this crate is built with `--features testnet_fork_rehearsal`.
    #[cfg(feature = "testnet_fork_rehearsal")]
    #[test]
    fn fork_rehearsal_gates_aligned_at_10_in_core_build() {
        use zion_cosmic_harmony::{
            body_root_v2_active, tx_hash_v2_active, BODY_ROOT_V2_ACTIVATION_HEIGHT,
            TX_HASH_V2_ACTIVATION_HEIGHT,
        };
        assert_eq!(TX_HASH_V2_ACTIVATION_HEIGHT, BODY_ROOT_V2_ACTIVATION_HEIGHT);
        assert_eq!(TX_HASH_V2_ACTIVATION_HEIGHT, 10);
        assert!(!tx_hash_v2_active(9) && tx_hash_v2_active(10));
        assert!(!body_root_v2_active(9) && body_root_v2_active(10));
    }

    /// Production default: coordinated gates at genesis (height 0).
    #[cfg(not(feature = "testnet_fork_rehearsal"))]
    #[test]
    fn production_fork_gates_at_genesis_in_core_build() {
        use zion_cosmic_harmony::{
            body_root_v2_active, tx_hash_v2_active, BODY_ROOT_V2_ACTIVATION_HEIGHT,
            TX_HASH_V2_ACTIVATION_HEIGHT,
        };
        assert_eq!(TX_HASH_V2_ACTIVATION_HEIGHT, BODY_ROOT_V2_ACTIVATION_HEIGHT);
        assert_eq!(TX_HASH_V2_ACTIVATION_HEIGHT, 0);
        assert!(tx_hash_v2_active(0) && body_root_v2_active(0));
    }
}
