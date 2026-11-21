# Bản đồ chuyển code từ dự án cũ sang Concat

Dự án cũ ở `C:\Users\Hello\Coding\ToolAutoMakeCartoonVideo2DFromComics` (ở dưới viết tắt là `OLD/`).
Python có sẵn thư viện AI: `OLD/AIVoice/.venv/Scripts/python.exe`. Chỉ chạy được trên Windows, có CUDA.

Cách chuyển:
- **Rust** = viết lại bằng Rust trong Concat.
- **Sidecar** = giữ nguyên Python, Concat gọi bằng tiến trình con. Mỗi dòng stdout là một JSON (`{"type":"progress",...}`), và có thể huỷ.
- **Server** = chạy trên server `rr`.

Trước khi viết mới, luôn xem Concat đã có sẵn gì để dùng lại.

## Phase 1: Preview 4K
| Nguồn OLD | Đích Concat | Cách | Ghi chú |
|---|---|---|---|
| `orchestrator/editor/media.py` (`can_proxy_ngay`, proxy H.264 720p, `LUON_PROXY`) | `concat-host/src/proxy.rs`, `concat/src/prefs.rs` (`proxies`) | Rust | Chỉ lấy ý tưởng: tạo proxy ngay khi nhập, file gốc chỉ dùng khi xuất |

## Phase 2: Tải video và chuẩn hoá file
| Nguồn OLD | Đích Concat | Cách | Ghi chú |
|---|---|---|---|
| `AIVoice/apps/MediaComposer/adapter_download_cli.py`, `app/services/video_downloader.py` | module mới (vd `concat-host/src/download.rs`) + panel trong `concat/src/panes/` | Rust gọi `yt-dlp` | Port các tham số: playlist/kênh, cookie Netscape, lọc cookie WAF TikTok. Tải `yt-dlp.exe` về như các model |
| `orchestrator/editor/tai_link_cli.py` | như trên | Rust | Bản nhẹ, không cần AIVoice. Bản này dễ port nhất |
| `orchestrator/editor/chuan_hoa_file.py` | bước nhập media (`concat-host`, `media.import`) | Rust (FFmpeg) | HEIC→JPG, CAF/AMR→M4A, MTS/MXF→proxy. Kiểm tra trước FFmpeg của Concat đọc được gì |
| `orchestrator/project.py` (đổi tên `001_ten.mp4`, sắp theo tên tự nhiên) | dùng cho Phase 4 | Rust | Chỉ lấy hàm sắp theo tên tự nhiên |

## Phase 3: Phụ đề và dịch hàng loạt
| Nguồn OLD | Đích Concat | Cách | Ghi chú |
|---|---|---|---|
| `orchestrator/editor/ai_bridge.py` (tác vụ `phu-de`, `dich`) | module sidecar chung (mới), theo mẫu `concat-host/src/tts.rs` | Rust | Dựng dòng lệnh, đọc JSON, ghi kết quả vào thư mục project |
| `AIVoice/apps/MediaComposer/adapter_autosub_cli.py` (`--sub-source whisper --translate-only --no-translate`, PhoWhisper, Demucs) | sidecar | Sidecar | Phương án (b) của Phase 3 |
| `orchestrator/editor/whisper_truc_tiep.py` | không cần | — | Concat đã có whisper.cpp, có tiếng Việt (`concat-speech/src/transcribe.rs:105`) |
| `orchestrator/editor/dich.py`, `dich_lo_json.py`, `de_xuat_dich.py` | CLI Python mới bọc lại (vào .srt → ra .srt) | Sidecar, sau này chuyển dần sang Rust | Giữ bảng thuật ngữ theo project và các kiểm tra chất lượng bản dịch tiếng Việt (`VIETNAMESE_SYLLABLE_RE`, từ nguồn lọt vào bản dịch, mất dấu) |
| `orchestrator/llm.py`, `ollama_manager.py`, `gemini_proxy.py` | sidecar (cùng CLI dịch) | Sidecar | Tự khởi động Ollama, tự pull model, proxy cookie Gemini cổng 7860 |
| `orchestrator/editor/hang_loat.py`, `webui/editor/phu_de_hang_loat.js` | hộp thoại hàng loạt trong `concat/ui` + `concat/src` | Rust | Chỉ lấy các bước phụ đề và dịch |
| `orchestrator/editor/hang_doi.py`, `orchestrator/process_manager.py` | module sidecar | Rust | Hàng đợi GPU chạy một việc một lúc, huỷ/giết cả cây tiến trình con |
| `orchestrator/editor/srt.py` (đọc cp1258, UTF-16) | `concat-project/src/srt.rs` | Rust | Thêm việc nhận diện bảng mã nếu Concat chưa có |

## Phase 4: Ghép hàng loạt
| Nguồn OLD | Đích Concat | Cách | Ghi chú |
|---|---|---|---|
| `orchestrator/video_merger.py` | lệnh mới (API `clips_add_sequence` + export), cộng thêm đường tắt `-c copy` | Rust | Khác cỡ thì chuẩn hoá: viền đen, 30fps, AAC 48k, chèn tiếng im lặng cho video câm |

## Phase 5: Đóng gói .zip
| Nguồn OLD | Đích Concat | Cách | Ghi chú |
|---|---|---|---|
| `orchestrator/editor/goi_du_an.py` | `concat-host` (projects/relink) + menu File | Rust | Bỏ `cache/`, proxy, bản xuất. Giải nén thì relink theo đường dẫn tương đối |

## Phase 6: Scopes
| Nguồn OLD | Đích Concat | Cách | Ghi chú |
|---|---|---|---|
| `webui/editor/scopes_mau.js` | panel màu (`concat/src/grading.rs`, inspector) | Rust (CPU trên frame thu nhỏ, hoặc WGSL) | Waveform/parade, vectorscope, histogram |

## Phase 7: Motion tracking
Không có code cũ. Viết mới. Kết quả là keyframe `OffsetX/OffsetY/Scale` (`concat-project/src/model.rs`, `KeyProperty`).

## Phase 8: Server
| Nguồn OLD | Đích | Cách | Ghi chú |
|---|---|---|---|
| `assets/thu_vien_am/` (`nhac/`, `sfx/`, `thu_vien.json`) | `audio/manifest.json` + file trên server, định dạng theo `docs/AUDIO_LIBRARY.md` | Server | Viết script đổi `thu_vien.json` sang manifest có `sha256` |
| `orchestrator/editor/thu_vien_am.py` (thư viện "của tôi" dùng chung) | Concat: đổi `SOUNDS_MANIFEST`/`SOUNDS_REPO`/`SOUNDS_RELEASE` (`concat-host/src/models.rs`) | Rust + Server | Cho chỉnh URL trong Settings |
| `orchestrator/main.py` (FastAPI, Basic Auth) | API đồng bộ project trên `rr` | Server | Có thể dùng lại khung FastAPI. Dùng token, upload/download .zip |

## Không chuyển
OCR (`app/services/subtitle_extractor.py`), che phụ đề (`webui/editor/bang_che.js`), TTS và lồng tiếng (`AIVoice/src/engines/*`, `dubbing.py`),
tạo ảnh SD, minh hoạ stock (`material.py`), `can_gio.py`, `khoang_lang.py`, `nhip.py` (Concat đã có `beats.rs`),
`mau_nang_cao.py` (Concat đã có LUT, wheels, curves, HSL), xoá nền (Concat có sẵn), máy xuất từ xa (`may_xuat.py`, `may_ai.py`, `xuat_tu_xa.py`),
`webui/editor/xuat_thiet_bi/*`, `tai_len.py`, toàn bộ editor web.
