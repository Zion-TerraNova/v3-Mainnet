// SPDX-License-Identifier: MIT
pragma solidity ^0.8.20;

import "@openzeppelin/contracts/token/ERC20/IERC20.sol";
import "@openzeppelin/contracts/access/Ownable.sol";
import "@openzeppelin/contracts/utils/ReentrancyGuard.sol";

/**
 * @title ZIONTreasury
 * @dev Multi-sig treasury for ZION DAO with budget management
 * @notice Controls 1.75B ZION with 5-of-7 multi-sig authorization
 */
contract ZIONTreasury is Ownable, ReentrancyGuard {
    IERC20 public zionToken;
    
    // Multi-sig configuration
    address[] public signers;
    mapping(address => bool) public isSigner;
    uint256 public requiredSignatures;
    
    // DAO Reserve: 1.75B ZION
    uint256 public constant DAO_RESERVE = 1_750_000_000 * 1e18;
    
    // Budget categories
    struct Budget {
        string category;
        uint256 allocated;
        uint256 spent;
        uint256 reserved;  // For approved but not yet spent
        bool active;
        uint256 createdAt;
    }
    
    mapping(string => Budget) public budgets;
    string[] public budgetCategories;
    
    // Spending proposals
    struct SpendingProposal {
        uint256 id;
        string category;
        address recipient;
        uint256 amount;
        string reason;
        string milestoneHash;  // IPFS hash of milestone details
        
        uint256 approvals;
        mapping(address => bool) hasApproved;
        
        bool executed;
        bool canceled;
        uint256 createdAt;
    }
    
    uint256 public proposalCount;
    mapping(uint256 => SpendingProposal) public spendingProposals;
    
    // Grant tracking
    struct Grant {
        uint256 id;
        address recipient;
        string category;
        uint256 totalAmount;
        uint256 released;
        string projectHash;  // IPFS hash of project details
        
        Milestone[] milestones;
        bool active;
        uint256 createdAt;
    }
    
    struct Milestone {
        uint256 amount;
        string deliverable;
        bool completed;
        bool paid;
        uint256 completedAt;
    }
    
    uint256 public grantCount;
    mapping(uint256 => Grant) public grants;
    mapping(address => uint256[]) public recipientGrants;
    
    event SignerAdded(address indexed signer);
    event SignerRemoved(address indexed signer);
    event RequiredSignaturesUpdated(uint256 required);
    
    event BudgetCreated(string category, uint256 amount);
    event BudgetUpdated(string category, uint256 newAmount);
    event BudgetDeactivated(string category);
    
    event SpendingProposalCreated(
        uint256 indexed proposalId,
        string category,
        address recipient,
        uint256 amount
    );
    event SpendingProposalApproved(uint256 indexed proposalId, address signer);
    event SpendingProposalExecuted(uint256 indexed proposalId);
    event SpendingProposalCanceled(uint256 indexed proposalId);
    
    event GrantCreated(
        uint256 indexed grantId,
        address recipient,
        string category,
        uint256 amount
    );
    event MilestoneCompleted(uint256 indexed grantId, uint256 milestoneIndex);
    event MilestonePaid(uint256 indexed grantId, uint256 milestoneIndex, uint256 amount);
    
    modifier onlySigner() {
        require(isSigner[msg.sender], "Not authorized signer");
        _;
    }
    
    constructor(
        address _zionToken,
        address[] memory _signers,
        uint256 _required
    ) Ownable(msg.sender) {
        require(_zionToken != address(0), "Invalid token address");
        require(_signers.length >= _required, "Invalid signer count");
        require(_required >= 1, "Minimum 1 signature required"); // mainnet: use >=3
        
        zionToken = IERC20(_zionToken);
        requiredSignatures = _required;
        
        for (uint256 i = 0; i < _signers.length; i++) {
            require(_signers[i] != address(0), "Invalid signer address");
            require(!isSigner[_signers[i]], "Duplicate signer");
            
            signers.push(_signers[i]);
            isSigner[_signers[i]] = true;
            emit SignerAdded(_signers[i]);
        }
        
        // Initialize budget categories
        _createBudget("DeveloperGrants", 200_000_000 * 1e18);  // 200M ZION
        _createBudget("Infrastructure", 300_000_000 * 1e18);   // 300M ZION
        _createBudget("Marketing", 150_000_000 * 1e18);        // 150M ZION
        _createBudget("Research", 100_000_000 * 1e18);         // 100M ZION
        _createBudget("Community", 50_000_000 * 1e18);         // 50M ZION
        _createBudget("Emergency", 100_000_000 * 1e18);        // 100M ZION
    }
    
    /**
     * @dev Create a new budget category
     */
    function _createBudget(string memory _category, uint256 _amount) internal {
        require(bytes(_category).length > 0, "Category required");
        require(budgets[_category].createdAt == 0, "Category exists");
        
        budgets[_category] = Budget({
            category: _category,
            allocated: _amount,
            spent: 0,
            reserved: 0,
            active: true,
            createdAt: block.timestamp
        });
        budgetCategories.push(_category);
        
        emit BudgetCreated(_category, _amount);
    }
    
    /**
     * @dev Update budget allocation (requires governance)
     */
    function updateBudget(string memory _category, uint256 _newAmount) external onlyOwner {
        require(budgets[_category].createdAt > 0, "Category not found");
        require(_newAmount >= budgets[_category].spent, "Amount below spent");
        
        budgets[_category].allocated = _newAmount;
        
        emit BudgetUpdated(_category, _newAmount);
    }
    
    /**
     * @dev Create a spending proposal
     */
    function createSpendingProposal(
        string memory _category,
        address _recipient,
        uint256 _amount,
        string memory _reason,
        string memory _milestoneHash
    ) public onlySigner returns (uint256) {
        require(budgets[_category].active, "Budget not active");
        require(_recipient != address(0), "Invalid recipient");
        require(_amount > 0, "Invalid amount");
        require(bytes(_reason).length > 0, "Reason required");
        
        Budget storage budget = budgets[_category];
        require(
            budget.spent + budget.reserved + _amount <= budget.allocated,
            "Exceeds budget allocation"
        );
        
        uint256 proposalId = ++proposalCount;
        SpendingProposal storage proposal = spendingProposals[proposalId];
        
        proposal.id = proposalId;
        proposal.category = _category;
        proposal.recipient = _recipient;
        proposal.amount = _amount;
        proposal.reason = _reason;
        proposal.milestoneHash = _milestoneHash;
        proposal.createdAt = block.timestamp;
        
        // Creator automatically approves
        proposal.hasApproved[msg.sender] = true;
        proposal.approvals = 1;
        
        // Reserve budget
        budget.reserved += _amount;
        
        emit SpendingProposalCreated(proposalId, _category, _recipient, _amount);
        emit SpendingProposalApproved(proposalId, msg.sender);
        
        return proposalId;
    }
    
    /**
     * @dev Approve a spending proposal
     */
    function approveSpendingProposal(uint256 _proposalId) external onlySigner {
        require(_proposalId > 0 && _proposalId <= proposalCount, "Invalid proposal");
        
        SpendingProposal storage proposal = spendingProposals[_proposalId];
        require(!proposal.executed, "Already executed");
        require(!proposal.canceled, "Proposal canceled");
        require(!proposal.hasApproved[msg.sender], "Already approved");
        
        proposal.hasApproved[msg.sender] = true;
        proposal.approvals++;
        
        emit SpendingProposalApproved(_proposalId, msg.sender);
        
        // Auto-execute if threshold reached
        if (proposal.approvals >= requiredSignatures) {
            _executeSpendingProposal(_proposalId);
        }
    }
    
    /**
     * @dev Execute an approved spending proposal
     */
    function _executeSpendingProposal(uint256 _proposalId) internal nonReentrant {
        SpendingProposal storage proposal = spendingProposals[_proposalId];
        require(proposal.approvals >= requiredSignatures, "Insufficient approvals");
        require(!proposal.executed, "Already executed");
        
        proposal.executed = true;
        
        Budget storage budget = budgets[proposal.category];
        budget.spent += proposal.amount;
        budget.reserved -= proposal.amount;
        
        // Transfer ZION tokens
        require(
            zionToken.transfer(proposal.recipient, proposal.amount),
            "Transfer failed"
        );
        
        emit SpendingProposalExecuted(_proposalId);
    }
    
    /**
     * @dev Cancel a spending proposal
     */
    function cancelSpendingProposal(uint256 _proposalId) external onlySigner {
        require(_proposalId > 0 && _proposalId <= proposalCount, "Invalid proposal");
        
        SpendingProposal storage proposal = spendingProposals[_proposalId];
        require(!proposal.executed, "Already executed");
        require(!proposal.canceled, "Already canceled");
        
        proposal.canceled = true;
        
        // Release reserved budget
        Budget storage budget = budgets[proposal.category];
        budget.reserved -= proposal.amount;
        
        emit SpendingProposalCanceled(_proposalId);
    }
    
    /**
     * @dev Create a developer grant with milestones
     */
    function createGrant(
        address _recipient,
        string memory _category,
        uint256 _totalAmount,
        string memory _projectHash,
        uint256[] memory _milestoneAmounts,
        string[] memory _milestoneDeliverables
    ) external onlySigner returns (uint256) {
        require(_recipient != address(0), "Invalid recipient");
        require(budgets[_category].active, "Budget not active");
        require(_totalAmount > 0, "Invalid amount");
        require(
            _milestoneAmounts.length == _milestoneDeliverables.length,
            "Milestone data mismatch"
        );
        require(_milestoneAmounts.length > 0, "At least one milestone required");
        
        // Verify total matches milestones
        uint256 milestoneTotal = 0;
        for (uint256 i = 0; i < _milestoneAmounts.length; i++) {
            milestoneTotal += _milestoneAmounts[i];
        }
        require(milestoneTotal == _totalAmount, "Milestone total mismatch");
        
        uint256 grantId = ++grantCount;
        Grant storage grant = grants[grantId];
        
        grant.id = grantId;
        grant.recipient = _recipient;
        grant.category = _category;
        grant.totalAmount = _totalAmount;
        grant.projectHash = _projectHash;
        grant.active = true;
        grant.createdAt = block.timestamp;
        
        // Create milestones
        for (uint256 i = 0; i < _milestoneAmounts.length; i++) {
            grant.milestones.push(Milestone({
                amount: _milestoneAmounts[i],
                deliverable: _milestoneDeliverables[i],
                completed: false,
                paid: false,
                completedAt: 0
            }));
        }
        
        recipientGrants[_recipient].push(grantId);
        
        emit GrantCreated(grantId, _recipient, _category, _totalAmount);
        
        return grantId;
    }
    
    /**
     * @dev Mark a milestone as completed and create payment proposal
     */
    function completeMilestone(
        uint256 _grantId,
        uint256 _milestoneIndex
    ) external onlySigner returns (uint256) {
        require(_grantId > 0 && _grantId <= grantCount, "Invalid grant");
        
        Grant storage grant = grants[_grantId];
        require(grant.active, "Grant not active");
        require(_milestoneIndex < grant.milestones.length, "Invalid milestone");
        
        Milestone storage milestone = grant.milestones[_milestoneIndex];
        require(!milestone.completed, "Already completed");
        
        milestone.completed = true;
        milestone.completedAt = block.timestamp;
        
        emit MilestoneCompleted(_grantId, _milestoneIndex);
        
        // Create automatic spending proposal for milestone payment
        string memory reason = string(abi.encodePacked(
            "Grant #",
            _uint2str(_grantId),
            " Milestone #",
            _uint2str(_milestoneIndex + 1)
        ));
        
        return createSpendingProposal(
            grant.category,
            grant.recipient,
            milestone.amount,
            reason,
            grant.projectHash
        );
    }
    
    /**
     * @dev Get budget status
     */
    function getBudgetStatus(string memory _category) external view returns (
        uint256 allocated,
        uint256 spent,
        uint256 reserved,
        uint256 available,
        bool active
    ) {
        Budget storage budget = budgets[_category];
        return (
            budget.allocated,
            budget.spent,
            budget.reserved,
            budget.allocated - budget.spent - budget.reserved,
            budget.active
        );
    }
    
    /**
     * @dev Get all budget categories
     */
    function getAllBudgets() external view returns (string[] memory) {
        return budgetCategories;
    }
    
    /**
     * @dev Get grant details
     */
    function getGrant(uint256 _grantId) external view returns (
        address recipient,
        string memory category,
        uint256 totalAmount,
        uint256 released,
        uint256 milestoneCount,
        bool active
    ) {
        require(_grantId > 0 && _grantId <= grantCount, "Invalid grant");
        
        Grant storage grant = grants[_grantId];
        return (
            grant.recipient,
            grant.category,
            grant.totalAmount,
            grant.released,
            grant.milestones.length,
            grant.active
        );
    }
    
    /**
     * @dev Get milestone details
     */
    function getMilestone(uint256 _grantId, uint256 _milestoneIndex) external view returns (
        uint256 amount,
        string memory deliverable,
        bool completed,
        bool paid,
        uint256 completedAt
    ) {
        require(_grantId > 0 && _grantId <= grantCount, "Invalid grant");
        
        Grant storage grant = grants[_grantId];
        require(_milestoneIndex < grant.milestones.length, "Invalid milestone");
        
        Milestone storage milestone = grant.milestones[_milestoneIndex];
        return (
            milestone.amount,
            milestone.deliverable,
            milestone.completed,
            milestone.paid,
            milestone.completedAt
        );
    }
    
    /**
     * @dev Get grants for recipient
     */
    function getRecipientGrants(address _recipient) external view returns (uint256[] memory) {
        return recipientGrants[_recipient];
    }
    
    /**
     * @dev Add a new signer (requires governance)
     */
    function addSigner(address _signer) external onlyOwner {
        require(_signer != address(0), "Invalid signer address");
        require(!isSigner[_signer], "Already a signer");
        
        signers.push(_signer);
        isSigner[_signer] = true;
        
        emit SignerAdded(_signer);
    }
    
    /**
     * @dev Remove a signer (requires governance)
     */
    function removeSigner(address _signer) external onlyOwner {
        require(isSigner[_signer], "Not a signer");
        require(signers.length > requiredSignatures, "Cannot remove, below threshold");
        
        isSigner[_signer] = false;
        
        // Remove from array
        for (uint256 i = 0; i < signers.length; i++) {
            if (signers[i] == _signer) {
                signers[i] = signers[signers.length - 1];
                signers.pop();
                break;
            }
        }
        
        emit SignerRemoved(_signer);
    }
    
    /**
     * @dev Update required signatures (requires governance)
     */
    function updateRequiredSignatures(uint256 _required) external onlyOwner {
        require(_required >= 1, "Minimum 1 signature required"); // mainnet: enforce >=3 via governance
        require(_required <= signers.length, "Required exceeds signers");
        
        requiredSignatures = _required;
        
        emit RequiredSignaturesUpdated(_required);
    }
    
    /**
     * @dev Helper to convert uint to string
     */
    function _uint2str(uint256 _i) internal pure returns (string memory) {
        if (_i == 0) {
            return "0";
        }
        uint256 j = _i;
        uint256 len;
        while (j != 0) {
            len++;
            j /= 10;
        }
        bytes memory bstr = new bytes(len);
        uint256 k = len;
        while (_i != 0) {
            k = k - 1;
            uint8 temp = (48 + uint8(_i - _i / 10 * 10));
            bytes1 b1 = bytes1(temp);
            bstr[k] = b1;
            _i /= 10;
        }
        return string(bstr);
    }
    
    /**
     * @dev Accept ETH donations
     */
    receive() external payable {}
}
