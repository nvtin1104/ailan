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

* `WS_POPUP` + `WS_EX_TOPMOST | WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE`. `WM_MOUSEACTIVATE` trả `MA_NOACTIVATE` (trừ lúc bấm ô chữ).
* HWND = cap **560×128 DIP**. Pill vẽ bên trong và đổi kích thước bằng spring + `SetWindowRgn`. Không `SetWindowPos` mỗi frame. Không low-level mouse hook.
* **Vùng nhận chuột** = `SetWindowRgn` bo tròn, thụt 1 px. Ngoài region, click xuống app phía dưới.
* Click chrome mới mở hoặc đóng. Rê chuột chỉ đổi màu hairline. Rời chuột không thu pill.
* Kéo sau `SM_CXDRAG` / `SM_CYDRAG`. Thả chuột: kẹp pill trong `rcWork`, dính mép trên nếu cách ≤ 24 DIP, lưu tên monitor + phân số work area.
* Mặc định: giữa mép trên `rcWork` của màn hình chính. Monitor đã lưu bị rút → về primary, top-center.
* Tray trái: bật/tắt hub. Chuột phải trên tray: menu. Island không mở menu.
* Ô to-do: `EDIT` con. Cùng click đó bỏ `WS_EX_NOACTIVATE`, `SetForegroundWindow`, rồi trả lại khi blur.
* `WM_DPICHANGED` / `WM_DISPLAYCHANGE`: nếu đang kéo thì chỉ đổi scale; không thì `place()` lại từ anchor.

### Trạng thái hiển thị (thu gọn), theo ưu tiên

1. OSD (âm lượng / độ sáng) — 1.5 s, cao 34, rộng đo chữ, kẹp 150–420
2. Banner (nhắc việc, sạc, tai nghe, báo thức, hết hẹn giờ) — một dòng, cùng khoảng rộng
3. Đồng hồ đếm ngược đang bật
4. Bấm giờ đang bật
5. Nhạc đang phát
6. Việc hôm nay chưa xong — tiến độ `xong/tổng`
7. Không có gì → capsule 150×32, nhãn "Ailan". Không mắt, không raw mouse.

Thêm việc từ Hub: một icon bay từ nút trong Hub tới tâm pill.

### Trạng thái mở rộng

Một cỡ **380×128** cho mọi trang. Trang: **Nhạc · Việc cần làm**. Không tab Tập trung.

### Hub

Cửa sổ app kéo viền được. Min 380×520 DIP, max min(1280×900, 90% work area). Dưới 640 DIP là dock; từ 640 là sidebar. Cỡ và trang được lưu.

## 5. Lưu trữ

| File | Nội dung |
|---|---|
| `%APPDATA%\Ailan\settings.toml` | ngôn ngữ, tab, giảm chuyển động, neo island (monitor + phân số), cỡ hub, trang hub |
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
