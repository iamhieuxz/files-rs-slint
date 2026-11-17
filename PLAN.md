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

- `cargo test --workspace`: 45 test, 0 hỏng.
- `cargo build`: 0 warning.
- Chạy thật trên Windows 11: cửa sổ mở, liệt kê đúng thư mục Home, breadcrumb
  hiển thị `C: › Users › <tên>`.

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
