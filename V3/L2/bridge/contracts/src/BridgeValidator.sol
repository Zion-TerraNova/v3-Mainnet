// SPDX-License-Identifier: MIT
pragma solidity ^0.8.19;

/**
 * @title BridgeValidator
 * @notice N-of-5 Guardian multisig for ZION Bridge critical operations.
 * @dev Deployer is automatically added as Guardian #1.
 *      Remaining 4 guardians are added via `addGuardian()`.
 *      Threshold is immutable after deployment.
 */
contract BridgeValidator {
    uint256 public immutable threshold;
    uint256 public guardianCount;

    mapping(address => bool) public isGuardian;
    mapping(bytes32 => mapping(address => bool)) public hasSigned;
    mapping(bytes32 => uint256) public signatureCount;

    // Enumerable guardian list so resetSignatures can clear per-guardian flags.
    address[5] public guardianAddresses;

    event GuardianAdded(address indexed guardian, uint256 index);
    event GuardianRemoved(address indexed guardian);
    event OperationSigned(bytes32 indexed opHash, address indexed guardian);
    event OperationReset(bytes32 indexed opHash);
    event ThresholdChanged(uint256 newThreshold); // kept for interface compatibility; never emitted after constructor

    modifier onlyGuardian() {
        require(isGuardian[msg.sender], "BridgeValidator: not a guardian");
        _;
    }

    constructor(uint256 _threshold, uint256 _maxGuardians) {
        require(_threshold > 0 && _threshold <= _maxGuardians, "BridgeValidator: invalid threshold");
        require(_maxGuardians == 5, "BridgeValidator: maxGuardians must be 5");
        threshold = _threshold;
        guardianCount = 1;
        isGuardian[msg.sender] = true;
        guardianAddresses[0] = msg.sender;
        emit GuardianAdded(msg.sender, 1);
    }

    /**
     * @notice Add a new guardian. Only existing guardians can add.
     * @param _guardian Address to add
     */
    function addGuardian(address _guardian) external onlyGuardian {
        require(!isGuardian[_guardian], "BridgeValidator: already guardian");
        require(_guardian != address(0), "BridgeValidator: zero address");
        require(guardianCount < 5, "BridgeValidator: max guardians reached");
        isGuardian[_guardian] = true;
        guardianAddresses[guardianCount] = _guardian;
        guardianCount++;
        emit GuardianAdded(_guardian, guardianCount);
    }

    /**
     * @notice Remove a guardian. Must stay above threshold.
     * @param _guardian Address to remove
     */
    function removeGuardian(address _guardian) external onlyGuardian {
        require(isGuardian[_guardian], "BridgeValidator: not a guardian");
        require(guardianCount > threshold, "BridgeValidator: cannot go below threshold");
        isGuardian[_guardian] = false;
        // Remove from enumerable list by swapping with the last element.
        for (uint256 i = 0; i < guardianCount; i++) {
            if (guardianAddresses[i] == _guardian) {
                guardianAddresses[i] = guardianAddresses[guardianCount - 1];
                guardianAddresses[guardianCount - 1] = address(0);
                break;
            }
        }
        guardianCount--;
        emit GuardianRemoved(_guardian);
    }

    /**
     * @notice Sign an operation hash. Reusable per-guardian per-op.
     * @param opHash keccak256 hash of the operation data
     */
    function signOperation(bytes32 opHash) external onlyGuardian {
        require(!hasSigned[opHash][msg.sender], "BridgeValidator: already signed");
        hasSigned[opHash][msg.sender] = true;
        signatureCount[opHash]++;
        emit OperationSigned(opHash, msg.sender);
    }

    /**
     * @notice Check if an operation has reached threshold signatures.
     * @param opHash keccak256 hash of the operation data
     * @return bool true if threshold is met
     */
    function isOperationApproved(bytes32 opHash) external view returns (bool) {
        return signatureCount[opHash] >= threshold;
    }

    /**
     * @notice Get current signature count for an operation.
     */
    function getSignatureCount(bytes32 opHash) external view returns (uint256) {
        return signatureCount[opHash];
    }

    /**
     * @notice Get the list of current guardian addresses.
     */
    function getGuardians() external view returns (address[5] memory) {
        return guardianAddresses;
    }

    /**
     * @notice Reset signatures for an operation (e.g., after execution or cancellation).
     * Only callable by a guardian.
     * @dev Clears both the count and the per-guardian hasSigned flags.
     */
    function resetSignatures(bytes32 opHash) external onlyGuardian {
        for (uint256 i = 0; i < guardianCount; i++) {
            hasSigned[opHash][guardianAddresses[i]] = false;
        }
        signatureCount[opHash] = 0;
        emit OperationReset(opHash);
    }
}
