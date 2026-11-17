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
```

## Kiến trúc

```
crates/
  files-core/   dữ liệu thuần: Entry, Listing, sắp xếp, breadcrumb, lỗi (không I/O)
  files-fs/     lớp duy nhất chạm OS: đọc thư mục, ổ đĩa, theo dõi, thao tác tệp
app/
  ui/theme.slint    token Fluent 2 (light/dark)
  ui/icons.slint    Fluent System Icons 24px (sinh tự động, 133 glyph)
  ui/widgets.slint  control Fluent: Button, IconButton, Tab, TextBox, Menu, Dialog
  ui/app.slint      bố cục: tab strip, command bar, omnibar, sidebar, preview, status
  src/main.rs       trạng thái, callback, hai bơm sự kiện (kết quả nền + watcher)
  src/history.rs    lịch sử back/forward (có test)
  src/format.rs     nhãn, icon, kích thước, breadcrumb (có test)
  src/ops_worker.rs luồng nền cho mọi lời gọi hệ thống tệp
```

Phụ thuộc một chiều: `app -> files-fs -> files-core`.

## Chức năng

**Điều hướng** — back/forward/up, breadcrumb, omnibar (Ctrl+L), tab (Ctrl+T/W,
đóng bằng dấu ×), sidebar Home/Desktop/Documents/Downloads/Pictures/Music/Videos
và các ổ đĩa.

**Xem** — chi tiết và lưới, sắp xếp theo tên/ngày sửa/loại/kích thước, thư mục
luôn trước, lọc nhanh (Ctrl+F), ẩn/hiện mục ẩn (Ctrl+H), panel xem trước cho ảnh
và văn bản, light/dark.

**Thao tác tệp** — thư mục mới, tệp mới, đổi tên (F2), sao chép/cắt/dán
(Ctrl+C/X/V), xoá vào **Thùng rác thật** qua `SHFileOperationW` + `FOF_ALLOWUNDO`
(Delete). Chọn nhiều bằng Ctrl/Shift+click.

**Tự động cập nhật** — `notify` theo dõi thư mục đang mở, danh sách tự làm mới
trong khoảng 400 ms.

**Không chặn UI** — mọi lời gọi đĩa chạy trên luồng nền; kết quả về qua kênh và
được bơm vào event loop bằng `Timer`.

## Ghi chú kỹ thuật

- Slint đọc màu 8 chữ số là `#RRGGBBAA`, không phải `#AARRGGBB` như WinUI. Bảng
  màu chép từ Fluent phải đảo thứ tự alpha.
- `alignment` cố định trên `HorizontalLayout` làm các con có `horizontal-stretch`
  co về 0. Chỉ dùng khi thật sự muốn bám một phía.
- Con của layout không tự quyết bề rộng, nên breadcrumb vẽ thành một `Text`
  duy nhất thay vì từng chip bấm được.
- Ẩn tệp dựa trên `FILE_ATTRIBUTE_HIDDEN`/`SYSTEM` (`GetFileAttributesW`), cộng
  thêm quy ước dấu chấm để test chạy được trên mọi nền.

## Giấy phép

MIT. Icon từ [Fluent System Icons](https://github.com/microsoft/fluentui-system-icons) (MIT).
