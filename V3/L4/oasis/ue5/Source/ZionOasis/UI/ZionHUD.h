// Copyright 2026 ZION TerraNova. All Rights Reserved.
#pragma once

#include "CoreMinimal.h"
#include "GameFramework/HUD.h"
#include "ZionHUD.generated.h"

/**
 * ZionHUD
 * Root HUD class — manages UMG widget stack (main menu, HUD overlay,
 * consciousness indicator, guild panel, territory map, Golden Egg tracker).
 */
UCLASS(BlueprintType, meta=(DisplayName="ZION HUD"))
class ZIONOASIS_API AZionHUD : public AHUD
{
	GENERATED_BODY()

public:
	AZionHUD();
	virtual void BeginPlay() override;

	/** Push a named widget to the viewport stack */
	UFUNCTION(BlueprintCallable, Category = "ZION|UI")
	UUserWidget* PushWidget(TSubclassOf<UUserWidget> WidgetClass, int32 ZOrder = 0);

	/** Remove widget from viewport by class */
	UFUNCTION(BlueprintCallable, Category = "ZION|UI")
	void PopWidget(TSubclassOf<UUserWidget> WidgetClass);

	// === Pre-configured Widget Classes (set in BP_ZionHUD) ===
	UPROPERTY(EditDefaultsOnly, Category = "ZION|UI|Widgets")
	TSubclassOf<UUserWidget> MainMenuWidgetClass;

	UPROPERTY(EditDefaultsOnly, Category = "ZION|UI|Widgets")
	TSubclassOf<UUserWidget> HudOverlayWidgetClass;

	UPROPERTY(EditDefaultsOnly, Category = "ZION|UI|Widgets")
	TSubclassOf<UUserWidget> ConsciousnessBarWidgetClass;

	UPROPERTY(EditDefaultsOnly, Category = "ZION|UI|Widgets")
	TSubclassOf<UUserWidget> GuildPanelWidgetClass;

	UPROPERTY(EditDefaultsOnly, Category = "ZION|UI|Widgets")
	TSubclassOf<UUserWidget> TerritoryMapWidgetClass;

	UPROPERTY(EditDefaultsOnly, Category = "ZION|UI|Widgets")
	TSubclassOf<UUserWidget> GoldenEggTrackerWidgetClass;

	UPROPERTY(EditDefaultsOnly, Category = "ZION|UI|Widgets")
	TSubclassOf<UUserWidget> AvatarQuestDialogWidgetClass;

	UPROPERTY(EditDefaultsOnly, Category = "ZION|UI|Widgets")
	TSubclassOf<UUserWidget> WalletLoginWidgetClass;

private:
	UPROPERTY()
	TMap<TSubclassOf<UUserWidget>, TObjectPtr<UUserWidget>> ActiveWidgets;
};
