// SPDX-License-Identifier: MIT
pragma solidity ^0.8.20;

import "@openzeppelin/contracts/token/ERC20/IERC20.sol";
import "@openzeppelin/contracts/token/ERC20/utils/SafeERC20.sol";
import "@openzeppelin/contracts/access/AccessControl.sol";
import "@openzeppelin/contracts/utils/ReentrancyGuard.sol";
import "@openzeppelin/contracts/utils/Pausable.sol";

/**
 * @title ZIONStaking
 * @author ZION TerraNova Core Team
 * @notice Stake wZION to earn rewards and accrue governance weight.
 *
 * ## Design
 *   - Stake wZION → earn wZION rewards at configurable APR
 *   - Unstake with 7-day cooldown (prevents governance attacks)
 *   - Governance weight = staked balance (used by ZIONGovernance)
 *   - Reward pool funded by treasury; paused when exhausted
 *   - Emergency withdraw (forfeit pending rewards)
 *
 * ## Roles
 *   - DEFAULT_ADMIN_ROLE  — multisig; set APR, fund rewards, update cooldown
 *   - GUARDIAN_ROLE       — pause/unpause in emergencies
 *   - REWARD_FUNDER_ROLE  — fund reward pool (treasury contract)
 *
 * ## Math
 *   Rewards are tracked per-second via `rewardPerTokenStored`.
 *   Each stake/unstake snapshots the user's reward debt.
 *   Same model as Synthetix StakingRewards (battle-tested).
 *
 * @dev Decimals: wZION uses 8 decimals (matching native ZION).
 *      All internal accounting is in wei (8-decimal units).
 */
contract ZIONStaking is AccessControl, ReentrancyGuard, Pausable {
    using SafeERC20 for IERC20;

    // ─── Roles ─────────────────────────────────────────────────────────────

    bytes32 public constant GUARDIAN_ROLE     = keccak256("GUARDIAN_ROLE");
    bytes32 public constant REWARD_FUNDER_ROLE = keccak256("REWARD_FUNDER_ROLE");

    // ─── Constants ──────────────────────────────────────────────────────────

    uint256 public constant PRECISION        = 1e18;   // reward math precision
    uint256 public constant MIN_STAKE        = 100e8;  // 100 wZION minimum
    uint256 public constant MAX_APR_BPS      = 5_000;  // 50% max APR (basis pts)

    // ─── State ──────────────────────────────────────────────────────────────

    IERC20 public immutable wzion;

    // Reward configuration
    uint256 public rewardRatePerSecond;       // wZION wei per second (global)
    uint256 public aprBps;                    // current APR in basis points
    uint256 public lastUpdateTime;            // last rewardPerToken update
    uint256 public rewardPerTokenStored;      // accumulated reward per staked token
    uint256 public rewardPoolBalance;         // unfunded reward reserve

    // Cooldown
    uint256 public cooldownSeconds = 7 days;

    // Totals
    uint256 public totalStaked;

    // Per-user state
    struct StakeInfo {
        uint256 staked;                 // wZION staked
        uint256 rewardPerTokenPaid;     // snapshot at last interaction
        uint256 pendingRewards;         // accrued but not claimed
        uint256 cooldownStarted;        // timestamp when unstake requested (0 = none)
        uint256 cooldownAmount;         // amount queued for cooldown
    }
    mapping(address => StakeInfo) public stakes;

    // ─── Events ─────────────────────────────────────────────────────────────

    event Staked(address indexed user, uint256 amount);
    event UnstakeQueued(address indexed user, uint256 amount, uint256 claimableAt);
    event Unstaked(address indexed user, uint256 amount);
    event RewardClaimed(address indexed user, uint256 amount);
    event RewardPoolFunded(address indexed funder, uint256 amount);
    event AprUpdated(uint256 oldAprBps, uint256 newAprBps);
    event CooldownUpdated(uint256 oldSeconds, uint256 newSeconds);
    event EmergencyWithdraw(address indexed user, uint256 amount);

    // ─── Constructor ────────────────────────────────────────────────────────

    /**
     * @param _wzion    wZION ERC-20 token address
     * @param _admin    admin multisig address
     * @param _guardian guardian address (emergency pause)
     * @param _aprBps   initial APR in basis points (e.g. 1200 = 12%)
     */
    constructor(
        address _wzion,
        address _admin,
        address _guardian,
        uint256 _aprBps
    ) {
        require(_wzion   != address(0), "zero wzion");
        require(_admin   != address(0), "zero admin");
        require(_guardian != address(0), "zero guardian");
        require(_aprBps <= MAX_APR_BPS, "apr too high");

        wzion = IERC20(_wzion);

        _grantRole(DEFAULT_ADMIN_ROLE, _admin);
        _grantRole(GUARDIAN_ROLE,      _guardian);
        _grantRole(REWARD_FUNDER_ROLE, _admin);

        aprBps = _aprBps;
        lastUpdateTime = block.timestamp;
    }

    // ─── Modifiers ──────────────────────────────────────────────────────────

    modifier updateReward(address account) {
        rewardPerTokenStored = rewardPerToken();
        lastUpdateTime       = block.timestamp;
        if (account != address(0)) {
            StakeInfo storage s = stakes[account];
            s.pendingRewards   = earned(account);
            s.rewardPerTokenPaid = rewardPerTokenStored;
        }
        _;
    }

    // ─── View functions ─────────────────────────────────────────────────────

    /**
     * @notice Accumulated reward per staked token (wei), up to now.
     */
    function rewardPerToken() public view returns (uint256) {
        if (totalStaked == 0) return rewardPerTokenStored;
        uint256 elapsed = block.timestamp - lastUpdateTime;
        uint256 reward  = elapsed * rewardRatePerSecond;
        // Cap to reward pool balance
        uint256 maxReward = rewardPoolBalance;
        if (reward > maxReward) reward = maxReward;
        return rewardPerTokenStored + (reward * PRECISION) / totalStaked;
    }

    /**
     * @notice Total wZION rewards accrued by `account`, including unclaimed.
     */
    function earned(address account) public view returns (uint256) {
        StakeInfo storage s = stakes[account];
        return s.pendingRewards
            + (s.staked * (rewardPerToken() - s.rewardPerTokenPaid)) / PRECISION;
    }

    /**
     * @notice Governance weight of `account` = staked amount.
     *         Used by ZIONGovernance for token-weighted voting.
     */
    function votingWeight(address account) external view returns (uint256) {
        return stakes[account].staked;
    }

    /**
     * @notice Timestamp when the user's cooldown expires (0 if not in cooldown).
     */
    function cooldownExpiresAt(address account) external view returns (uint256) {
        StakeInfo storage s = stakes[account];
        if (s.cooldownStarted == 0) return 0;
        return s.cooldownStarted + cooldownSeconds;
    }

    // ─── Staking ────────────────────────────────────────────────────────────

    /**
     * @notice Stake `amount` wZION tokens. Tokens are transferred from caller.
     * @param amount Amount of wZION to stake (in wei, 8 decimals).
     */
    function stake(uint256 amount)
        external
        nonReentrant
        whenNotPaused
        updateReward(msg.sender)
    {
        require(amount >= MIN_STAKE, "below minimum stake");

        stakes[msg.sender].staked += amount;
        totalStaked               += amount;

        wzion.safeTransferFrom(msg.sender, address(this), amount);
        emit Staked(msg.sender, amount);
    }

    /**
     * @notice Begin the cooldown process to unstake `amount` wZION.
     *         Tokens remain in the contract during cooldown.
     *         Only one active cooldown per address at a time.
     * @param amount Amount to queue for unstaking.
     */
    function queueUnstake(uint256 amount)
        external
        nonReentrant
        updateReward(msg.sender)
    {
        StakeInfo storage s = stakes[msg.sender];
        require(amount > 0,              "zero amount");
        require(amount <= s.staked,      "insufficient stake");
        require(s.cooldownStarted == 0,  "cooldown already active");

        s.staked        -= amount;
        totalStaked     -= amount;
        s.cooldownStarted = block.timestamp;
        s.cooldownAmount  = amount;

        uint256 claimableAt = block.timestamp + cooldownSeconds;
        emit UnstakeQueued(msg.sender, amount, claimableAt);
    }

    /**
     * @notice Claim tokens after the cooldown period has elapsed.
     *         Pending rewards are NOT auto-claimed here — call claimRewards().
     */
    function unstake()
        external
        nonReentrant
    {
        StakeInfo storage s = stakes[msg.sender];
        require(s.cooldownStarted > 0,                        "no cooldown");
        require(block.timestamp >= s.cooldownStarted + cooldownSeconds, "cooldown not elapsed");

        uint256 amount    = s.cooldownAmount;
        s.cooldownStarted = 0;
        s.cooldownAmount  = 0;

        wzion.safeTransfer(msg.sender, amount);
        emit Unstaked(msg.sender, amount);
    }

    /**
     * @notice Claim all accrued wZION rewards.
     */
    function claimRewards()
        external
        nonReentrant
        whenNotPaused
        updateReward(msg.sender)
    {
        uint256 reward = stakes[msg.sender].pendingRewards;
        require(reward > 0, "no rewards");
        require(rewardPoolBalance >= reward, "reward pool exhausted");

        stakes[msg.sender].pendingRewards = 0;
        rewardPoolBalance -= reward;

        wzion.safeTransfer(msg.sender, reward);
        emit RewardClaimed(msg.sender, reward);
    }

    /**
     * @notice Emergency withdraw: reclaim all staked tokens immediately,
     *         forfeiting all pending rewards.
     *         Bypasses cooldown. Rewards are returned to the pool.
     */
    function emergencyWithdraw() external nonReentrant {
        StakeInfo storage s = stakes[msg.sender];

        uint256 stakedAmount   = s.staked;
        uint256 cooldownAmount = s.cooldownAmount;
        uint256 total          = stakedAmount + cooldownAmount;

        require(total > 0, "nothing to withdraw");

        // Clear state
        if (stakedAmount > 0) totalStaked -= stakedAmount;
        s.staked         = 0;
        s.pendingRewards = 0;
        s.rewardPerTokenPaid = rewardPerTokenStored;
        s.cooldownStarted = 0;
        s.cooldownAmount  = 0;

        wzion.safeTransfer(msg.sender, total);
        emit EmergencyWithdraw(msg.sender, total);
    }

    // ─── Admin ──────────────────────────────────────────────────────────────

    /**
     * @notice Fund the reward pool. Called by ZIONTreasury or admin.
     *         Transfers wZION from caller to this contract.
     * @param amount Amount of wZION to add to the reward pool.
     */
    function fundRewardPool(uint256 amount)
        external
        onlyRole(REWARD_FUNDER_ROLE)
        updateReward(address(0))
    {
        require(amount > 0, "zero amount");
        rewardPoolBalance += amount;
        wzion.safeTransferFrom(msg.sender, address(this), amount);
        _recalcRewardRate();
        emit RewardPoolFunded(msg.sender, amount);
    }

    /**
     * @notice Update the annual percentage rate (APR).
     * @param _aprBps New APR in basis points (1 bps = 0.01%).
     *                Example: 1200 = 12% APR.
     */
    function setApr(uint256 _aprBps)
        external
        onlyRole(DEFAULT_ADMIN_ROLE)
        updateReward(address(0))
    {
        require(_aprBps <= MAX_APR_BPS, "apr too high");
        uint256 old = aprBps;
        aprBps = _aprBps;
        _recalcRewardRate();
        emit AprUpdated(old, _aprBps);
    }

    /**
     * @notice Update the cooldown period.
     * @param _seconds New cooldown in seconds. Min 1h, max 30 days.
     */
    function setCooldown(uint256 _seconds)
        external
        onlyRole(DEFAULT_ADMIN_ROLE)
    {
        require(_seconds >= 1 hours,  "cooldown too short");
        require(_seconds <= 30 days,  "cooldown too long");
        uint256 old = cooldownSeconds;
        cooldownSeconds = _seconds;
        emit CooldownUpdated(old, _seconds);
    }

    // ─── Guardian ───────────────────────────────────────────────────────────

    function pause()   external onlyRole(GUARDIAN_ROLE) { _pause(); }
    function unpause() external onlyRole(GUARDIAN_ROLE) { _unpause(); }

    // ─── Internal ───────────────────────────────────────────────────────────

    /**
     * @dev Recompute reward-per-second from totalStaked and current APR.
     *
     *   APR (annual)  = aprBps / 10_000
     *   Yearly reward = totalStaked * apr
     *   Per-second    = yearlyReward / 365.25 days
     *
     * Note: when totalStaked = 0 the rate is set to 0 (no waste).
     * Capped so that the pool would last at least 7 days.
     */
    function _recalcRewardRate() internal {
        if (totalStaked == 0 || aprBps == 0) {
            rewardRatePerSecond = 0;
            return;
        }
        uint256 yearlyReward = (totalStaked * aprBps) / 10_000;
        uint256 rate         = yearlyReward / 365.25 days;

        // Safety cap: pool must fund at least 7 days of emissions
        uint256 maxRate = rewardPoolBalance / 7 days;
        if (rate > maxRate) rate = maxRate;

        rewardRatePerSecond = rate;
    }
}
