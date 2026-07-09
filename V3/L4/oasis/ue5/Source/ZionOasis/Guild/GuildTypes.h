// Copyright 2026 ZION TerraNova. All Rights Reserved.
#pragma once

#include "CoreMinimal.h"
#include "ZionOasis/Consciousness/ConsciousnessTypes.h"
#include "GuildTypes.generated.h"

/** Guild spiritual order type */
UENUM(BlueprintType)
enum class EGuildOrder : uint8
{
	OrderOfLight        UMETA(DisplayName = "Order of Light — Blue Ray"),
	SeekersOfWisdom     UMETA(DisplayName = "Seekers of Wisdom — Yellow Ray"),
	HeartBrotherhood    UMETA(DisplayName = "Heart Brotherhood — Pink Ray"),
	AscensionTemple     UMETA(DisplayName = "Ascension Temple — White Ray"),
	HealingCircle       UMETA(DisplayName = "Healing Circle — Green Ray"),
	ServantWarriors     UMETA(DisplayName = "Servant Warriors — Ruby-Gold Ray"),
	VioletFlame         UMETA(DisplayName = "Violet Flame Order — Violet Ray"),
	ZionGuardians       UMETA(DisplayName = "ZION Guardians — All Rays"),
};

/** Single guild member entry */
USTRUCT(BlueprintType)
struct FGuildMember
{
	GENERATED_BODY()

	UPROPERTY(BlueprintReadOnly)
	FString WalletAddress;

	UPROPERTY(BlueprintReadOnly)
	FString DisplayName;

	UPROPERTY(BlueprintReadOnly)
	EConsciousnessLevel Level = EConsciousnessLevel::Physical;

	UPROPERTY(BlueprintReadOnly)
	int64 Contribution = 0;  // XP contributed to guild

	UPROPERTY(BlueprintReadOnly)
	bool bIsOfficer = false;

	UPROPERTY(BlueprintReadOnly)
	bool bIsFounder = false;
};

/** Full guild data — mirrors Guild struct in guild.rs */
USTRUCT(BlueprintType)
struct FGuildData
{
	GENERATED_BODY()

	UPROPERTY(BlueprintReadOnly)
	FString GuildId;

	UPROPERTY(BlueprintReadOnly)
	FString GuildName;

	UPROPERTY(BlueprintReadOnly)
	FString FounderWallet;

	UPROPERTY(BlueprintReadOnly)
	EGuildOrder Order = EGuildOrder::ZionGuardians;

	UPROPERTY(BlueprintReadOnly)
	TArray<FGuildMember> Members;  // max 100

	UPROPERTY(BlueprintReadOnly)
	int64 GuildXp = 0;

	UPROPERTY(BlueprintReadOnly)
	int32 GuildLevel = 1;

	/** Territory IDs controlled by this guild */
	UPROPERTY(BlueprintReadOnly)
	TArray<FString> ControlledTerritories;

	UPROPERTY(BlueprintReadOnly)
	FDateTime CreatedAt;
};

/** Minimum XP to join a guild (Emotional level = 1000 XP) */
static const int64 GUILD_MIN_XP_JOIN   = 1000;
/** Minimum XP to found a guild (Mental level = 5000 XP) */
static const int64 GUILD_MIN_XP_CREATE = 5000;
/** Max members per guild */
static const int32 GUILD_MAX_MEMBERS   = 100;
