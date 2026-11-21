# Ghi chú kiến trúc Concat (để làm fork)

Đọc từ mã nguồn ở commit `178eb64`, ngày 2026-11-22. Đường dẫn tính từ `src/crates/`.

## Crate
- **UI**: chỉ có `concat`, gồm khoảng 45k dòng Rust và 41k dòng `.slint` (`concat/ui/`). Ngôn ngữ giao diện ở `concat/locales/*.json`, **đã có `vi.json`**.
  Các pane nằm ở `concat/src/panes/`, logic editor ở `concat/src/studio/`.
- **Engine**:
  - `concat-core`: thời gian, frame, timeline.
  - `concat-project`: document, command, undo, `srt.rs`, nhập CapCut.
  - `concat-media`: FFmpeg.
  - `concat-render`: compositor wgpu.
  - `concat-export`.
  - `concat-host`: session, project, preview, playback, model, proxy, tts.
  - `concat-effects`: 132 package TOML/WGSL trong `concat-effects/packages/`.
  - `concat-text`.
  - `concat-vision`: cutout dùng ONNX.
  - `concat-speech`: whisper.cpp + Chatterbox.
  - `concat-doc`: format mới có ticks và typed id, mới thêm vào. Rủi ro format sẽ đổi.
- **Giao tiếp**:
  - `concat-api`: dispatcher JSON-RPC 2.0, nằm ở `message.rs` và `rpc.rs`.
  - `concat-server`: TCP/Unix socket, không có HTTP/WS, cần token.
  - `concat-mcp`: xem `docs/MCP.md`.
  - `concat-cli`: `render`, `api`, `serve`.

## Thêm tính năng ở đâu
- Tác vụ chạy tiến trình ngoài rồi đưa file vào project: làm theo mẫu `concat-host/src/tts.rs` (ghi WAV vào `audio/` rồi import).
- Muốn CLI/server/MCP đều gọi được thì thêm variant `Request` vào `concat-api/src/message.rs`.
- Phụ đề: dùng `concat_project::srt::parse` và các command thêm title. Caption là clip Text nằm trên lane.
- Effect mới: viết package dữ liệu (`effect.toml` + `[ffmpeg]` hoặc `[wgsl]` + `fixtures.toml`). Package có sẵn được nhúng lúc build (`concat-effects/build.rs`); package của người dùng nạp lúc chạy (`Catalogue::load_dir`).
- Không có plugin system cho code.

## Mô hình dữ liệu
- Project là một thư mục gồm `concat.json` (document v2, `MIGRATIONS` chỉ được nối thêm vào cuối), `concat.opened.json`, `audio/`, `cache/` và `concat.lock`.
- Cấu trúc `Project → Timeline → Track → Clip`. Các loại `ClipKind`: Video, Audio, Image, Text, Layer, SoundLayer, Shape, Transition.
- Keyframe là `ClipKey { property, at: 0..1 của clip, value, ease: cubic-bezier [f64;4] }`. Các `KeyProperty`: Scale, OffsetX/Y, Rotation, StretchX/Y, Opacity, Volume.

## Preview và proxy (Phase 1)
- `concat/src/prefs.rs:132`: `proxies` mặc định **tắt**. Proxy chỉ được đọc khi preview ở Half/Quarter.
- `concat-host/src/proxy.rs`: `PIXELS_WITHOUT_PROXY = 1920*1080`, `DIVISOR = 4`, `SHORTEST_LONG_SIDE = 960`.
- Giải mã có hwaccel: `concat-media/src/decode.rs`.
- Compositor bắt buộc chạy wgpu. Máy không có GPU thì dùng software adapter (WARP trên Windows, lavapipe trên Linux), chậm.

## AI
- Model khai báo trong `models/manifest.toml` (bắt buộc sha256, có mirror) và được tải bằng `concat-host/src/models.rs`.
- ONNX chạy qua `ort =2.0.0-rc.13` (DirectML trên Windows). Whisper chạy qua whisper-rs 0.16.
- Có tiếng Việt khi tạo caption. TTS Chatterbox không có tiếng Việt. Không có tính năng dịch hay LLM.

## Thư viện SFX
- Xem `docs/AUDIO_LIBRARY.md`: app tải manifest khi người dùng bấm nút, kiểm sha256, lưu cache.
- URL là hằng `SOUNDS_MANIFEST`, `SOUNDS_REPO`, `SOUNDS_RELEASE` trong `concat-host/src/models.rs`.

## Xuất
- Encoder nằm ở `concat-media/src/encode.rs:195-245`. Thử phần cứng trước (nvenc/qsv/amf, Windows có thêm h264_mf).
- Software: H.264 dùng `libopenh264` (không có libx264), HEVC dùng `libkvazaar`, AV1 dùng svtav1/aom.
- Server chỉ chạy một export mỗi lúc; lần thứ hai trả `Busy`.

## Build và test
- Rust 1.93+, FFmpeg 7 shared (BtbN `win64-gpl-shared`, `FFMPEG_DIR`), LLVM (`LIBCLANG_PATH`), cmake, C++.
- Chạy: `cd src; cargo run -p concat`. Bản release: `cargo build --profile app -p concat`.
- Test: `cargo test` / `cargo nextest`, clippy `-D warnings`. Crate UI build rất lâu, nên test logic ở crate khác.
- Repo có sẵn skill `.claude/skills/run-concat/SKILL.md` để chạy app.

## Giấy phép
- AGPL-3.0-or-later. Dùng cá nhân thì không có nghĩa vụ gì thêm.
- Phát hành bản đã sửa thì phải công bố mã, **và phải đổi tên cùng icon** (`TRADEMARK.md`).
