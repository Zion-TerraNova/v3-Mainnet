// Copyright 2026 ZION TerraNova. All Rights Reserved.
#include "GoldenEggManager.h"
#include "ZionOasis/Blockchain/ZionBlockchainBridge.h"
#include "Net/UnrealNetwork.h"
#include "Engine/World.h"
#include "Kismet/GameplayStatics.h"
#include "Logging/LogMacros.h"

DEFINE_LOG_CATEGORY_STATIC(LogGoldenEgg, Log, All);

AGoldenEggManager::AGoldenEggManager()
{
	PrimaryActorTick.bCanEverTick = false;
	bReplicates = true;
}

void AGoldenEggManager::GetLifetimeReplicatedProps(TArray<FLifetimeProperty>& OutLifetimeProps) const
{
	Super::GetLifetimeReplicatedProps(OutLifetimeProps);
	DOREPLIFETIME(AGoldenEggManager, bThreeWinnersFilled);
	DOREPLIFETIME(AGoldenEggManager, Winner1st);
	DOREPLIFETIME(AGoldenEggManager, Winner2nd);
	DOREPLIFETIME(AGoldenEggManager, Winner3rd);
	DOREPLIFETIME(AGoldenEggManager, SolvedClueIds);
}

void AGoldenEggManager::BeginPlay()
{
	Super::BeginPlay();
	if (HasAuthority())
	{
		LoadClues();
		LoadMasterKeys();
	}
}

AGoldenEggManager* AGoldenEggManager::Get(UWorld* World)
{
	if (!World) return nullptr;
	TArray<AActor*> Found;
	UGameplayStatics::GetAllActorsOfClass(World, AGoldenEggManager::StaticClass(), Found);
	return Found.Num() > 0 ? Cast<AGoldenEggManager>(Found[0]) : nullptr;
}

void AGoldenEggManager::LoadClues()
{
	AllClues.Empty();
	// Seed clue #1 (Genesis)
	{
		FGoldenEggClue C;
		C.ClueId       = 1;
		C.Category     = EClueCategory::Genesis;
		C.Title        = TEXT("The Beginning");
		C.Riddle       = FText::FromString(TEXT(
			"In the first breath of ZION's dawn,\n"
			"Where blocks begin and light is drawn,\n"
			"A golden womb holds all creation,\n"
			"Seek the Sanskrit incantation.\n\n"
			"Five thousand years of wisdom old,\n"
			"In Rig Veda's verses told,\n"
			"The cosmic egg that births the All,\n"
			"Name it right, and heed the call.\n\n"
			"हिरण्य + गर्भ = ?"
		));
		C.SolutionHash = TEXT("e3b9e4c7f8a2d5b6c1f4e7a0d3b6c9f2e5a8d1b4c7f0e3a6d9b2c5f8e1a4d7b0");
		C.KarmaReward  = 1000;
		C.Difficulty   = 3;
		C.Hint1 = FText::FromString(TEXT("Look up Rig Veda 10.121. What is the Sanskrit term for 'Golden Womb'?"));
		C.Hint2 = FText::FromString(TEXT("The answer is a compound Sanskrit word: Hiranya (golden) + Garbha (womb/egg)"));
		C.Hint3 = FText::FromString(TEXT("Check docs/GOLDEN_EGG_GAME/README.md — the answer is in the first section!"));
		AllClues.Add(C);
	}
	UE_LOG(LogGoldenEgg, Log, TEXT("Loaded %d clue(s) (full 108 via DataTable in production)"), AllClues.Num());
}

void AGoldenEggManager::LoadMasterKeys()
{
	MasterKeys.Empty();
	FGoldenEggMasterKey K1; K1.KeyId = 1; K1.Name = TEXT("Key of Genesis");   K1.RequiredClues = 36; MasterKeys.Add(K1);
	FGoldenEggMasterKey K2; K2.KeyId = 2; K2.Name = TEXT("Key of Wisdom");     K2.RequiredClues = 72; MasterKeys.Add(K2);
	FGoldenEggMasterKey K3; K3.KeyId = 3; K3.Name = TEXT("Key of Enlightenment");K3.RequiredClues = 108; MasterKeys.Add(K3);
}

int32 AGoldenEggManager::SubmitSolution(const FString& WalletAddress, int32 ClueId, const FString& Answer)
{
	if (!HasAuthority() || bThreeWinnersFilled) return 0;

	FGoldenEggClue* Clue = AllClues.FindByPredicate([ClueId](const FGoldenEggClue& C){ return C.ClueId == ClueId; });
	if (!Clue || !Clue->SolvedByWallet.IsEmpty()) return 0;

	// In production, verify hash against backend. Here we accept any non-empty answer for skeleton.
	if (Answer.IsEmpty()) return 0;

	FGoldenEggPlayerProgress& PS = PlayerStates.FindOrAdd(WalletAddress);
	PS.WalletAddress = WalletAddress;

	Clue->SolvedByWallet = WalletAddress;
	SolvedClueIds.AddUnique(ClueId);
	PS.CluesSolved++;
	PS.KarmaPoints += Clue->KarmaReward;
	PS.CurrentClueId = ClueId + 1;

	UpdateMasterKeys(WalletAddress);

	OnKarmaAwarded.Broadcast(WalletAddress, Clue->KarmaReward);
	OnClueRevealed.Broadcast(ClueId, Clue->Riddle);

	UE_LOG(LogGoldenEgg, Log, TEXT("%s solved clue #%d (+%d karma)"), *WalletAddress, ClueId, Clue->KarmaReward);

	return Clue->KarmaReward;
}

FText AGoldenEggManager::PurchaseHint(const FString& WalletAddress, int32 ClueId, int32 HintNumber)
{
	if (!HasAuthority()) return FText::GetEmpty();
	if (HintNumber < 1 || HintNumber > 3) return FText::GetEmpty();

	FGoldenEggClue* Clue = AllClues.FindByPredicate([ClueId](const FGoldenEggClue& C){ return C.ClueId == ClueId; });
	if (!Clue) return FText::GetEmpty();

	const int32 HintKey = ClueId * 10 + HintNumber;
	if (HintsPurchased.FindOrAdd(WalletAddress).Contains(HintKey))
	{
		return HintNumber == 1 ? Clue->Hint1 : HintNumber == 2 ? Clue->Hint2 : Clue->Hint3;
	}

	FGoldenEggPlayerProgress& PS = PlayerStates.FindOrAdd(WalletAddress);
	const int32 Cost = GetHintCost(HintNumber);

	if (PS.AvailableKarma() < Cost)
	{
		UE_LOG(LogGoldenEgg, Warning, TEXT("%s cannot afford hint %d (need %d, have %d)"),
			*WalletAddress, HintNumber, Cost, PS.AvailableKarma());
		return FText::GetEmpty();
	}

	PS.KarmaSpent += Cost;
	HintsPurchased.FindOrAdd(WalletAddress).Add(HintKey);

	UE_LOG(LogGoldenEgg, Log, TEXT("%s purchased hint %d for clue #%d (-%d karma)"),
		*WalletAddress, HintNumber, ClueId, Cost);

	return HintNumber == 1 ? Clue->Hint1 : HintNumber == 2 ? Clue->Hint2 : Clue->Hint3;
}

void AGoldenEggManager::CheckClueReveal(const FString& WalletAddress, EConsciousnessLevel Level)
{
	if (!HasAuthority()) return;
	const int32 LevelInt = (int32)Level;
	UE_LOG(LogGoldenEgg, Log, TEXT("CL%d reached by %s — check clue reveal"), LevelInt, *WalletAddress);
}

bool AGoldenEggManager::AttemptClaim(const FString& WalletAddress, EConsciousnessLevel Level, int32 TotalAvatarQuestsCompleted)
{
	if (!HasAuthority() || bThreeWinnersFilled) return false;

	const bool bMaxLevel  = Level == EConsciousnessLevel::OnTheStar;
	const bool bAllQuests = TotalAvatarQuestsCompleted >= TOTAL_QUESTS_REQUIRED;

	FGoldenEggPlayerProgress* PS = PlayerStates.Find(WalletAddress);
	const bool bAllClues  = PS && PS->CluesSolved >= TOTAL_CLUES;
	const bool bAllKeys   = PS && PS->MasterKeysUnlocked >= TOTAL_KEYS_REQUIRED;

	if (!bMaxLevel || !bAllQuests || !bAllClues || !bAllKeys)
	{
		UE_LOG(LogGoldenEgg, Log, TEXT("Claim failed — CL9:%d Quests:%d/%d Clues:%s Keys:%s"),
			bMaxLevel, TotalAvatarQuestsCompleted, TOTAL_QUESTS_REQUIRED,
			bAllClues ? TEXT("yes") : TEXT("no"), bAllKeys ? TEXT("yes") : TEXT("no"));
		return false;
	}

	int32 Place = 0;
	if (Winner1st.IsEmpty())      { Winner1st = WalletAddress; Place = 1; }
	else if (Winner2nd.IsEmpty()) { Winner2nd = WalletAddress; Place = 2; }
	else if (Winner3rd.IsEmpty()) { Winner3rd = WalletAddress; Place = 3; bThreeWinnersFilled = true; }
	else return false;

	AwardPlace(Place, WalletAddress);
	return true;
}

void AGoldenEggManager::AwardPlace(int32 Place, const FString& WalletAddress)
{
	static const int64 Prizes[] = { 0, PRIZE_1ST, PRIZE_2ND, PRIZE_3RD };
	static const TCHAR* Titles[] = { TEXT(""), TEXT("1st"), TEXT("2nd"), TEXT("3rd") };

	UE_LOG(LogGoldenEgg, Log, TEXT("WINNER #%d %s: %s — %lld ZION!"),
		Place, Titles[Place], *WalletAddress, Prizes[Place]);

	OnWinner.Broadcast(Place, WalletAddress);

	if (UZionBlockchainBridge* Bridge = UZionBlockchainBridge::Get(GetWorld()))
	{
		const FString Source = FString::Printf(TEXT("golden_egg_place_%d"), Place);
		Bridge->AwardXp(WalletAddress, Source, Prizes[Place], FZionHttpCallback::CreateLambda(
			[Place](const FString& Json, bool bOk)
		{
			if (!bOk) UE_LOG(LogGoldenEgg, Error, TEXT("Prize #%d notification FAILED — manual payout needed"), Place);
		}));
	}
}

bool AGoldenEggManager::HasWinner(int32 Place) const
{
	if (Place == 1) return !Winner1st.IsEmpty();
	if (Place == 2) return !Winner2nd.IsEmpty();
	if (Place == 3) return !Winner3rd.IsEmpty();
	return false;
}

FString AGoldenEggManager::GetWinnerWallet(int32 Place) const
{
	if (Place == 1) return Winner1st;
	if (Place == 2) return Winner2nd;
	if (Place == 3) return Winner3rd;
	return TEXT("");
}

int32 AGoldenEggManager::GetSolvedClueCount() const
{
	return SolvedClueIds.Num();
}

FGoldenEggPlayerProgress AGoldenEggManager::GetPlayerState(const FString& WalletAddress) const
{
	if (const FGoldenEggPlayerProgress* PS = PlayerStates.Find(WalletAddress)) return *PS;
	FGoldenEggPlayerProgress Empty;
	Empty.WalletAddress = WalletAddress;
	return Empty;
}

int32 AGoldenEggManager::GetHintCost(int32 HintNumber) const
{
	if (HintNumber == 1) return HINT_COST_1;
	if (HintNumber == 2) return HINT_COST_2;
	return HINT_COST_3;
}

bool AGoldenEggManager::HasPlayerSolvedClue(const FString& WalletAddress, int32 ClueId) const
{
	const FGoldenEggClue* C = AllClues.FindByPredicate([ClueId](const FGoldenEggClue& X){ return X.ClueId == ClueId; });
	return C && C->SolvedByWallet == WalletAddress;
}

void AGoldenEggManager::UpdateMasterKeys(const FString& WalletAddress)
{
	FGoldenEggPlayerProgress& PS = PlayerStates.FindOrAdd(WalletAddress);
	int32 Unlocked = 0;
	for (const FGoldenEggMasterKey& Key : MasterKeys)
	{
		if (PS.CluesSolved >= Key.RequiredClues)
			Unlocked++;
	}
	PS.MasterKeysUnlocked = Unlocked;
}
