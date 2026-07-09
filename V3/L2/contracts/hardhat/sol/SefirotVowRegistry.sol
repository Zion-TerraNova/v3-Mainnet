// SPDX-License-Identifier: MIT
pragma solidity ^0.8.20;

import "@openzeppelin/contracts/access/Ownable.sol";
import "@openzeppelin/contracts/utils/ReentrancyGuard.sol";
import "./SefirotVowToken.sol";

/**
 * @title SefirotVowRegistry
 * @dev On-chain registry for Sefirot Vow proposals — the ceremony layer.
 *
 * Lifecycle of a vow:
 *   1. submitProposal()  — 2 sponsoring validators submit a candidate
 *   2. 7-day review      — existing validators attest "witness" or "object"
 *   3. Auto-confirmation — if no objection after 7 days, anyone calls confirm()
 *   4. mint()            — registry mints SefirotVowToken to candidate
 *
 * If objection: emergency review, objection must be resolved off-chain,
 * then a new 7-day period starts (resubmit).
 *
 * See: V3/L5/docs/GOVERNANCE/sefirot-vow.md §4.1 On-chain ceremony
 */
contract SefirotVowRegistry is Ownable, ReentrancyGuard {

    // ══════════════════════════════════════════════════════════════════
    // Types
    // ══════════════════════════════════════════════════════════════════

    enum ProposalState {
        Pending,     // 0 — in 7-day review
        Confirmed,   // 1 — vow minted
        Objection,   // 2 — objection raised, needs resubmit
        Expired      // 3 — review period lapsed without confirmation
    }

    struct VowProposal {
        address candidate;
        uint8 validatorClass;
        bytes32 vowHash;
        address sponsor1;
        address sponsor2;
        uint64 submittedAt;
        uint64 reviewEndsAt;
        uint32 witnessCount;
        uint32 objectionCount;
        ProposalState state;
    }

    // ══════════════════════════════════════════════════════════════════
    // Storage
    // ══════════════════════════════════════════════════════════════════

    SefirotVowToken public immutable vowToken;

    /// @dev Review period (seconds)
    uint64 public constant REVIEW_PERIOD = 7 days;

    /// @dev Required sponsors per proposal
    uint256 public constant REQUIRED_SPONSORS = 2;

    uint256 public proposalCount;
    mapping(uint256 => VowProposal) public proposals;

    /// @dev Tracks which validators have witnessed a given proposal
    mapping(uint256 => mapping(address => bool)) public hasWitnessed;
    mapping(uint256 => mapping(address => bool)) public hasObjected;

    /// @dev Set of validators authorized to witness/object (initially: those
    ///      who already hold an active vow). Owner can add/remove during bootstrap.
    mapping(address => bool) public isAuthorizedValidator;

    // ══════════════════════════════════════════════════════════════════
    // Events
    // ══════════════════════════════════════════════════════════════════

    event ProposalSubmitted(uint256 indexed proposalId, address indexed candidate, uint8 validatorClass, bytes32 vowHash);
    event Witnessed(uint256 indexed proposalId, address indexed witness);
    event Objected(uint256 indexed proposalId, address indexed objector, string reason);
    event ProposalConfirmed(uint256 indexed proposalId, uint256 indexed tokenId);
    event ProposalExpired(uint256 indexed proposalId);
    event ValidatorAuthorized(address indexed validator, bool authorized);

    // ══════════════════════════════════════════════════════════════════
    // Constructor
    // ══════════════════════════════════════════════════════════════════

    constructor(address _vowToken) Ownable(msg.sender) {
        vowToken = SefirotVowToken(_vowToken);
    }

    // ══════════════════════════════════════════════════════════════════
    // Admin — validator authorization (bootstrap)
    // ══════════════════════════════════════════════════════════════════

    function setAuthorizedValidator(address validator, bool authorized) external onlyOwner {
        isAuthorizedValidator[validator] = authorized;
        emit ValidatorAuthorized(validator, authorized);
    }

    // ══════════════════════════════════════════════════════════════════
    // Submit proposal
    // ══════════════════════════════════════════════════════════════════

    /**
     * @notice Submit a Sefirot Vow proposal for a candidate validator.
     * @param candidate     Address of the validator taking the vow.
     * @param validatorClass 0=L1Miner, 1=L2Guardian, 2=L3Warp, 3=L3AI, 4=PoC
     * @param vowHash       BLAKE3 hash of the vow text.
     * @param sponsor2      Second sponsoring validator (msg.sender is sponsor1).
     */
    function submitProposal(
        address candidate,
        uint8 validatorClass,
        bytes32 vowHash,
        address sponsor2
    ) external nonReentrant returns (uint256) {
        require(isAuthorizedValidator[msg.sender], "SefirotVowRegistry: sponsor1 not authorized");
        require(isAuthorizedValidator[sponsor2], "SefirotVowRegistry: sponsor2 not authorized");
        require(msg.sender != sponsor2, "SefirotVowRegistry: sponsors must differ");
        require(candidate != address(0), "SefirotVowRegistry: zero candidate");
        require(validatorClass <= 4, "SefirotVowRegistry: invalid class");
        require(vowToken.hasActiveVow(candidate) == false, "SefirotVowRegistry: already has vow");

        uint256 proposalId = ++proposalCount;
        uint64 now_ = uint64(block.timestamp);

        proposals[proposalId] = VowProposal({
            candidate: candidate,
            validatorClass: validatorClass,
            vowHash: vowHash,
            sponsor1: msg.sender,
            sponsor2: sponsor2,
            submittedAt: now_,
            reviewEndsAt: now_ + REVIEW_PERIOD,
            witnessCount: 0,
            objectionCount: 0,
            state: ProposalState.Pending
        });

        emit ProposalSubmitted(proposalId, candidate, validatorClass, vowHash);
        return proposalId;
    }

    // ══════════════════════════════════════════════════════════════════
    // Witness / Object
    // ══════════════════════════════════════════════════════════════════

    function witness(uint256 proposalId) external nonReentrant {
        require(isAuthorizedValidator[msg.sender], "SefirotVowRegistry: not authorized");
        VowProposal storage p = proposals[proposalId];
        require(p.state == ProposalState.Pending, "SefirotVowRegistry: not pending");
        require(!hasWitnessed[proposalId][msg.sender], "SefirotVowRegistry: already witnessed");
        require(!hasObjected[proposalId][msg.sender], "SefirotVowRegistry: already objected");

        hasWitnessed[proposalId][msg.sender] = true;
        p.witnessCount++;
        emit Witnessed(proposalId, msg.sender);
    }

    function object(uint256 proposalId, string calldata reason) external nonReentrant {
        require(isAuthorizedValidator[msg.sender], "SefirotVowRegistry: not authorized");
        VowProposal storage p = proposals[proposalId];
        require(p.state == ProposalState.Pending, "SefirotVowRegistry: not pending");
        require(!hasObjected[proposalId][msg.sender], "SefirotVowRegistry: already objected");

        hasObjected[proposalId][msg.sender] = true;
        p.objectionCount++;
        p.state = ProposalState.Objection;
        emit Objected(proposalId, msg.sender, reason);
    }

    // ══════════════════════════════════════════════════════════════════
    // Confirm — after 7 days with no objection
    // ══════════════════════════════════════════════════════════════════

    /**
     * @notice Confirm a proposal after the review period. Anyone can call.
     *         Mints the SefirotVowToken to the candidate.
     */
    function confirm(uint256 proposalId) external nonReentrant returns (uint256) {
        VowProposal storage p = proposals[proposalId];
        require(p.state == ProposalState.Pending, "SefirotVowRegistry: not pending");
        require(block.timestamp >= p.reviewEndsAt, "SefirotVowRegistry: review period not ended");

        p.state = ProposalState.Confirmed;
        uint256 tokenId = vowToken.mint(p.candidate, p.validatorClass, p.vowHash);

        // Auto-authorize the new validator so they can witness future proposals
        isAuthorizedValidator[p.candidate] = true;

        emit ProposalConfirmed(proposalId, tokenId);
        return tokenId;
    }

    // ══════════════════════════════════════════════════════════════════
    // Expire — mark old pending proposals as expired (housekeeping)
    // ══════════════════════════════════════════════════════════════════

    function expire(uint256 proposalId) external {
        VowProposal storage p = proposals[proposalId];
        require(p.state == ProposalState.Pending, "SefirotVowRegistry: not pending");
        require(block.timestamp >= p.reviewEndsAt + 30 days, "SefirotVowRegistry: too early to expire");
        p.state = ProposalState.Expired;
        emit ProposalExpired(proposalId);
    }

    // ══════════════════════════════════════════════════════════════════
    // View
    // ══════════════════════════════════════════════════════════════════

    function getProposal(uint256 proposalId) external view returns (VowProposal memory) {
        return proposals[proposalId];
    }

    function proposalState(uint256 proposalId) external view returns (ProposalState) {
        return proposals[proposalId].state;
    }
}
