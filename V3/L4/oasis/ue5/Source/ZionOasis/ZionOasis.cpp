// Copyright 2026 ZION TerraNova. All Rights Reserved.
#include "ZionOasis.h"
#include "Modules/ModuleManager.h"
#include "Logging/LogMacros.h"

IMPLEMENT_PRIMARY_GAME_MODULE(FZionOasisModule, ZionOasis, "ZionOasis");

DEFINE_LOG_CATEGORY_STATIC(LogZionOasis, Log, All);

void FZionOasisModule::StartupModule()
{
	UE_LOG(LogZionOasis, Log, TEXT("=== ZION OASIS V3 — Module Startup ==="));
	UE_LOG(LogZionOasis, Log, TEXT("The Golden Egg Chronicles — Unreal Engine 5.4"));
	UE_LOG(LogZionOasis, Log, TEXT("Rust backend port: %d"), ZION_OASIS_API_PORT);
}

void FZionOasisModule::ShutdownModule()
{
	UE_LOG(LogZionOasis, Log, TEXT("=== ZION OASIS V3 — Module Shutdown ==="));
}
