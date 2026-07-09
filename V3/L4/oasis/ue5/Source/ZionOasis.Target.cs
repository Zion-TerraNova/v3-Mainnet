// Copyright 2026 ZION TerraNova. All Rights Reserved.
using UnrealBuildTool;
using System.Collections.Generic;

public class ZionOasisTarget : TargetRules
{
	public ZionOasisTarget(TargetInfo Target) : base(Target)
	{
		Type = TargetType.Game;
		DefaultBuildSettings = BuildSettingsVersion.V5;
		IncludeOrderVersion = EngineIncludeOrderVersion.Unreal5_7;
		ExtraModuleNames.Add("ZionOasis");
	}
}
