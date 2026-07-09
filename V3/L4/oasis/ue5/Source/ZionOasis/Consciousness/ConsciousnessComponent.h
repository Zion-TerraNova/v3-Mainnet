// Copyright 2026 ZION TerraNova. All Rights Reserved.
#pragma once

#include "CoreMinimal.h"
#include "Components/ActorComponent.h"
#include "ConsciousnessTypes.h"
#include "ConsciousnessComponent.generated.h"

DECLARE_DYNAMIC_MULTICAST_DELEGATE_TwoParams(FOnLevelUp,
	EConsciousnessLevel, OldLevel, EConsciousnessLevel, NewLevel);
DECLARE_DYNAMIC_MULTICAST_DELEGATE_OneParam(FOnXpAwarded, FXpAward, Award);
DECLARE_DYNAMIC_MULTICAST_DELEGATE(FOnDailyCapped);

/**
 * ConsciousnessComponent
 *
 * Tracks player XP progression through 9 consciousness levels.
 * Communicates with zion-oasis Rust backend via ZionBlockchainBridge.
 * Mirrors ConsciousnessLevel enum and XpSystem logic from consciousness.rs / xp.rs.
 *
 * Attach to: ZionCharacter (player pawn)
 */
UCLASS(ClassGroup=(ZionOasis), meta=(BlueprintSpawnableComponent),
	   DisplayName="Consciousness Component")
class ZIONOASIS_API UConsciousnessComponent : public UActorComponent
{
	GENERATED_BODY()

public:
	UConsciousnessComponent();

	// === Events ===
	UPROPERTY(BlueprintAssignable, Category = "Consciousness|Events")
	FOnLevelUp OnLevelUp;

	UPROPERTY(BlueprintAssignable, Category = "Consciousness|Events")
	FOnXpAwarded OnXpAwarded;

	UPROPERTY(BlueprintAssignable, Category = "Consciousness|Events")
	FOnDailyCapped OnDailyCapped;

	// === Read-only State ===
	UFUNCTION(BlueprintPure, Category = "Consciousness")
	FORCEINLINE int64 GetTotalXp() const { return TotalXp; }

	UFUNCTION(BlueprintPure, Category = "Consciousness")
	FORCEINLINE int64 GetDailyXpEarned() const { return DailyXpEarned; }

	UFUNCTION(BlueprintPure, Category = "Consciousness")
	FORCEINLINE EConsciousnessLevel GetLevel() const { return CurrentLevel; }

	UFUNCTION(BlueprintPure, Category = "Consciousness")
	float GetLevelProgress() const;

	UFUNCTION(BlueprintPure, Category = "Consciousness")
	int64 GetXpToNextLevel() const;

	UFUNCTION(BlueprintPure, Category = "Consciousness")
	FText GetLevelDisplayName() const;

	UFUNCTION(BlueprintPure, Category = "Consciousness")
	FText GetSefiraName() const;

	UFUNCTION(BlueprintPure, Category = "Consciousness")
	float GetXpMultiplier() const;

	// === Actions ===
	/**
	 * Award XP to this player. Sends POST to oasis REST API if wallet is set.
	 * @param Source    - where the XP came from
	 * @param Amount    - raw XP before multiplier
	 */
	UFUNCTION(BlueprintCallable, Category = "Consciousness")
	FXpAward AwardXp(EXpSource Source, int64 Amount);

	/** Force sync state from backend (on login / session resume) */
	UFUNCTION(BlueprintCallable, Category = "Consciousness")
	void SyncFromBackend(const FString& WalletAddress);

	/** Called by backend after HTTP response */
	void ApplyStateFromJson(const FString& JsonPayload);

	// === Config ===
	UPROPERTY(EditDefaultsOnly, Category = "Consciousness|Config")
	int64 DailyXpCap = 10000;

protected:
	virtual void BeginPlay() override;
	virtual void GetLifetimeReplicatedProps(TArray<FLifetimeProperty>& OutLifetimeProps) const override;

private:
	UPROPERTY(Replicated, VisibleAnywhere, Category = "Consciousness|State")
	int64 TotalXp = 0;

	UPROPERTY(Replicated, VisibleAnywhere, Category = "Consciousness|State")
	int64 DailyXpEarned = 0;

	UPROPERTY(Replicated, VisibleAnywhere, Category = "Consciousness|State")
	EConsciousnessLevel CurrentLevel = EConsciousnessLevel::Physical;

	EConsciousnessLevel ComputeLevel(int64 Xp) const;
};
