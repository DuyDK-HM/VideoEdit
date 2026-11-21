# Plan: fork Concat thành editor cá nhân

Ngày chốt: 2026-11-22. Fork từ `jub0t/Concat` tại commit `178eb64` (v0.2.12).

Repo này là **engine và giao diện chính**. Dự án cũ `C:\Users\Hello\Coding\ToolAutoMakeCartoonVideo2DFromComics`
(Python/FastAPI + editor web) chỉ còn dùng làm **nguồn để chuyển code sang**, cùng với phần AI Python (`AIVoice/`).

Tài liệu liên quan:
- [PORTING-MAP.md](PORTING-MAP.md): file nào của dự án cũ chuyển vào chỗ nào trong Concat.
- [CONCAT-NOTES.md](CONCAT-NOTES.md): kiến trúc Concat, những điểm cần biết trước khi sửa.

## Quyết định đã chốt

| # | Quyết định | Lý do |
|---|---|---|
| D1 | Chỉ làm bản desktop (Concat). **Bỏ editor web** `game.ririmimivn.id.vn` | Giao diện Slint không build được ra wasm. Viết lại web thì tốn quá nhiều công |
| D2 | Dùng cá nhân, nếu cần thì công khai mã (AGPL) | Không cần commercial licence |
| D3 | Phần AI **giữ bằng Python** (`AIVoice/.venv`), Rust gọi sang bằng tiến trình con đọc JSON từng dòng | Dùng lại code đã chạy tốt. Phần dịch LLM có thể chuyển dần sang Rust sau |
| D4 | AI hàng loạt chỉ gồm **tạo phụ đề hàng loạt** và **dịch hàng loạt** | Không làm che phụ đề, lồng tiếng, OCR |
| D5 | Server riêng (`rr`) làm 2 việc: **thư viện SFX dùng chung** và **lưu/đồng bộ project**. Thêm việc khác sau, khi server mạnh hơn | Server hiện chỉ có CPU yếu |
| D6 | Code mới để trong crate hoặc module riêng, sửa code upstream càng ít càng tốt | Upstream cập nhật gần như hằng ngày, làm vậy để merge ít xung đột |
| D7 | `main` bám theo upstream, code riêng làm ở nhánh `vn/main` | Merge upstream vào `main` rồi mới merge `main` vào `vn/main`, nhờ vậy xung đột chỉ nằm ở một chỗ |
| D8 | Engine phụ đề dùng **sidecar PhoWhisper + Demucs** (`adapter_autosub_cli.py`) | Chất lượng tiếng Việt đã kiểm chứng ở dự án cũ, không phải đổi model sang ggml |
| D9 | Server đồng bộ dùng **FastAPI**. Media lưu theo nội dung: mỗi file đặt tên bằng sha256 của nó, chỉ upload những blob server chưa có. Gửi `concat.json` kèm danh sách hash | Có đồng bộ media. File chỉ lên server một lần, dùng lại được giữa các project. CPU yếu vẫn chạy được vì server chỉ ghi file, không xử lý gì thêm |
| D10 | Motion tracking track cả **vị trí, cỡ và góc xoay**: track 2 điểm, suy ra phép biến đổi đồng dạng, ra keyframe `OffsetX/OffsetY/Scale/Rotation` | `KeyProperty` đã có sẵn đủ các trường này |

## Phạm vi

**Làm:**
1. Preview video 4K mượt (hiện giật hơn editor cũ)
2. Tải video từ TikTok/YouTube/Douyin bằng yt-dlp (cookie, playlist, cả kênh)
3. Chuẩn hoá file từ điện thoại khi nhập (HEIC, CAF/AMR, MTS/MXF)
4. Tạo phụ đề hàng loạt và dịch hàng loạt
5. Ghép hàng loạt nhiều video thành một
6. Đóng gói project thành .zip và mở lại
7. Scopes: waveform, vectorscope, histogram
8. Motion tracking: chữ hoặc mask bám theo vật
9. Thư viện SFX có sẵn khi mở project, lấy từ server riêng
10. Lưu/đồng bộ project lên server riêng

**Không làm, ít nhất lúc này:** OCR phụ đề cứng, che phụ đề, TTS tiếng Việt và lồng tiếng, tạo ảnh SD, video
minh hoạ stock, căn giờ phụ đề, cắt khoảng lặng, ducking, máy xuất từ xa, xuất trên trình duyệt, tải lên theo phần,
editor web.

## Các phase

Mỗi phase xong phải có: `cargo test` của crate bị đụng tới chạy pass, `cargo clippy -D warnings` sạch,
và đã chạy app thử tận tay.

### Phase 0: Môi trường và nhánh
- [x] Build được trên Windows: rustup (toolchain 1.93.1 ghim trong `src/rust-toolchain.toml`), MSVC BuildTools, CMake, LLVM (`LIBCLANG_PATH`), FFmpeg 8.1 `win64-lgpl-shared` (`FFMPEG_DIR`, lấy từ `scripts/ci/deps.lock`)
- [x] Thêm remote upstream: `git remote add upstream https://github.com/jub0t/Concat.git`. Quy trình cập nhật: `git fetch upstream; git merge upstream/main`
- [x] Tạo nhánh `vn/main` (D7)
- [x] `setup.bat` (→ `scripts/vn/setup.ps1`): cài qua winget những gì thiếu, tải FFmpeg vào `vendor/`, ghi `vendor/env.cmd`. `run.bat`: nạp env, `cargo build --profile quick -p concat` (cargo tự biết có cần build lại không), mở `src/target/quick/concat.exe`
- [ ] Chuyển sang Phase 3: cấu hình đường dẫn Python sidecar (`AIVoice/.venv/Scripts/python.exe` + thư mục adapter)
- Máy code (ổ đầy, GPU AMD) chỉ chạy `cargo check`/`test` từng crate. Máy chạy chính (NVIDIA) cài bằng `setup.bat` rồi chạy `run.bat`.
- Xong khi: app chạy được, mở và xuất thử một project (kiểm tra trên máy chạy chính).

### Phase 1: Preview 4K mượt
Nguyên nhân đã tìm ra: `Proxies` mặc định tắt (`src/crates/concat/src/prefs.rs:132`), và proxy chỉ dùng khi preview ở Half/Quarter.
Editor cũ thì tạo proxy H.264 720p ngay khi nhập, trình duyệt giải mã bằng phần cứng.
- [ ] Đo trước: fps lúc phát file 4K H.264 và HEVC ở Full/Half, CPU/GPU%
- [ ] Bật Proxies mặc định. Khi phát thì đọc proxy ở mọi mức preview (giữ file gốc cho lúc dừng/xuất, nếu cần)
- [ ] Xem lại cỡ proxy (`src/crates/concat-host/src/proxy.rs`: `DIVISOR`, `SHORTEST_LONG_SIDE`) và encoder dùng để tạo proxy
- [ ] Kiểm tra hwaccel giải mã trên Windows (`concat-media/src/decode.rs`: d3d11va/dxva/cuda) có được bật cho preview không
- Xong khi: file 4K phát ≥ 30 fps ổn định trên laptop, không khựng lúc tua.

### Phase 2: Tải video và chuẩn hoá file khi nhập
- [ ] Panel "Tải từ link": nhận link, playlist, kênh; nhận file cookie Netscape; lọc cookie WAF của TikTok. Gọi `yt-dlp` thành tiến trình con, báo tiến độ, tải xong thì nhập vào Media bin
- [ ] Chuẩn hoá khi nhập: trước hết kiểm tra FFmpeg của Concat đã đọc được những định dạng nào. Chỉ chuyển đổi những cái không đọc được (HEIC→JPG, CAF/AMR→M4A, MTS/MXF→proxy)
- Xong khi: dán link TikTok và YouTube đều tải về và nằm trong bin, file HEIC/CAF từ iPhone nhập được.

### Phase 3: Phụ đề hàng loạt và dịch hàng loạt
- [ ] Phụ đề: gọi `adapter_autosub_cli.py` (PhoWhisper + Demucs CUDA) qua sidecar (D8)
- [ ] Dịch: đóng gói `dich.py` + `dich_lo_json.py` + `de_xuat_dich.py` thành CLI Python (vào .srt → ra .srt), giữ phần kiểm tra chất lượng bản dịch tiếng Việt. Ollama/Gemini quản lý bằng `ollama_manager.py`, `gemini_proxy.py`
- [ ] Phía Concat: một module sidecar chung (chạy tiến trình con, đọc JSON từng dòng, huỷ được, có hàng đợi GPU một việc một lúc), bắt chước mẫu `concat-host/src/tts.rs`
- [ ] Hộp thoại hàng loạt: chọn nhiều media/clip → phụ đề → (tuỳ chọn) dịch → đặt lên lane caption bằng `concat_project::srt`
- [ ] Thêm variant `Request` trong `concat-api/src/message.rs` để MCP/CLI dùng được
- Xong khi: 10 video chạy một lượt ra phụ đề tiếng Việt đã dịch trên timeline, huỷ giữa chừng không treo.

### Phase 4: Ghép hàng loạt
- [ ] Chọn nhiều file (sắp theo tên tự nhiên như `001_ten.mp4`) → đặt nối tiếp lên một timeline (`clips_add_sequence`) → xuất
- [ ] Đường tắt: nếu các file cùng codec, cỡ và fps thì ghép `-c copy` không mã hoá lại, theo logic của `video_merger.py`
- Xong khi: ghép 20 file khác cỡ ra một video đúng thứ tự, có tiếng (video câm thì chèn tiếng im lặng).

### Phase 5: Đóng gói project .zip
- [ ] Gói thư mục project (`concat.json` + media được dùng), bỏ `cache/`, proxy và bản xuất. Mở .zip thì giải nén rồi relink
- Xong khi: gói trên máy A, mở trên máy B đủ media, không có clip offline.

### Phase 6: Scopes
- [ ] Waveform (luma/RGB parade), vectorscope, histogram, tính trên frame preview đã thu nhỏ. Tham khảo `scopes_mau.js` của dự án cũ
- [ ] Hiện trong panel màu (`concat/src/grading.rs`, inspector)
- Xong khi: chỉnh màu thì scopes cập nhật theo thời gian thực, không làm tụt fps preview.

### Phase 7: Motion tracking
- [ ] Nghiên cứu trước: tracker template/NCC trên frame thu nhỏ (thuần Rust), hay model ONNX qua `ort`
- [ ] Track 2 điểm → suy ra phép đồng dạng (dịch, cỡ, xoay) → keyframe `OffsetX/OffsetY/Scale/Rotation` gắn vào clip chữ hoặc mask (D10). `concat-project` đã có trường `motionTrack` dự phòng (`lib.rs:2800`)
- Xong khi: chữ bám theo một vật trong 10 giây video (vật có lại gần/ra xa và xoay) không bị trôi rõ.

### Phase 8: Server riêng (SFX và đồng bộ project)
- [ ] SFX: dựng `audio/manifest.json` + file trên server theo đúng định dạng `docs/AUDIO_LIBRARY.md`. Đổi các hằng `SOUNDS_MANIFEST`, `SOUNDS_REPO`, `SOUNDS_RELEASE` (`concat-host/src/models.rs`) sang server mình hoặc cho chỉnh trong Settings. Chuyển kho SFX CC0 của dự án cũ (`assets/thu_vien_am/`) lên
- [ ] Đồng bộ project (D9): FastAPI trên `rr`, xác thực bằng token. API:
  - `POST /blobs/missing` (gửi danh sách sha256, nhận lại những hash server chưa có)
  - `PUT /blobs/{sha256}` (upload kiểu stream; server tính lại hash rồi mới nhận file)
  - `GET /blobs/{sha256}`
  - `PUT/GET /projects/{id}` (`concat.json` + bảng đường dẫn tương đối → hash, lưu nhiều phiên bản)
  - `GET /projects`
  - Bỏ qua `cache/`, proxy và bản xuất. Máy nhận kéo những blob còn thiếu về rồi relink
- [ ] Deploy theo cách chép file qua `scp` lên `rr`, chạy lệnh từ PowerShell
- Xong khi: mở project trên máy khác thấy đủ SFX, đẩy và kéo project qua server được.

## Thứ tự và phụ thuộc
- Phase 0 → 1 → 2 → 3 → 4 → 5 → 8. Phase 6 và 7 làm lúc nào cũng được, sau Phase 0.
- Phase 8 dùng lại phần liệt kê media và relink của Phase 5 (không dùng .zip). Phần SFX của Phase 8 làm độc lập được.

## Rủi ro
- Upstream đổi định dạng project (`concat-doc` mới có ticks, typed id) → merge xung đột. Giảm rủi ro: merge upstream thường xuyên, code riêng để tách khỏi code upstream.
- Crate UI build rất lâu (Slint sinh khoảng 970k dòng Rust). Nên tách logic ra crate khác để test nhanh.
- Sidecar Python phụ thuộc vào `AIVoice/.venv` của dự án cũ. Phải ghi rõ đường dẫn cấu hình, báo lỗi dễ hiểu khi thiếu.
- Muốn phát hành bản đã sửa thì phải đổi tên và icon (`TRADEMARK.md`). Dùng cá nhân thì không cần.

## Câu hỏi còn mở
- Upload blob lớn (file 4K vài GB) có cần chia phần để tải tiếp khi rớt mạng không? Chốt khi đo mạng tới `rr`.
