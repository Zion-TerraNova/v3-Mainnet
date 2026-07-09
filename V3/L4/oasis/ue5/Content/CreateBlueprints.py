#!/usr/bin/env python3
"""
ZION OASIS -- UE5 Blueprint Creation Script
Run inside UE5 Editor: File -> Execute Python Script

This script creates the required Blueprints and Input assets
for ZION OASIS to compile and run.
"""

import unreal

EDITOR = unreal.EditorAssetLibrary
ASSET_TOOLS = unreal.AssetToolsHelpers.get_asset_tools()

# ---------------------------------------------------------------------------
# Configuration
# ---------------------------------------------------------------------------
CONTENT = "/Game"
BLUEPRINTS = f"{CONTENT}/Blueprints"
INPUT_PATH = f"{CONTENT}/Input"
MAPS = f"{CONTENT}/Maps"

# Parent classes (UE5 Python API: load_class(outer, name))
ZION_GM = unreal.load_class(None, "/Script/ZionOasis.ZionOasisGameMode")
ZION_CHAR = unreal.load_class(None, "/Script/ZionOasis.ZionCharacter")
ZION_PC = unreal.load_class(None, "/Script/ZionOasis.ZionPlayerController")
ZION_HUD = unreal.load_class(None, "/Script/ZionOasis.ZionHUD")
ZION_GEM = unreal.load_class(None, "/Script/ZionOasis.GoldenEggManager")
ZION_TM = unreal.load_class(None, "/Script/ZionOasis.TerritoryManager")

# ---------------------------------------------------------------------------
# Helpers
# ---------------------------------------------------------------------------
def create_blueprint(name: str, parent: unreal.Class, path: str):
    full_path = f"{path}/{name}"
    if EDITOR.does_asset_exist(full_path):
        print(f"  [SKIP] {full_path}")
        return EDITOR.load_asset(full_path)

    factory = unreal.BlueprintFactory()
    try:
        factory.ParentClass = parent
    except AttributeError:
        factory.set_editor_property("ParentClass", parent)

    asset = ASSET_TOOLS.create_asset(
        asset_name=name,
        package_path=path.replace("/Game", "/Game"),
        asset_class=unreal.Blueprint,
        factory=factory
    )
    print(f"  [CREATE] {full_path}")
    return asset

def create_input_action(name: str, value_type: int):
    full_path = f"{INPUT_PATH}/{name}"
    if EDITOR.does_asset_exist(full_path):
        print(f"  [SKIP] {full_path}")
        return EDITOR.load_asset(full_path)

    # Try multiple ways to get the factory (UE5 Python exposure varies)
    factory = None
    for factory_name in [
        "InputActionFactory",
        "EnhancedInputActionFactory",
    ]:
        try:
            factory_cls = getattr(unreal, factory_name)
            factory = factory_cls()
            break
        except Exception:
            pass

    if factory is None:
        # Fallback: try to instantiate via load_class
        try:
            factory_cls = unreal.load_class(None, "/Script/EnhancedInput.InputActionFactory")
            factory = unreal.new_object(factory_cls)
        except Exception:
            pass

    if factory is None:
        print(f"  [WARN] Cannot create {full_path} -- InputActionFactory not exposed to Python. Create manually.")
        return None

    asset = ASSET_TOOLS.create_asset(
        asset_name=name,
        package_path=INPUT_PATH.replace("/Game", "/Game"),
        asset_class=unreal.InputAction,
        factory=factory
    )
    if asset:
        try:
            asset.value_type = value_type
        except AttributeError:
            asset.set_editor_property("value_type", value_type)
        try:
            EDITOR.save_loaded_asset(asset)
        except AttributeError:
            EDITOR.save_asset(full_path)
    print(f"  [CREATE] {full_path}")
    return asset

def create_mapping_context(name: str):
    full_path = f"{INPUT_PATH}/{name}"
    if EDITOR.does_asset_exist(full_path):
        print(f"  [SKIP] {full_path}")
        return EDITOR.load_asset(full_path)

    factory = None
    for factory_name in [
        "InputMappingContextFactory",
        "EnhancedInputMappingContextFactory",
    ]:
        try:
            factory_cls = getattr(unreal, factory_name)
            factory = factory_cls()
            break
        except Exception:
            pass

    if factory is None:
        try:
            factory_cls = unreal.load_class(None, "/Script/EnhancedInput.InputMappingContextFactory")
            factory = unreal.new_object(factory_cls)
        except Exception:
            pass

    if factory is None:
        print(f"  [WARN] Cannot create {full_path} -- InputMappingContextFactory not exposed to Python. Create manually.")
        return None

    asset = ASSET_TOOLS.create_asset(
        asset_name=name,
        package_path=INPUT_PATH.replace("/Game", "/Game"),
        asset_class=unreal.InputMappingContext,
        factory=factory
    )
    print(f"  [CREATE] {full_path}")
    return asset

def create_level(name: str):
    full_path = f"{MAPS}/{name}"
    if EDITOR.does_asset_exist(full_path):
        print(f"  [SKIP] {full_path}")
        return

    try:
        # Try EditorLevelLibrary first
        if hasattr(unreal, "EditorLevelLibrary"):
            unreal.EditorLevelLibrary.new_level(f"{MAPS}/{name}")
        else:
            # Fallback: use WorldFactory to create a .umap asset
            factory = unreal.WorldFactory()
            ASSET_TOOLS.create_asset(
                asset_name=name,
                package_path=MAPS.replace("/Game", "/Game"),
                asset_class=unreal.World,
                factory=factory
            )
        print(f"  [CREATE] {full_path}")
    except Exception as e:
        print(f"  [WARN] Could not create level {full_path}: {e}")

# ---------------------------------------------------------------------------
# Create Folders
# ---------------------------------------------------------------------------
print("Creating folders...")
for folder in [f"{BLUEPRINTS}/Game", f"{BLUEPRINTS}/Player", f"{BLUEPRINTS}/UI", INPUT_PATH, MAPS]:
    EDITOR.make_directory(folder)
    print(f"  {folder}")

# ---------------------------------------------------------------------------
# Create Input Actions
# ---------------------------------------------------------------------------
print("\nCreating Input Actions...")
# EInputActionValueType enum values: Boolean=0, Axis1D=1, Axis2D=2, Axis3D=3
IA_MOVE = create_input_action("IA_Move", 2)
IA_LOOK = create_input_action("IA_Look", 2)
IA_JUMP = create_input_action("IA_Jump", 0)
IA_MEDITATE = create_input_action("IA_Meditate", 0)
IA_INTERACT = create_input_action("IA_Interact", 0)
IA_SPRINT = create_input_action("IA_Sprint", 0)
IA_TOGGLE_MAP = create_input_action("IA_ToggleMap", 0)
IA_TOGGLE_QUEST = create_input_action("IA_ToggleQuestLog", 0)

# ---------------------------------------------------------------------------
# Create Mapping Context
# ---------------------------------------------------------------------------
print("\nCreating Input Mapping Context...")
IMC = create_mapping_context("IMC_ZionDefault")

# ---------------------------------------------------------------------------
# Create Blueprints
# ---------------------------------------------------------------------------
print("\nCreating Blueprints...")
BP_GM = create_blueprint("BP_ZionOasisGameMode", ZION_GM, f"{BLUEPRINTS}/Game")
BP_CHAR = create_blueprint("BP_ZionCharacter", ZION_CHAR, f"{BLUEPRINTS}/Player")
BP_PC = create_blueprint("BP_ZionPlayerController", ZION_PC, f"{BLUEPRINTS}/Player")
BP_HUD = create_blueprint("BP_ZionHUD", ZION_HUD, f"{BLUEPRINTS}/UI")
BP_GEM = create_blueprint("BP_GoldenEggManager", ZION_GEM, f"{BLUEPRINTS}/Game")
BP_TM = create_blueprint("BP_TerritoryManager", ZION_TM, f"{BLUEPRINTS}/Game")

# ---------------------------------------------------------------------------
# Create Levels
# ---------------------------------------------------------------------------
print("\nCreating Levels...")
create_level("LV_MainMenu")
create_level("LV_World")

# ---------------------------------------------------------------------------
# Save All
# ---------------------------------------------------------------------------
print("\nSaving assets...")
try:
    EDITOR.save_directory("/Game", only_if_is_dirty=False, recursive=True)
except Exception as e:
    print(f"  [WARN] save_directory failed: {e}")

print("\n" + "=" * 60)
print("Done! Restart the editor and set:")
print("  Project Settings -> Maps & Modes -> Default GameMode = BP_ZionOasisGameMode")
print("  Project Settings -> Maps & Modes -> Editor Startup Map = LV_World")
print("  Project Settings -> Maps & Modes -> Game Default Map = LV_MainMenu")
print("=" * 60)
