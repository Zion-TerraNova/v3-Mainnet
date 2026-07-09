// Copyright 2026 ZION TerraNova. All Rights Reserved.
#pragma once

#include "CoreMinimal.h"
#include "GameFramework/Actor.h"
#include "TerritoryManager.generated.h"

/** 8 genesis territory regions — mirrors territory.rs Region enum */
UENUM(BlueprintType)
enum class ETerritoryRegion : uint8
{
	Mountains,
	Forest,
	Desert,
	Ocean,
	Volcano,
	CrystalCaves,
};

USTRUCT(BlueprintType)
struct FTerritoryInfo
{
	GENERATED_BODY()

	UPROPERTY(BlueprintReadOnly) FString TerritoryId;
	UPROPERTY(BlueprintReadOnly) FText   Name;
	UPROPERTY(BlueprintReadOnly) FText   Description;
	UPROPERTY(BlueprintReadOnly) ETerritoryRegion Region = ETerritoryRegion::Mountains;
	UPROPERTY(BlueprintReadOnly) FString ControllerGuildId;
	UPROPERTY(BlueprintReadOnly) float   MiningBonus  = 0.10f;
	UPROPERTY(BlueprintReadOnly) float   XpBonus      = 0.05f;
	UPROPERTY(BlueprintReadOnly) FVector WorldLocation = FVector::ZeroVector;
};

DECLARE_DYNAMIC_MULTICAST_DELEGATE_TwoParams(FOnTerritoryClaimed,
	FString, TerritoryId, FString, GuildId);
DECLARE_DYNAMIC_MULTICAST_DELEGATE_OneParam(FOnTerritoryLost,
	FString, TerritoryId);

/**
 * TerritoryManager
 *
 * Manages 8 genesis territories on the server.
 * Syncs claimed state via ZionBlockchainBridge (oasis REST /api/v1/oasis/map).
 * Applies mining/XP bonuses to players inside controlled zones.
 */
UCLASS(BlueprintType, meta=(DisplayName="Territory Manager"))
class ZIONOASIS_API ATerritoryManager : public AActor
{
	GENERATED_BODY()

public:
	ATerritoryManager();
	virtual void BeginPlay() override;

	static ATerritoryManager* Get(UWorld* World);

	UPROPERTY(BlueprintAssignable, Category = "Territory|Events")
	FOnTerritoryClaimed OnTerritoryClaimed;

	UPROPERTY(BlueprintAssignable, Category = "Territory|Events")
	FOnTerritoryLost OnTerritoryLost;

	UFUNCTION(BlueprintPure, Category = "Territory")
	TArray<FTerritoryInfo> GetAllTerritories() const { return Territories; }

	UFUNCTION(BlueprintPure, Category = "Territory")
	FTerritoryInfo GetTerritory(const FString& TerritoryId) const;

	/** Attempt guild claim — requires 10,000 ZION and 24h defense window */
	UFUNCTION(BlueprintCallable, Category = "Territory")
	void ClaimTerritory(const FString& TerritoryId, const FString& GuildId,
						const FString& ClaimerWallet);

	/** Contest an existing claimed territory */
	UFUNCTION(BlueprintCallable, Category = "Territory")
	void ContestTerritory(const FString& TerritoryId, const FString& AttackerGuildId);

	/** Get mining bonus for a given territory (0.0 if not controlled) */
	UFUNCTION(BlueprintPure, Category = "Territory")
	float GetMiningBonus(const FString& TerritoryId) const;

	void SyncFromBackend();

private:
	UPROPERTY(Replicated)
	TArray<FTerritoryInfo> Territories;

	void LoadGenesisMap();
};
