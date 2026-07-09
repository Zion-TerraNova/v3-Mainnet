// Copyright 2026 ZION TerraNova. All Rights Reserved.
#include "ZionOasisGameMode.h"
#include "ZionGameInstance.h"
#include "ZionOasis/Player/ZionPlayerController.h"
#include "ZionOasis/Player/ZionCharacter.h"
#include "ZionOasis/Consciousness/ConsciousnessComponent.h"
#include "ZionOasis/Blockchain/ZionBlockchainBridge.h"
#include "GameFramework/PlayerState.h"
#include "Engine/World.h"
#include "Kismet/GameplayStatics.h"

AZionOasisGameMode::AZionOasisGameMode()
{
	MaxPlayersPerRealm = 10000;
}

void AZionOasisGameMode::InitGame(const FString& MapName, const FString& Options, FString& ErrorMessage)
{
	Super::InitGame(MapName, Options, ErrorMessage);

	UE_LOG(LogTemp, Log, TEXT("[ZionOasisGameMode] InitGame — map: %s, options: %s"), *MapName, *Options);

	if (UZionBlockchainBridge* Bridge = UZionBlockchainBridge::Get(GetWorld()))
	{
		Bridge->HealthCheck(FZionHttpCallback::CreateLambda([](const FString& Msg, bool bOk)
		{
			if (bOk)
				UE_LOG(LogTemp, Log, TEXT("[ZionOasisGameMode] Backend healthy: %s"), *Msg)
			else
				UE_LOG(LogTemp, Warning, TEXT("[ZionOasisGameMode] Backend OFFLINE: %s"), *Msg);
		}));
	}
}

APlayerController* AZionOasisGameMode::Login(UPlayer* NewPlayer, ENetRole InRemoteRole,
	const FString& Portal, const FString& Options, const FUniqueNetIdRepl& UniqueId,
	FString& ErrorMessage)
{
	const FString Wallet = UGameplayStatics::ParseOption(Options, TEXT("wallet"));
	if (!Wallet.IsEmpty())
	{
		UE_LOG(LogTemp, Log, TEXT("[ZionOasisGameMode] Login — wallet: %s"), *Wallet);
	}

	return Super::Login(NewPlayer, InRemoteRole, Portal, Options, UniqueId, ErrorMessage);
}

void AZionOasisGameMode::BroadcastBlockMined(const FString& MinerWallet, int32 BlockHeight)
{
	UE_LOG(LogTemp, Log, TEXT("[ZionOasisGameMode] Block #%lld mined by %s — broadcasting XP"),
		BlockHeight, *MinerWallet);

	for (FConstPlayerControllerIterator It = GetWorld()->GetPlayerControllerIterator(); It; ++It)
	{
		AZionPlayerController* PC = Cast<AZionPlayerController>(It->Get());
		if (!PC) continue;

		AZionCharacter* ZionChar = Cast<AZionCharacter>(PC->GetPawn());
		if (!ZionChar) continue;

		if (MinerWallet.IsEmpty() || ZionChar->WalletAddress == MinerWallet)
		{
			ZionChar->OnBlockMined();
		}
	}
}
