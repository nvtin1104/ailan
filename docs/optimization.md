# Ailan — Tối ưu hệ thống

## 1. Ngân sách hiệu năng

| Chỉ số | Mục tiêu | Cách đo |
|---|---|---|
| CPU khi rảnh | **0%** (0 frame/phút, ≤ 1 lần thức/phút cho đồng hồ) | `--stats`, Task Manager |
| RAM (working set) | ≤ 30 MB | `--stats`, Task Manager → Details |
| Kích thước binary | ≤ 5 MB | `target/release/ailan.exe` |
| Khởi động lạnh | ≤ 150 ms tới frame đầu | log `first frame in … ms` |

Kết quả đo thực tế: xem mục 5.

## 2. Nguyên tắc

1. **Chờ thay vì kiểm tra** — mọi nguồn dữ liệu là sự kiện (WinRT event, COM callback, `WM_POWERBROADCAST`). Không có vòng polling.
2. **Chỉ vẽ khi có gì thay đổi** — vòng lặp chính `GetMessageW` chặn hoàn toàn. Chỉ khi spring animation đang chạy mới chuyển sang chế độ frame-loop (`Present(1)` đồng bộ vsync); spring dừng → quay lại chặn.
3. **Một deadline duy nhất** — `scheduler::next_deadline` gộp mọi nhu cầu thời gian thành một `SetCoalescableTimer` (cho phép Windows gộp wakeup với tolerance).
4. **Không runtime async** — callback WinRT + message loop là đủ; tránh tokio/threads thừa.

## 3. Kỹ thuật cụ thể

### Build
```toml
[profile.release]
opt-level = 3
lto = "fat"
codegen-units = 1
panic = "abort"
strip = true
```
Phụ thuộc tối thiểu: `windows`, `serde`, `serde_json`, `toml`, `log`. Feature của crate `windows` chỉ bật đúng namespace dùng tới.

### Render
* Tạo `IDWriteTextFormat` và `ID2D1SolidColorBrush` **một lần**, dùng lại mọi frame (đổi màu brush bằng `SetColor`, không tạo mới).
* Artwork được WIC giải mã + thu nhỏ **một lần trên worker** về 96×96 BGRA premultiplied, tạo `ID2D1Bitmap` một lần mỗi bài.
* Swapchain 2 buffer, `FLIP_SEQUENTIAL`, kích thước = notch mở rộng (≈ 440×190 DIP), không phải toàn màn hình.
* Icon dùng glyph font hệ thống `Segoe MDL2 Assets` → không cần ảnh, không tốn RAM.

### Khi rảnh
* 0 frame khi spring đã dừng; đồng hồ thu gọn chỉ hiện giờ:phút → thức 1 lần/phút.
* Tiến độ bài hát nội suy cục bộ từ timeline snapshot, chỉ tick 1 s khi notch **đang mở rộng** ở tab Nhạc.
* Pomodoro thu gọn hiển thị theo phút; tick từng giây chỉ khi tab Tập trung đang mở.
* **EcoQoS** (`SetProcessInformation(ProcessPowerThrottling)`) bật khi thu gọn, tắt khi mở rộng/animation.

### Bộ nhớ
* Ẩn (fullscreen) → `IDXGIDevice3::Trim()` + bỏ artwork.
* `EmptyWorkingSet` một lần sau khởi động (trả lại trang dùng khi init).
* Không giữ lịch sử; danh sách to-do giới hạn hiển thị.

## 4. Đo lường

Chạy:
```
ailan.exe --stats
```
Mỗi 60 s ghi vào `%LOCALAPPDATA%\Ailan\logs\ailan.log`:
```
stats: frames=0 wakeups=1 ws=18.4MB
```

Bổ sung:
* Task Manager → Details → cột *CPU*, *Memory (active private working set)*.
* `typeperf "\Process(ailan)\% Processor Time" "\Process(ailan)\Working Set" -si 5`
* PerfView / WPA để xem wakeup & stack khi có frame bất thường.

## 5. Kết quả đo

(cập nhật sau mỗi đợt tối ưu)

| Ngày | Build | Binary | WS khi rảnh | Frames/phút khi rảnh | Ghi chú |
|---|---|---|---|---|---|
| 2026-11-10 | release 0.1.0 | **675 KB** | **8–10 MB** (private 33 MB) | **1** (đồng hồ đổi phút) | CPU đo bằng `TotalProcessorTime`: **0 ms trong 125 s**. Frame đầu tiên 126 ms. ~3 lần thức/phút (1 tick đồng hồ + 1 timer `--stats` + 1 sự kiện hệ thống). Win 11, 1920×1080 @100%. |

Nhận xét:
* Private bytes ~33 MB chủ yếu là driver D3D/DWM và thread pool COM/WinRT; working set thực tế chỉ ~10 MB.
* Thanh "equalizer" khi nhạc phát được vẽ **tĩnh** (không animation) để phát nhạc vẫn tốn 0 frame.
* Còn có thể tối ưu: giảm số thread bằng cách gộp worker media/brightness; dùng `--stats` sau mỗi thay đổi lớn.
