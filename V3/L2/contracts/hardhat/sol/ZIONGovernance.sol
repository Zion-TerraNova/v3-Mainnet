// SPDX-License-Identifier: MIT
pragma solidity ^0.8.20;

import "@openzeppelin/contracts/token/ERC20/IERC20.sol";
import "@openzeppelin/contracts/access/Ownable.sol";
import "@openzeppelin/contracts/utils/ReentrancyGuard.sol";

/// @dev Minimal interface to ZIONStaking — used for stake-weighted voting
interface IZIONStaking {
    function votingWeight(address account) external view returns (uint256);
}

/**
 * @title ZIONGovernance
 * @dev On-chain governance system for ZION blockchain
 * @notice Allows ZION token holders to create and vote on proposals
 */
contract ZIONGovernance is Ownable, ReentrancyGuard {
    IERC20 public zionToken;
    /// @notice Optional: ZIONStaking contract. When set, voting power includes staked wZION.
    IZIONStaking public stakingContract;
    uint256 public proposalThreshold = 1_000_000 * 1e18;  // 1M ZION to propose
    uint256 public votingPeriod = 7 days;
    uint256 public timelockDuration = 2 days;
    uint256 public quorumPercentage = 10;  // 10% of total supply
    
    enum ProposalState {
        Pending,
        Active,
        Canceled,
        Defeated,
        Succeeded,
        Queued,
        Executed
    }
    
    enum VoteType {
        Against,
        For,
        Abstain
    }
    
    struct Proposal {
        uint256 id;
        address proposer;
        string title;
        string description;
        string ipfsHash;  // Full proposal on IPFS
        
        address[] targets;    // Contracts to call
        uint256[] values;     // ETH values to send
        bytes[] calldatas;    // Function calls
        
        uint256 startBlock;
        uint256 endBlock;
        uint256 eta;          // Execution time (after timelock)
        
        uint256 forVotes;
        uint256 againstVotes;
        uint256 abstainVotes;
        
        bool canceled;
        bool executed;
    }
    
    struct Receipt {
        bool hasVoted;
        VoteType support;
        uint256 votes;
    }
    
    uint256 public proposalCount;
    mapping(uint256 => Proposal) public proposals;
    mapping(uint256 => mapping(address => Receipt)) public receipts;
    
    event ProposalCreated(
        uint256 indexed proposalId,
        address indexed proposer,
        string title,
        uint256 startBlock,
        uint256 endBlock
    );
    
    event VoteCast(
        address indexed voter,
        uint256 indexed proposalId,
        VoteType support,
        uint256 votes
    );
    
    event ProposalQueued(uint256 indexed proposalId, uint256 eta);
    event ProposalExecuted(uint256 indexed proposalId);
    event ProposalCanceled(uint256 indexed proposalId);
    
    event ParametersUpdated(
        uint256 proposalThreshold,
        uint256 votingPeriod,
        uint256 timelockDuration,
        uint256 quorumPercentage
    );
    
    constructor(address _zionToken) Ownable(msg.sender) {
        require(_zionToken != address(0), "Invalid token address");
        zionToken = IERC20(_zionToken);
    }

    /**
     * @notice Set or update the staking contract address.
     *         Once set, voting power = wZION balance + staked wZION.
     * @param _staking ZIONStaking contract address, or address(0) to disable.
     */
    function setStakingContract(address _staking) external onlyOwner {
        stakingContract = IZIONStaking(_staking);
    }

    /**
     * @dev Combined voting power: raw wZION holdings + staked wZION (if staking set).
     */
    function _votingPower(address account) internal view returns (uint256) {
        uint256 power = zionToken.balanceOf(account);
        if (address(stakingContract) != address(0)) {
            power += stakingContract.votingWeight(account);
        }
        return power;
    }
    
    /**
     * @dev Create a new governance proposal
     * @param _title Short title of the proposal
     * @param _description Brief description
     * @param _ipfsHash Hash of full proposal on IPFS
     * @param _targets Contract addresses to call
     * @param _values ETH values to send
     * @param _calldatas Encoded function calls
     * @return proposalId The ID of the created proposal
     */
    function propose(
        string memory _title,
        string memory _description,
        string memory _ipfsHash,
        address[] memory _targets,
        uint256[] memory _values,
        bytes[] memory _calldatas
    ) external returns (uint256) {
        require(
            _votingPower(msg.sender) >= proposalThreshold,
            "Insufficient ZION to propose"
        );
        require(
            _targets.length == _values.length && _targets.length == _calldatas.length,
            "Proposal function information arity mismatch"
        );
        require(_targets.length > 0, "Must provide actions");
        require(_targets.length <= 10, "Too many actions");
        require(bytes(_title).length > 0, "Title required");
        require(bytes(_ipfsHash).length > 0, "IPFS hash required");
        
        uint256 proposalId = ++proposalCount;
        Proposal storage proposal = proposals[proposalId];
        
        proposal.id = proposalId;
        proposal.proposer = msg.sender;
        proposal.title = _title;
        proposal.description = _description;
        proposal.ipfsHash = _ipfsHash;
        proposal.targets = _targets;
        proposal.values = _values;
        proposal.calldatas = _calldatas;
        proposal.startBlock = block.number + 1;
        proposal.endBlock = block.number + (votingPeriod / 12);  // ~12s per block
        
        emit ProposalCreated(
            proposalId,
            msg.sender,
            _title,
            proposal.startBlock,
            proposal.endBlock
        );
        
        return proposalId;
    }
    
    /**
     * @dev Cast a vote on a proposal
     * @param _proposalId ID of the proposal
     * @param _support Vote type (0=Against, 1=For, 2=Abstain)
     */
    function castVote(uint256 _proposalId, VoteType _support) external {
        return _castVote(msg.sender, _proposalId, _support);
    }
    
    /**
     * @dev Cast a vote using EIP-712 signature
     * @param _proposalId ID of the proposal
     * @param _support Vote type
     * @param v Signature parameter
     * @param r Signature parameter
     * @param s Signature parameter
     */
    function castVoteBySig(
        uint256 _proposalId,
        VoteType _support,
        uint8 v,
        bytes32 r,
        bytes32 s
    ) external {
        bytes32 domainSeparator = keccak256(
            abi.encode(
                keccak256("EIP712Domain(string name,uint256 chainId,address verifyingContract)"),
                keccak256(bytes("ZION Governance")),
                block.chainid,
                address(this)
            )
        );
        
        bytes32 structHash = keccak256(
            abi.encode(
                keccak256("Ballot(uint256 proposalId,uint8 support)"),
                _proposalId,
                uint8(_support)
            )
        );
        
        bytes32 digest = keccak256(abi.encodePacked("\x19\x01", domainSeparator, structHash));
        address signer = ecrecover(digest, v, r, s);
        require(signer != address(0), "Invalid signature");
        
        return _castVote(signer, _proposalId, _support);
    }
    
    /**
     * @dev Internal vote casting logic
     */
    function _castVote(
        address _voter,
        uint256 _proposalId,
        VoteType _support
    ) internal {
        require(_proposalId > 0 && _proposalId <= proposalCount, "Invalid proposal ID");
        require(state(_proposalId) == ProposalState.Active, "Voting is closed");
        
        Proposal storage proposal = proposals[_proposalId];
        Receipt storage receipt = receipts[_proposalId][_voter];
        
        require(!receipt.hasVoted, "Already voted");
        
        uint256 votes = _votingPower(_voter);
        require(votes > 0, "No voting power");
        
        if (_support == VoteType.Against) {
            proposal.againstVotes += votes;
        } else if (_support == VoteType.For) {
            proposal.forVotes += votes;
        } else if (_support == VoteType.Abstain) {
            proposal.abstainVotes += votes;
        }
        
        receipt.hasVoted = true;
        receipt.support = _support;
        receipt.votes = votes;
        
        emit VoteCast(_voter, _proposalId, _support, votes);
    }
    
    /**
     * @dev Queue a successful proposal for execution
     * @param _proposalId ID of the proposal
     */
    function queue(uint256 _proposalId) external {
        require(
            state(_proposalId) == ProposalState.Succeeded,
            "Proposal can only be queued if it is succeeded"
        );
        
        Proposal storage proposal = proposals[_proposalId];
        uint256 eta = block.timestamp + timelockDuration;
        proposal.eta = eta;
        
        emit ProposalQueued(_proposalId, eta);
    }
    
    /**
     * @dev Execute a queued proposal
     * @param _proposalId ID of the proposal
     */
    function execute(uint256 _proposalId) external payable nonReentrant {
        require(
            state(_proposalId) == ProposalState.Queued,
            "Proposal can only be executed if it is queued"
        );
        
        Proposal storage proposal = proposals[_proposalId];
        require(block.timestamp >= proposal.eta, "Timelock not expired");
        
        proposal.executed = true;
        
        for (uint256 i = 0; i < proposal.targets.length; i++) {
            (bool success, bytes memory returnData) = proposal.targets[i].call{
                value: proposal.values[i]
            }(proposal.calldatas[i]);
            
            require(success, string(returnData));
        }
        
        emit ProposalExecuted(_proposalId);
    }
    
    /**
     * @dev Cancel a proposal
     * @param _proposalId ID of the proposal
     */
    function cancel(uint256 _proposalId) external {
        require(_proposalId > 0 && _proposalId <= proposalCount, "Invalid proposal ID");
        
        ProposalState currentState = state(_proposalId);
        require(
            currentState != ProposalState.Executed,
            "Cannot cancel executed proposal"
        );
        
        Proposal storage proposal = proposals[_proposalId];
        require(
            msg.sender == proposal.proposer ||
            _votingPower(proposal.proposer) < proposalThreshold,
            "Proposer above threshold"
        );
        
        proposal.canceled = true;
        
        emit ProposalCanceled(_proposalId);
    }
    
    /**
     * @dev Get the current state of a proposal
     * @param _proposalId ID of the proposal
     * @return Current state of the proposal
     */
    function state(uint256 _proposalId) public view returns (ProposalState) {
        require(_proposalId > 0 && _proposalId <= proposalCount, "Invalid proposal ID");
        
        Proposal storage proposal = proposals[_proposalId];
        
        if (proposal.canceled) {
            return ProposalState.Canceled;
        } else if (block.number <= proposal.startBlock) {
            return ProposalState.Pending;
        } else if (block.number <= proposal.endBlock) {
            return ProposalState.Active;
        } else if (
            proposal.forVotes <= proposal.againstVotes ||
            proposal.forVotes < _getQuorum()
        ) {
            return ProposalState.Defeated;
        } else if (proposal.eta == 0) {
            return ProposalState.Succeeded;
        } else if (proposal.executed) {
            return ProposalState.Executed;
        } else {
            return ProposalState.Queued;
        }
    }
    
    /**
     * @dev Get quorum threshold
     * @return Minimum votes needed for quorum
     */
    function _getQuorum() internal view returns (uint256) {
        return (zionToken.totalSupply() * quorumPercentage) / 100;
    }
    
    /**
     * @dev Get proposal details
     * @param _proposalId ID of the proposal
     */
    function getProposal(uint256 _proposalId) external view returns (
        address proposer,
        string memory title,
        string memory description,
        string memory ipfsHash,
        uint256 forVotes,
        uint256 againstVotes,
        uint256 abstainVotes,
        ProposalState currentState
    ) {
        require(_proposalId > 0 && _proposalId <= proposalCount, "Invalid proposal ID");
        
        Proposal storage p = proposals[_proposalId];
        return (
            p.proposer,
            p.title,
            p.description,
            p.ipfsHash,
            p.forVotes,
            p.againstVotes,
            p.abstainVotes,
            state(_proposalId)
        );
    }
    
    /**
     * @dev Get proposal actions
     * @param _proposalId ID of the proposal
     */
    function getProposalActions(uint256 _proposalId) external view returns (
        address[] memory targets,
        uint256[] memory values,
        bytes[] memory calldatas
    ) {
        require(_proposalId > 0 && _proposalId <= proposalCount, "Invalid proposal ID");
        
        Proposal storage p = proposals[_proposalId];
        return (p.targets, p.values, p.calldatas);
    }
    
    /**
     * @dev Get vote receipt for an address
     * @param _proposalId ID of the proposal
     * @param _voter Address of the voter
     */
    function getReceipt(uint256 _proposalId, address _voter) external view returns (
        bool hasVoted,
        VoteType support,
        uint256 votes
    ) {
        Receipt storage receipt = receipts[_proposalId][_voter];
        return (receipt.hasVoted, receipt.support, receipt.votes);
    }
    
    /**
     * @dev Update governance parameters (only via governance)
     */
    function updateParameters(
        uint256 _proposalThreshold,
        uint256 _votingPeriod,
        uint256 _timelockDuration,
        uint256 _quorumPercentage
    ) external onlyOwner {
        require(_proposalThreshold > 0, "Invalid threshold");
        require(_votingPeriod >= 1 days, "Voting period too short");
        require(_timelockDuration >= 1 days, "Timelock too short");
        require(_quorumPercentage > 0 && _quorumPercentage <= 100, "Invalid quorum");
        
        proposalThreshold = _proposalThreshold;
        votingPeriod = _votingPeriod;
        timelockDuration = _timelockDuration;
        quorumPercentage = _quorumPercentage;
        
        emit ParametersUpdated(
            _proposalThreshold,
            _votingPeriod,
            _timelockDuration,
            _quorumPercentage
        );
    }
    
    /**
     * @dev Accept ETH for treasury
     */
    receive() external payable {}
}
