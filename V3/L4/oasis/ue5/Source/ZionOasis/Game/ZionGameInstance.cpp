// Copyright 2026 ZION TerraNova. All Rights Reserved.
#include "ZionGameInstance.h"
#include "ZionOasis/Blockchain/ZionBlockchainBridge.h"
#include "Engine/World.h"

UZionGameInstance::UZionGameInstance()
{
}

void UZionGameInstance::Init()
{
	Super::Init();

	BlockchainBridge = NewObject<UZionBlockchainBridge>(this, TEXT("BlockchainBridge"));
	if (BlockchainBridge)
	{
		BlockchainBridge->Initialize(OasisApiHost, ChainRpcHost);
		UE_LOG(LogTemp, Log, TEXT("[ZionGameInstance] BlockchainBridge initialised — oasis: %s  rpc: %s"),
			*OasisApiHost, *ChainRpcHost);
	}
}

void UZionGameInstance::Shutdown()
{
	BlockchainBridge = nullptr;
	Super::Shutdown();
}
