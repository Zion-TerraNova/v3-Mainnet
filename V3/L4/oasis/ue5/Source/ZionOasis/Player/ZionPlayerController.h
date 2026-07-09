// Copyright 2026 ZION TerraNova. All Rights Reserved.
#pragma once

#include "CoreMinimal.h"
#include "GameFramework/PlayerController.h"
#include "ZionOasis/Avatar/AvatarTypes.h"
#include "ZionPlayerController.generated.h"

class UZionBlockchainBridge;
class AZionCharacter;

DECLARE_DYNAMIC_MULTICAST_DELEGATE_OneParam(FOnWalletConnected, const FString&, WalletAddress);
DECLARE_DYNAMIC_MULTICAST_DELEGATE(FOnWalletDisconnected);

/**
 * ZionPlayerController
 *
 * Manages player session: wallet login, avatar selection, interaction with
 * NPCs/avatars, server RPC dispatch.
 */
UCLASS(BlueprintType, meta=(DisplayName="ZION Player Controller"))
class ZIONOASIS_API AZionPlayerController : public APlayerController
{
	GENERATED_BODY()

public:
	AZionPlayerController();

	UPROPERTY(BlueprintAssignable, Category = "ZION|Events")
	FOnWalletConnected OnWalletConnected;

	UPROPERTY(BlueprintAssignable, Category = "ZION|Events")
	FOnWalletDisconnected OnWalletDisconnected;

	/** Connect player wallet — called from login screen */
	UFUNCTION(BlueprintCallable, Category = "ZION|Wallet")
	void ConnectWallet(const FString& WalletAddress);

	/** Disconnect wallet (logout) */
	UFUNCTION(BlueprintCallable, Category = "ZION|Wallet")
	void DisconnectWallet();

	UFUNCTION(BlueprintPure, Category = "ZION|Wallet")
	FORCEINLINE bool IsWalletConnected() const { return !ConnectedWallet.IsEmpty(); }

	UFUNCTION(BlueprintPure, Category = "ZION|Wallet")
	FORCEINLINE FString GetWallet() const { return ConnectedWallet; }

	/** Interact with world object / NPC (triggered by ZionCharacter) */
	UFUNCTION(Server, Reliable)
	void ServerInteract();

	/** Open avatar selection for a quest NPC encounter */
	UFUNCTION(BlueprintCallable, Category = "ZION|Avatar")
	void OpenAvatarQuestUI(EAvatarID AvatarID);

	UFUNCTION(BlueprintCallable, Category = "ZION|Avatar")
	AZionCharacter* GetZionCharacter() const;

protected:
	virtual void BeginPlay() override;
	virtual void OnPossess(APawn* InPawn) override;

private:
	UPROPERTY(Replicated)
	FString ConnectedWallet;

	void SyncPlayerFromBackend();
};
