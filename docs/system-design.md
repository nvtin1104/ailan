# Ailan — Thiết kế hệ thống

Ailan là một "notch sống" cho Windows 10/11: một viên thuốc (pill) nhỏ ở mép trên giữa màn hình, hiển thị nhạc đang phát, đồng hồ, bộ hẹn giờ tập trung, việc cần làm và trạng thái hệ thống, rồi tự thu gọn khi không cần.

Mục tiêu thiết kế: **native thuần Rust**, **không polling**, **0% CPU khi rảnh**, RAM ~20–30 MB.

---

## 1. Tổng quan kiến trúc

```
                    ┌──────────────────────── UI thread (STA) ─────────────────────────┐
                    │                                                                  │
 WinRT / COM        │   Win32 message loop  (GetMessage — chặn khi rảnh)               │
 callbacks          │        │                                                         │
 (thread pool)      │        ▼                                                         │
┌──────────────┐    │   WM_APP_EVENT ──► drain mpsc ──► AppState::reduce(event, now)   │
│ media worker │────┼──►   (PostMessage)                     │  (thuần, có unit test)   │
│ audio cb     │────┤                                        ▼                          │
│ brightness cb│────┤                                  dirty / anim?                    │
│ network cb   │────┤                                        │                          │
└──────────────┘    │   WM_POWERBROADCAST ─────────────────► │                          │
                    │   WM_TIMER (1 deadline duy nhất) ────► │                          │
                    │   chuột / bàn phím / tray ───────────► ▼                          │
                    │                              ui::draw (immediate mode)            │
                    │                                   │  → hit-targets               │
                    │                                   ▼                              │
                    │                  Direct2D → swapchain → DirectComposition         │
                    └──────────────────────────────────────────────────────────────────┘
```

### Crates

| Crate | Vai trò |
|---|---|
| `crates/ailan-core` | Logic thuần, không phụ thuộc Windows: `AppEvent`, `AppState` + reducer, Pomodoro, to-do + parser `@HH:MM`, scheduler deadline, spring animation, settings/persistence, i18n (en/vi), tiện ích thời gian. **Toàn bộ unit test nằm ở đây.** |
| `crates/ailan` | Binary Windows: cửa sổ notch, Direct2D/DirectComposition, vẽ UI, tray, autostart, các *provider* hệ thống (media, âm lượng, độ sáng, pin, mạng, tai nghe), ẩn khi fullscreen. |

### Module trong `ailan` (bin)

| Module | Trách nhiệm |
|---|---|
| `main.rs` | Single-instance (named mutex), DPI awareness Per-Monitor V2, logger, khởi động `App`. |
| `app.rs` | Sở hữu `AppState`, cửa sổ, `Gfx`; window procedure; điều phối sự kiện → reducer → vẽ; vòng lặp animation; lập lịch timer. |
| `gfx.rs` | D3D11 device → D2D device context → swapchain *for composition* (premultiplied alpha) → DComposition visual. Cache text format & brush. Phục hồi khi device lost. `trim()` khi ẩn. |
| `ui.rs` | Bố cục + vẽ dạng immediate-mode cho trạng thái thu gọn và mở rộng; trả về danh sách vùng bấm (`HitTarget`). |
| `platform/media.rs` | Worker thread (MTA) dùng `GlobalSystemMediaTransportControlsSessionManager`; giải mã artwork bằng WIC một lần. |
| `platform/audio.rs` | `IAudioEndpointVolumeCallback` (âm lượng/mute) + `IMMNotificationClient` (đổi thiết bị mặc định, nhận diện tai nghe qua form factor). |
| `platform/brightness.rs` | WMI `WmiMonitorBrightnessEvent` qua `IWbemObjectSink` (chỉ laptop). |
| `platform/network.rs` | WinRT `NetworkInformation.NetworkStatusChanged` + SSID/số vạch sóng. |
| `platform/power.rs` | `RegisterPowerSettingNotification` (pin %, AC/DC) + `GetSystemPowerStatus`. |
| `platform/tray.rs` | `Shell_NotifyIconW`, menu, thông báo balloon (dùng cho nhắc việc — hoạt động cả với app không đóng gói, không cần AUMID như WinRT toast). Icon được vẽ bằng code (không cần file .ico). |
| `platform/autostart.rs` | Khoá `HKCU\Software\Microsoft\Windows\CurrentVersion\Run`. |
| `platform/appbar.rs` | Đăng ký app bar rỗng (`ABM_NEW`) để shell gửi `ABN_FULLSCREENAPP` khi có app/game toàn màn hình → ẩn notch. Thuần sự kiện, không cần hook foreground hay theo dõi vị trí cửa sổ. `SHQueryUserNotificationState` dùng khi khởi động. |
| `platform/sys.rs` | Đường dẫn `%APPDATA%`, offset múi giờ, working set, EcoQoS, logger file. |

---

## 2. Mô hình luồng (threading)

* **UI thread (STA)** — chủ của mọi trạng thái. Không có `Mutex` quanh `AppState`.
* **Media worker (MTA)** — giữ session SMTC, xử lý lệnh play/pause/next qua một `mpsc` riêng; các callback WinRT chỉ gửi `Refresh` vào kênh của worker.
* **Callback COM/WinRT** (audio, WMI, network) chạy trên thread pool của hệ thống; chúng **chỉ** `send(AppEvent)` rồi `PostMessageW(hwnd, WM_APP_EVENT)`.

`EventSink` (bin) gói `Sender<AppEvent>` + `HWND` → `Clone + Send`, là đường duy nhất đi vào UI thread.

## 3. Luồng dữ liệu

1. Provider phát `AppEvent` (vd. `Volume { level, muted }`).
2. UI thread nhận `WM_APP_EVENT`, rút hết kênh, gọi `state.apply(event, now)`.
3. Reducer cập nhật trạng thái & side-effect thuần (vd. bật OSD 1.5 s, thông báo "Đã kết nối tai nghe").
4. Nếu đổi kích thước pill → đặt target cho spring; nếu chỉ đổi nội dung → vẽ 1 frame.
5. `scheduler::next_deadline()` tính **một** thời điểm cần thức dậy tiếp theo (phút đồng hồ kế, hết giờ Pomodoro, nhắc việc, ẩn OSD, lưu file debounce, tick tiến độ nhạc khi đang mở rộng) → `SetCoalescableTimer` một lần (tolerance 500 ms cho deadline ≥ 5 s).

## 4. Cửa sổ notch

* `WS_POPUP` + `WS_EX_TOPMOST | WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE | WS_EX_NOREDIRECTIONBITMAP`.
* Kích thước cửa sổ = kích thước mở rộng tối đa; nội dung vẽ trên swapchain trong suốt.
* **Vùng nhận chuột** = `SetWindowRgn` theo khung pill hiện tại → phần trong suốt không chặn click của app phía dưới.
* Hover vào pill ≥ 160 ms (hover-intent, tránh mở nhầm khi rê chuột qua thanh tab trình duyệt) → mở rộng (spring); `WM_MOUSELEAVE` → thu gọn sau 380 ms.
* Click icon tray → mở notch 4 s; chuột phải → menu (Khởi động cùng Windows, Notch tối giản, Ngôn ngữ, Giới thiệu, Thoát).
* Ô nhập to-do: khi bấm vào, tạm bỏ `WS_EX_NOACTIVATE` và lấy focus để nhận `WM_CHAR`; mất focus/thu gọn → trả lại.
* `WM_DPICHANGED` / `WM_DISPLAYCHANGE` → đặt lại vị trí & kích thước swapchain.

### Trạng thái hiển thị (thu gọn), theo ưu tiên

1. OSD (âm lượng / độ sáng) — 1.5 s
2. Banner (nhắc việc, sạc pin, tai nghe, hết phiên Pomodoro) — 4 s
3. Pomodoro đang chạy — "Tập trung · 24p"
4. Nhạc đang phát — artwork nhỏ + biểu tượng sóng nhạc
5. Đồng hồ (hoặc pill trống nếu bật *Minimal notch*)

### Trạng thái mở rộng

Hàng đầu: giờ/ngày + trạng thái (Wi-Fi, tai nghe, âm lượng, pin). Thanh tab: **Nhạc · Tập trung · Việc cần làm**. Vùng nội dung theo tab.

## 5. Lưu trữ

| File | Nội dung |
|---|---|
| `%APPDATA%\Ailan\settings.toml` | ngôn ngữ, minimal, thời lượng Pomodoro, tab mặc định |
| `%APPDATA%\Ailan\data.json` | to-do, số phiên tập trung trong ngày |
| `%LOCALAPPDATA%\Ailan\logs\ailan.log` | log xoay vòng (cắt khi > 512 KB) |

* Ghi **nguyên tử**: ghi `*.tmp` → `rename` đè.
* **Debounce** 1 s sau thay đổi cuối.
* File hỏng → đổi tên `*.bak`, nạp mặc định, ghi log.
* Autostart lưu ở registry (nguồn sự thật), không trùng trong settings.

## 6. Xử lý lỗi

* Mỗi provider khởi tạo độc lập; lỗi → ghi log và **chỉ tắt widget đó** (vd. máy bàn không có WMI brightness).
* Device D3D bị mất (`DXGI_ERROR_DEVICE_REMOVED/RESET`) → tạo lại toàn bộ `Gfx`.
* `TaskbarCreated` (Explorer khởi động lại) → thêm lại icon tray.

## 7. Công cụ phát triển

* `ailan.exe --stats` — ghi frames / wakeups / working set mỗi 60 s vào log.
* `ailan.exe --expand=music|focus|todo` — mở sẵn notch ở tab chỉ định và giữ mở (để chụp màn hình / kiểm thử UI).

## 8. Mở rộng (roadmap)

Widget mới = thêm biến thể `AppEvent`, trường trong `AppState`, provider trong `platform/`, và một hàm vẽ trong `ui.rs`. Ứng viên: lời bài hát đồng bộ, kệ file (drag-drop), lịch sử clipboard, nhắc uống nước, lịch/Today, phản chiếu thông báo, installer (MSIX/Inno Setup), tự cập nhật.
