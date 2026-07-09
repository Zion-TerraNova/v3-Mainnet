// Copyright 2026 ZION TerraNova. All Rights Reserved.
using UnrealBuildTool;
using System.Collections.Generic;

public class ZionOasisEditorTarget : TargetRules
{
	public ZionOasisEditorTarget(TargetInfo Target) : base(Target)
	{
		Type = TargetType.Editor;
		DefaultBuildSettings = BuildSettingsVersion.V5;
		IncludeOrderVersion = EngineIncludeOrderVersion.Unreal5_7;
		bOverrideBuildEnvironment = true;
		ExtraModuleNames.Add("ZionOasis");
	}
}
