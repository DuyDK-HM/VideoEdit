"""Tests for make_sound_manifest: `python -m pytest server`."""
import hashlib
import json
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).parent))
import make_sound_manifest  # noqa: E402


def test_cc0_sounds_are_copied_flat_with_their_digest(tmp_path):
    src, out = tmp_path / "src", tmp_path / "out"
    (src / "sfx" / "kit").mkdir(parents=True)
    (src / "nhac").mkdir()
    (src / "sfx" / "kit" / "a.m4a").write_bytes(b"whoosh")
    (src / "nhac" / "b.mp3").write_bytes(b"music")
    (src / "nhac" / "c.mp3").write_bytes(b"not free")
    (src / "thu_vien.json").write_text(json.dumps({"muc": [
        {"id": "Kit Whoosh", "loai": "sfx", "nhom": "Whoosh", "ten": "Whoosh", "file": "sfx/kit/a.m4a",
         "thoi_luong": 0.25, "giay_phep": "CC0-1.0", "nguon": "https://x", "tac_gia": "me"},
        {"id": "bed", "loai": "nhac", "nhom": "Chill", "ten": "Bed", "file": "nhac/b.mp3",
         "thoi_luong": 30, "giay_phep": "CC0-1.0", "nguon": "https://y"},
        {"id": "paid", "loai": "nhac", "ten": "Paid", "file": "nhac/c.mp3", "giay_phep": "CC-BY-4.0"},
    ]}), encoding="utf-8")

    manifest = make_sound_manifest.build(str(src), str(out))

    assert [s["id"] for s in manifest["sounds"]] == ["kit-whoosh", "bed"]
    whoosh, bed = manifest["sounds"]
    assert whoosh["file"] == "kit-whoosh.m4a" and (out / "kit-whoosh.m4a").read_bytes() == b"whoosh"
    assert whoosh["sha256"] == hashlib.sha256(b"whoosh").hexdigest() and whoosh["bytes"] == 6
    assert whoosh["category"] == "Whoosh" and bed["category"] == "Music"
    assert json.loads((out / "manifest.json").read_text(encoding="utf-8")) == manifest
