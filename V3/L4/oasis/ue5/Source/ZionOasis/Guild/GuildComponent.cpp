// Copyright 2026 ZION TerraNova. All Rights Reserved.
#include "GuildComponent.h"
#include "ZionOasis/Blockchain/ZionBlockchainBridge.h"
#include "Net/UnrealNetwork.h"
#include "Engine/World.h"
#include "Logging/LogMacros.h"

DEFINE_LOG_CATEGORY_STATIC(LogGuildComponent, Log, All);

UGuildComponent::UGuildComponent()
{
	PrimaryComponentTick.bCanEverTick = false;
	SetIsReplicatedByDefault(true);
}

void UGuildComponent::GetLifetimeReplicatedProps(TArray<FLifetimeProperty>& OutLifetimeProps) const
{
	Super::GetLifetimeReplicatedProps(OutLifetimeProps);
	DOREPLIFETIME(UGuildComponent, CurrentGuildId);
	DOREPLIFETIME(UGuildComponent, CachedGuild);
}

void UGuildComponent::BeginPlay()
{
	Super::BeginPlay();
}

void UGuildComponent::JoinGuild(const FString& GuildId, const FString& WalletAddress)
{
	UZionBlockchainBridge* Bridge = UZionBlockchainBridge::Get(GetWorld());
	if (!Bridge) return;

	Bridge->JoinGuild(GuildId, WalletAddress, FZionHttpCallback::CreateLambda(
		[this, GuildId](const FString& Json, bool bSuccess)
	{
		if (bSuccess)
		{
			CurrentGuildId = GuildId;
			OnGuildJoined.Broadcast(CachedGuild);
			RefreshGuildData(GuildId);
			UE_LOG(LogGuildComponent, Log, TEXT("Joined guild: %s"), *GuildId);
		}
		else
		{
			UE_LOG(LogGuildComponent, Warning, TEXT("JoinGuild failed for %s"), *GuildId);
		}
	}));
}

void UGuildComponent::CreateGuild(const FString& GuildName, EGuildOrder Order, const FString& FounderWallet)
{
	UZionBlockchainBridge* Bridge = UZionBlockchainBridge::Get(GetWorld());
	if (!Bridge) return;

	Bridge->CreateGuild(GuildName, FounderWallet, FZionHttpCallback::CreateLambda(
		[this](const FString& Json, bool bSuccess)
	{
		if (!bSuccess)
		{
			UE_LOG(LogGuildComponent, Warning, TEXT("CreateGuild failed"));
			return;
		}

		TSharedPtr<FJsonObject> Obj;
		TSharedRef<TJsonReader<>> Reader = TJsonReaderFactory<>::Create(Json);
		if (FJsonSerializer::Deserialize(Reader, Obj) && Obj.IsValid())
		{
			const FString NewId = Obj->GetStringField(TEXT("guild_id"));
			CurrentGuildId = NewId;
			OnGuildJoined.Broadcast(CachedGuild);
			RefreshGuildData(NewId);
			UE_LOG(LogGuildComponent, Log, TEXT("Created guild: %s"), *NewId);
		}
	}));
}

void UGuildComponent::LeaveGuild()
{
	const FString OldId = CurrentGuildId;
	CurrentGuildId = TEXT("");
	CachedGuild = FGuildData{};
	OnGuildLeft.Broadcast();
	UE_LOG(LogGuildComponent, Log, TEXT("Left guild: %s"), *OldId);
}

void UGuildComponent::RefreshGuildData(const FString& GuildId)
{
	UZionBlockchainBridge* Bridge = UZionBlockchainBridge::Get(GetWorld());
	if (!Bridge) return;

	Bridge->GetGuild(GuildId, FZionHttpCallback::CreateLambda(
		[this](const FString& Json, bool bSuccess)
	{
		if (!bSuccess) return;

		TSharedPtr<FJsonObject> Obj;
		TSharedRef<TJsonReader<>> Reader = TJsonReaderFactory<>::Create(Json);
		if (!FJsonSerializer::Deserialize(Reader, Obj) || !Obj.IsValid()) return;

		CachedGuild.GuildId   = Obj->GetStringField(TEXT("guild_id"));
		CachedGuild.GuildName = Obj->GetStringField(TEXT("name"));
		CachedGuild.GuildXp   = (int64)Obj->GetNumberField(TEXT("guild_xp"));
		CachedGuild.GuildLevel= (int32)Obj->GetNumberField(TEXT("guild_level"));

		OnGuildUpdated.Broadcast(CachedGuild);
	}));
}
