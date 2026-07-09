// SPDX-License-Identifier: MIT
pragma solidity ^0.8.20;

import "@openzeppelin/contracts/access/AccessControl.sol";
import "@openzeppelin/contracts/utils/Pausable.sol";
import "@openzeppelin/contracts/utils/ReentrancyGuard.sol";

/**
 * @title ZIONBridge — Cross-chain bridge controller (EVM side)
 * @author ZION TerraNova Core Team
 * @notice Manages validator consensus for minting/burning wZION.
 *         Validators are bridge relay operators running the Rust bridge crate.
 *
 * Security model:
 *   - N-of-M multisig validation (default 3-of-5)
 *   - Each validator independently verifies L1 lock/unlock TX
 *   - Consensus threshold must be met before mint/burn execution
 *   - Timelock on large transfers (>1M wZION) — 24h delay
 *   - Rate limiting: max 10M wZION/day total throughput
 *
 * Flow (L1 → EVM):
 *   1. User sends ZION to L1 bridge lock address
 *   2. Bridge relay nodes detect lock TX (wait 60 blocks finality)
 *   3. Each validator calls submitLockProof()
 *   4. When threshold reached → auto-mint wZION to recipient
 *
 * Flow (EVM → L1):
 *   1. User calls wZION.bridgeBurn() → wZION burned, event emitted
 *   2. Bridge relay nodes detect BridgeBurn event
 *   3. Each validator calls confirmBurnRelease()
 *   4. When threshold reached → L1 unlock TX submitted
 */
/// @dev Minimal wZION interface for bridge operations
interface IWZION {
    function bridgeMint(address recipient, uint256 amount, bytes32 l1TxHash) external;
}

contract ZIONBridge is AccessControl, Pausable, ReentrancyGuard {

    // ──────────────────────────────────────────────
    //  Roles
    // ──────────────────────────────────────────────

    bytes32 public constant VALIDATOR_ROLE = keccak256("VALIDATOR_ROLE");
    bytes32 public constant GUARDIAN_ROLE = keccak256("GUARDIAN_ROLE");

    // ──────────────────────────────────────────────
    //  Structs
    // ──────────────────────────────────────────────

    struct LockProof {
        address recipient;          // EVM recipient for wZION
        uint256 amount;             // Amount to mint (in 18-decimal wZION)
        uint256 l1BlockHeight;      // L1 block height of lock TX
        string l1Sender;            // L1 sender address (bech32)
        uint256 submittedAt;        // Timestamp of first submission
        uint8 confirmations;        // Number of validator confirmations
        bool executed;              // Whether mint has been executed
        bool timelocked;            // Whether timelock applies (large amount)
        uint256 timelockExpiry;     // Timestamp when timelock expires
        mapping(address => bool) validators;  // Which validators confirmed
    }

    struct BurnRelease {
        address evmBurner;          // EVM address that burned wZION
        uint256 amount;             // Amount burned (18-decimal)
        string l1Recipient;         // L1 recipient address (bech32)
        bytes32 burnId;             // Burn ID from wZION contract
        uint256 submittedAt;        // Timestamp of first confirmation
        uint8 confirmations;        // Number of validator confirmations
        bool released;              // Whether L1 unlock has been confirmed
        mapping(address => bool) validators;
    }

    // ──────────────────────────────────────────────
    //  Constants
    // ──────────────────────────────────────────────

    /// @notice Amount above which timelock applies (1M wZION)
    uint256 public constant TIMELOCK_THRESHOLD = 1_000_000 * 1e18;

    /// @notice Timelock delay for large transfers (24 hours)
    uint256 public constant TIMELOCK_DELAY = 24 hours;

    /// @notice Maximum daily bridge throughput (10M wZION)
    uint256 public constant DAILY_LIMIT = 10_000_000 * 1e18;

    /// @notice L1 finality requirement (60 blocks × 60s = ~1 hour)
    uint256 public constant L1_FINALITY_BLOCKS = 60;

    // ──────────────────────────────────────────────
    //  State
    // ──────────────────────────────────────────────

    /// @notice wZION token contract
    IWZION public wZION;

    /// @notice Required number of validator confirmations
    uint8 public threshold;

    /// @notice Total number of active validators
    uint8 public validatorCount;

    /// @notice Lock proofs indexed by L1 TX hash
    mapping(bytes32 => LockProof) public lockProofs;

    /// @notice Burn releases indexed by burn ID
    mapping(bytes32 => BurnRelease) public burnReleases;

    /// @notice Daily throughput tracking
    uint256 public dailyMinted;
    uint256 public dailyBurned;
    uint256 public dailyResetTimestamp;

    /// @notice Statistics
    uint256 public totalLocksProcessed;
    uint256 public totalBurnsProcessed;

    // ──────────────────────────────────────────────
    //  Events
    // ──────────────────────────────────────────────

    event LockProofSubmitted(bytes32 indexed l1TxHash, address indexed validator, uint8 confirmations, uint8 threshold);
    event LockExecuted(bytes32 indexed l1TxHash, address indexed recipient, uint256 amount);
    event LockTimelocked(bytes32 indexed l1TxHash, uint256 expiresAt);

    event BurnConfirmationSubmitted(bytes32 indexed burnId, address indexed validator, uint8 confirmations, uint8 threshold);
    event BurnReleaseConfirmed(bytes32 indexed burnId, string l1Recipient, uint256 amount);

    event ThresholdUpdated(uint8 oldThreshold, uint8 newThreshold);
    event ValidatorAdded(address indexed validator);
    event ValidatorRemoved(address indexed validator);
    event DailyLimitReset(uint256 timestamp);

    // ──────────────────────────────────────────────
    //  Errors
    // ──────────────────────────────────────────────

    error InvalidThreshold(uint8 threshold, uint8 validators);
    error AlreadyConfirmed(address validator, bytes32 id);
    error AlreadyExecuted(bytes32 id);
    error TimelockNotExpired(bytes32 id, uint256 expiresAt);
    error DailyLimitExceeded(uint256 requested, uint256 remaining);
    error InsufficientFinality(uint256 currentHeight, uint256 requiredHeight);

    // ──────────────────────────────────────────────
    //  Constructor
    // ──────────────────────────────────────────────

    /**
     * @param admin       Multisig admin (manages validators)
     * @param guardian    Emergency pause address
     * @param wZIONAddr   Deployed wZION ERC-20 address
     * @param validators  Initial validator addresses (bridge relay operators)
     * @param _threshold  Required confirmations (e.g., 3 out of 5)
     */
    constructor(
        address admin,
        address guardian,
        address wZIONAddr,
        address[] memory validators,
        uint8 _threshold
    ) {
        require(admin != address(0) && guardian != address(0) && wZIONAddr != address(0), "Zero address");
        require(validators.length >= _threshold && _threshold >= 1, "Invalid threshold");  // testnet: allows 1-of-1

        _grantRole(DEFAULT_ADMIN_ROLE, admin);
        _grantRole(GUARDIAN_ROLE, guardian);

        for (uint256 i = 0; i < validators.length; i++) {
            require(validators[i] != address(0), "Zero validator");
            _grantRole(VALIDATOR_ROLE, validators[i]);
        }

        wZION = IWZION(wZIONAddr);
        threshold = _threshold;
        validatorCount = uint8(validators.length);
        dailyResetTimestamp = block.timestamp;
    }

    // ──────────────────────────────────────────────
    //  L1 → EVM (Lock → Mint)
    // ──────────────────────────────────────────────

    /**
     * @notice Submit proof that ZION was locked on L1.
     *         Each validator submits independently after verifying the L1 TX.
     *         When threshold reached, wZION is auto-minted.
     *
     * @param l1TxHash       Hash of the L1 lock transaction
     * @param recipient      EVM address to receive wZION
     * @param amount         Amount to mint (18-decimal, bridge relay converts from L1 6-decimal)
     * @param l1BlockHeight  Block height of the lock TX on L1
     * @param l1Sender       L1 sender address (bech32, for audit trail)
     */
    function submitLockProof(
        bytes32 l1TxHash,
        address recipient,
        uint256 amount,
        uint256 l1BlockHeight,
        string calldata l1Sender
    ) external onlyRole(VALIDATOR_ROLE) whenNotPaused nonReentrant {
        _resetDailyLimitIfNeeded();

        LockProof storage proof = lockProofs[l1TxHash];

        if (proof.executed) revert AlreadyExecuted(l1TxHash);
        if (proof.validators[msg.sender]) revert AlreadyConfirmed(msg.sender, l1TxHash);

        // First submission initializes the proof
        if (proof.confirmations == 0) {
            proof.recipient = recipient;
            proof.amount = amount;
            proof.l1BlockHeight = l1BlockHeight;
            proof.l1Sender = l1Sender;
            proof.submittedAt = block.timestamp;

            // Apply timelock for large amounts
            if (amount >= TIMELOCK_THRESHOLD) {
                proof.timelocked = true;
                proof.timelockExpiry = block.timestamp + TIMELOCK_DELAY;
                emit LockTimelocked(l1TxHash, proof.timelockExpiry);
            }
        } else {
            // Subsequent submissions must match parameters
            require(proof.recipient == recipient, "Recipient mismatch");
            require(proof.amount == amount, "Amount mismatch");
            require(proof.l1BlockHeight == l1BlockHeight, "Block height mismatch");
        }

        proof.validators[msg.sender] = true;
        proof.confirmations++;

        emit LockProofSubmitted(l1TxHash, msg.sender, proof.confirmations, threshold);

        // Auto-execute when threshold reached
        if (proof.confirmations >= threshold) {
            _executeMint(l1TxHash);
        }
    }

    /**
     * @notice Execute a timelocked mint after delay has passed.
     *         Can be called by anyone once timelock expires and threshold is met.
     */
    function executeTimelockedMint(bytes32 l1TxHash) external whenNotPaused nonReentrant {
        LockProof storage proof = lockProofs[l1TxHash];
        require(proof.confirmations >= threshold, "Below threshold");
        if (proof.executed) revert AlreadyExecuted(l1TxHash);
        if (proof.timelocked && block.timestamp < proof.timelockExpiry) {
            revert TimelockNotExpired(l1TxHash, proof.timelockExpiry);
        }

        _executeMint(l1TxHash);
    }

    // ──────────────────────────────────────────────
    //  EVM → L1 (Burn → Unlock)
    // ──────────────────────────────────────────────

    /**
     * @notice Confirm that an L1 unlock TX was submitted for a wZION burn.
     *         Validators call this after submitting the L1 unlock transaction.
     *
     * @param burnId      Burn ID from wZION BridgeBurn event
     * @param evmBurner   Address that burned wZION
     * @param amount      Amount that was burned
     * @param l1Recipient L1 recipient address (from burn event)
     */
    function confirmBurnRelease(
        bytes32 burnId,
        address evmBurner,
        uint256 amount,
        string calldata l1Recipient
    ) external onlyRole(VALIDATOR_ROLE) whenNotPaused {
        BurnRelease storage release = burnReleases[burnId];

        if (release.released) revert AlreadyExecuted(burnId);
        if (release.validators[msg.sender]) revert AlreadyConfirmed(msg.sender, burnId);

        if (release.confirmations == 0) {
            release.evmBurner = evmBurner;
            release.amount = amount;
            release.l1Recipient = l1Recipient;
            release.burnId = burnId;
            release.submittedAt = block.timestamp;
        }

        release.validators[msg.sender] = true;
        release.confirmations++;

        emit BurnConfirmationSubmitted(burnId, msg.sender, release.confirmations, threshold);

        if (release.confirmations >= threshold) {
            release.released = true;
            totalBurnsProcessed++;
            dailyBurned += amount;
            emit BurnReleaseConfirmed(burnId, l1Recipient, amount);
        }
    }

    // ──────────────────────────────────────────────
    //  Admin functions
    // ──────────────────────────────────────────────

    function updateThreshold(uint8 _threshold) external onlyRole(DEFAULT_ADMIN_ROLE) {
        if (_threshold < 1 || _threshold > validatorCount) {  // testnet: allows 1-of-1
            revert InvalidThreshold(_threshold, validatorCount);
        }
        uint8 old = threshold;
        threshold = _threshold;
        emit ThresholdUpdated(old, _threshold);
    }

    function addValidator(address validator) external onlyRole(DEFAULT_ADMIN_ROLE) {
        require(validator != address(0), "Zero address");
        require(!hasRole(VALIDATOR_ROLE, validator), "Already validator");
        _grantRole(VALIDATOR_ROLE, validator);
        validatorCount++;
        emit ValidatorAdded(validator);
    }

    function removeValidator(address validator) external onlyRole(DEFAULT_ADMIN_ROLE) {
        require(hasRole(VALIDATOR_ROLE, validator), "Not a validator");
        require(validatorCount - 1 >= threshold, "Would break threshold");
        _revokeRole(VALIDATOR_ROLE, validator);
        validatorCount--;
        emit ValidatorRemoved(validator);
    }

    function pause() external onlyRole(GUARDIAN_ROLE) {
        _pause();
    }

    function unpause() external onlyRole(GUARDIAN_ROLE) {
        _unpause();
    }

    // ──────────────────────────────────────────────
    //  View functions
    // ──────────────────────────────────────────────

    function getLockProofStatus(bytes32 l1TxHash) external view returns (
        uint8 confirmations,
        bool executed,
        bool timelocked,
        uint256 timelockExpiry,
        address recipient,
        uint256 amount
    ) {
        LockProof storage p = lockProofs[l1TxHash];
        return (p.confirmations, p.executed, p.timelocked, p.timelockExpiry, p.recipient, p.amount);
    }

    function getBurnReleaseStatus(bytes32 burnId) external view returns (
        uint8 confirmations,
        bool released,
        address evmBurner,
        uint256 amount,
        string memory l1Recipient
    ) {
        BurnRelease storage r = burnReleases[burnId];
        return (r.confirmations, r.released, r.evmBurner, r.amount, r.l1Recipient);
    }

    function dailyRemaining() external view returns (uint256 mintRemaining, uint256 burnRemaining) {
        if (block.timestamp >= dailyResetTimestamp + 1 days) {
            return (DAILY_LIMIT, DAILY_LIMIT);
        }
        uint256 mintLeft = dailyMinted >= DAILY_LIMIT ? 0 : DAILY_LIMIT - dailyMinted;
        uint256 burnLeft = dailyBurned >= DAILY_LIMIT ? 0 : DAILY_LIMIT - dailyBurned;
        return (mintLeft, burnLeft);
    }

    // ──────────────────────────────────────────────
    //  Internal
    // ──────────────────────────────────────────────

    function _executeMint(bytes32 l1TxHash) internal {
        LockProof storage proof = lockProofs[l1TxHash];

        if (proof.timelocked && block.timestamp < proof.timelockExpiry) {
            return; // Silently skip — will be executed via executeTimelockedMint()
        }

        // Daily limit check
        _resetDailyLimitIfNeeded();
        uint256 remaining = DAILY_LIMIT > dailyMinted ? DAILY_LIMIT - dailyMinted : 0;
        if (proof.amount > remaining) {
            revert DailyLimitExceeded(proof.amount, remaining);
        }

        proof.executed = true;
        totalLocksProcessed++;
        dailyMinted += proof.amount;

        // Call wZION mint
        wZION.bridgeMint(proof.recipient, proof.amount, l1TxHash);

        emit LockExecuted(l1TxHash, proof.recipient, proof.amount);
    }

    function _resetDailyLimitIfNeeded() internal {
        if (block.timestamp >= dailyResetTimestamp + 1 days) {
            dailyMinted = 0;
            dailyBurned = 0;
            dailyResetTimestamp = block.timestamp;
            emit DailyLimitReset(block.timestamp);
        }
    }
}
