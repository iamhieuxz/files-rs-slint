# files-rs-slint

Trình quản lý tệp Windows viết bằng Rust + Slint, giao diện theo **Fluent 2** —
cùng hệ token, cùng kiểu control và cùng bố cục với
[Files](https://github.com/files-community/Files) (MIT).

## Chạy

```powershell
cargo run -p files-rs-app
```

Yêu cầu: Rust 1.75+, MSVC toolchain, Windows 10/11.

## Kiểm thử

```powershell
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
cargo fmt --all -- --check
```

Hiện có 249 test. Checklist thủ công trước phát hành: `docs/manual-test.md`.

## Kiến trúc

```
crates/
  files-core/   dữ liệu thuần: Entry, Listing, sắp xếp, breadcrumb, lỗi (không I/O)
  files-fs/     lớp duy nhất chạm OS: đọc thư mục, ổ đĩa, theo dõi, thao tác tệp,
                archive, search, thumbnail, preview data, Git status, Shell
app/
  ui/theme.slint    token Fluent 2 (light/dark)
  ui/icons.slint    Fluent System Icons 24px (sinh tự động, 133 glyph)
  ui/widgets.slint  control Fluent: Button, IconButton, Tab, TextBox, Menu, Dialog
  ui/app.slint      bố cục: tab strip, command bar, omnibar, sidebar, preview, status
  ui/dual_pane.slint khung phải của chế độ hai khung
  src/main.rs       khởi động, event loop, bộ dọn cửa sổ
  src/windows.rs    danh sách cửa sổ sống, mỗi cửa sổ giữ timer của nó
  src/events.rs     bốn bơm sự kiện (kết quả nền, tiến trình, watcher, status)
  src/ops_worker.rs luồng nền cho thao tác tệp và việc trang trí danh sách
  src/persist.rs    ghi settings/tags nguyên tử, quarantine tệp hỏng
  src/undo.rs       ngăn xếp hoàn tác/làm lại
  src/panes.rs      chuyển khung, bật/tắt hai khung, panel xem trước
  src/drag.rs       kéo-thả nội bộ; native_drop.rs nhận tệp từ Explorer
```

Phụ thuộc một chiều: `app -> files-fs -> files-core`.

## Chức năng

**Điều hướng** — back/forward/up, breadcrumb, omnibar (Ctrl+L), tab (Ctrl+T/W,
đóng bằng dấu ×, nhân bản, đóng nhóm, mở lại tab đã đóng), sidebar ghim được
Home/Desktop/Documents/Downloads/Pictures/Music/Videos và các ổ đĩa.

**Xem** — chi tiết và lưới, hai khung cạnh nhau, sắp xếp theo tên/ngày sửa/loại/
kích thước, thư mục luôn trước, lọc nhanh (Ctrl+F), tìm đệ quy (Ctrl+Shift+F),
ẩn/hiện mục ẩn (Ctrl+H), cột động kéo giãn và đổi thứ tự, panel xem trước cho ảnh
và văn bản, cột trạng thái Git, light/dark.

**Thao tác tệp** — thư mục mới, tệp mới, đổi tên (F2), sao chép/cắt/dán
(Ctrl+C/X/V) với ba chính sách khi trùng tên (bỏ qua / giữ cả hai / thay thế),
xoá vào **Thùng rác thật** qua `SHFileOperationW` + `FOF_ALLOWUNDO` (Delete),
xoá vĩnh viễn (Shift+Delete), hoàn tác/làm lại (Ctrl+Z / Ctrl+Y), nén và giải nén
ZIP, mã băm MD5/SHA-1/SHA-256, kích thước thư mục, xoay ảnh, kéo-thả nội bộ và
kéo tệp từ Explorer, thẻ màu, mở bằng ứng dụng khác.

**Tự động cập nhật** — `notify` theo dõi thư mục đang mở, danh sách tự làm mới
trong khoảng 400 ms.

**Không chặn UI** — lời gọi đĩa, giải mã ảnh, đọc văn bản xem trước và đếm số mục
đều chạy trên luồng nền; kết quả về qua kênh và được bơm vào event loop bằng
`Timer`. Timer thuộc về cửa sổ và dừng khi cửa sổ đóng.

## Ghi chú kỹ thuật

- Slint đọc màu 8 chữ số là `#RRGGBBAA`, không phải `#AARRGGBB` như WinUI. Bảng
  màu chép từ Fluent phải đảo thứ tự alpha.
- `alignment` cố định trên `HorizontalLayout` làm các con có `horizontal-stretch`
  co về 0. Chỉ dùng khi thật sự muốn bám một phía.
- Con của layout không tự quyết bề rộng, nên breadcrumb vẽ thành một `Text`
  duy nhất thay vì từng chip bấm được.
- Ẩn tệp dựa trên `FILE_ATTRIBUTE_HIDDEN`/`SYSTEM` (`GetFileAttributesW`), cộng
  thêm quy ước dấu chấm để test chạy được trên mọi nền.

## Giới hạn đã biết

- `notify` không phát sự kiện trên một số ổ mạng; nút Refresh luôn có sẵn.
- `SHFileOperationW` chạy đồng bộ trên luồng nền: thư mục rất lớn sẽ lâu nhưng
  không treo UI.
- Thùng rác chỉ làm rỗng được; duyệt và khôi phục từng mục chưa có.
- Chưa có trang Settings trong UI; cấu hình sửa qua `settings.json`.
- Danh sách chưa ảo hoá: thư mục hàng chục nghìn mục vẫn dựng toàn bộ row.
- Không sao chép ACL hay alternate data stream khi copy/move.
- `cargo fmt --check` và clippy phải xanh trước khi commit; xem `AGENTS.md`.

## Gỡ đăng ký trình duyệt mặc định

`unregister` trong `files-fs/src/assoc.rs` xoá đúng bốn khoá dưới `HKCU` và một
giá trị trong `RegisteredApplications`. Chạy tương đương bằng PowerShell:

```powershell
$keys = @(
  'HKCU:\Software\Classes\FilesRS.Folder',
  'HKCU:\Software\Classes\FilesRS.Drive',
  'HKCU:\Software\Classes\Applications\FilesRS.exe',
  'HKCU:\Software\FilesRS'
)
foreach ($k in $keys) {
  if (Test-Path $k) { Remove-Item -Recurse -Force $k }
}
Remove-ItemProperty -Path 'HKCU:\Software\RegisteredApplications' -Name 'FilesRS' -ErrorAction SilentlyContinue
```

Sau đó mở `ms-settings:defaultapps` để Windows cập nhật danh sách. Không cần
khởi động lại máy.

## Lộ trình

Xem [`ROADMAP.md`](ROADMAP.md) cho danh sách chức năng còn thiếu so với
[Files](https://github.com/files-community/Files). Các kế hoạch đã hoàn thành
nằm trong [`docs/archive/`](docs/archive/).

## Giấy phép

MIT. Icon từ [Fluent System Icons](https://github.com/microsoft/fluentui-system-icons) (MIT).
