// Copyright 2026 ZION TerraNova. All Rights Reserved.
#pragma once

#include "CoreMinimal.h"
#include "GameFramework/Actor.h"
#include "ZionOasis/Consciousness/ConsciousnessTypes.h"
#include "GoldenEggManager.generated.h"

UENUM(BlueprintType)
enum class EClueCategory : uint8
{
	Genesis        UMETA(DisplayName="Genesis Block"),
	SacredTrinity  UMETA(DisplayName="Sacred Trinity (51 Avatars)"),
	VedicWisdom    UMETA(DisplayName="Vedic Wisdom (Vedas/Upanishads)"),
	Epics          UMETA(DisplayName="Epics (Ramayana/Mahabharata)"),
	Ekam           UMETA(DisplayName="Ekam Temple (Pilgrimage)"),
	Consciousness  UMETA(DisplayName="9 Consciousness Levels"),
	Economics      UMETA(DisplayName="ZION Economics/Blockchain"),
	Final          UMETA(DisplayName="Final Master Key"),
};

USTRUCT(BlueprintType)
struct FGoldenEggClue
{
	GENERATED_BODY()

	UPROPERTY(BlueprintReadOnly) int32 ClueId = 0;
	UPROPERTY(BlueprintReadOnly) EClueCategory Category = EClueCategory::Genesis;
	UPROPERTY(BlueprintReadOnly) FString Title;
	UPROPERTY(BlueprintReadOnly) FText   Riddle;
	/** SHA-256 hash of the correct answer (lowercase, trimmed) */
	UPROPERTY(BlueprintReadOnly) FString SolutionHash;
	/** Karma reward for solving */
	UPROPERTY(BlueprintReadOnly) int32   KarmaReward = 1000;
	/** Difficulty 1-10 */
	UPROPERTY(BlueprintReadOnly) int32   Difficulty  = 5;
	/** Wallet that first solved it (empty = unsolved) */
	UPROPERTY(BlueprintReadOnly) FString SolvedByWallet;

	UPROPERTY(BlueprintReadOnly) FText Hint1;
	UPROPERTY(BlueprintReadOnly) FText Hint2;
	UPROPERTY(BlueprintReadOnly) FText Hint3;
};

USTRUCT(BlueprintType)
struct FGoldenEggMasterKey
{
	GENERATED_BODY()

	UPROPERTY(BlueprintReadOnly) int32 KeyId = 0;
	UPROPERTY(BlueprintReadOnly) FString Name;
	UPROPERTY(BlueprintReadOnly) int32 RequiredClues = 36;
	UPROPERTY(BlueprintReadOnly) bool bUnlocked = false;
};

USTRUCT(BlueprintType)
struct FGoldenEggPlayerProgress
{
	GENERATED_BODY()

	UPROPERTY(BlueprintReadOnly) FString WalletAddress;
	UPROPERTY(BlueprintReadOnly) int32   CluesSolved   = 0;
	UPROPERTY(BlueprintReadOnly) int32   MasterKeysUnlocked = 0;
	UPROPERTY(BlueprintReadOnly) int32   KarmaPoints   = 0;
	UPROPERTY(BlueprintReadOnly) int32   KarmaSpent    = 0;
	UPROPERTY(BlueprintReadOnly) int32   CurrentClueId = 1;
	UPROPERTY(BlueprintReadOnly) bool    bEligibleForPrize = false;

	int32 AvailableKarma() const { return KarmaPoints - KarmaSpent; }
};

DECLARE_DYNAMIC_MULTICAST_DELEGATE_TwoParams(FOnGoldenEggWinner,
	int32, Place, const FString&, WalletAddress);
DECLARE_DYNAMIC_MULTICAST_DELEGATE_TwoParams(FOnGoldenEggClueRevealed,
	int32, ClueId, FText, ClueText);
DECLARE_DYNAMIC_MULTICAST_DELEGATE_TwoParams(FOnKarmaAwarded,
	const FString&, WalletAddress, int32, KarmaAmount);

/**
 * GoldenEggManager — Brahmanda (Hiranyagarbha) Treasure Hunt
 *
 * 108 clues, 3 master keys, 10 prize tiers.
 * Server-authoritative; syncs with zion-oasis Rust backend.
 */
UCLASS(BlueprintType, meta=(DisplayName="Golden Egg Manager"))
class ZIONOASIS_API AGoldenEggManager : public AActor
{
	GENERATED_BODY()

public:
	AGoldenEggManager();
	virtual void BeginPlay() override;
	virtual void GetLifetimeReplicatedProps(TArray<FLifetimeProperty>& OutLifetimeProps) const override;

	static AGoldenEggManager* Get(UWorld* World);

	// ── Events ──
	UPROPERTY(BlueprintAssignable, Category = "GoldenEgg|Events")
	FOnGoldenEggWinner OnWinner;

	UPROPERTY(BlueprintAssignable, Category = "GoldenEgg|Events")
	FOnGoldenEggClueRevealed OnClueRevealed;

	UPROPERTY(BlueprintAssignable, Category = "GoldenEgg|Events")
	FOnKarmaAwarded OnKarmaAwarded;

	// ── Query ──
	UFUNCTION(BlueprintPure, Category = "GoldenEgg")
	bool IsHuntActive() const { return !bThreeWinnersFilled; }

	UFUNCTION(BlueprintPure, Category = "GoldenEgg")
	bool HasWinner(int32 Place) const;

	UFUNCTION(BlueprintPure, Category = "GoldenEgg")
	FString GetWinnerWallet(int32 Place) const;

	UFUNCTION(BlueprintPure, Category = "GoldenEgg")
	int32 GetTotalClues() const { return TOTAL_CLUES; }

	UFUNCTION(BlueprintPure, Category = "GoldenEgg")
	int32 GetSolvedClueCount() const;

	// ── Gameplay (server-authoritative) ──
	/** Submit a solution attempt. Returns karma earned (0 = wrong). */
	UFUNCTION(BlueprintCallable, Category = "GoldenEgg")
	int32 SubmitSolution(const FString& WalletAddress, int32 ClueId, const FString& Answer);

	/** Purchase a hint (1/2/3) using karma. Returns hint text or empty on fail. */
	UFUNCTION(BlueprintCallable, Category = "GoldenEgg")
	FText PurchaseHint(const FString& WalletAddress, int32 ClueId, int32 HintNumber);

	/** Called on level-up — may reveal a progressive clue. */
	UFUNCTION(BlueprintCallable, Category = "GoldenEgg")
	void CheckClueReveal(const FString& WalletAddress, EConsciousnessLevel Level);

	/** Final claim attempt — requires CL9 + 108 clues + 3 master keys. */
	UFUNCTION(BlueprintCallable, Category = "GoldenEgg")
	bool AttemptClaim(const FString& WalletAddress, EConsciousnessLevel Level, int32 TotalAvatarQuestsCompleted);

	UFUNCTION(BlueprintPure, Category = "GoldenEgg")
	FGoldenEggPlayerProgress GetPlayerState(const FString& WalletAddress) const;

private:
	static constexpr int32 TOTAL_CLUES           = 108;
	static constexpr int32 TOTAL_KEYS_REQUIRED   = 3;
	static constexpr int32 TOTAL_QUESTS_REQUIRED = 255; // 51 avatars x 5 quests

	static constexpr int64 PRIZE_1ST = 1000000000LL;
	static constexpr int64 PRIZE_2ND = 500000000LL;
	static constexpr int64 PRIZE_3RD = 250000000LL;

	static constexpr int32 HINT_COST_1 = 100;
	static constexpr int32 HINT_COST_2 = 500;
	static constexpr int32 HINT_COST_3 = 1000;

	UPROPERTY(Replicated) bool bThreeWinnersFilled = false;
	UPROPERTY(Replicated) FString Winner1st;
	UPROPERTY(Replicated) FString Winner2nd;
	UPROPERTY(Replicated) FString Winner3rd;
	UPROPERTY(Replicated) TArray<int32> SolvedClueIds;

	TArray<FGoldenEggClue> AllClues;
	TArray<FGoldenEggMasterKey> MasterKeys;
	TMap<FString, FGoldenEggPlayerProgress> PlayerStates;
	TMap<FString, TSet<int32>> HintsPurchased;

	void LoadClues();
	void LoadMasterKeys();
	void AwardPlace(int32 Place, const FString& WalletAddress);
	bool HasPlayerSolvedClue(const FString& WalletAddress, int32 ClueId) const;
	int32 GetHintCost(int32 HintNumber) const;
	void UpdateMasterKeys(const FString& WalletAddress);
};
