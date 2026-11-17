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
| Nén / giải nén ZIP | xong |
| Mã băm MD5/SHA-1/SHA-256 | xong |
| Tính kích thước thư mục | xong |
| Bảng thuộc tính trong preview pane | xong |
| Xoá vào Thùng rác hệ thống | xong |
| Chọn nhiều (Ctrl/Shift), chọn tất cả | xong |
| Menu ngữ cảnh chuột phải (mục + nền) | xong |
| Tự động làm mới khi thư mục đổi | xong |
| Chạy nền, không chặn UI | xong |
| Light/dark chuyển tại chỗ | xong |

## Kiểm chứng

- `cargo test --workspace`: 64 test, 0 hỏng.
- `cargo build`: 0 warning.
- Chạy thật trên Windows 11, đã thao tác bằng chuột thật:
  - mở tab mới bằng nút `+`, đóng tab bằng nút `×`;
  - Ctrl+click chọn rời (3 mục), Shift+click chọn khoảng (5 mục);
  - chuyển details ↔ grid, grid tự xuống dòng theo bề rộng;
  - bật/tắt dark mode;
  - preview ảnh (đọc đúng favicon 4.8 KB);
  - breadcrumb hiển thị `C: › Users › <tên>`;
  - bảng thuộc tính hiện ngay khi chọn mục.

## Bố cục bảng tệp

Cột có **bề rộng cố định** khai báo trong `theme.slint`:

| Cột | Rộng | Căn |
|---|---|---|
| Tên | 280px | trái, tên dài bị `elide` |
| Ngày sửa | 150px | trái |
| Loại | 120px | trái |
| Kích thước | 110px | phải |

Header và hàng dữ liệu cùng đọc các token này, nên một cái tên dài không thể
đẩy cột khác lệch đi — đây là lỗi đã gặp hai lần khi còn dùng `horizontal-stretch`.

## Chức năng đã bổ sung

| Chức năng | Cách làm |
|---|---|
| Nén ZIP | `zip` crate, giữ cấu trúc thư mục, có test round-trip |
| Giải nén ZIP | chặn zip-slip bằng `enclosed_name` |
| Mã băm MD5/SHA-1/SHA-256 | `md5` + `sha1` + `sha2`, đọc 64 KB mỗi lần |
| Kích thước thư mục | `directory_size` đệ quy, chạy ở luồng nền, điền vào cột |
| Bảng thuộc tính | tên, loại, kích thước, số mục, đường dẫn, ngày, thuộc tính |
| Command Palette | 29 lệnh, Ctrl+Shift+P, lọc theo tên, Enter chạy lệnh đầu |
| Mở lại tab đã đóng | ngăn xếp `closed_tabs`, Ctrl+Shift+T |
| Nhân bản tab | Ctrl+D |
| Chuyển tab | Ctrl+Tab / Ctrl+Shift+Tab, có vòng lặp |
| Đóng nhóm tab | đóng tab khác / bên trái / bên phải / tất cả |
| Xoá vĩnh viễn | `delete_permanently`, Shift+Delete, có hộp thoại cảnh báo |
| Sao chép đường dẫn | `copy_text_to_clipboard` qua Win32 clipboard |
| Mở Terminal | Windows Terminal `-d`, dự phòng `cmd /K cd` |
| Ẩn/hiện phần mở rộng | `display_name`, không làm rỗng tên `.gitignore` |
| Thư mục trước tệp | bật/tắt `folders_first`, sắp xếp lại tại chỗ |
| Kích thước thư mục tự động | luồng riêng `files-size`, token chống kết quả cũ, điền từng ô |
| Icon thật của Windows | `SHGetFileInfoW` + `GetDIBits`, cho exe/msi/lnk/pdf/văn phòng |

## Đóng gói như ứng dụng gốc

| Hạng mục | Cách làm | Kiểm chứng |
|---|---|---|
| Icon ứng dụng | `scripts/make-icon.cs` sinh `.ico` 9 kích thước (16→256), nhúng qua `winres` | 128×128 trích ra đúng, thư mục trắng trên nền xanh |
| Quyền administrator | `app/app.manifest` → `requestedExecutionLevel level="highestAvailable"` | chuỗi có trong `.exe` sau khi build |
| DPI | `PerMonitorV2` + `longPathAware` trong manifest | có trong `.exe` |
| Bỏ viền thừa | `no-frame: true` + title bar tự vẽ 36px | chụp màn hình: không còn khung Windows |
| 3 nút cửa sổ | thu nhỏ / phóng / đóng, 46×32, hover đỏ ở nút đóng | bấm được, icon đổi khi maximize |
| Kéo cửa sổ | `WM_NCLBUTTONDOWN` + `HTCAPTION` để Windows tự lo, kể cả snap | kéo được, double-click maximize |
| Trình duyệt tệp mặc định | Nút ⭐ trên status bar → hộp thoại xác nhận → `HKCU\Software\Classes\FilesRS.Folder` + `RegisteredApplications` → mở `ms-settings:defaultapps` | có hộp thoại xác nhận trước khi đổi; gỡ sạch bằng `unregister` |

**Về quyền administrator:** manifest dùng `highestAvailable` chứ không phải
`requireAdministrator`. Tài khoản thuộc nhóm Administrators sẽ được nâng quyền;
tài khoản thường vẫn chạy được thay vì bị chặn hoàn toàn. Đổi thành
`requireAdministrator` nếu bắt buộc — nhưng khi đó app không chạy nổi trên máy
bị hạn chế quyền.

**Về đặt làm mặc định:** Windows 10/11 không cho chương trình tự đặt mình làm
mặc định. Module `assoc` đăng ký ProgID và capabilities trong HKCU, sau đó mở
`ms-settings:defaultapps` để người dùng bấm chọn. Mọi cách lách đều bị Windows
hoàn nguyên ở lần đăng nhập sau, nên không làm.

## Icon

Hai tầng:

1. **Glyph Fluent** (143 icon) cho mọi loại tệp — nhẹ, vẽ tức thì.
2. **Icon thật của Windows** cho những định dạng thực sự có icon nhà phát hành:
   `exe`, `msi`, `lnk`, `pdf`, `docx`, `xlsx`, `pptx`, `zip`, media, ảnh.

Tầng 2 chạy ở luồng nền sau khi bảng đã hiện, nên cửa sổ không chờ. Chỉ những
đuôi này được hỏi Shell — một thư mục 5000 tệp `.txt` sẽ không giải mã 5000
bitmap. Test kiểm tra trên `notepad.exe` và `cmd.exe` thật: icon phải có pixel
nhìn thấy được, không phải một màu phẳng, và hai tệp khác nhau phải cho hai
bitmap khác nhau.


## Đối chiếu với Files gốc

Files có 130+ action. Những cái **đã có** ở đây: điều hướng, tab, copy/cut/paste,
xoá (thùng rác + vĩnh viễn), đổi tên, tạo mới, nén/giải nén, mã băm, kích thước
thư mục, sắp xếp, lọc, ẩn mục ẩn, ẩn phần mở rộng, sidebar, preview, theme,
command palette, sao chép đường dẫn, mở terminal.

Những cái **chưa làm** (cần dịch vụ ngoài hoặc phạm vi lớn): dual pane, kéo-thả,
tìm kiếm đệ quy, Git, FTP, cloud drive, tags, shelf, shortcut (.lnk), pin to
Start, format drive, recycle bin browser, rotate ảnh, set wallpaper.


## Sửa sau vòng kiểm thử đầu

| Lỗi | Nguyên nhân | Cách sửa |
|---|---|---|
| Nút `×` không đóng được tab | `TouchArea` của tab khai báo sau nút đóng nên nằm trên, nuốt click | Vùng bấm của tab chỉ phủ phần chữ (`width - 26px`) |
| Header lệch cột so với dữ liệu | Header dùng padding `s4` còn hàng dùng `s3`, và cột "Tên" không tính bề rộng icon | Chuyển sang cột rộng cố định dùng chung token |
| Tên dài đẩy lệch cột | Cột dùng `horizontal-stretch` nên co theo nội dung | Bề rộng cố định + `elide` |
| Grid chỉ hiện 1 hàng | `GridLayout` của Slint 1.19 không tự xuống dòng | Chia hàng trong Rust (`format::chunk_rows`) |
| Ctrl/Shift+click không chọn nhiều | Callback chỉ nhận `bool` và luôn truyền `false` | Đổi sang `int` mode (0/1/2), đọc `e.modifiers` |
| Chip clipboard không hiện | `set_clipboard` không cập nhật UI | Cập nhật `clipboard-mode`/`clipboard-count` + status bar |
| Watcher chồng nhiều lần đọc | `busy_now()` luôn trả `false` | Thêm cờ `load_in_flight`, đặt lại khi có kết quả |
| "Mở trong tab mới" không làm gì | Lệnh chưa có nhánh xử lý | Thêm nhánh tạo `Tab` mới |
| Bảng thuộc tính nằm đáy panel | `VerticalLayout` chia đều chiều cao | Bọc khối trong layout `alignment: start` |
| Bảng thuộc tính nằm đáy panel | `VerticalLayout` chia đều chiều cao | Bọc khối trong layout `alignment: start` |
| Binding loop ở bảng thuộc tính | Chiều cao suy ra từ `parent.height` | Chiều cao chỉ phụ thuộc số dòng |
| **App chết ngay cú click đầu** | `a.borrow_mut().show_hidden = !a.borrow().show_hidden` mượn mutable và immutable cùng lúc → `RefCell already borrowed` panic | Đọc ra biến trước rồi mới ghi |
| **Tạo/đổi tên không có tác dụng** | Ô nhập trong hộp thoại không được focus và binding một chiều, nên gõ không vào giá trị, nút "Tạo" luôn bị disable | `init => { self.focus(); }` + binding hai chiều `text <=> root.dialog-value`; cho phép ô trống với lệnh tự sinh tên |
| **Nút "Tạo" chết khi ô trống** | `primary-enabled` đòi `dialog-value != ""` | Thêm `dialog-allows-empty` cho lệnh tạo thư mục/tệp |
| **Icon chặn thao tác tệp** | Icon và tạo/đổi tên/xoá dùng chung một luồng, một icon chậm làm mọi lệnh sau đứng im | Tách `AuxJob` sang luồng `files-bg`; có test xếp 300 icon trước rồi kiểm tra lệnh tạo vẫn chạy |
| **App mở kèm cửa sổ console** | Thiếu `windows_subsystem` | `#![cfg_attr(windows, windows_subsystem = "windows")]` |
| **"Access is denied (os error 5)" khó hiểu** | Hiển thị nguyên văn lỗi Windows | `format::explain_error` dịch sang câu hành động được |
| **Đóng tab chưa đủ** | Chỉ có nút × và Ctrl+W; các lệnh đóng nhóm chỉ nằm trong Command Palette nên khó tìm | Menu chuột phải trên tab: Nhân bản, Đóng tab, Đóng các tab khác, Đóng tab bên phải/trái, Đóng tất cả |
| **× và menu đóng tab lệch hành vi** | Hai đường xử lý riêng biệt | Gộp về `close_tab_at`; phép toán chỉ số tách thành `close_index_result` với 7 test |
| **Vùng bấm tab chồng nhau** | Hai TouchArea chồng lên nhau (nút × và cả tab) | Một TouchArea phủ toàn tab, tự xác định vùng × theo `mouse-x` |

## Thẻ màu (Tags)

Lấy đúng bảng màu của Files (`FileTagsSettingsService.cs`):

| Thẻ | Màu |
|---|---|
| Home | `#0072BD` xanh dương |
| Work | `#D95319` cam |
| Photos | `#EDB120` vàng |
| Important | `#77AC30` xanh lá |

Cách dùng: chuột phải lên tệp hoặc thư mục → **Thẻ màu** → chọn thẻ. Icon đổi
sang màu của thẻ, nên một thư mục được gắn thẻ đọc ra ngay là "cái màu xanh".
Bấm lại đúng thẻ đó để bỏ. "Bỏ thẻ" xoá toàn bộ thẻ của mục.

Thẻ cũng nằm trong Command Palette (`Thẻ: Home`, `Thẻ: Work`…) để tìm bằng phím.

Lưu trữ: một tệp JSON gồm định nghĩa thẻ và bản đồ `đường dẫn → uid`.
Vị trí ưu tiên `%LOCALAPPDATA%\FilesRS\tags.json`; nếu profile bị chặn ghi thì
lùi về `files-rs-tags.json` cạnh file exe, giống cách logger làm.

Đường dẫn là danh tính (giống Files), nên đổi tên tệp sẽ mất thẻ. Khi mở một
thư mục, thẻ của những mục đã biến mất trong thư mục đó được dọn đi để tệp cấu
hình không phình ra và đường dẫn bị dùng lại không thừa hưởng màu cũ.

## Ghi log

App không có console nên `stderr` mất hút. Mọi sự kiện chẩn đoán ghi vào tệp,
chọn chỗ ghi đầu tiên hoạt động được:

1. `%LOCALAPPDATA%\FilesRS\log.txt`
2. `files-rs-log.txt` cạnh file exe (dự phòng khi profile bị chặn ghi)

Log ghi: khởi động, mỗi lần xác nhận hộp thoại (kèm thư mục đích), lỗi thao tác,
và mọi panic kèm vị trí. Tệp tự xoá khi vượt 1 MB.
| Kéo cửa sổ làm app đứng | `SendMessageW(WM_NCLBUTTONDOWN)` chạy vòng lặp modal ngay trong handler của Slint, và Slint không nhận `WM_LBUTTONUP` nên tưởng chuột vẫn nhấn | Dùng `winit::Window::drag_window()`, trả về ngay và giữ đúng trạng thái chuột |
| Nút cửa sổ không bấm được | Vùng kéo khai báo sau các nút nên nằm trên, nuốt click | Khai báo vùng kéo trước, và giới hạn bề rộng `parent.width - 138px` |



## Chưa làm (cố ý)

- Nhiều cửa sổ / dual pane.
- Kéo-thả giữa các panel.
- Sao chép/giữ nguyên thuộc tính NTFS phức tạp (ACL, alternate data stream).
- Thumbnail ảnh trong danh sách (chỉ có icon theo loại).
- Tags, Git, FTP, cloud drive.
- Hoàn tác (undo) sau khi xoá — dựa vào Thùng rác của Windows.

## Rủi ro đã biết

- `notify` không phát sự kiện trên một số ổ mạng; nút Refresh luôn có sẵn.
- `SHFileOperationW` chạy đồng bộ trên luồng nền: thư mục rất lớn sẽ lâu nhưng
  không treo UI.
- Bảng màu chép từ WinUI phải đảo alpha (`#RRGGBBAA`), đã ghi trong README.
- Tính kích thước thư mục đệ quy toàn bộ cây: thư mục vài trăm nghìn tệp sẽ
  chạy lâu, dù không chặn UI.
