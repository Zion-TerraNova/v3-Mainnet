// SPDX-License-Identifier: MIT
pragma solidity ^0.8.20;

import "@openzeppelin/contracts/token/ERC20/ERC20.sol";
import "@openzeppelin/contracts/token/ERC20/extensions/ERC20Burnable.sol";
import "@openzeppelin/contracts/token/ERC20/extensions/ERC20Permit.sol";
import "@openzeppelin/contracts/access/AccessControl.sol";
import "@openzeppelin/contracts/utils/Pausable.sol";

/**
 * @title wZION — Wrapped ZION (ERC-20)
 * @author ZION TerraNova Core Team
 * @notice Wrapped representation of native ZION L1 tokens on EVM chains.
 *         1 wZION = 1 ZION locked on L1 bridge address.
 *         Only the bridge contract can mint/burn.
 *
 * @dev Deployment targets (priority order):
 *      1. Base / Arbitrum  (Uniswap v3)
 *      2. BNB Chain        (PancakeSwap)
 *      3. Polygon          (QuickSwap)
 *
 * Security model:
 *   - BRIDGE_ROLE can mint (on L1 lock proof) and burn (on L1 unlock request)
 *   - GUARDIAN_ROLE can pause/unpause in emergencies
 *   - DEFAULT_ADMIN_ROLE manages role assignments (multisig)
 *   - No owner, no single point of failure
 *
 * ZION L1 parameters (mirrored):
 *   - Total supply cap:  144,000,000,000 ZION (144B)
 *   - Decimals:          18 (matching L1 atomic units: 1 ZION = 1e6 atomic → scaled to 1e18 for EVM)
 *   - Symbol:            wZION
 */
contract WZION is ERC20, ERC20Burnable, ERC20Permit, AccessControl, Pausable {

    // ──────────────────────────────────────────────
    //  Roles
    // ──────────────────────────────────────────────

    /// @notice Role that allows minting and burning (bridge contract only)
    bytes32 public constant BRIDGE_ROLE = keccak256("BRIDGE_ROLE");

    /// @notice Role that can pause/unpause in emergencies
    bytes32 public constant GUARDIAN_ROLE = keccak256("GUARDIAN_ROLE");

    // ──────────────────────────────────────────────
    //  Constants
    // ──────────────────────────────────────────────

    /// @notice Maximum supply that can ever be minted (matches L1 total supply)
    uint256 public constant MAX_SUPPLY = 144_000_000_000 * 1e18; // 144B wZION

    /// @notice Minimum mint/burn amount to prevent dust attacks
    uint256 public constant MIN_BRIDGE_AMOUNT = 100 * 1e18; // 100 wZION

    // ──────────────────────────────────────────────
    //  State
    // ──────────────────────────────────────────────

    /// @notice Total amount ever minted through bridge (for audit trail)
    uint256 public totalBridgeMinted;

    /// @notice Total amount ever burned through bridge (for audit trail)
    uint256 public totalBridgeBurned;

    /// @notice Nonce for each L1 lock TX to prevent replay (L1 tx hash → used)
    mapping(bytes32 => bool) public processedL1Locks;

    /// @notice Nonce for each burn request to prevent replay
    mapping(bytes32 => bool) public processedBurnRequests;

    // ──────────────────────────────────────────────
    //  Events
    // ──────────────────────────────────────────────

    /// @notice Emitted when wZION is minted (L1 lock confirmed)
    event BridgeMint(
        address indexed recipient,
        uint256 amount,
        bytes32 indexed l1TxHash,
        uint256 timestamp
    );

    /// @notice Emitted when wZION is burned (L1 unlock requested)
    event BridgeBurn(
        address indexed from,
        uint256 amount,
        string l1Recipient,       // ZION L1 address (bech32)
        bytes32 indexed burnId,
        uint256 timestamp
    );

    /// @notice Emitted on emergency pause
    event EmergencyPause(address indexed guardian, string reason);

    /// @notice Emitted on unpause
    event EmergencyUnpause(address indexed guardian);

    // ──────────────────────────────────────────────
    //  Errors
    // ──────────────────────────────────────────────

    error ExceedsMaxSupply(uint256 requested, uint256 available);
    error BelowMinBridgeAmount(uint256 amount, uint256 minimum);
    error L1LockAlreadyProcessed(bytes32 l1TxHash);
    error BurnRequestAlreadyProcessed(bytes32 burnId);
    error InvalidL1Address(string l1Address);
    error ZeroAddress();
    error ZeroAmount();

    // ──────────────────────────────────────────────
    //  Constructor
    // ──────────────────────────────────────────────

    /**
     * @param admin      Multisig address that manages roles (3-of-5 recommended)
     * @param bridge     Bridge relay contract address
     * @param guardian   Emergency pause address (can be same multisig)
     */
    constructor(
        address admin,
        address bridge,
        address guardian
    ) ERC20("Wrapped ZION", "wZION") ERC20Permit("Wrapped ZION") {
        if (admin == address(0) || bridge == address(0) || guardian == address(0)) {
            revert ZeroAddress();
        }

        _grantRole(DEFAULT_ADMIN_ROLE, admin);
        _grantRole(BRIDGE_ROLE, bridge);
        _grantRole(GUARDIAN_ROLE, guardian);
    }

    // ──────────────────────────────────────────────
    //  Bridge functions (BRIDGE_ROLE only)
    // ──────────────────────────────────────────────

    /**
     * @notice Mint wZION when ZION is locked on L1 bridge address.
     *         Called by bridge relay after L1 lock confirmation (≥60 blocks finality).
     * @param recipient  EVM address to receive wZION
     * @param amount     Amount of wZION to mint (1:1 with locked ZION on L1)
     * @param l1TxHash   Hash of the L1 lock transaction (replay protection)
     */
    function bridgeMint(
        address recipient,
        uint256 amount,
        bytes32 l1TxHash
    ) external onlyRole(BRIDGE_ROLE) whenNotPaused {
        if (recipient == address(0)) revert ZeroAddress();
        if (amount == 0) revert ZeroAmount();
        if (amount < MIN_BRIDGE_AMOUNT) revert BelowMinBridgeAmount(amount, MIN_BRIDGE_AMOUNT);
        if (processedL1Locks[l1TxHash]) revert L1LockAlreadyProcessed(l1TxHash);
        if (totalSupply() + amount > MAX_SUPPLY) {
            revert ExceedsMaxSupply(amount, MAX_SUPPLY - totalSupply());
        }

        processedL1Locks[l1TxHash] = true;
        totalBridgeMinted += amount;

        _mint(recipient, amount);

        emit BridgeMint(recipient, amount, l1TxHash, block.timestamp);
    }

    /**
     * @notice Burn wZION to unlock native ZION on L1.
     *         User calls this → bridge relay observes event → unlocks on L1.
     * @param amount       Amount of wZION to burn
     * @param l1Recipient  ZION L1 bech32 address (e.g. "zion1q...")
     * @param burnId       Unique ID for this burn request (client-generated, prevents replay)
     */
    function bridgeBurn(
        uint256 amount,
        string calldata l1Recipient,
        bytes32 burnId
    ) external whenNotPaused {
        if (amount == 0) revert ZeroAmount();
        if (amount < MIN_BRIDGE_AMOUNT) revert BelowMinBridgeAmount(amount, MIN_BRIDGE_AMOUNT);
        if (processedBurnRequests[burnId]) revert BurnRequestAlreadyProcessed(burnId);
        if (!_isValidL1Address(l1Recipient)) revert InvalidL1Address(l1Recipient);

        processedBurnRequests[burnId] = true;
        totalBridgeBurned += amount;

        _burn(msg.sender, amount);

        emit BridgeBurn(msg.sender, amount, l1Recipient, burnId, block.timestamp);
    }

    // ──────────────────────────────────────────────
    //  Guardian functions (emergency)
    // ──────────────────────────────────────────────

    /**
     * @notice Emergency pause — stops all minting and burning.
     * @param reason Human-readable reason for the pause
     */
    function emergencyPause(string calldata reason) external onlyRole(GUARDIAN_ROLE) {
        _pause();
        emit EmergencyPause(msg.sender, reason);
    }

    /**
     * @notice Unpause after emergency is resolved.
     */
    function emergencyUnpause() external onlyRole(GUARDIAN_ROLE) {
        _unpause();
        emit EmergencyUnpause(msg.sender);
    }

    // ──────────────────────────────────────────────
    //  View functions
    // ──────────────────────────────────────────────

    /**
     * @notice Returns the amount of wZION that can still be minted.
     */
    function mintableSupply() external view returns (uint256) {
        return MAX_SUPPLY - totalSupply();
    }

    /**
     * @notice Returns bridge statistics.
     */
    function bridgeStats() external view returns (
        uint256 minted,
        uint256 burned,
        uint256 outstanding,
        uint256 supply,
        uint256 maxSupply
    ) {
        return (
            totalBridgeMinted,
            totalBridgeBurned,
            totalBridgeMinted - totalBridgeBurned,
            totalSupply(),
            MAX_SUPPLY
        );
    }

    /**
     * @notice Check if an L1 lock transaction has already been processed.
     */
    function isL1LockProcessed(bytes32 l1TxHash) external view returns (bool) {
        return processedL1Locks[l1TxHash];
    }

    // ──────────────────────────────────────────────
    //  Internal helpers
    // ──────────────────────────────────────────────

    /**
     * @dev Basic ZION L1 address validation.
     *      ZION addresses start with "zion1" and are 44 characters (bech32).
     */
    function _isValidL1Address(string calldata addr) internal pure returns (bool) {
        bytes memory b = bytes(addr);
        if (b.length < 40 || b.length > 62) return false;

        // Must start with "zion1"
        if (b[0] != 'z' || b[1] != 'i' || b[2] != 'o' || b[3] != 'n' || b[4] != '1') {
            return false;
        }
        return true;
    }

    /**
     * @notice Returns the number of decimals (18, standard ERC-20).
     * @dev Note: ZION L1 uses 6 decimal places (1 ZION = 1,000,000 atomic units).
     *      The bridge relay handles the conversion (multiply by 1e12 on mint, divide on burn).
     */
    function decimals() public pure override returns (uint8) {
        return 18;
    }
}
