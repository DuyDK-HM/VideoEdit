# Cách dùng các tính năng của fork

Mọi tính năng mới nằm trong menu **File** của Concat.

## Cài và chạy (máy mới)
1. `git clone` repo, chuyển sang nhánh `vn/main`.
2. Chạy `setup.bat` để cài Rust, MSVC, CMake, LLVM, yt-dlp và FFmpeg 8.1 rồi ghi `vendor\env.cmd`. Nếu có dự án cũ `ToolAutoMakeCartoonVideo2DFromComics` nằm cạnh repo thì `setup.bat` bật luôn phụ đề và dịch.
3. Tạo `vendor\local.cmd` (không đưa lên git) để dùng server riêng:
   ```bat
   set "CONCAT_SYNC_URL=http://100.114.205.62:8700"
   set "CONCAT_SYNC_TOKEN=<token trong rr:~/concat-sync/token>"
   set "CONCAT_SOUNDS_BASE=http://100.114.205.62:8700/audio"
   ```
4. Chạy `run.bat`. Nó build nếu mã nguồn đã đổi, rồi mở app.

## Tính năng
| Menu | Làm gì |
|---|---|
| (tự động) | Video lớn hơn HD sẽ có proxy bằng nửa cỡ gốc, phát ở mọi mức chất lượng, nên 4K không giật. Có thể tắt trong Settings |
| (tự động) | Ảnh HEIC của iPhone được đổi sang JPG khi nhập, ra đủ cỡ và đúng chiều. File CAF, AMR, MTS, MXF nhập thẳng được |
| **Tải từ link…** | Dán link TikTok, YouTube, Douyin, playlist hoặc cả kênh, mỗi dòng một link. File cookie (dạng Netscape) không bắt buộc. Video tải xong tự vào bin |
| **Ghép các file đã chọn** | Chọn từ 2 file trở lên trong bin. File được sắp theo tên (`2_a` đứng trước `10_a`). Nếu các file cùng thông số thì ghép không mã hoá lại và kết quả vào bin; nếu khác thì đặt nối tiếp lên timeline, sau đó bấm Export |
| **Phụ đề hàng loạt** / **Phụ đề + dịch hàng loạt** | Chạy cho các clip video đang chọn trên timeline, hoặc tất cả nếu không chọn gì. Dùng PhoWhisper (và Ollama/Gemini nếu có dịch) của dự án cũ, mỗi lần một việc trên GPU. Phụ đề được đặt thành caption đúng chỗ clip |
| **Dừng việc AI** | Dừng việc phụ đề hoặc dịch đang chạy |
| **Bám theo vật** | Đặt sẵn một clip chữ, sticker hoặc hình lên đúng vật ở frame đầu, chọn clip đó rồi bấm. Clip sẽ nhận keyframe vị trí, cỡ và góc xoay, 10 mốc mỗi giây. Ctrl+Z để bỏ |
| **Đóng gói project (.zip)** | Ra `<tên project>.zip` nằm cạnh thư mục project, gồm mọi file project dùng, bỏ `cache/` |
| **Mở gói project (.zip)…** | Giải nén ra thư mục cạnh file zip rồi mở |
| **Đẩy project lên server** | Lưu project rồi đẩy lên `rr`. Chỉ upload những file server chưa có |
| **Kéo project mới nhất từ server** | Kéo về `%USERPROFILE%\Concat Sync\<id>` rồi mở. Kéo lại thì chỉ tải phần thay đổi |
| Scopes | Mở hiệu ứng có bánh xe màu trong inspector. Bên dưới bánh xe là waveform, vectorscope và histogram của frame đang dừng |
| Thư viện SFX | Có `CONCAT_SOUNDS_BASE` thì trang Audio sẽ lấy 1263 âm thanh CC0 từ `rr` |

## Dùng qua AI agent (MCP)
- Build bằng `run.bat` một lần, rồi đăng ký: `claude mcp add concat -- F:\programfiles\VideoEdit\mcp.bat`. `mcp.bat` nạp `vendor\env.cmd`, nên agent có đủ FFmpeg, yt-dlp, sidecar và token sync.
- Ngoài các tool gốc (xem `docs/MCP.md`), có thêm tool cho từng tính năng của fork: `media_download`, `media_merge`, `captions_transcribe` (trả về phụ đề đã đặt đúng chỗ clip; agent gọi tiếp `captions_add` để đưa lên timeline), `project_pack`, `project_unpack`, `sync_push`, `sync_pull`.
- Các tool này chạy xong mới trả kết quả. Tải video hoặc làm phụ đề có thể mất vài phút.
- Khi app đang mở và bật Settings > Automation, agent làm việc qua app. Project đang mở trên màn hình thì agent không mở được, giống bản gốc.
- Bám theo vật và scopes chưa có tool, vì chúng cần chọn trên màn hình.

## Server `rr`
- Deploy hoặc cập nhật: `powershell -ExecutionPolicy Bypass -File server\deploy.ps1`. Thêm `-Sounds <thư mục thu_vien_am>` để dựng lại thư viện âm thanh.
- Server chạy dạng user service `concat-sync`, chỉ nghe trên IP Tailscale, cổng 8700.

## Giới hạn hiện tại
- Bám theo vật giả định video nằm dưới phủ kín khung hình (scale 1, không bị dời).
- Phụ đề hàng loạt tính đúng tốc độ clip nếu clip có speed cố định. Clip có đường cong tốc độ thì bị bỏ qua phần đường cong.
- Ghép nhanh `-c copy` chỉ dùng khi codec, cỡ, fps và thông số tiếng của các file giống hệt nhau.
