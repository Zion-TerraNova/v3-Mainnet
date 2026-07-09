// SPDX-License-Identifier: MIT
pragma solidity ^0.8.20;

import "@openzeppelin/contracts/token/ERC20/IERC20.sol";
import "@openzeppelin/contracts/token/ERC20/utils/SafeERC20.sol";
import "@openzeppelin/contracts/access/AccessControl.sol";
import "@openzeppelin/contracts/utils/ReentrancyGuard.sol";
import "@openzeppelin/contracts/utils/Pausable.sol";

/**
 * @title ZIONAtomicSwap — EVM HTLC (Hash Time-Lock Contract)
 * @author ZION TerraNova Core Team
 * @notice Trustless cross-chain atomic swaps for ETH and any ERC-20 token.
 *
 * ## Design
 *   - Initiator locks ETH or ERC-20 with SHA-256 hashlock + timelock
 *   - Recipient (or anyone with the preimage) can claim before timelock
 *   - Initiator can refund after timelock expires
 *   - Counterparty chain is recorded in each lock (e.g. "zion", "btc", "base")
 *   - Purpose: ZION ↔ ETH/wZION atomic swaps without trusted intermediary
 *
 * ## Roles
 *   - DEFAULT_ADMIN_ROLE  — update fee rate, pause
 *   - GUARDIAN_ROLE       — emergency pause
 *
 * ## Security
 *   - SHA-256 preimage verification (same as L1 HTLC daemon)
 *   - Reentrancy guard on all state-changing functions
 *   - No partial claims — atomic settlement only
 *   - Minimum timelock: 30 minutes; maximum: 7 days
 *
 * ## Fee
 *   - Flat fee in ETH (default 0) deducted on lock from ETH swaps
 *   - Fee for ERC-20 swaps: deducted proportionally from amount
 *   - Fees accumulate in contract, admin can withdraw
 *
 * ## Usage (ZION ↔ ETH flow)
 *   1. Alice generates secret S, H = SHA-256(S), sends to Bob off-chain
 *   2. Bob locks ETH: lock(id, H, timelock, address(0), 0, aliceEvmAddr)
 *   3. Alice sees lock, creates L1 ZION HTLC with same H
 *   4. Bob claims ZION on L1 (reveals S)
 *   5. Alice calls claim(id, S) to receive ETH on EVM
 *   6. If Bob never locks / Alice never claims → refund after timelock
 */
contract ZIONAtomicSwap is AccessControl, ReentrancyGuard, Pausable {
    using SafeERC20 for IERC20;

    // ─── Roles ─────────────────────────────────────────────────────────────

    bytes32 public constant GUARDIAN_ROLE = keccak256("GUARDIAN_ROLE");

    // ─── Constants ─────────────────────────────────────────────────────────

    uint256 public constant MIN_TIMELOCK    = 30 minutes;
    uint256 public constant MAX_TIMELOCK    = 7 days;
    uint256 public constant FEE_DENOMINATOR = 10_000;  // basis points

    // ─── State ─────────────────────────────────────────────────────────────

    /// feeBps: protocol fee in basis points (default 0, max 100 = 1%)
    uint256 public feeBps;

    /// Accumulated fees per token (address(0) = ETH)
    mapping(address => uint256) public accruedFees;

    // ─── Structs ────────────────────────────────────────────────────────────

    struct HTLCLock {
        address initiator;          // who locked
        address recipient;          // who can claim (address(0) = anyone)
        address token;              // ERC-20 address; address(0) = ETH
        uint256 amount;             // locked amount (net of fee)
        bytes32 hashlock;           // SHA-256(preimage)
        uint256 timelock;           // UNIX expiry timestamp
        bool    claimed;
        bool    refunded;
        string  counterpartyChain;  // "zion" | "btc" | "base" | ...
        string  counterpartyAddr;   // address on counterparty chain (for UI/indexer)
    }

    /// Primary storage: lockId → HTLCLock
    mapping(bytes32 => HTLCLock) public locks;

    // ─── Events ─────────────────────────────────────────────────────────────

    event Locked(
        bytes32 indexed id,
        address indexed initiator,
        address indexed recipient,
        address  token,
        uint256  amount,
        bytes32  hashlock,
        uint256  timelock,
        string   counterpartyChain,
        string   counterpartyAddr
    );

    event Claimed(
        bytes32 indexed id,
        address indexed claimedBy,
        bytes32 preimage
    );

    event Refunded(bytes32 indexed id, address indexed initiator);

    event FeesWithdrawn(address indexed token, uint256 amount);
    event FeeBpsUpdated(uint256 oldFeeBps, uint256 newFeeBps);

    // ─── Errors ─────────────────────────────────────────────────────────────

    error LockExists(bytes32 id);
    error LockNotFound(bytes32 id);
    error AlreadyClaimed(bytes32 id);
    error AlreadyRefunded(bytes32 id);
    error TimelockNotExpired(bytes32 id, uint256 expiresAt);
    error TimelockExpired(bytes32 id);
    error InvalidPreimage(bytes32 provided, bytes32 expected);
    error UnauthorizedRecipient(address caller, address expected);
    error InvalidTimelock(uint256 timelock, uint256 min, uint256 max);
    error ZeroAmount();
    error InsufficientEth(uint256 sent, uint256 required);
    error FeeTooHigh(uint256 feeBps, uint256 max);
    error NoFeesToWithdraw();

    // ─── Constructor ────────────────────────────────────────────────────────

    constructor(address admin, address guardian) {
        require(admin != address(0), "admin = zero");
        require(guardian != address(0), "guardian = zero");
        _grantRole(DEFAULT_ADMIN_ROLE, admin);
        _grantRole(GUARDIAN_ROLE, guardian);
    }

    // ─── Lock ───────────────────────────────────────────────────────────────

    /**
     * @notice Lock ETH or ERC-20 tokens in an HTLC.
     * @param id                Unique swap ID (generated off-chain, e.g. keccak256(H, block.timestamp))
     * @param hashlock          SHA-256 hash of the preimage secret
     * @param timelockDuration  How many SECONDS from now before refund is allowed
     * @param token             ERC-20 token address; address(0) to lock ETH
     * @param amount            Amount of ERC-20 to lock (ignored for ETH; use msg.value)
     * @param recipient         L2 address that can claim (address(0) = anyone)
     * @param counterpartyChain Human-readable chain name of the other side ("zion", "btc", ...)
     * @param counterpartyAddr  Address on the counterparty chain (for indexing / UI)
     */
    function lock(
        bytes32 id,
        bytes32 hashlock,
        uint256 timelockDuration,
        address token,
        uint256 amount,
        address recipient,
        string calldata counterpartyChain,
        string calldata counterpartyAddr
    ) external payable nonReentrant whenNotPaused {
        if (locks[id].timelock != 0) revert LockExists(id);
        if (timelockDuration < MIN_TIMELOCK || timelockDuration > MAX_TIMELOCK)
            revert InvalidTimelock(timelockDuration, MIN_TIMELOCK, MAX_TIMELOCK);

        uint256 netAmount;

        if (token == address(0)) {
            // ETH lock
            if (msg.value == 0) revert ZeroAmount();
            uint256 fee = (msg.value * feeBps) / FEE_DENOMINATOR;
            netAmount = msg.value - fee;
            if (fee > 0) accruedFees[address(0)] += fee;
        } else {
            // ERC-20 lock
            if (amount == 0) revert ZeroAmount();
            uint256 fee = (amount * feeBps) / FEE_DENOMINATOR;
            netAmount = amount - fee;
            IERC20(token).safeTransferFrom(msg.sender, address(this), amount);
            if (fee > 0) accruedFees[token] += fee;
        }

        uint256 expiry = block.timestamp + timelockDuration;

        locks[id] = HTLCLock({
            initiator:          msg.sender,
            recipient:          recipient,
            token:              token,
            amount:             netAmount,
            hashlock:           hashlock,
            timelock:           expiry,
            claimed:            false,
            refunded:           false,
            counterpartyChain:  counterpartyChain,
            counterpartyAddr:   counterpartyAddr
        });

        emit Locked(id, msg.sender, recipient, token, netAmount, hashlock, expiry,
                    counterpartyChain, counterpartyAddr);
    }

    // ─── Claim ──────────────────────────────────────────────────────────────

    /**
     * @notice Claim locked funds by revealing the preimage.
     * @param id        Swap ID
     * @param preimage  32-byte secret; SHA-256(preimage) must equal hashlock
     */
    function claim(bytes32 id, bytes32 preimage) external nonReentrant whenNotPaused {
        HTLCLock storage s = locks[id];
        if (s.timelock == 0)   revert LockNotFound(id);
        if (s.claimed)         revert AlreadyClaimed(id);
        if (s.refunded)        revert AlreadyRefunded(id);
        if (block.timestamp >= s.timelock) revert TimelockExpired(id);

        // Recipient check (if set)
        if (s.recipient != address(0) && msg.sender != s.recipient)
            revert UnauthorizedRecipient(msg.sender, s.recipient);

        // Verify SHA-256 preimage
        bytes32 computedHash = sha256(abi.encodePacked(preimage));
        if (computedHash != s.hashlock)
            revert InvalidPreimage(computedHash, s.hashlock);

        s.claimed = true;

        address receiver = s.recipient == address(0) ? msg.sender : s.recipient;

        if (s.token == address(0)) {
            // ETH
            (bool ok, ) = receiver.call{value: s.amount}("");
            require(ok, "ETH transfer failed");
        } else {
            IERC20(s.token).safeTransfer(receiver, s.amount);
        }

        emit Claimed(id, msg.sender, preimage);
    }

    // ─── Refund ─────────────────────────────────────────────────────────────

    /**
     * @notice Refund locked funds after timelock expiry. Only initiator can call.
     * @param id  Swap ID
     */
    function refund(bytes32 id) external nonReentrant {
        HTLCLock storage s = locks[id];
        if (s.timelock == 0)   revert LockNotFound(id);
        if (s.claimed)         revert AlreadyClaimed(id);
        if (s.refunded)        revert AlreadyRefunded(id);
        if (block.timestamp < s.timelock)
            revert TimelockNotExpired(id, s.timelock);

        s.refunded = true;

        if (s.token == address(0)) {
            (bool ok, ) = s.initiator.call{value: s.amount}("");
            require(ok, "ETH transfer failed");
        } else {
            IERC20(s.token).safeTransfer(s.initiator, s.amount);
        }

        emit Refunded(id, s.initiator);
    }

    // ─── Admin ──────────────────────────────────────────────────────────────

    /**
     * @notice Update protocol fee. Max 1% (100 bps).
     */
    function setFeeBps(uint256 _feeBps) external onlyRole(DEFAULT_ADMIN_ROLE) {
        if (_feeBps > 100) revert FeeTooHigh(_feeBps, 100);
        emit FeeBpsUpdated(feeBps, _feeBps);
        feeBps = _feeBps;
    }

    /**
     * @notice Withdraw accumulated protocol fees.
     * @param token  ERC-20 address; address(0) to withdraw ETH fees
     */
    function withdrawFees(address token) external onlyRole(DEFAULT_ADMIN_ROLE) {
        uint256 amount = accruedFees[token];
        if (amount == 0) revert NoFeesToWithdraw();
        accruedFees[token] = 0;
        if (token == address(0)) {
            (bool ok, ) = msg.sender.call{value: amount}("");
            require(ok, "Fee withdrawal failed");
        } else {
            IERC20(token).safeTransfer(msg.sender, amount);
        }
        emit FeesWithdrawn(token, amount);
    }

    function pause()   external onlyRole(GUARDIAN_ROLE) { _pause(); }
    function unpause() external onlyRole(GUARDIAN_ROLE) { _unpause(); }

    // ─── View ────────────────────────────────────────────────────────────────

    /// @notice Get full HTLC record.
    function getLock(bytes32 id) external view returns (HTLCLock memory) {
        return locks[id];
    }

    /// @notice Check if an HTLC is claimable (exists, not settled, not expired).
    function isClaimable(bytes32 id) external view returns (bool) {
        HTLCLock storage s = locks[id];
        return s.timelock != 0
            && !s.claimed
            && !s.refunded
            && block.timestamp < s.timelock;
    }

    /// @notice Check if an HTLC is refundable (exists, not settled, expired).
    function isRefundable(bytes32 id) external view returns (bool) {
        HTLCLock storage s = locks[id];
        return s.timelock != 0
            && !s.claimed
            && !s.refunded
            && block.timestamp >= s.timelock;
    }

    receive() external payable {}
}
