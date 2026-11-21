"""Concat fork's own server: project sync with content-addressed media, and the SFX library.

Run:  SYNC_TOKEN=... SYNC_DATA=/srv/concat uvicorn sync_server:app --host 0.0.0.0 --port 8700

Layout under SYNC_DATA:
  blobs/<aa>/<sha256>        every media file once, named by its SHA-256
  projects/<id>/<n>.json     version n of a project: {name, document, files: {relpath: sha256}}
  audio/manifest.json + files  the sound library, in docs/AUDIO_LIBRARY.md's format (public, no token)

The server only stores bytes and checks digests; the CPU it has is weak.
"""
import hashlib
import json
import os
import re
import secrets
import tempfile
from pathlib import Path

from fastapi import Depends, FastAPI, Header, HTTPException, Request
from fastapi.responses import FileResponse
from fastapi.staticfiles import StaticFiles
from pydantic import BaseModel

DATA = Path(os.environ.get("SYNC_DATA", "data")).resolve()
TOKEN = os.environ.get("SYNC_TOKEN", "")
SHA256 = re.compile(r"^[0-9a-f]{64}$")
PROJECT_ID = re.compile(r"^[A-Za-z0-9_-]{1,64}$")
CHUNK = 1 << 20

for sub in ("blobs", "projects", "audio"):
    (DATA / sub).mkdir(parents=True, exist_ok=True)

app = FastAPI(title="concat-sync")
app.mount("/audio", StaticFiles(directory=DATA / "audio"), name="audio")


def authorised(authorization: str = Header(default="")) -> None:
    """Bearer token from SYNC_TOKEN; a server started without one refuses everything."""
    given = authorization.removeprefix("Bearer ").strip()
    if not TOKEN or not secrets.compare_digest(given, TOKEN):
        raise HTTPException(401, "wrong or missing token")


def blob_path(digest: str) -> Path:
    if not SHA256.match(digest):
        raise HTTPException(400, "not a lowercase sha256")
    return DATA / "blobs" / digest[:2] / digest


def project_dir(project_id: str) -> Path:
    if not PROJECT_ID.match(project_id):
        raise HTTPException(400, "project id must be 1-64 of A-Z a-z 0-9 _ -")
    return DATA / "projects" / project_id


def versions(folder: Path) -> list[int]:
    if not folder.is_dir():
        return []
    return sorted(int(p.stem) for p in folder.glob("*.json") if p.stem.isdigit())


class Hashes(BaseModel):
    hashes: list[str]


class ProjectVersion(BaseModel):
    name: str
    document: dict
    files: dict[str, str]


@app.post("/blobs/missing", dependencies=[Depends(authorised)])
def missing(body: Hashes) -> dict:
    """The digests the server does not hold yet: the only ones a client uploads."""
    return {"missing": [h for h in dict.fromkeys(body.hashes) if not blob_path(h).is_file()]}


@app.put("/blobs/{digest}", dependencies=[Depends(authorised)])
async def put_blob(digest: str, request: Request) -> dict:
    """Streams the body to a temporary file and keeps it only if its SHA-256 is `digest`."""
    target = blob_path(digest)
    if target.is_file():
        return {"stored": False}
    target.parent.mkdir(parents=True, exist_ok=True)
    hasher = hashlib.sha256()
    fd, partial = tempfile.mkstemp(dir=target.parent, suffix=".part")
    try:
        with os.fdopen(fd, "wb") as out:
            async for chunk in request.stream():
                hasher.update(chunk)
                out.write(chunk)
        if hasher.hexdigest() != digest:
            raise HTTPException(400, f"body hashes to {hasher.hexdigest()}, not {digest}")
        os.replace(partial, target)
    finally:
        if os.path.exists(partial):
            os.remove(partial)
    return {"stored": True}


@app.get("/blobs/{digest}", dependencies=[Depends(authorised)])
def get_blob(digest: str) -> FileResponse:
    path = blob_path(digest)
    if not path.is_file():
        raise HTTPException(404, "no such blob")
    return FileResponse(path, media_type="application/octet-stream")


@app.get("/projects", dependencies=[Depends(authorised)])
def list_projects() -> dict:
    out = []
    for folder in sorted((DATA / "projects").iterdir()):
        vs = versions(folder)
        if vs:
            path = folder / f"{vs[-1]}.json"
            latest = json.loads(path.read_text(encoding="utf-8"))
            out.append({"id": folder.name, "name": latest["name"], "latest": vs[-1],
                        "updated": path.stat().st_mtime})
    # The one pushed last first: what a second machine wants to pull.
    out.sort(key=lambda p: p["updated"], reverse=True)
    return {"projects": out}


@app.put("/projects/{project_id}", dependencies=[Depends(authorised)])
def put_project(project_id: str, body: ProjectVersion) -> dict:
    """Saves a new version. Every file it names must already be uploaded, so a version on
    the server is always one another machine can open whole."""
    for rel in body.files:
        if rel.startswith("/") or "\\" in rel or ".." in Path(rel).parts or ":" in rel:
            raise HTTPException(400, f"{rel}: file paths must be relative to the project")
    absent = sorted({h for h in body.files.values() if not blob_path(h).is_file()})
    if absent:
        raise HTTPException(409, {"missing": absent})
    folder = project_dir(project_id)
    folder.mkdir(parents=True, exist_ok=True)
    number = (versions(folder) or [0])[-1] + 1
    (folder / f"{number}.json").write_text(body.model_dump_json(), encoding="utf-8")
    return {"version": number}


@app.get("/projects/{project_id}", dependencies=[Depends(authorised)])
def get_project(project_id: str, version: int | None = None) -> dict:
    folder = project_dir(project_id)
    vs = versions(folder)
    if not vs:
        raise HTTPException(404, "no such project")
    number = vs[-1] if version is None else version
    if number not in vs:
        raise HTTPException(404, f"no version {number}; there are {vs}")
    return {"version": number, **json.loads((folder / f"{number}.json").read_text(encoding="utf-8"))}
