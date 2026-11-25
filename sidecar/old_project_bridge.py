"""Concat fork's bridge to the old project's AI (ToolAutoMakeCartoonVideo2DFromComics).

Run with the old project's Python (OLD/AIVoice/.venv/Scripts/python.exe):

    python old_project_bridge.py subtitle  --old-root OLD --video V --out-dir D [--source-lang auto] [--clean-audio]
    python old_project_bridge.py translate --old-root OLD --srt IN --out OUT [--source-lang Chinese] [--target-lang Vietnamese]

Every line on stdout is one JSON object:
    {"type": "progress", "percent": 0-100 or null, "message": "..."}
    {"type": "warning", "message": "..."}
    {"type": "done", "srt": "<path of the .srt written>"}
    {"type": "error", "message": "..."}
The caller cancels by killing the process tree.

`subtitle` runs the old adapter (PhoWhisper, optional Demucs) exactly as the old
editor's ai_bridge did; `translate` calls the old `dich.dich` (Ollama/Gemini with
its Vietnamese quality checks and per-folder glossary) with a stand-in job context.
"""
import argparse
import json
import os
import subprocess
import sys
import threading


def emit(kind: str, **data) -> None:
    print(json.dumps({"type": kind, **data}, ensure_ascii=False), flush=True)


class Job:
    """What the old code's queue gave a job: progress, cancel flag, result."""

    def __init__(self):
        self.huy_event = threading.Event()
        self.tien_do = 0.0
        self.thong_diep = ""
        self.ket_qua = None


class Context:
    """Stands in for orchestrator.editor.hang_doi.NguCanh: progress goes to stdout."""

    def __init__(self):
        self.viec = Job()

    @property
    def da_huy(self) -> bool:
        return False

    def kiem_tra_huy(self) -> None:
        pass

    def bao(self, tien_do=None, thong_diep=None) -> None:
        emit("progress", percent=None if tien_do is None else round(float(tien_do), 1),
             message=thong_diep or "")


def subtitle(args) -> int:
    from orchestrator.config import load_global_config
    from orchestrator.pipeline import AIVOICE_DIR, VideoPipeline

    os.makedirs(args.out_dir, exist_ok=True)
    g = load_global_config()
    job = {"video_path": os.path.abspath(args.video), "output_dir": args.out_dir, "srt_dir": args.out_dir}
    from orchestrator.llm import resolve_llm

    autosub, tr = g.get("autosub") or {}, g.get("translate") or {}
    engine = tr.get("engine") or autosub.get("llm_engine") or "gemini_api"
    key, base_url, model = resolve_llm(engine, {}, g, autosub.get("llm_model") or "")
    # The keys the old editor's ai_bridge.dung_lenh gave the adapter for a subtitle job.
    options = {"sub_source": "whisper", "no_translate": True, "translate_only": True,
               "clean_audio": args.clean_audio, "source_lang": args.source_lang,
               "target_lang": tr.get("target_lang") or "Vietnamese", "llm_engine": engine,
               "llm_model": model, "llm_base_url": base_url, "llm_api_key": key}
    cmd = VideoPipeline.build_translate_cmd(None, job, options, g)
    process = subprocess.Popen(cmd, cwd=AIVOICE_DIR, stdout=subprocess.PIPE, stderr=subprocess.STDOUT,
                               text=True, encoding="utf-8", errors="replace")
    done = None
    for line in process.stdout:
        line = line.strip()
        if not line.startswith("{"):
            continue
        try:
            event = json.loads(line)
        except ValueError:
            continue
        kind = event.get("event")
        if kind == "autosub_progress":
            emit("progress", percent=event.get("percent"), message=event.get("message") or "")
        elif kind == "autosub_warn":
            emit("warning", message=event.get("message") or "")
        elif kind == "autosub_error":
            emit("error", message=event.get("error") or "the subtitle adapter failed")
            process.wait()
            return 1
        elif kind == "autosub_done":
            done = event
    code = process.wait()
    path = (done or {}).get("srt_source") or (done or {}).get("output")
    if not path or not os.path.isfile(path):
        emit("error", message=f"the subtitle adapter ended (code {code}) without a .srt")
        return 1
    emit("done", srt=os.path.abspath(path))
    return 0


def unload_ollama(model: str, base_url: str) -> None:
    """Drops the model from VRAM now rather than after Ollama's 5-minute keep-alive:
    held there, its ~3 GB leaves too little of a 6 GB card for a 4K render."""
    import urllib.request
    url = (base_url or "http://127.0.0.1:11434").rstrip("/")
    if url.endswith("/v1"):
        url = url[:-3]
    body = json.dumps({"model": model, "keep_alive": 0}).encode()
    request = urllib.request.Request(url + "/api/generate", data=body,
                                     headers={"Content-Type": "application/json"})
    try:
        urllib.request.urlopen(request, timeout=10).read()
    except Exception as error:  # unloading is best effort; the translation already succeeded
        emit("warning", message=f"could not unload {model} from the GPU: {error}")


def translate(args) -> int:
    from orchestrator.config import load_global_config
    from orchestrator.editor import dich, srt

    sentences = srt.phan_tich(srt.doc_file(args.srt))
    if not sentences:
        emit("error", message=f"{args.srt} holds no subtitles")
        return 1
    options = {"source_lang": args.source_lang, "target_lang": args.target_lang,
               "cau": [dict(c, text_goc=c["text"]) for c in sentences], "ids": []}

    tr = load_global_config().get("translate") or {}
    model = tr.get("mt_model") or "hy-mt2:1.8b"
    started = []

    def start_ollama():
        from orchestrator import ollama_manager
        result = ollama_manager.ensure_ready(
            model, tr.get("ollama_base_url") or "", autostart=bool(tr.get("autostart_ollama", True)),
            progress_cb=lambda msg, pct=-1, *_: emit("progress", percent=pct if pct and pct >= 0 else None,
                                                    message=msg))
        if not result.get("ok"):
            raise RuntimeError(result.get("reason") or "Ollama is not running")
        started.append(True)

    try:
        if dich.chon_engine(options, tr) not in dich.ENGINE_GEMINI:
            start_ollama()
        # The glossary (thuat_ngu.json) is read from the folder the .srt is in.
        result = dich.dich(Context(), os.path.dirname(os.path.abspath(args.srt)), "dich", {"id": "srt"},
                           options, dam_bao_ollama=start_ollama)
    finally:
        if started:
            unload_ollama(model, tr.get("ollama_base_url") or "")
    for warning in result.get("canh_bao") or []:
        emit("warning", message=warning)
    translated = [dict(c, text=c.get("text", "")) for c in result["cau"]]
    os.makedirs(os.path.dirname(os.path.abspath(args.out)), exist_ok=True)
    with open(args.out, "w", encoding="utf-8") as out:
        out.write(srt.ghi_srt(translated))
    emit("done", srt=os.path.abspath(args.out))
    return 0


def main(argv=None) -> int:
    parser = argparse.ArgumentParser()
    commands = parser.add_subparsers(dest="command", required=True)
    sub = commands.add_parser("subtitle")
    sub.add_argument("--video", required=True)
    sub.add_argument("--out-dir", required=True)
    sub.add_argument("--clean-audio", action="store_true")
    tr = commands.add_parser("translate")
    tr.add_argument("--srt", required=True)
    tr.add_argument("--out", required=True)
    tr.add_argument("--target-lang", default="Vietnamese")
    # Whisper is only told the language for English and Chinese; anything else
    # ("auto") lets it detect, so Vietnamese speech is not forced into Chinese.
    sub.add_argument("--source-lang", default="auto")
    tr.add_argument("--source-lang", default="Chinese")
    for each in (sub, tr):
        each.add_argument("--old-root", required=True)
    args = parser.parse_args(argv)
    sys.path.insert(0, os.path.abspath(args.old_root))
    os.chdir(os.path.abspath(args.old_root))
    try:
        return subtitle(args) if args.command == "subtitle" else translate(args)
    except Exception as error:  # the old code raises plain exceptions with readable messages
        emit("error", message=f"{type(error).__name__}: {error}")
        return 1


if __name__ == "__main__":
    sys.exit(main())
