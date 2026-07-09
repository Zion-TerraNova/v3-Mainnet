// SPDX-License-Identifier: MIT
pragma solidity ^0.8.20;

import "@openzeppelin/contracts/token/ERC20/IERC20.sol";
import "@openzeppelin/contracts/token/ERC20/utils/SafeERC20.sol";
import "@openzeppelin/contracts/access/AccessControl.sol";
import "@openzeppelin/contracts/utils/ReentrancyGuard.sol";
import "@openzeppelin/contracts/utils/Pausable.sol";

/**
 * @title ZIONFarm — Multi-pool Yield Farming / Liquidity Mining
 * @author ZION TerraNova Core Team
 * @notice MasterChef-style farm: stake LP tokens (or any ERC-20) to earn wZION.
 *
 * ## Design (MasterChef v2 pattern)
 *   - Admin creates pools, each with an LP token and allocation points
 *   - Total wZION rewards/second distributed proportionally to pool allocs
 *   - Within each pool, rewards distributed proportionally to user stake
 *   - Reward halving every `halvingInterval` seconds (default 90 days)
 *   - Reward pool funded manually via `fundRewards()` — no minting rights needed
 *
 * ## Pools
 *   Pool 0: wZION (single-asset staking for boosted APR)
 *   Pool 1: wZION/WETH Uni V3 LP (added after pool deploy)
 *   Pool N: any future LP token
 *
 * ## Math (accumulated reward per share)
 *   accRewardPerShare[pid] += (elapsed × rewardPerSecond × pool.allocPoints / totalAlloc) / pool.totalStaked
 *   pending(user) = user.staked × accRewardPerShare - user.rewardDebt
 *
 * ## Roles
 *   - DEFAULT_ADMIN_ROLE  — add/update pools, set reward rate, pause
 *   - GUARDIAN_ROLE       — emergency pause
 *   - REWARD_FUNDER_ROLE  — fund the reward pool
 *
 * @dev wZION uses 18 decimals on L2 (EVM standard).
 */
contract ZIONFarm is AccessControl, ReentrancyGuard, Pausable {
    using SafeERC20 for IERC20;

    // ─── Roles ─────────────────────────────────────────────────────────────

    bytes32 public constant GUARDIAN_ROLE     = keccak256("GUARDIAN_ROLE");
    bytes32 public constant REWARD_FUNDER_ROLE = keccak256("REWARD_FUNDER_ROLE");

    // ─── Constants ─────────────────────────────────────────────────────────

    uint256 public constant PRECISION          = 1e18;
    uint256 public constant MAX_POOLS          = 50;
    uint256 public constant MAX_ALLOC_POINTS   = 100_000;
    uint256 public constant DEFAULT_HALVING_INTERVAL = 90 days;

    // ─── Structs ───────────────────────────────────────────────────────────

    struct PoolInfo {
        IERC20  lpToken;              // staked token (LP or any ERC-20)
        uint256 allocPoints;          // reward weight relative to totalAllocPoints
        uint256 lastRewardTime;       // last time rewards were computed
        uint256 accRewardPerShare;    // accumulated reward per unit staked (×PRECISION)
        uint256 totalStaked;          // total LP tokens staked in this pool
        bool    active;               // false after removing a pool (no new stakes)
        string  name;                 // human-readable pool name (e.g. "wZION/WETH")
    }

    struct UserInfo {
        uint256 staked;               // LP tokens staked
        uint256 rewardDebt;           // reward debt (snapshot at last interaction)
        uint256 pendingHarvest;       // rewards pending but not yet sent (safety buffer)
    }

    // ─── State ─────────────────────────────────────────────────────────────

    IERC20 public immutable rewardToken;       // wZION
    PoolInfo[] public pools;
    mapping(uint256 => mapping(address => UserInfo)) public users;

    uint256 public totalAllocPoints;
    uint256 public rewardPerSecond;            // wZION wei / second
    uint256 public rewardPoolBalance;          // funded rewards not yet distributed

    // Halving
    uint256 public halvingInterval;            // seconds between halvings
    uint256 public nextHalvingTime;            // UNIX timestamp of next halving
    uint256 public halvingCount;               // number of halvings so far

    // ─── Events ────────────────────────────────────────────────────────────

    event PoolAdded(uint256 indexed pid, address lpToken, uint256 allocPoints, string name);
    event PoolUpdated(uint256 indexed pid, uint256 allocPoints);
    event Deposit(uint256 indexed pid, address indexed user, uint256 amount);
    event Withdraw(uint256 indexed pid, address indexed user, uint256 amount);
    event Harvest(uint256 indexed pid, address indexed user, uint256 reward);
    event EmergencyWithdraw(uint256 indexed pid, address indexed user, uint256 amount);
    event RewardsFunded(address indexed funder, uint256 amount);
    event RewardPerSecondUpdated(uint256 oldRate, uint256 newRate);
    event Halving(uint256 indexed halvingNumber, uint256 newRewardPerSecond);

    // ─── Errors ────────────────────────────────────────────────────────────

    error PoolNotFound(uint256 pid);
    error PoolInactive(uint256 pid);
    error InvalidLpToken();
    error TooManyPools();
    error ZeroAmount();
    error InsufficientRewardPool();
    error DuplicatePool(address lpToken);

    // ─── Constructor ───────────────────────────────────────────────────────

    /**
     * @param _rewardToken      wZION ERC-20 address
     * @param _admin            admin address
     * @param _guardian         guardian (pause) address
     * @param _rewardPerSecond  initial wZION per second (e.g. 3e18 = 3 wZION/s)
     * @param _halvingInterval  halving period in seconds (0 = no halving)
     */
    constructor(
        address _rewardToken,
        address _admin,
        address _guardian,
        uint256 _rewardPerSecond,
        uint256 _halvingInterval
    ) {
        require(_rewardToken != address(0), "reward = zero");
        require(_admin != address(0), "admin = zero");
        require(_guardian != address(0), "guardian = zero");

        rewardToken       = IERC20(_rewardToken);
        rewardPerSecond   = _rewardPerSecond;
        halvingInterval   = _halvingInterval;
        nextHalvingTime   = _halvingInterval > 0 ? block.timestamp + _halvingInterval : type(uint256).max;

        _grantRole(DEFAULT_ADMIN_ROLE, _admin);
        _grantRole(GUARDIAN_ROLE, _guardian);
        _grantRole(REWARD_FUNDER_ROLE, _admin);
    }

    // ─── Admin: Pool management ─────────────────────────────────────────────

    /**
     * @notice Add a new reward pool.
     * @param _allocPoints  Reward weight (relative to other pools)
     * @param _lpToken      Staked token address
     * @param _name         Human-readable name (e.g. "wZION/WETH UNI-V3")
     * @param _withUpdate   Update all pools before adding (prevents reward dilution)
     */
    function addPool(
        uint256 _allocPoints,
        address _lpToken,
        string calldata _name,
        bool _withUpdate
    ) external onlyRole(DEFAULT_ADMIN_ROLE) {
        if (pools.length >= MAX_POOLS) revert TooManyPools();
        if (_lpToken == address(0)) revert InvalidLpToken();

        // Prevent duplicate LP tokens
        uint256 len = pools.length;
        for (uint256 i = 0; i < len; i++) {
            if (address(pools[i].lpToken) == _lpToken) revert DuplicatePool(_lpToken);
        }

        if (_withUpdate) massUpdatePools();

        totalAllocPoints += _allocPoints;

        pools.push(PoolInfo({
            lpToken:          IERC20(_lpToken),
            allocPoints:      _allocPoints,
            lastRewardTime:   block.timestamp,
            accRewardPerShare: 0,
            totalStaked:      0,
            active:           true,
            name:             _name
        }));

        emit PoolAdded(pools.length - 1, _lpToken, _allocPoints, _name);
    }

    /**
     * @notice Update allocation points for an existing pool.
     */
    function updatePool(
        uint256 _pid,
        uint256 _allocPoints,
        bool _withUpdate
    ) external onlyRole(DEFAULT_ADMIN_ROLE) {
        if (_pid >= pools.length) revert PoolNotFound(_pid);
        if (_withUpdate) massUpdatePools();
        totalAllocPoints = totalAllocPoints - pools[_pid].allocPoints + _allocPoints;
        pools[_pid].allocPoints = _allocPoints;
        emit PoolUpdated(_pid, _allocPoints);
    }

    /**
     * @notice Fund the reward pool (wZION transferred from caller).
     */
    function fundRewards(uint256 amount) external onlyRole(REWARD_FUNDER_ROLE) {
        if (amount == 0) revert ZeroAmount();
        rewardToken.safeTransferFrom(msg.sender, address(this), amount);
        rewardPoolBalance += amount;
        emit RewardsFunded(msg.sender, amount);
    }

    /**
     * @notice Manually update reward rate (admin).
     */
    function setRewardPerSecond(uint256 _rate) external onlyRole(DEFAULT_ADMIN_ROLE) {
        massUpdatePools();
        emit RewardPerSecondUpdated(rewardPerSecond, _rate);
        rewardPerSecond = _rate;
    }

    // ─── Halving ────────────────────────────────────────────────────────────

    /**
     * @notice Trigger halving if interval elapsed. Anyone can call.
     * Rate is halved: rewardPerSecond /= 2.
     */
    function triggerHalving() external {
        require(halvingInterval > 0, "halving disabled");
        require(block.timestamp >= nextHalvingTime, "halving not due");
        massUpdatePools();
        rewardPerSecond = rewardPerSecond / 2;
        halvingCount++;
        nextHalvingTime += halvingInterval;
        emit Halving(halvingCount, rewardPerSecond);
    }

    // ─── Core: Deposit / Withdraw / Harvest ─────────────────────────────────

    /**
     * @notice Stake LP tokens in a pool.
     * @dev Also harvests pending rewards.
     */
    function deposit(uint256 pid, uint256 amount) external nonReentrant whenNotPaused {
        if (pid >= pools.length) revert PoolNotFound(pid);
        PoolInfo storage pool = pools[pid];
        if (!pool.active) revert PoolInactive(pid);
        if (amount == 0) revert ZeroAmount();

        _updatePool(pid);

        UserInfo storage user = users[pid][msg.sender];

        // Harvest pending
        if (user.staked > 0) {
            uint256 pending = _pendingReward(pool, user);
            if (pending > 0) _safeRewardTransfer(msg.sender, pending);
        }

        pool.lpToken.safeTransferFrom(msg.sender, address(this), amount);
        user.staked     += amount;
        pool.totalStaked += amount;
        user.rewardDebt  = (user.staked * pool.accRewardPerShare) / PRECISION;

        emit Deposit(pid, msg.sender, amount);
    }

    /**
     * @notice Unstake LP tokens from a pool.
     * @dev Also harvests pending rewards.
     */
    function withdraw(uint256 pid, uint256 amount) external nonReentrant {
        if (pid >= pools.length) revert PoolNotFound(pid);
        PoolInfo storage pool = pools[pid];
        UserInfo storage user = users[pid][msg.sender];
        require(user.staked >= amount, "insufficient staked");
        if (amount == 0) revert ZeroAmount();

        _updatePool(pid);

        uint256 pending = _pendingReward(pool, user);
        if (pending > 0) _safeRewardTransfer(msg.sender, pending);

        user.staked     -= amount;
        pool.totalStaked -= amount;
        user.rewardDebt  = (user.staked * pool.accRewardPerShare) / PRECISION;

        pool.lpToken.safeTransfer(msg.sender, amount);

        emit Withdraw(pid, msg.sender, amount);
        if (pending > 0) emit Harvest(pid, msg.sender, pending);
    }

    /**
     * @notice Harvest pending rewards without withdrawing stake.
     */
    function harvest(uint256 pid) external nonReentrant whenNotPaused {
        if (pid >= pools.length) revert PoolNotFound(pid);
        PoolInfo storage pool = pools[pid];
        UserInfo storage user = users[pid][msg.sender];

        _updatePool(pid);

        uint256 pending = _pendingReward(pool, user);
        if (pending == 0) return;

        user.rewardDebt = (user.staked * pool.accRewardPerShare) / PRECISION;
        _safeRewardTransfer(msg.sender, pending);

        emit Harvest(pid, msg.sender, pending);
    }

    /**
     * @notice Emergency withdraw — forfeit all pending rewards.
     *         Used when contract is paused or in case of emergency.
     */
    function emergencyWithdraw(uint256 pid) external nonReentrant {
        if (pid >= pools.length) revert PoolNotFound(pid);
        PoolInfo storage pool = pools[pid];
        UserInfo storage user = users[pid][msg.sender];

        uint256 amount = user.staked;
        require(amount > 0, "nothing staked");

        pool.totalStaked -= amount;
        user.staked       = 0;
        user.rewardDebt   = 0;
        user.pendingHarvest = 0;

        pool.lpToken.safeTransfer(msg.sender, amount);
        emit EmergencyWithdraw(pid, msg.sender, amount);
    }

    // ─── Pool updater ───────────────────────────────────────────────────────

    /// @notice Update accRewardPerShare for all pools (gas-intensive, use sparingly).
    function massUpdatePools() public {
        uint256 len = pools.length;
        for (uint256 i = 0; i < len; i++) {
            _updatePool(i);
        }
    }

    function _updatePool(uint256 pid) internal {
        PoolInfo storage pool = pools[pid];
        if (block.timestamp <= pool.lastRewardTime) return;
        if (pool.totalStaked == 0 || totalAllocPoints == 0) {
            pool.lastRewardTime = block.timestamp;
            return;
        }

        uint256 elapsed = block.timestamp - pool.lastRewardTime;
        uint256 reward  = (elapsed * rewardPerSecond * pool.allocPoints) / totalAllocPoints;

        // Cap reward to what's actually funded
        if (reward > rewardPoolBalance) reward = rewardPoolBalance;
        if (reward > 0) {
            rewardPoolBalance -= reward;
            pool.accRewardPerShare += (reward * PRECISION) / pool.totalStaked;
        }

        pool.lastRewardTime = block.timestamp;
    }

    // ─── Internal helpers ────────────────────────────────────────────────────

    function _pendingReward(PoolInfo storage pool, UserInfo storage user)
        internal view returns (uint256)
    {
        uint256 acc = pool.accRewardPerShare;
        if (block.timestamp > pool.lastRewardTime && pool.totalStaked > 0 && totalAllocPoints > 0) {
            uint256 elapsed = block.timestamp - pool.lastRewardTime;
            uint256 reward  = (elapsed * rewardPerSecond * pool.allocPoints) / totalAllocPoints;
            if (reward > rewardPoolBalance) reward = rewardPoolBalance;
            acc += (reward * PRECISION) / pool.totalStaked;
        }
        return (user.staked * acc) / PRECISION - user.rewardDebt + user.pendingHarvest;
    }

    function _safeRewardTransfer(address to, uint256 amount) internal {
        uint256 bal = rewardToken.balanceOf(address(this));
        // Exclude staked wZION from reward balance (if pool 0 uses wZION as LP)
        // We track rewardPoolBalance separately, use it as cap
        uint256 cap = bal > rewardPoolBalance ? rewardPoolBalance : bal;
        uint256 send = amount > cap ? cap : amount;
        if (send > 0) {
            rewardPoolBalance -= send;
            rewardToken.safeTransfer(to, send);
        }
    }

    // ─── View ────────────────────────────────────────────────────────────────

    /**
     * @notice Pending rewards for a user in a pool (includes virtual update).
     */
    function pendingReward(uint256 pid, address account) external view returns (uint256) {
        if (pid >= pools.length) return 0;
        PoolInfo storage pool = pools[pid];
        UserInfo storage user = users[pid][account];
        return _pendingReward(pool, user);
    }

    /// @notice Number of pools.
    function poolCount() external view returns (uint256) {
        return pools.length;
    }

    /// @notice Get pool info.
    function getPool(uint256 pid) external view returns (PoolInfo memory) {
        if (pid >= pools.length) revert PoolNotFound(pid);
        return pools[pid];
    }

    /// @notice Get user staking info in a pool.
    function getUser(uint256 pid, address account) external view returns (UserInfo memory) {
        return users[pid][account];
    }

    function pause()   external onlyRole(GUARDIAN_ROLE) { _pause(); }
    function unpause() external onlyRole(GUARDIAN_ROLE) { _unpause(); }
}
