// Copyright 2026 ZION TerraNova. All Rights Reserved.
#pragma once

#include "CoreMinimal.h"
#include "GameFramework/Character.h"
#include "ZionOasis/Avatar/AvatarTypes.h"
#include "ZionOasis/Consciousness/ConsciousnessTypes.h"
#include "ZionCharacter.generated.h"

class UConsciousnessComponent;
class UGuildComponent;
class USpringArmComponent;
class UCameraComponent;
class UInputMappingContext;
class UInputAction;

/**
 * ZionCharacter
 *
 * The player pawn in ZION OASIS MMORPG.
 * - Carries ConsciousnessComponent (XP / levels)
 * - Carries GuildComponent (guild membership)
 * - MetaHuman-compatible skeleton
 * - Third-person camera (dark souls-style)
 */
UCLASS(Config=Game, BlueprintType, meta=(DisplayName="ZION Character"))
class ZIONOASIS_API AZionCharacter : public ACharacter
{
	GENERATED_BODY()

public:
	AZionCharacter();

	// === Components ===
	UPROPERTY(VisibleAnywhere, BlueprintReadOnly, Category = "Components")
	TObjectPtr<UConsciousnessComponent> ConsciousnessComp;

	UPROPERTY(VisibleAnywhere, BlueprintReadOnly, Category = "Components")
	TObjectPtr<UGuildComponent> GuildComp;

	UPROPERTY(VisibleAnywhere, BlueprintReadOnly, Category = "Components")
	TObjectPtr<USpringArmComponent> CameraBoom;

	UPROPERTY(VisibleAnywhere, BlueprintReadOnly, Category = "Components")
	TObjectPtr<UCameraComponent> FollowCamera;

	// === Player Identity ===
	UPROPERTY(Replicated, BlueprintReadOnly, Category = "Identity")
	FString WalletAddress;

	UPROPERTY(Replicated, BlueprintReadOnly, Category = "Identity")
	FString PlayerDisplayName;

	UPROPERTY(Replicated, BlueprintReadOnly, Category = "Identity")
	FEquippedAvatar EquippedAvatar;

	UPROPERTY(Replicated, BlueprintReadOnly, Category = "Progress")
	int32 TotalAvatarQuestsCompleted = 0;

	// === Gameplay ===
	UFUNCTION(BlueprintCallable, Category = "ZION|Combat")
	void PerformMeditation();

	UFUNCTION(BlueprintCallable, Category = "ZION|XP")
	void OnBlockMined();

	UFUNCTION(BlueprintCallable, Category = "ZION|XP")
	void CompleteAvatarQuest(EAvatarID AvatarID, int32 QuestIndex);

	UFUNCTION(BlueprintPure, Category = "ZION")
	EConsciousnessLevel GetConsciousnessLevel() const;

	UFUNCTION(BlueprintPure, Category = "ZION")
	bool CanJoinGuild() const;

	UFUNCTION(BlueprintPure, Category = "ZION")
	bool CanCreateGuild() const;

protected:
	virtual void BeginPlay() override;
	virtual void SetupPlayerInputComponent(UInputComponent* PlayerInputComponent) override;
	virtual void GetLifetimeReplicatedProps(TArray<FLifetimeProperty>& OutLifetimeProps) const override;

	// === Input Config ===
	UPROPERTY(EditDefaultsOnly, Category = "Input")
	TObjectPtr<UInputMappingContext> DefaultMappingContext;

	UPROPERTY(EditDefaultsOnly, Category = "Input")
	TObjectPtr<UInputAction> MoveAction;

	UPROPERTY(EditDefaultsOnly, Category = "Input")
	TObjectPtr<UInputAction> LookAction;

	UPROPERTY(EditDefaultsOnly, Category = "Input")
	TObjectPtr<UInputAction> JumpAction;

	UPROPERTY(EditDefaultsOnly, Category = "Input")
	TObjectPtr<UInputAction> MeditateAction;

	UPROPERTY(EditDefaultsOnly, Category = "Input")
	TObjectPtr<UInputAction> InteractAction;

private:
	void Move(const struct FInputActionValue& Value);
	void Look(const struct FInputActionValue& Value);
	void Interact();
};
