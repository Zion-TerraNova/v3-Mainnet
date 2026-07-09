// SPDX-License-Identifier: MIT
pragma solidity ^0.8.20;

import "@openzeppelin/contracts/token/ERC721/ERC721.sol";
import "@openzeppelin/contracts/access/Ownable.sol";
import "@openzeppelin/contracts/utils/ReentrancyGuard.sol";

/**
 * @title SefirotVowToken
 * @dev Soulbound (non-transferable) ERC-721 token representing a ZION
 *      validator's Sefirot Vow — 11 vows (10 sephirot + Da'at) structured
 *      pledge of care for the protocol.
 *
 * @notice See V3/L5/docs/GOVERNANCE/sefirot-vow.md for the full vow text
 *         and ceremony protocol.
 *
 * Lifecycle:
 *   1. mint()  — called by Governance (or owner) after 7-day DAO review
 *   2. renew() — validator re-signs vow hash annually
 *   3. suspend() — first break (30-day grace)
 *   4. revoke() — permanent revocation after 3 suspensions or refusal to renew
 *
 * Soulbound: _update() overridden to block all transfers except mint/burn.
 */
contract SefirotVowToken is ERC721, Ownable, ReentrancyGuard {

    // ══════════════════════════════════════════════════════════════════
    // Types
    // ══════════════════════════════════════════════════════════════════

    enum ValidatorClass {
        L1Miner,      // 0
        L2Guardian,   // 1
        L3Warp,       // 2
        L3AI,         // 3
        PoC           // 4 — future Proof-of-Care
    }

    enum VowState {
        Active,       // 0 — vow in good standing
        Suspended,    // 1 — broken, 30-day grace to renew
        Revoked       // 2 — permanently revoked
    }

    struct Vow {
        address validator;
        ValidatorClass validatorClass;
        bytes32 vowHash;          // BLAKE3(vow_text_in_validator_native_language)
        uint64 mintedAt;
        uint64 lastRenewedAt;
        uint64 suspensionCount;
        VowState state;
    }

    // ══════════════════════════════════════════════════════════════════
    // Storage
    // ══════════════════════════════════════════════════════════════════

    /// @dev Authorized minter — typically ZIONGovernance contract
    address public authorizedMinter;

    /// @dev Annual renewal period (seconds)
    uint256 public constant RENEWAL_PERIOD = 365 days;

    /// @dev Grace period after suspension before revocation
    uint256 public constant GRACE_PERIOD = 30 days;

    /// @dev Max suspensions before auto-revocation
    uint256 public constant MAX_SUSPENSIONS = 3;

    /// @dev Cooldown after permanent revocation before re-vow allowed
    uint256 public constant REVOVATION_COOLDOWN = 365 days;

    uint256 private _nextTokenId = 1;

    mapping(uint256 => Vow) public vows;
    mapping(address => uint256) public validatorToTokenId; // 0 = no vow
    mapping(address => uint64) public revokedAt;           // 0 = never

    // ══════════════════════════════════════════════════════════════════
    // Events
    // ══════════════════════════════════════════════════════════════════

    event VowMinted(uint256 indexed tokenId, address indexed validator, ValidatorClass class, bytes32 vowHash);
    event VowRenewed(uint256 indexed tokenId, bytes32 newVowHash, uint64 renewedAt);
    event VowSuspended(uint256 indexed tokenId, string reason);
    event VowRevoked(uint256 indexed tokenId, string reason);
    event AuthorizedMinterUpdated(address indexed oldMinter, address indexed newMinter);

    // ══════════════════════════════════════════════════════════════════
    // Constructor
    // ══════════════════════════════════════════════════════════════════

    constructor(address _authorizedMinter) ERC721("ZION Sefirot Vow", "SEFIROT-VOW") Ownable(msg.sender) {
        authorizedMinter = _authorizedMinter;
        emit AuthorizedMinterUpdated(address(0), _authorizedMinter);
    }

    // ══════════════════════════════════════════════════════════════════
    // Modifiers
    // ══════════════════════════════════════════════════════════════════

    modifier onlyAuthorizedMinter() {
        require(msg.sender == authorizedMinter || msg.sender == owner(), "SefirotVow: not authorized");
        _;
    }

    // ══════════════════════════════════════════════════════════════════
    // Soulbound core — block all transfers
    // ══════════════════════════════════════════════════════════════════

    /**
     * @dev Overridden to make token soulbound (non-transferable).
     *      Only mint (from == 0) and burn (to == 0) are allowed.
     */
    function _update(address to, uint256 tokenId, address auth) internal override returns (address) {
        address from = _ownerOf(tokenId);
        require(
            from == address(0) || to == address(0),
            "SefirotVow: soulbound - non-transferable"
        );
        return super._update(to, tokenId, auth);
    }

    // ══════════════════════════════════════════════════════════════════
    // Mint — called after 7-day DAO review
    // ══════════════════════════════════════════════════════════════════

    /**
     * @notice Mint a Sefirot Vow soulbound token to a validator.
     * @param validator  Address of the validator taking the vow.
     * @param classId    Validator class (0=L1Miner, 1=L2Guardian, 2=L3Warp, 3=L3AI, 4=PoC).
     * @param vowHash    BLAKE3 hash of the vow text in the validator's native language.
     */
    function mint(address validator, uint8 classId, bytes32 vowHash) external onlyAuthorizedMinter nonReentrant returns (uint256) {
        require(validator != address(0), "SefirotVow: zero address");
        require(validatorToTokenId[validator] == 0, "SefirotVow: already has vow");
        require(classId <= uint8(ValidatorClass.PoC), "SefirotVow: invalid class");

        // Check revocation cooldown
        uint64 revokedTime = revokedAt[validator];
        if (revokedTime > 0) {
            require(
                block.timestamp >= revokedTime + REVOVATION_COOLDOWN,
                "SefirotVow: revocation cooldown active"
            );
            delete revokedAt[validator];
        }

        uint256 tokenId = _nextTokenId++;
        vows[tokenId] = Vow({
            validator: validator,
            validatorClass: ValidatorClass(classId),
            vowHash: vowHash,
            mintedAt: uint64(block.timestamp),
            lastRenewedAt: uint64(block.timestamp),
            suspensionCount: 0,
            state: VowState.Active
        });
        validatorToTokenId[validator] = tokenId;

        _mint(validator, tokenId);
        emit VowMinted(tokenId, validator, ValidatorClass(classId), vowHash);
        return tokenId;
    }

    // ══════════════════════════════════════════════════════════════════
    // Renew — annual re-signing of the vow
    // ══════════════════════════════════════════════════════════════════

    /**
     * @notice Renew the vow. Validator re-signs the vow hash.
     * @param newVowHash  Updated BLAKE3 hash (may be same text or renewed).
     */
    function renew(bytes32 newVowHash) external nonReentrant {
        uint256 tokenId = validatorToTokenId[msg.sender];
        require(tokenId != 0, "SefirotVow: no vow");
        Vow storage vow = vows[tokenId];
        require(vow.state != VowState.Revoked, "SefirotVow: revoked");

        vow.vowHash = newVowHash;
        vow.lastRenewedAt = uint64(block.timestamp);
        if (vow.state == VowState.Suspended) {
            vow.state = VowState.Active; // renewal clears suspension
        }

        emit VowRenewed(tokenId, newVowHash, vow.lastRenewedAt);
    }

    // ══════════════════════════════════════════════════════════════════
    // Suspend — first break (grace period)
    // ══════════════════════════════════════════════════════════════════

    /**
     * @notice Suspend a validator's vow (first break). Called by Governance
     *         or owner. 30-day grace to renew before auto-revocation.
     * @param validator  Address of the validator.
     * @param reason     Human-readable reason for suspension.
     */
    function suspend(address validator, string calldata reason) external onlyAuthorizedMinter nonReentrant {
        uint256 tokenId = validatorToTokenId[validator];
        require(tokenId != 0, "SefirotVow: no vow");
        Vow storage vow = vows[tokenId];
        require(vow.state != VowState.Revoked, "SefirotVow: already revoked");

        vow.suspensionCount++;
        vow.state = VowState.Suspended;

        emit VowSuspended(tokenId, reason);

        // Auto-revoke after MAX_SUSPENSIONS
        if (vow.suspensionCount >= MAX_SUSPENSIONS) {
            _revoke(tokenId, "Max suspensions exceeded");
        }
    }

    // ══════════════════════════════════════════════════════════════════
    // Revoke — permanent revocation
    // ══════════════════════════════════════════════════════════════════

    /**
     * @notice Revoke a vow permanently. Called by Governance or owner.
     * @param validator  Address of the validator.
     * @param reason     Human-readable reason.
     */
    function revoke(address validator, string calldata reason) external onlyAuthorizedMinter nonReentrant {
        uint256 tokenId = validatorToTokenId[validator];
        require(tokenId != 0, "SefirotVow: no vow");
        Vow storage vow = vows[tokenId];
        require(vow.state != VowState.Revoked, "SefirotVow: already revoked");
        _revoke(tokenId, reason);
    }

    function _revoke(uint256 tokenId, string memory reason) internal {
        Vow storage vow = vows[tokenId];
        address validator = vow.validator;
        vow.state = VowState.Revoked;
        revokedAt[validator] = uint64(block.timestamp);
        delete validatorToTokenId[validator];
        _burn(tokenId);
        emit VowRevoked(tokenId, reason);
    }

    // ══════════════════════════════════════════════════════════════════
    // View functions
    // ══════════════════════════════════════════════════════════════════

    function getVow(address validator) external view returns (Vow memory) {
        uint256 tokenId = validatorToTokenId[validator];
        require(tokenId != 0, "SefirotVow: no vow");
        return vows[tokenId];
    }

    function hasActiveVow(address validator) external view returns (bool) {
        uint256 tokenId = validatorToTokenId[validator];
        if (tokenId == 0) return false;
        return vows[tokenId].state == VowState.Active;
    }

    function isRenewalDue(address validator) external view returns (bool) {
        uint256 tokenId = validatorToTokenId[validator];
        if (tokenId == 0) return false;
        Vow storage vow = vows[tokenId];
        if (vow.state == VowState.Revoked) return false;
        return block.timestamp >= vow.lastRenewedAt + RENEWAL_PERIOD;
    }

    function totalVowsMinted() external view returns (uint256) {
        return _nextTokenId - 1;
    }

    // ══════════════════════════════════════════════════════════════════
    // Admin
    // ══════════════════════════════════════════════════════════════════

    function setAuthorizedMinter(address newMinter) external onlyOwner {
        emit AuthorizedMinterUpdated(authorizedMinter, newMinter);
        authorizedMinter = newMinter;
    }
}
