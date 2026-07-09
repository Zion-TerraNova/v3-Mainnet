// Copyright 2026 ZION TerraNova. All Rights Reserved.
#pragma once

#include "CoreMinimal.h"
#include "Engine/DataTable.h"
#include "ZionOasis/Consciousness/ConsciousnessTypes.h"
#include "AvatarTypes.generated.h"

/**
 * 51 sacred avatars roster — matches AVATAR_ROSTER.md
 * Used as DataTable primary key enum.
 */
UENUM(BlueprintType)
enum class EAvatarID : uint8
{
	// === Hindu Deities (00-16) ===
	KrishnaMaitreya     = 0   UMETA(DisplayName = "00. Krishna-Maitreya"),
	Rama                = 1   UMETA(DisplayName = "01. Rama"),
	Sita                = 2   UMETA(DisplayName = "02. Sita"),
	Hanuman             = 3   UMETA(DisplayName = "03. Hanuman"),
	Maitreya            = 4   UMETA(DisplayName = "04. Maitreya"),
	Saraswati           = 5   UMETA(DisplayName = "05. Saraswati"),
	IsisEnamataru       = 6   UMETA(DisplayName = "06. Isis Enamataru"),
	// === Ascended Masters (07-16) ===
	ElMorya             = 7   UMETA(DisplayName = "07. El Morya"),
	Lanto               = 8   UMETA(DisplayName = "08. Lanto"),
	PaulTheVenetian     = 9   UMETA(DisplayName = "09. Paul the Venetian"),
	Hilarion            = 10  UMETA(DisplayName = "10. Hilarion"),
	LadyNada            = 11  UMETA(DisplayName = "11. Lady Nada"),
	SaintGermain        = 12  UMETA(DisplayName = "12. Saint Germain"),
	SerapisBey          = 13  UMETA(DisplayName = "13. Serapis Bey"),
	SanatKumara         = 14  UMETA(DisplayName = "14. Sanat Kumara"),
	MahavatarBabaji     = 15  UMETA(DisplayName = "15. Mahavatar Babaji"),
	LadyGaia            = 16  UMETA(DisplayName = "16. Lady Gaia Vywamus"),
	// === Buddhist Masters (17-20) ===
	Avalokiteshvara     = 17  UMETA(DisplayName = "17. Avalokiteshvara"),
	Vajrasattva         = 18  UMETA(DisplayName = "18. Vajrasattva"),
	TaraLiberator       = 19  UMETA(DisplayName = "19. Tara the Liberator"),
	DalaiLama           = 20  UMETA(DisplayName = "20. Dalai Lama XIV"),
	// === Christian Saints (21-24) ===
	YeshuaSananda       = 21  UMETA(DisplayName = "21. Yeshua Sananda"),
	PannaMaria          = 22  UMETA(DisplayName = "22. Panna Maria"),
	MeriamRose          = 23  UMETA(DisplayName = "23. Meriam Rose"),
	BronuChrist         = 24  UMETA(DisplayName = "24. Bronu Christ"),
	// === Historical Legends (25-30) ===
	KingArthur          = 25  UMETA(DisplayName = "25. King Arthur"),
	MahatmaGandhi       = 26  UMETA(DisplayName = "26. Mahatma Gandhi"),
	AlbertEinstein      = 27  UMETA(DisplayName = "27. Albert Einstein"),
	KarelIV             = 28  UMETA(DisplayName = "28. Karel IV"),
	SriChaitanya        = 29  UMETA(DisplayName = "29. Sri Chaitanya Mahaprabhu"),
	HiranyagarbhaAvatar = 30  UMETA(DisplayName = "30. Hiranyagarbha"),
	// === Matrix Heroes (31-34) ===
	NeoTheOne           = 31  UMETA(DisplayName = "31. Neo — The One"),
	TrinityBeliever     = 32  UMETA(DisplayName = "32. Trinity — The Believer"),
	MorpheusGuide       = 33  UMETA(DisplayName = "33. Morpheus — The Guide"),
	ZionLastCity        = 34  UMETA(DisplayName = "34. ZION — The Last City"),
	// === ZION Originals (35-40) ===
	IssobelaGuardian    = 35  UMETA(DisplayName = "35. Issobela Guardian"),
	Shanti              = 36  UMETA(DisplayName = "36. Shanti"),
	ArjunaBrother       = 37  UMETA(DisplayName = "37. Arjuna Brother"),
	MilanBhima          = 38  UMETA(DisplayName = "38. Milan Bhima"),
	ArtemVudce          = 39  UMETA(DisplayName = "39. Artem Vudce"),
	MamaYashoda         = 40  UMETA(DisplayName = "40. Mama Yashoda"),
	// === Sanskrit Ascended (41-50) ===
	VishnuDev           = 41  UMETA(DisplayName = "41. Vishwakarma Dev"),
	Radha               = 42  UMETA(DisplayName = "42. Radha"),
	VyasaSage           = 43  UMETA(DisplayName = "43. Vyasa Sage"),
	SriDattatreya       = 44  UMETA(DisplayName = "44. Sri Dattatreya"),
	SriAnaghaLakshmi    = 45  UMETA(DisplayName = "45. Sri Anagha Lakshmi"),
	MalyPrinc           = 46  UMETA(DisplayName = "46. Maly Princ"),
	Elizabet            = 47  UMETA(DisplayName = "47. Elizabet"),
	Vasudeva            = 48  UMETA(DisplayName = "48. Vasudeva"),
	SriKalkiAvatar      = 49  UMETA(DisplayName = "49. Sri Kalki Avatar"),
	Subhadra            = 50  UMETA(DisplayName = "50. Subhadra"),
};

/** Avatar NFT rarity */
UENUM(BlueprintType)
enum class EAvatarRarity : uint8
{
	Common      UMETA(DisplayName = "Common"),
	Uncommon    UMETA(DisplayName = "Uncommon"),
	Rare        UMETA(DisplayName = "Rare"),
	Epic        UMETA(DisplayName = "Epic"),
	Legendary   UMETA(DisplayName = "Legendary"),
	OneOfOne    UMETA(DisplayName = "1/1 — Unique"),
};

/** One row of the Avatar Data Table */
USTRUCT(BlueprintType)
struct FAvatarRow : public FTableRowBase
{
	GENERATED_BODY()

	UPROPERTY(EditAnywhere, BlueprintReadOnly, Category = "Avatar")
	EAvatarID AvatarID = EAvatarID::Rama;

	UPROPERTY(EditAnywhere, BlueprintReadOnly, Category = "Avatar")
	FText DisplayName;

	UPROPERTY(EditAnywhere, BlueprintReadOnly, Category = "Avatar")
	FText Title;

	UPROPERTY(EditAnywhere, BlueprintReadOnly, Category = "Avatar")
	FText Teaching;

	UPROPERTY(EditAnywhere, BlueprintReadOnly, Category = "Avatar")
	FText SpecialAbilityName;

	UPROPERTY(EditAnywhere, BlueprintReadOnly, Category = "Avatar")
	FText SpecialAbilityDesc;

	/** Minimum consciousness level required to access this avatar */
	UPROPERTY(EditAnywhere, BlueprintReadOnly, Category = "Avatar")
	EConsciousnessLevel MinConsciousnessLevel = EConsciousnessLevel::Physical;

	UPROPERTY(EditAnywhere, BlueprintReadOnly, Category = "Avatar")
	ESacredRay Ray = ESacredRay::Blue;

	UPROPERTY(EditAnywhere, BlueprintReadOnly, Category = "Avatar")
	EAvatarRarity Rarity = EAvatarRarity::Rare;

	/** World map region where avatar resides */
	UPROPERTY(EditAnywhere, BlueprintReadOnly, Category = "Avatar")
	FText RegionName;

	/** Quest lines count */
	UPROPERTY(EditAnywhere, BlueprintReadOnly, Category = "Avatar")
	int32 QuestCount = 5;

	/** XP reward for completing all quests */
	UPROPERTY(EditAnywhere, BlueprintReadOnly, Category = "Avatar")
	int64 TotalQuestXpReward = 0;

	/** Soft reference to MetaHuman / 3D character Blueprint */
	UPROPERTY(EditAnywhere, BlueprintReadOnly, Category = "Avatar|Assets")
	TSoftClassPtr<AActor> CharacterClass;

	/** Portrait texture for UI */
	UPROPERTY(EditAnywhere, BlueprintReadOnly, Category = "Avatar|Assets")
	TSoftObjectPtr<UTexture2D> PortraitTexture;

	/** NFT metadata URI (IPFS / zionterranova.com/nft/) */
	UPROPERTY(EditAnywhere, BlueprintReadOnly, Category = "Avatar|Blockchain")
	FString NftMetadataUri;
};

/** One row of the Avatar Quest Data Table */
USTRUCT(BlueprintType)
struct FAvatarQuestRow : public FTableRowBase
{
	GENERATED_BODY()

	/** Which avatar this quest belongs to */
	UPROPERTY(EditAnywhere, BlueprintReadOnly, Category = "AvatarQuest")
	EAvatarID AvatarID = EAvatarID::Rama;

	/** Quest index within avatar's quest list (0-based) */
	UPROPERTY(EditAnywhere, BlueprintReadOnly, Category = "AvatarQuest")
	int32 QuestIndex = 0;

	/** Quest title shown in UI */
	UPROPERTY(EditAnywhere, BlueprintReadOnly, Category = "AvatarQuest")
	FText QuestTitle;

	/** Quest description / objective text */
	UPROPERTY(EditAnywhere, BlueprintReadOnly, Category = "AvatarQuest")
	FText QuestDescription;

	/** XP reward on completion */
	UPROPERTY(EditAnywhere, BlueprintReadOnly, Category = "AvatarQuest")
	int32 XpReward = 500;
};

/** Player-equipped avatar slot */
USTRUCT(BlueprintType)
struct FEquippedAvatar
{
	GENERATED_BODY()

	UPROPERTY(BlueprintReadOnly)
	EAvatarID AvatarID = EAvatarID::NeoTheOne;

	/** Blockchain NFT token ID if owned on-chain */
	UPROPERTY(BlueprintReadOnly)
	FString NftTokenId;

	/** Customization: skin tone, outfit color */
	UPROPERTY(BlueprintReadOnly)
	FLinearColor PrimaryColor = FLinearColor::White;
};
