// Copyright 2026 ZION TerraNova. All Rights Reserved.
#include "ConsciousnessComponent.h"
#include "ZionOasis/Blockchain/ZionBlockchainBridge.h"
#include "Net/UnrealNetwork.h"
#include "Logging/LogMacros.h"

DEFINE_LOG_CATEGORY_STATIC(LogConsciousness, Log, All);

UConsciousnessComponent::UConsciousnessComponent()
{
	PrimaryComponentTick.bCanEverTick = false;
	SetIsReplicatedByDefault(true);
}

void UConsciousnessComponent::BeginPlay()
{
	Super::BeginPlay();
}

void UConsciousnessComponent::GetLifetimeReplicatedProps(
	TArray<FLifetimeProperty>& OutLifetimeProps) const
{
	Super::GetLifetimeReplicatedProps(OutLifetimeProps);
	DOREPLIFETIME(UConsciousnessComponent, TotalXp);
	DOREPLIFETIME(UConsciousnessComponent, DailyXpEarned);
	DOREPLIFETIME(UConsciousnessComponent, CurrentLevel);
}

FXpAward UConsciousnessComponent::AwardXp(EXpSource Source, int64 Amount)
{
	FXpAward Award;

	const int64 Remaining = DailyXpCap - DailyXpEarned;
	if (Remaining <= 0)
	{
		Award.bDailyCapped = true;
		OnDailyCapped.Broadcast();
		return Award;
	}

	const float Multiplier = CONSCIOUSNESS_MULTIPLIERS[
		FMath::Clamp((int32)CurrentLevel - 1, 0, 8)];
	const int64 RawXp     = FMath::Min(Amount, Remaining);
	const int64 FinalXp   = FMath::CeilToInt64(RawXp * Multiplier);
	const int64 Capped    = FMath::Min(FinalXp, Remaining);

	EConsciousnessLevel OldLevel = CurrentLevel;

	TotalXp        += Capped;
	DailyXpEarned  += Capped;
	CurrentLevel    = ComputeLevel(TotalXp);

	Award.XpAwarded     = Capped;
	Award.TotalXp       = TotalXp;
	Award.NewLevel      = CurrentLevel;
	Award.bLeveledUp    = (CurrentLevel != OldLevel);
	Award.DailyXpEarned = DailyXpEarned;

	OnXpAwarded.Broadcast(Award);

	if (Award.bLeveledUp)
	{
		UE_LOG(LogConsciousness, Log, TEXT("Level up! %d → %d (TotalXP=%lld)"),
			(int32)OldLevel, (int32)CurrentLevel, TotalXp);
		OnLevelUp.Broadcast(OldLevel, CurrentLevel);
	}

	return Award;
}

void UConsciousnessComponent::SyncFromBackend(const FString& WalletAddress)
{
	if (UZionBlockchainBridge* Bridge = UZionBlockchainBridge::Get(GetWorld()))
	{
		Bridge->GetPlayer(WalletAddress, FZionHttpCallback::CreateLambda([this](const FString& Json, bool bSuccess)
		{
			if (bSuccess) ApplyStateFromJson(Json);
		}));
	}
}

void UConsciousnessComponent::ApplyStateFromJson(const FString& JsonPayload)
{
	TSharedPtr<FJsonObject> Root;
	TSharedRef<TJsonReader<>> Reader = TJsonReaderFactory<>::Create(JsonPayload);
	if (!FJsonSerializer::Deserialize(Reader, Root) || !Root.IsValid()) return;

	TSharedPtr<FJsonObject> Data = Root->GetObjectField(TEXT("data"));
	if (!Data.IsValid()) return;

	TotalXp       = (int64)Data->GetNumberField(TEXT("total_xp"));
	DailyXpEarned = (int64)Data->GetNumberField(TEXT("daily_xp"));
	CurrentLevel  = ComputeLevel(TotalXp);
}

float UConsciousnessComponent::GetLevelProgress() const
{
	int32 LvlIdx = FMath::Clamp((int32)CurrentLevel - 1, 0, 8);
	int64 Current = CONSCIOUSNESS_XP_THRESHOLDS[LvlIdx];
	int64 Next    = (LvlIdx < 8) ? CONSCIOUSNESS_XP_THRESHOLDS[LvlIdx + 1] : INT64_MAX;
	if (Next == Current) return 1.0f;
	return FMath::Clamp((float)(TotalXp - Current) / (float)(Next - Current), 0.0f, 1.0f);
}

int64 UConsciousnessComponent::GetXpToNextLevel() const
{
	int32 LvlIdx = FMath::Clamp((int32)CurrentLevel - 1, 0, 8);
	if (LvlIdx >= 8) return 0;
	return CONSCIOUSNESS_XP_THRESHOLDS[LvlIdx + 1] - TotalXp;
}

FText UConsciousnessComponent::GetLevelDisplayName() const
{
	static const TCHAR* Names[] = {
		TEXT("Physical"),TEXT("Emotional"),TEXT("Mental"),TEXT("Intuitional"),
		TEXT("Spiritual"),TEXT("Cosmic"),TEXT("Divine"),TEXT("Unity"),TEXT("On The Star")
	};
	int32 i = FMath::Clamp((int32)CurrentLevel - 1, 0, 8);
	return FText::FromString(Names[i]);
}

FText UConsciousnessComponent::GetSefiraName() const
{
	static const TCHAR* Sefirot[] = {
		TEXT("Malkuth"),TEXT("Yesod"),TEXT("Hod/Netzach"),TEXT("Tiferet"),
		TEXT("Gevurah/Chesed"),TEXT("Binah"),TEXT("Chokmah"),TEXT("Da'at"),TEXT("Keter")
	};
	int32 i = FMath::Clamp((int32)CurrentLevel - 1, 0, 8);
	return FText::FromString(Sefirot[i]);
}

float UConsciousnessComponent::GetXpMultiplier() const
{
	return CONSCIOUSNESS_MULTIPLIERS[FMath::Clamp((int32)CurrentLevel - 1, 0, 8)];
}

EConsciousnessLevel UConsciousnessComponent::ComputeLevel(int64 Xp) const
{
	for (int32 i = 8; i >= 0; i--)
	{
		if (Xp >= CONSCIOUSNESS_XP_THRESHOLDS[i])
			return static_cast<EConsciousnessLevel>(i + 1);
	}
	return EConsciousnessLevel::Physical;
}
