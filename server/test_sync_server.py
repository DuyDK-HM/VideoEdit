"""Tests for sync_server: run with `python -m pytest server` from the repo root."""
import hashlib
import importlib
import sys
from pathlib import Path

import pytest
from fastapi.testclient import TestClient

AUTH = {"Authorization": "Bearer test-token"}


@pytest.fixture()
def client(tmp_path, monkeypatch):
    monkeypatch.setenv("SYNC_DATA", str(tmp_path))
    monkeypatch.setenv("SYNC_TOKEN", "test-token")
    sys.path.insert(0, str(Path(__file__).parent))
    import sync_server
    importlib.reload(sync_server)
    return TestClient(sync_server.app)


def sha(data: bytes) -> str:
    return hashlib.sha256(data).hexdigest()


def test_without_the_token_everything_is_refused(client):
    assert client.get("/projects").status_code == 401
    assert client.get("/projects", headers={"Authorization": "Bearer nope"}).status_code == 401


def test_only_missing_blobs_are_asked_for_and_a_wrong_digest_is_not_kept(client):
    data = b"frame bytes"
    h = sha(data)
    assert client.post("/blobs/missing", json={"hashes": [h, h]}, headers=AUTH).json() == {"missing": [h]}
    bad = client.put(f"/blobs/{'0' * 64}", content=data, headers=AUTH)
    assert bad.status_code == 400
    assert client.put(f"/blobs/{h}", content=data, headers=AUTH).json() == {"stored": True}
    assert client.put(f"/blobs/{h}", content=data, headers=AUTH).json() == {"stored": False}
    assert client.post("/blobs/missing", json={"hashes": [h]}, headers=AUTH).json() == {"missing": []}
    assert client.get(f"/blobs/{h}", headers=AUTH).content == data
    assert client.get("/blobs/not-a-hash", headers=AUTH).status_code == 400


def test_a_project_version_needs_its_media_uploaded_first(client):
    data = b"clip"
    h = sha(data)
    body = {"name": "Tap 1", "document": {"version": 2}, "files": {"media/a.mp4": h}}
    refused = client.put("/projects/tap-1", json=body, headers=AUTH)
    assert refused.status_code == 409 and refused.json()["detail"] == {"missing": [h]}
    client.put(f"/blobs/{h}", content=data, headers=AUTH)
    assert client.put("/projects/tap-1", json=body, headers=AUTH).json() == {"version": 1}
    assert client.put("/projects/tap-1", json=body, headers=AUTH).json() == {"version": 2}
    assert client.get("/projects", headers=AUTH).json() == {
        "projects": [{"id": "tap-1", "name": "Tap 1", "latest": 2}]}
    got = client.get("/projects/tap-1?version=1", headers=AUTH).json()
    assert got["version"] == 1 and got["files"] == {"media/a.mp4": h}
    assert client.get("/projects/tap-1?version=9", headers=AUTH).status_code == 404


@pytest.mark.parametrize("rel", ["../x.mp4", "/etc/passwd", "C:/x.mp4", "a/../../b"])
def test_file_paths_outside_the_project_are_refused(client, rel):
    body = {"name": "x", "document": {}, "files": {rel: "0" * 64}}
    assert client.put("/projects/p", json=body, headers=AUTH).status_code == 400


def test_bad_project_ids_are_refused(client):
    assert client.get("/projects/..%2Fetc", headers=AUTH).status_code in (400, 404)
    assert client.get("/projects/a b", headers=AUTH).status_code == 400


def test_the_sound_library_is_public(client, tmp_path):
    (tmp_path / "audio" / "manifest.json").write_text('{"version": 1, "sounds": []}')
    assert client.get("/audio/manifest.json").json() == {"version": 1, "sounds": []}
