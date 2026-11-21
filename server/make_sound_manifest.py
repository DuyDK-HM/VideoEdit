"""Turns the old project's sound library into Concat's (docs/AUDIO_LIBRARY.md).

    python make_sound_manifest.py --src OLD/assets/thu_vien_am --out DATA/audio

Reads `thu_vien.json` ({"muc": [{id, loai, nhom, ten, file, thoi_luong, tac_gia,
giay_phep, nguon}]}), copies each CC0 file flat into --out as `<id><ext>` and writes
`manifest.json` with each file's size and SHA-256. Anything not CC0 is left out, as
the app would leave it off the shelf anyway. Rerunning replaces the manifest.
"""
import argparse
import hashlib
import json
import os
import re
import shutil

CATEGORIES = {"Whoosh", "Impact", "Riser", "Ambience", "Music"}


def category(entry: dict) -> str:
    if entry.get("loai") == "nhac":
        return "Music"
    group = (entry.get("nhom") or "").strip()
    return group if group in CATEGORIES else (group or "Other")


def sha256(path: str) -> str:
    digest = hashlib.sha256()
    with open(path, "rb") as fh:
        for chunk in iter(lambda: fh.read(1 << 20), b""):
            digest.update(chunk)
    return digest.hexdigest()


def build(src: str, out: str) -> dict:
    with open(os.path.join(src, "thu_vien.json"), encoding="utf-8") as fh:
        library = json.load(fh)
    os.makedirs(out, exist_ok=True)
    sounds, skipped = [], 0
    for entry in library.get("muc") or []:
        source = os.path.join(src, entry.get("file") or "")
        sound_id = re.sub(r"[^a-z0-9-]+", "-", str(entry.get("id") or "").lower()).strip("-")
        if entry.get("giay_phep") != "CC0-1.0" or not sound_id or not os.path.isfile(source):
            skipped += 1
            continue
        name = sound_id + os.path.splitext(source)[1].lower()
        target = os.path.join(out, name)
        shutil.copyfile(source, target)
        sounds.append({
            "id": sound_id,
            "name": entry.get("ten") or sound_id,
            "category": category(entry),
            "file": name,
            "duration": round(float(entry.get("thoi_luong") or 0), 3),
            "bytes": os.path.getsize(target),
            "license": "CC0-1.0",
            "source": entry.get("nguon") or "",
            "author": entry.get("tac_gia") or "",
            "sha256": sha256(target),
        })
    manifest = {"version": 1, "sounds": sounds}
    with open(os.path.join(out, "manifest.json"), "w", encoding="utf-8") as fh:
        json.dump(manifest, fh, ensure_ascii=False, indent=1)
    print(f"{len(sounds)} sounds written to {out}, {skipped} left out")
    return manifest


if __name__ == "__main__":
    parser = argparse.ArgumentParser()
    parser.add_argument("--src", required=True)
    parser.add_argument("--out", required=True)
    args = parser.parse_args()
    build(args.src, args.out)
