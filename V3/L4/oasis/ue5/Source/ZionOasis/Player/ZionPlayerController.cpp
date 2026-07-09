// Copyright 2026 ZION TerraNova. All Rights Reserved.
#include "ZionPlayerController.h"
#include "ZionCharacter.h"
#include "ZionOasis/Blockchain/ZionBlockchainBridge.h"
#include "ZionOasis/Consciousness/ConsciousnessComponent.h"
#include "ZionOasis/Game/ZionGameInstance.h"
#include "Engine/World.h"
#include "Net/UnrealNetwork.h"

AZionPlayerController::AZionPlayerController()
{
}

void AZionPlayerController::BeginPlay()
{
	Super::BeginPlay();
}

void AZionPlayerController::OnPossess(APawn* InPawn)
{
	Super::OnPossess(InPawn);
}

void AZionPlayerController::ConnectWallet(const FString& WalletAddress)
{
	if (WalletAddress.IsEmpty()) return;

	ConnectedWallet = WalletAddress;

	if (UZionGameInstance* GI = GetGameInstance<UZionGameInstance>())
	{
		GI->ActiveWallet = WalletAddress;
	}

	OnWalletConnected.Broadcast(WalletAddress);
	SyncPlayerFromBackend();

	UE_LOG(LogTemp, Log, TEXT("[ZionPlayerController] Wallet connected: %s"), *WalletAddress);
}

void AZionPlayerController::DisconnectWallet()
{
	const FString OldWallet = ConnectedWallet;
	ConnectedWallet = TEXT("");

	if (UZionGameInstance* GI = GetGameInstance<UZionGameInstance>())
	{
		GI->ActiveWallet = TEXT("");
	}

	OnWalletDisconnected.Broadcast();
	UE_LOG(LogTemp, Log, TEXT("[ZionPlayerController] Wallet disconnected"));
}

void AZionPlayerController::ServerInteract_Implementation()
{
	AZionCharacter* ZionChar = Cast<AZionCharacter>(GetPawn());
	if (!ZionChar) return;

	FVector Start, Direction;
	FRotator Rot;
	GetPlayerViewPoint(Start, Rot);
	Direction = GetControlRotation().Vector();

	FHitResult Hit;
	FCollisionQueryParams Params;
	Params.AddIgnoredActor(ZionChar);

	const bool bHit = GetWorld()->LineTraceSingleByChannel(
		Hit, Start, Start + Direction * 500.0f, ECC_Visibility, Params);

	if (bHit && Hit.GetActor())
	{
		UE_LOG(LogTemp, Log, TEXT("[ZionPlayerController] Interact hit: %s"), *Hit.GetActor()->GetName());
	}
}

void AZionPlayerController::OpenAvatarQuestUI(EAvatarID AvatarID)
{
	UE_LOG(LogTemp, Log, TEXT("[ZionPlayerController] Opening quest UI for avatar %d"), (int32)AvatarID);
}

AZionCharacter* AZionPlayerController::GetZionCharacter() const
{
	return Cast<AZionCharacter>(GetPawn());
}

void AZionPlayerController::SyncPlayerFromBackend()
{
	if (ConnectedWallet.IsEmpty()) return;

	UZionBlockchainBridge* Bridge = UZionBlockchainBridge::Get(GetWorld());
	if (!Bridge) return;

	Bridge->GetPlayer(ConnectedWallet, FZionHttpCallback::CreateLambda([this](const FString& Json, bool bSuccess)
	{
		if (!bSuccess)
		{
			UE_LOG(LogTemp, Warning, TEXT("[ZionPlayerController] Failed to sync player %s"), *ConnectedWallet);
			return;
		}

		AZionCharacter* ZionChar = Cast<AZionCharacter>(GetPawn());
		if (ZionChar && ZionChar->ConsciousnessComp)
		{
			ZionChar->ConsciousnessComp->ApplyStateFromJson(Json);
		}
	}));
}

void AZionPlayerController::GetLifetimeReplicatedProps(TArray<FLifetimeProperty>& OutLifetimeProps) const
{
	Super::GetLifetimeReplicatedProps(OutLifetimeProps);
	DOREPLIFETIME(AZionPlayerController, ConnectedWallet);
}
