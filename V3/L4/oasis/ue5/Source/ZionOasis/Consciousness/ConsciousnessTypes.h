// Copyright 2026 ZION TerraNova. All Rights Reserved.
#pragma once

#include "CoreMinimal.h"
#include "ConsciousnessTypes.generated.h"

/**
 * 9 Consciousness Levels — mirrors zion-oasis Rust backend (V3).
 * Each level = Kabbalah Sefira (Tree of Life).
 * Must stay in sync with L4/oasis/src/consciousness.rs
 */
UENUM(BlueprintType)
enum class EConsciousnessLevel : uint8
{
	None          = 0  UMETA(DisplayName = "None"),
	Physical      = 1  UMETA(DisplayName = "Physical — Malkuth"),
	Emotional     = 2  UMETA(DisplayName = "Emotional — Yesod"),
	Mental        = 3  UMETA(DisplayName = "Mental — Hod/Netzach"),
	Intuitional   = 4  UMETA(DisplayName = "Intuitional — Tiferet"),
	Spiritual     = 5  UMETA(DisplayName = "Spiritual — Gevurah/Chesed"),
	Cosmic        = 6  UMETA(DisplayName = "Cosmic — Binah"),
	Divine        = 7  UMETA(DisplayName = "Divine — Chokmah"),
	Unity         = 8  UMETA(DisplayName = "Unity — Da'at"),
	OnTheStar     = 9  UMETA(DisplayName = "On The Star — Keter"),
};

/** XP source types — mirrors XpSource in Rust */
UENUM(BlueprintType)
enum class EXpSource : uint8
{
	BlockMined,
	AiChallenge,
	Quiz,
	Meditation,
	Tithe,
	GuildQuest,
	AvatarQuest,
	Referral,
	RaidCompletion,
	PvPVictory,
};

/** XP thresholds — must match consciousness.rs xp_threshold() */
static const int64 CONSCIOUSNESS_XP_THRESHOLDS[] = {
	0,          // Physical
	1000,       // Emotional
	5000,       // Mental
	15000,      // Intuitional
	50000,      // Spiritual
	150000,     // Cosmic
	500000,     // Divine
	2000000,    // Unity
	10000000,   // OnTheStar
};

/** XP multipliers per level — matches Rust consciousness.rs multiplier() (V3) */
static const float CONSCIOUSNESS_MULTIPLIERS[] = {
	1.0f,   // Physical
	1.2f,   // Emotional
	1.5f,   // Mental
	2.0f,   // Intuitional
	3.0f,   // Spiritual
	5.0f,   // Cosmic
	8.0f,   // Divine
	12.0f,  // Unity
	15.0f,  // OnTheStar
};

/** Result of an XP award operation */
USTRUCT(BlueprintType)
struct FXpAward
{
	GENERATED_BODY()

	UPROPERTY(BlueprintReadOnly)
	int64 XpAwarded = 0;

	UPROPERTY(BlueprintReadOnly)
	int64 TotalXp = 0;

	UPROPERTY(BlueprintReadOnly)
	EConsciousnessLevel NewLevel = EConsciousnessLevel::Physical;

	UPROPERTY(BlueprintReadOnly)
	bool bLeveledUp = false;

	UPROPERTY(BlueprintReadOnly)
	int64 DailyXpEarned = 0;

	UPROPERTY(BlueprintReadOnly)
	bool bDailyCapped = false;
};

/** 7 Sacred Rays — determines avatar alignment and region */
UENUM(BlueprintType)
enum class ESacredRay : uint8
{
	Blue        UMETA(DisplayName = "Blue Ray — Will/Power"),
	Yellow      UMETA(DisplayName = "Yellow Ray — Wisdom/Intelligence"),
	Pink        UMETA(DisplayName = "Pink Ray — Love/Beauty"),
	White       UMETA(DisplayName = "White Ray — Purity/Ascension"),
	Green       UMETA(DisplayName = "Green Ray — Truth/Healing"),
	RubyGold    UMETA(DisplayName = "Ruby-Gold Ray — Service/Peace"),
	Violet      UMETA(DisplayName = "Violet Ray — Freedom/Transmutation"),
	AllRays     UMETA(DisplayName = "All Rays — Cosmic Unity"),
};
