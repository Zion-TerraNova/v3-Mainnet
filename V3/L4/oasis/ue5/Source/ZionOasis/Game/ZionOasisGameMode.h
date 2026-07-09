// Copyright 2026 ZION TerraNova. All Rights Reserved.
#pragma once

#include "CoreMinimal.h"
#include "GameFramework/GameModeBase.h"
#include "ZionOasisGameMode.generated.h"

/**
 * ZionOasisGameMode
 * Server-side authority for ZION OASIS world sessions.
 */
UCLASS(BlueprintType, meta=(DisplayName="ZION Oasis Game Mode"))
class ZIONOASIS_API AZionOasisGameMode : public AGameModeBase
{
	GENERATED_BODY()
public:
	AZionOasisGameMode();

	virtual void InitGame(const FString& MapName, const FString& Options,
						  FString& ErrorMessage) override;
	virtual APlayerController* Login(UPlayer* NewPlayer, ENetRole InRemoteRole,
									 const FString& Portal, const FString& Options,
									 const FUniqueNetIdRepl& UniqueId,
									 FString& ErrorMessage) override;

	/** Called when a new player successfully logs in with a valid wallet */
	void OnPlayerWalletLogin(APlayerController* PC, const FString& Wallet);

	/** Award block-mined XP to all online miners (called from L1 block hook) */
	UFUNCTION(BlueprintCallable, Category = "ZION|Mining")
	void BroadcastBlockMined(const FString& MinerWallet, int32 BlockHeight);

protected:
	/** Default player class (BP_ZionCharacter) */
	UPROPERTY(EditDefaultsOnly, BlueprintReadOnly, Category = "Config")
	TSubclassOf<ACharacter> DefaultCharacterClass;

	UPROPERTY(EditDefaultsOnly, BlueprintReadOnly, Category = "Config")
	int32 MaxPlayersPerRealm = 10000;
};
