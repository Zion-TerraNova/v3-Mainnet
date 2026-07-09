// Copyright 2026 ZION TerraNova. All Rights Reserved.
#include "ZionHUD.h"
#include "Blueprint/UserWidget.h"
#include "Engine/World.h"

AZionHUD::AZionHUD()
{
}

void AZionHUD::BeginPlay()
{
	Super::BeginPlay();

	APlayerController* PC = GetOwningPlayerController();
	if (PC && HudOverlayWidgetClass)
	{
		PushWidget(HudOverlayWidgetClass, 0);
	}
	if (PC && ConsciousnessBarWidgetClass)
	{
		PushWidget(ConsciousnessBarWidgetClass, 1);
	}
}

UUserWidget* AZionHUD::PushWidget(TSubclassOf<UUserWidget> WidgetClass, int32 ZOrder)
{
	if (!WidgetClass) return nullptr;

	if (TObjectPtr<UUserWidget>* Found = ActiveWidgets.Find(WidgetClass))
	{
		(*Found)->SetVisibility(ESlateVisibility::Visible);
		return Found->Get();
	}

	APlayerController* PC = GetOwningPlayerController();
	if (!PC) return nullptr;

	UUserWidget* Widget = CreateWidget<UUserWidget>(PC, WidgetClass);
	if (Widget)
	{
		Widget->AddToViewport(ZOrder);
		ActiveWidgets.Add(WidgetClass, Widget);
	}
	return Widget;
}

void AZionHUD::PopWidget(TSubclassOf<UUserWidget> WidgetClass)
{
	if (!WidgetClass) return;

	if (TObjectPtr<UUserWidget>* Found = ActiveWidgets.Find(WidgetClass))
	{
		(*Found)->RemoveFromParent();
		ActiveWidgets.Remove(WidgetClass);
	}
}
