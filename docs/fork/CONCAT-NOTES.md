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
- Tác vụ chạy lâu rồi đưa file vào bin: mẫu là TTS ở `concat/src/panes/speech.rs` (`fn run`, nhánh `SpeechMsg::Finished`). (`concat-host/src/tts.rs` không tồn tại.)
  - Spawn bằng `crate::host::spawn_long_in_project(work, then)` (`concat/src/host.rs`). Job tự huỷ khi đóng project. `then` chạy trên event loop qua `slint::invoke_from_event_loop`.
  - Một việc một lúc, huỷ được: `concat_host::jobs::SingleFlight` (`gate.begin(..)?`, `job.cancelled()`, `gate.cancel()`). Mẫu gọn nhất: `concat-host/src/reverse.rs`.
  - Báo tiến độ: `on_ui_in_project(epoch, |studio,_,_| studio.handle(Msg::X(..)))`.
  - Nhập file: `studio.handle(Msg::Media(MediaMsg::Import(vec![path])))`, hoặc probe trước rồi `Command::AddMedia`.
- Đường nhập media: mọi lối vào (nút trong bin, File › Import, kéo thả) đều về `MediaMsg::Import` (`concat/src/panes/media_bin.rs`), đi tiếp `media::probe` (FFmpeg) rồi `Command::Batch` gồm các `AddMedia`. Không có allowlist nào chặn file; nút trong bin chỉ lọc đuôi file trong hộp thoại (`concat/src/lib.rs`, khoảng dòng 1079). Chỗ hook chuẩn hoá (Phase 2): worker của `MediaMsg::Import`, chạy trước `probe`.
- Dialog mới: theo mẫu `ui/dialogs/confirm.slint` (đơn giản) hoặc `relink.slint` + `panes/relink.rs` (có state). Phải sửa: `ui/app.slint` (property, callback, điều kiện modal, Escape), `panes/mod.rs` (`Msg::X`), `studio/project_files.rs` `handle()`, `studio/keyframes.rs` (`set_changed`, và `menu_bar()` cho menu), `lib.rs` (`app.on_*`, `on_app_menu_selected`), `locales/*.json`.
- Nối nhiều media lên timeline: `Command::AddClipsInSequence { media_ids, track_id, start, ripple }`. API không có request riêng, gửi qua `Request::EditApply`.
- SRT lên lane caption: `CaptionsPane::import_srt` (`panes/captions.rs`), lần lượt `srt::parse` → `srt_commands` → `Command::Batch` (một bước undo).
- `consolidate.rs` là render nhóm clip thành một file (merge), không phải đóng gói project. Hiện chưa có tính năng đóng gói.
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
- Rust 1.93.1 (ghim), FFmpeg 8.1 shared (BtbN `win64-lgpl-shared`, ghim trong `scripts/ci/deps.lock`, `FFMPEG_DIR`), LLVM (`LIBCLANG_PATH`), cmake, MSVC. Cài hết bằng `setup.bat`.
- Chạy: `cd src; cargo run -p concat`. Bản release: `cargo build --profile app -p concat`.
- Test: `cargo test` / `cargo nextest`, clippy `-D warnings`. Crate UI build rất lâu, nên test logic ở crate khác.
- Repo có sẵn skill `.claude/skills/run-concat/SKILL.md` để chạy app.

## Giấy phép
- AGPL-3.0-or-later. Dùng cá nhân thì không có nghĩa vụ gì thêm.
- Phát hành bản đã sửa thì phải công bố mã, **và phải đổi tên cùng icon** (`TRADEMARK.md`).
