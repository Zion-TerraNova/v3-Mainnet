// Copyright 2026 ZION TerraNova. All Rights Reserved.
#pragma once

#include "CoreMinimal.h"
#include "Components/ActorComponent.h"
#include "GuildTypes.h"
#include "GuildComponent.generated.h"

DECLARE_DYNAMIC_MULTICAST_DELEGATE_OneParam(FOnGuildJoined,  FGuildData, Guild);
DECLARE_DYNAMIC_MULTICAST_DELEGATE(FOnGuildLeft);
DECLARE_DYNAMIC_MULTICAST_DELEGATE_OneParam(FOnGuildUpdated, FGuildData, Guild);

/**
 * GuildComponent
 * Attach to: ZionCharacter
 * Manages guild membership, syncs with oasis REST backend.
 */
UCLASS(ClassGroup=(ZionOasis), meta=(BlueprintSpawnableComponent),
	   DisplayName="Guild Component")
class ZIONOASIS_API UGuildComponent : public UActorComponent
{
	GENERATED_BODY()

public:
	UGuildComponent();

	// === Events ===
	UPROPERTY(BlueprintAssignable, Category = "Guild|Events")
	FOnGuildJoined  OnGuildJoined;

	UPROPERTY(BlueprintAssignable, Category = "Guild|Events")
	FOnGuildLeft    OnGuildLeft;

	UPROPERTY(BlueprintAssignable, Category = "Guild|Events")
	FOnGuildUpdated OnGuildUpdated;

	// === State ===
	UFUNCTION(BlueprintPure, Category = "Guild")
	bool IsInGuild() const { return !CurrentGuildId.IsEmpty(); }

	UFUNCTION(BlueprintPure, Category = "Guild")
	FString GetGuildId() const { return CurrentGuildId; }

	UFUNCTION(BlueprintPure, Category = "Guild")
	FGuildData GetGuildData() const { return CachedGuild; }

	// === Actions ===
	UFUNCTION(BlueprintCallable, Category = "Guild")
	void JoinGuild(const FString& GuildId, const FString& WalletAddress);

	UFUNCTION(BlueprintCallable, Category = "Guild")
	void CreateGuild(const FString& GuildName, EGuildOrder Order, const FString& FounderWallet);

	UFUNCTION(BlueprintCallable, Category = "Guild")
	void LeaveGuild();

	UFUNCTION(BlueprintCallable, Category = "Guild")
	void RefreshGuildData(const FString& GuildId);

protected:
	virtual void BeginPlay() override;
	virtual void GetLifetimeReplicatedProps(TArray<FLifetimeProperty>& OutLifetimeProps) const override;

private:
	UPROPERTY(Replicated)
	FString CurrentGuildId;

	UPROPERTY()
	FGuildData CachedGuild;
};
