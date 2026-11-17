# files-rs-slint — kế hoạch

Mục tiêu: trình quản lý tệp Windows dạng demo, giao diện **Fluent 2** theo
[Files](https://github.com/files-community/Files) (MIT) — cùng bộ token, cùng
kiểu control, cùng bố cục. Không sao chép mã C#.

## Trạng thái

| Hạng mục | Trạng thái |
|---|---|
| Design token Fluent 2 (light/dark) | xong |
| Bộ icon Fluent System Icons (133 glyph) | xong |
| Control: Button, IconButton, Tab, Sidebar, TextBox, Menu, Dialog | xong |
| Tab strip + đóng tab + tab mới | xong |
| Command bar (New/Cut/Copy/Paste/Rename/Delete/Sort/View/Hidden) | xong |
| Omnibar: breadcrumb + gõ đường dẫn (Ctrl+L) | xong |
| Sidebar: thư mục mặc định có icon riêng + ổ đĩa | xong |
| Details view + Grid view | xong |
| Sắp xếp 4 cột, thư mục trước | xong |
| Lọc nhanh (Ctrl+F) | xong |
| Ẩn/hiện mục ẩn, đọc thuộc tính Windows | xong |
| Preview pane (ảnh, văn bản) | xong |
| Thao tác tệp: new/rename/copy/cut/paste/delete | xong |
| Xoá vào Thùng rác hệ thống | xong |
| Chọn nhiều (Ctrl/Shift), chọn tất cả | xong |
| Menu ngữ cảnh chuột phải (mục + nền) | xong |
| Tự động làm mới khi thư mục đổi | xong |
| Chạy nền, không chặn UI | xong |
| Light/dark chuyển tại chỗ | xong |

## Kiểm chứng

- `cargo test --workspace`: 48 test, 0 hỏng.
- `cargo build`: 0 warning.
- Chạy thật trên Windows 11, đã thao tác bằng chuột thật:
  - mở tab mới bằng nút `+`, đóng tab bằng nút `×`;
  - Ctrl+click chọn rời (3 mục), Shift+click chọn khoảng (5 mục);
  - chuyển details ↔ grid, grid tự xuống dòng theo bề rộng;
  - bật/tắt dark mode;
  - preview ảnh (đọc đúng favicon 4.8 KB);
  - breadcrumb hiển thị `C: › Users › <tên>`.

## Sửa sau vòng kiểm thử đầu

| Lỗi | Nguyên nhân | Cách sửa |
|---|---|---|
| Nút `×` không đóng được tab | `TouchArea` của tab khai báo sau nút đóng nên nằm trên, nuốt click | Vùng bấm của tab chỉ phủ phần chữ (`width - 26px`) |
| Header lệch cột so với dữ liệu | Header dùng padding `s4` còn hàng dùng `s3`, và cột "Tên" không tính bề rộng icon | Khớp padding/spacing, cộng `16px + s2` cho cột tên |
| Grid chỉ hiện 1 hàng | `GridLayout` của Slint 1.19 không tự xuống dòng | Chia hàng trong Rust (`format::chunk_rows`), số cột tính từ bề rộng |
| Ctrl/Shift+click không chọn nhiều | Callback chỉ nhận `bool` và luôn truyền `false` | Đổi sang `int` mode (0/1/2), đọc `e.modifiers` trong `pointer-event` |
| Chip clipboard không hiện | `set_clipboard` không cập nhật UI | Cập nhật `clipboard-mode`/`clipboard-count` + status bar |
| Watcher có thể chồng nhiều lần đọc | `busy_now()` luôn trả `false` | Thêm cờ `load_in_flight`, đặt lại khi có kết quả |
| "Mở trong tab mới" không làm gì | Lệnh chưa có nhánh xử lý | Thêm nhánh tạo `Tab` mới |


## Chưa làm (cố ý)

- Nhiều cửa sổ / dual pane.
- Kéo-thả giữa các panel.
- Sao chép/giữ nguyên thuộc tính NTFS phức tạp (ACL, alternate data stream).
- Thumbnail ảnh trong danh sách (chỉ có icon theo loại).
- Tags, Git, FTP, nén/giải nén, cloud drive.
- Bảng thuộc tính đầy đủ (hiện dùng preview pane).
- Hoàn tác (undo) sau khi xoá — dựa vào Thùng rác của Windows.

## Rủi ro đã biết

- `notify` không phát sự kiện trên một số ổ mạng; nút Refresh luôn có sẵn.
- `SHFileOperationW` chạy đồng bộ trên luồng nền: thư mục rất lớn sẽ lâu nhưng
  không treo UI.
- Bảng màu chép từ WinUI phải đảo alpha (`#RRGGBBAA`), đã ghi trong README.
