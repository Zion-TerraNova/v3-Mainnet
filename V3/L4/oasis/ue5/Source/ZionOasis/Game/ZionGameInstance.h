// Copyright 2026 ZION TerraNova. All Rights Reserved.
#pragma once

#include "CoreMinimal.h"
#include "Engine/GameInstance.h"
#include "ZionGameInstance.generated.h"

class UZionBlockchainBridge;

/**
 * ZionGameInstance
 *
 * Persistent across level loads.
 * Owns the ZionBlockchainBridge singleton.
 * Stores player session data (wallet, display name).
 */
UCLASS(BlueprintType, meta=(DisplayName="ZION Game Instance"))
class ZIONOASIS_API UZionGameInstance : public UGameInstance
{
	GENERATED_BODY()

public:
	UZionGameInstance();

	virtual void Init() override;
	virtual void Shutdown() override;

	UFUNCTION(BlueprintPure, Category = "ZION|Blockchain")
	UZionBlockchainBridge* GetBridge() const { return BlockchainBridge; }

	UPROPERTY(BlueprintReadOnly, Category = "ZION|Session")
	FString ActiveWallet;

	UPROPERTY(BlueprintReadOnly, Category = "ZION|Session")
	FString ActiveDisplayName;

	// API endpoints — override via Project Settings / CLI args
	UPROPERTY(Config, EditDefaultsOnly, Category = "ZION|Config")
	FString OasisApiHost = TEXT("http://localhost:8094");

	UPROPERTY(Config, EditDefaultsOnly, Category = "ZION|Config")
	FString ChainRpcHost = TEXT("http://localhost:8444");

private:
	UPROPERTY()
	TObjectPtr<UZionBlockchainBridge> BlockchainBridge;
};
