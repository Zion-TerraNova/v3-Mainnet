// Copyright 2026 ZION TerraNova. All Rights Reserved.
#include "ZionCharacter.h"
#include "ZionPlayerController.h"
#include "ZionOasis/Consciousness/ConsciousnessComponent.h"
#include "ZionOasis/Guild/GuildComponent.h"
#include "Camera/CameraComponent.h"
#include "GameFramework/SpringArmComponent.h"
#include "GameFramework/CharacterMovementComponent.h"
#include "EnhancedInputComponent.h"
#include "EnhancedInputSubsystems.h"
#include "Net/UnrealNetwork.h"
#include "Logging/LogMacros.h"

DEFINE_LOG_CATEGORY_STATIC(LogZionCharacter, Log, All);

AZionCharacter::AZionCharacter()
{
	CameraBoom = CreateDefaultSubobject<USpringArmComponent>(TEXT("CameraBoom"));
	CameraBoom->SetupAttachment(RootComponent);
	CameraBoom->TargetArmLength         = 400.f;
	CameraBoom->bUsePawnControlRotation = true;

	FollowCamera = CreateDefaultSubobject<UCameraComponent>(TEXT("FollowCamera"));
	FollowCamera->SetupAttachment(CameraBoom, USpringArmComponent::SocketName);
	FollowCamera->bUsePawnControlRotation = false;

	ConsciousnessComp = CreateDefaultSubobject<UConsciousnessComponent>(TEXT("ConsciousnessComp"));
	GuildComp         = CreateDefaultSubobject<UGuildComponent>(TEXT("GuildComp"));

	bUseControllerRotationPitch = false;
	bUseControllerRotationYaw   = false;
	bUseControllerRotationRoll  = false;
	GetCharacterMovement()->bOrientRotationToMovement = true;
	GetCharacterMovement()->RotationRate              = FRotator(0.f, 500.f, 0.f);
	GetCharacterMovement()->MaxWalkSpeed              = 400.f;
	GetCharacterMovement()->JumpZVelocity             = 700.f;
	GetCharacterMovement()->AirControl                = 0.35f;

	SetReplicates(true);
	SetReplicateMovement(true);
}

void AZionCharacter::BeginPlay()
{
	Super::BeginPlay();

	if (APlayerController* PC = Cast<APlayerController>(GetController()))
	{
		if (auto* Subsystem = ULocalPlayer::GetSubsystem<UEnhancedInputLocalPlayerSubsystem>(
			PC->GetLocalPlayer()))
		{
			Subsystem->AddMappingContext(DefaultMappingContext, 0);
		}
	}
}

void AZionCharacter::SetupPlayerInputComponent(UInputComponent* Comp)
{
	Super::SetupPlayerInputComponent(Comp);
	if (UEnhancedInputComponent* EI = Cast<UEnhancedInputComponent>(Comp))
	{
		EI->BindAction(MoveAction,    ETriggerEvent::Triggered, this, &AZionCharacter::Move);
		EI->BindAction(LookAction,    ETriggerEvent::Triggered, this, &AZionCharacter::Look);
		EI->BindAction(JumpAction,    ETriggerEvent::Started,   this, &ACharacter::Jump);
		EI->BindAction(JumpAction,    ETriggerEvent::Completed, this, &ACharacter::StopJumping);
		EI->BindAction(MeditateAction,ETriggerEvent::Started,   this, &AZionCharacter::PerformMeditation);
		EI->BindAction(InteractAction,ETriggerEvent::Started,   this, &AZionCharacter::Interact);
	}
}

void AZionCharacter::GetLifetimeReplicatedProps(TArray<FLifetimeProperty>& OutLifetimeProps) const
{
	Super::GetLifetimeReplicatedProps(OutLifetimeProps);
	DOREPLIFETIME(AZionCharacter, WalletAddress);
	DOREPLIFETIME(AZionCharacter, PlayerDisplayName);
	DOREPLIFETIME(AZionCharacter, EquippedAvatar);
	DOREPLIFETIME(AZionCharacter, TotalAvatarQuestsCompleted);
}

void AZionCharacter::Move(const FInputActionValue& Value)
{
	if (!Controller) return;
	const FVector2D Axis = Value.Get<FVector2D>();
	const FRotator Rot   = Controller->GetControlRotation();
	const FRotator YRot  = FRotator(0, Rot.Yaw, 0);
	AddMovementInput(FRotationMatrix(YRot).GetUnitAxis(EAxis::X), Axis.Y);
	AddMovementInput(FRotationMatrix(YRot).GetUnitAxis(EAxis::Y), Axis.X);
}

void AZionCharacter::Look(const FInputActionValue& Value)
{
	if (!Controller) return;
	const FVector2D Axis = Value.Get<FVector2D>();
	AddControllerYawInput(Axis.X);
	AddControllerPitchInput(Axis.Y);
}

void AZionCharacter::Interact()
{
	if (AZionPlayerController* PC = Cast<AZionPlayerController>(GetController()))
		PC->ServerInteract();
}

void AZionCharacter::PerformMeditation()
{
	if (ConsciousnessComp)
		ConsciousnessComp->AwardXp(EXpSource::Meditation, 100);
}

void AZionCharacter::OnBlockMined()
{
	if (ConsciousnessComp)
		ConsciousnessComp->AwardXp(EXpSource::BlockMined, 500);
}

void AZionCharacter::CompleteAvatarQuest(EAvatarID AvatarID, int32 QuestIndex)
{
	if (ConsciousnessComp)
	{
		const int64 QuestXp = 2000 * (QuestIndex + 1);
		ConsciousnessComp->AwardXp(EXpSource::AvatarQuest, QuestXp);
		TotalAvatarQuestsCompleted++;
	}
}

EConsciousnessLevel AZionCharacter::GetConsciousnessLevel() const
{
	return ConsciousnessComp ? ConsciousnessComp->GetLevel() : EConsciousnessLevel::Physical;
}

bool AZionCharacter::CanJoinGuild() const
{
	return ConsciousnessComp && ConsciousnessComp->GetTotalXp() >= GUILD_MIN_XP_JOIN;
}

bool AZionCharacter::CanCreateGuild() const
{
	return ConsciousnessComp && ConsciousnessComp->GetTotalXp() >= GUILD_MIN_XP_CREATE;
}
