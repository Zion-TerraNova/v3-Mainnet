// Copyright 2026 ZION TerraNova. All Rights Reserved.
#pragma once

#include "CoreMinimal.h"
#include "UObject/NoExportTypes.h"
#include "Http.h"
#include "ZionOasis/Consciousness/ConsciousnessTypes.h"
#include "ZionOasis/Avatar/AvatarTypes.h"
#include "ZionBlockchainBridge.generated.h"

/** C++ async callback for HTTP bridge requests (non-dynamic) */
DECLARE_DELEGATE_TwoParams(FZionHttpCallback, const FString&, bool /*bSuccess*/);

/**
 * ZionBlockchainBridge
 *
 * HTTP client to the zion-oasis Rust REST API (port 8094).
 * Also interfaces with zion-core JSON-RPC (port 8444) for wallet lookups.
 *
 * Usage: UZionBlockchainBridge::Get(World)->GetPlayer("zion1...", Callback);
 *
 * Singleton — one per GameInstance, accessible globally via Get().
 */
UCLASS(BlueprintType, Transient, meta=(DisplayName="ZION Blockchain Bridge"))
class ZIONOASIS_API UZionBlockchainBridge : public UObject
{
	GENERATED_BODY()

public:
	/** Initialize with config (called from ZionGameInstance) */
	void Initialize(const FString& OasisApiHost, const FString& ChainRpcHost);

	/** Global accessor — returns nullptr if not initialized */
	static UZionBlockchainBridge* Get(UWorld* World);

	// === Oasis REST API (Port 8094) ===

	/** GET /api/v1/oasis/player/:address */
	void GetPlayer(const FString& WalletAddress, FZionHttpCallback Callback);

	/** POST /api/v1/oasis/player/:address/xp  { source, amount } */
	void AwardXp(const FString& WalletAddress, const FString& Source, int64 Amount,
				 FZionHttpCallback Callback);

	/** GET /api/v1/oasis/leaderboard */
	void GetLeaderboard(FZionHttpCallback Callback);

	/** GET /api/v1/oasis/leaderboard/top100 */
	void GetTop100Leaderboard(FZionHttpCallback Callback);

	/** POST /api/v1/oasis/guild  { name, founder } */
	void CreateGuild(const FString& GuildName, const FString& FounderWallet,
					 FZionHttpCallback Callback);

	/** GET /api/v1/oasis/guild/:id */
	void GetGuild(const FString& GuildId, FZionHttpCallback Callback);

	/** POST /api/v1/oasis/guild/:id/join  { address } */
	void JoinGuild(const FString& GuildId, const FString& WalletAddress,
				   FZionHttpCallback Callback);

	/** GET /api/v1/oasis/map */
	void GetTerritoryMap(FZionHttpCallback Callback);

	/** GET /api/v1/oasis/rewards/pools */
	void GetRewardPools(FZionHttpCallback Callback);

	/** GET /api/v1/oasis/prize-tiers */
	void GetPrizeTiers(FZionHttpCallback Callback);

	/** GET /api/v1/oasis/golden-egg/progress/:address */
	void GetGoldenEggProgress(const FString& WalletAddress, FZionHttpCallback Callback);

	/** GET /api/v1/oasis/golden-egg/leaderboard */
	void GetGoldenEggLeaderboard(FZionHttpCallback Callback);

	/** POST /api/v1/oasis/raid-team  { name, leader } */
	void CreateRaidTeam(const FString& Name, const FString& LeaderWallet,
						FZionHttpCallback Callback);

	/** GET /api/v1/oasis/raid-team/:id */
	void GetRaidTeam(const FString& RaidId, FZionHttpCallback Callback);

	/** POST /api/v1/oasis/raid-team/:id/join  { address } */
	void JoinRaidTeam(const FString& RaidId, const FString& WalletAddress,
					  FZionHttpCallback Callback);

	/** GET /api/v1/oasis/raid-leaderboard */
	void GetRaidLeaderboard(FZionHttpCallback Callback);

	/** GET /health — check if backend is alive */
	void HealthCheck(FZionHttpCallback Callback);

	// === Chain RPC (Port 8444) ===

	/** Verify wallet address exists on L1 chain */
	void VerifyWalletAddress(const FString& WalletAddress, FZionHttpCallback Callback);

	/** Get ZION balance for an address */
	void GetBalance(const FString& WalletAddress, FZionHttpCallback Callback);

private:
	FString OasisBaseUrl;   // e.g. http://localhost:8094
	FString ChainRpcUrl;    // e.g. http://localhost:8444

	void SendGet(const FString& Url, FZionHttpCallback Callback);
	void SendPost(const FString& Url, const FString& JsonBody, FZionHttpCallback Callback);
};
