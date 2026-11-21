#!/usr/bin/env python3
"""Puts the fork's interface strings into every locale file.

    python scripts/vn/fork-strings.py

The strings live in scripts/vn/fork-strings.json as {key: {"en": ..., "vi": ...}}.
Every locale must carry every key (concat's i18n test), so vi.json gets the
Vietnamese, en.json and every other locale the English. Files are written the
way scripts/locales.py writes them: "_" first, the rest sorted, two spaces.
Run it again after an upstream merge or after adding a string.
"""
import json
import pathlib

ROOT = pathlib.Path(__file__).resolve().parents[2]
LOCALES = ROOT / "src" / "crates" / "concat" / "locales"
STRINGS = pathlib.Path(__file__).with_name("fork-strings.json")


def main() -> int:
    strings = json.loads(STRINGS.read_text(encoding="utf-8"))
    for path in sorted(LOCALES.glob("*.json")):
        data = json.loads(path.read_text(encoding="utf-8"))
        for key, texts in strings.items():
            data[key] = texts["vi"] if path.stem == "vi" else texts["en"]
        meta = {"_": data["_"]} if "_" in data else {}
        body = {**meta, **{k: data[k] for k in sorted(k for k in data if k != "_")}}
        path.write_text(json.dumps(body, ensure_ascii=False, indent=2) + "\n", encoding="utf-8")
    print(f"{len(strings)} fork strings written to {len(list(LOCALES.glob('*.json')))} locales")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
