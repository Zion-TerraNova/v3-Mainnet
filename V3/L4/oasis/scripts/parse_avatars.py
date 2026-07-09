#!/usr/bin/env python3
"""
Parse docs/docs2.9/ZION_OASIS/AVATAR_ROSTER.md into structured JSON.
Output: V3/L4/oasis/data/avatars.json

Usage:
    uv run V3/L4/oasis/scripts/parse_avatars.py
"""

import json
import re
from pathlib import Path
from typing import Any

MARKDOWN_PATH = Path("/Users/yeshuae/Projects/2.9.6/docs/docs2.9/ZION_OASIS/AVATAR_ROSTER.md")
OUTPUT_PATH = Path("/Users/yeshuae/Projects/2.9.6/V3/L4/oasis/data/avatars.json")

# Ray mapping
RAY_MAP = {
    "Blue": "Blue",
    "Yellow": "Yellow",
    "Pink": "Pink",
    "White": "White",
    "Green": "Green",
    "Ruby-Gold": "RubyGold",
    "Ruby Gold": "RubyGold",
    "Violet": "Violet",
    "All Rays": "AllRays",
    "All Ray": "AllRays",
}

RARITY_MAP = {
    "Common": "Common",
    "Uncommon": "Uncommon",
    "Rare": "Rare",
    "Epic": "Epic",
    "Legendary": "Legendary",
    "1/1": "OneOfOne",
    "1/1 - unique": "OneOfOne",
    "1/1 — unique": "OneOfOne",
}


def parse_cl_required(text: str) -> tuple[int, str]:
    """Extract consciousness level number and display name from value text."""
    # e.g. '9 (ON THE STAR)' or '4 (Heart Opening)' or just '5'
    m = re.search(r"(\d+)\s*\(([^)]+)\)", text)
    if m:
        return int(m.group(1)), m.group(2).strip()
    m = re.search(r"(\d+)", text)
    if m:
        return int(m.group(1)), ""
    return 1, ""


def parse_markdown(path: Path) -> list[dict[str, Any]]:
    text = path.read_text(encoding="utf-8")
    avatars: list[dict[str, Any]] = []

    # Split by ### headings that look like avatar entries
    # Example: ### **01. Rama** (Dharma King)
    raw_blocks = re.split(r'\n(?=###\s+\*\*\d+\.\s+)', text)

    for block in raw_blocks:
        block = block.strip()
        if not block.startswith("###"):
            continue

        # Extract ID and name from heading
        heading_match = re.match(r'###\s+\*\*(\d+)\.\s+([^*]+)\*\*(?:\s*\(([^)]+)\))?', block)
        if not heading_match:
            continue

        avatar_id = int(heading_match.group(1))
        name = heading_match.group(2).strip()
        subtitle = (heading_match.group(3) or "").strip()

        # Simple field extractor: **Key:** Value until newline (or next bold)
        def get_field(key: str) -> str:
            # Markdown uses **Key:** value  (colon inside the bold)
            # Also tolerate **Key**: value or **Key** : value
            escaped = re.escape(key)
            pattern = rf'\*\*{escaped}:\*\*\s*(.+?)(?=\n\*\*|\n---|\Z)'
            m = re.search(pattern, block, re.IGNORECASE | re.DOTALL)
            if m:
                return re.sub(r'\s+', ' ', m.group(1)).strip()
            # fallback: **Key**: ...
            pattern = rf'\*\*{escaped}\*\*\s*:\s*(.+?)(?=\n\*\*|\n---|\Z)'
            m = re.search(pattern, block, re.IGNORECASE | re.DOTALL)
            if m:
                return re.sub(r'\s+', ' ', m.group(1)).strip()
            return ""

        ray_raw = get_field("Ray")
        role = get_field("Role")
        location = get_field("Location")
        quest_line = get_field("Quest Line")
        teaching = get_field("Teaching")
        ability = get_field("Ability")
        cl_raw = get_field("CL Required")
        key_item = get_field("Key")
        rarity_raw = get_field("NFT Rarity")

        # Clean up ray
        ray = "Blue"
        for k, v in RAY_MAP.items():
            if k.lower() in ray_raw.lower():
                ray = v
                break
        if "all" in ray_raw.lower() and "ray" in ray_raw.lower():
            ray = "AllRays"

        # Clean up rarity
        rarity = "Rare"
        for k, v in RARITY_MAP.items():
            if k.lower() in rarity_raw.lower():
                rarity = v
                break

        cl_level, cl_name = parse_cl_required(cl_raw)

        # Parse quests list (1. "Title" - Description ...)
        quests: list[dict[str, str]] = []
        quest_section_match = re.search(r'\*\*Quests:\*\*(.+?)(?=\n---|\n###|\Z)', block, re.DOTALL)
        if quest_section_match:
            quest_text = quest_section_match.group(1)
            for line in quest_text.splitlines():
                line = line.strip()
                if not line:
                    continue
                # Match patterns like:
                # 1. "The Exile Test" - Choose between power and dharma
                # 2. The Golden Deer - Illusion vs reality puzzle
                m = re.match(r'\d+\.\s+"?([^"\-]+)"?\s*[-–]\s*(.+)', line)
                if m:
                    quests.append({"title": m.group(1).strip('" '), "description": m.group(2).strip()})
                elif re.match(r'\d+\.\s+', line):
                    # fallback: just take everything after number as title
                    rest = re.sub(r'^\d+\.\s+', '', line)
                    quests.append({"title": rest, "description": ""})

        avatars.append({
            "id": avatar_id,
            "name": name,
            "subtitle": subtitle,
            "ray": ray,
            "role": role,
            "location": location,
            "quest_line": quest_line,
            "teaching": teaching,
            "ability": ability,
            "consciousness_level_required": cl_level,
            "consciousness_level_name": cl_name,
            "key": key_item,
            "rarity": rarity,
            "quests": quests,
        })

    return avatars


def main() -> None:
    avatars = parse_markdown(MARKDOWN_PATH)
    print(f"Parsed {len(avatars)} avatars from {MARKDOWN_PATH}")

    OUTPUT_PATH.parent.mkdir(parents=True, exist_ok=True)
    OUTPUT_PATH.write_text(
        json.dumps({"avatars": avatars}, indent=2, ensure_ascii=False) + "\n",
        encoding="utf-8",
    )
    print(f"Wrote {OUTPUT_PATH}")


if __name__ == "__main__":
    main()
