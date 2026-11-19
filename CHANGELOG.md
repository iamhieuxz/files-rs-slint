# Changelog

Mọi thay đổi đáng chú ý của dự án được ghi ở đây. Định dạng theo
[Keep a Changelog](https://keepachangelog.com/vi/1.1.0/), phiên bản theo
[Semantic Versioning](https://semver.org/lang/vi/).

## [Chưa phát hành]

### Thay đổi
- **Ảo hoá danh sách chi tiết**: dùng `ListView` của Slint thay cho `ScrollView`
  cộng vòng lặp, nên chỉ những hàng trong vùng nhìn thấy mới được dựng.
- **Dựng danh sách nhanh hơn 41×** khi chọn nhiều mục: phép kiểm tra `selection`
  đổi từ tìm tuyến tính cho từng hàng sang `HashSet` dựng một lần. Đo trên
  10.000 mục chọn tất cả: 8.24 ms → 0.20 ms.
- Thêm `CHANGELOG.md`.

### Thêm
- **Tự vừa cột theo nội dung** (`autofit-columns`).
- **Nhóm danh sách** theo tên, ngày sửa, ngày tạo, kích thước, loại hoặc thẻ màu;
  đơn vị gom ngày chọn được Năm/Tháng/Ngày.
- **Ba cách tách nhóm thư mục/tệp**: thư mục trước, tệp trước, sắp chung. Nút
  lặp qua ba trạng thái và được lưu lại.
- **Đảo lựa chọn** (`Ctrl+Shift+I`), chỉ áp dụng cho mục đang hiển thị.
- **Thumbnail trong lưới**: ảnh thật thay cho glyph, giải mã ở 96px để không nhoè.

## [2.1.0] — 2026-11-19

Bản này đưa cấu hình vào trong ứng dụng và sửa một loạt lỗi an toàn dữ liệu.

### Thêm
- **Cửa sổ Settings riêng**, mở bằng `Ctrl+,`, nút "Cài đặt" ở sidebar hoặc
  Command Palette. Sáu trang: Giao diện, Bố cục, Thư mục, Thẻ màu, Nâng cao,
  Giới thiệu. Cửa sổ có kích thước độc lập nên không làm đổi kích thước cửa sổ
  duyệt tệp.
- **Quản lý thẻ màu** trong trang Thẻ màu: đổi tên, đổi màu, xoá, thêm. Giữ
  nguyên `uid` nên tệp đã gắn thẻ không mất màu.
- **Trang Nâng cao** hiện đường dẫn `settings.json`, `tags.json` và tệp log;
  bấm để điều hướng tới thư mục chứa.
- **Kiểm tra khoảng trắng trong CI**: bước `Diff hygiene` quét tệp văn bản đã
  track, bỏ qua tệp nhị phân.
- `AGENTS.md`, `ROADMAP.md` và `docs/archive/` làm nguồn hướng dẫn và lộ trình.

### Sửa
- **Ghi `settings.json` và `tags.json` nguyên tử**: ghi qua tệp tạm rồi đổi tên,
  không còn nguy cơ để lại tệp cụt. Tệp hỏng được giữ lại dưới tên `.corrupt`
  thay vì bị ghi đè bằng mặc định.
- **Tệp JSON rỗng không còn bị coi là hỏng**: bước kiểm tra quyền ghi tạo tệp 0
  byte; trước đây nó bị đổi tên thành `.corrupt` ở mọi lần khởi động.
- **Chép/chuyển thư mục vào chính thư mục con của nó bị từ chối** (mã
  `into_self`) thay vì đệ quy và có thể mất dữ liệu.
- **`Replace` không còn xoá đích trước**: bản cũ được dời sang tên tạm, ghi bản
  mới, rồi mới xoá bản cũ; lỗi giữa chừng thì trả bản cũ về.
- **Fallback `rename` → copy chỉ khi khác volume**, không còn áp cho mọi lỗi.
- **Redo tạo đúng tên tệp**, không còn thêm `.txt` (`a.md` không thành `a.md.txt`).
- **Undo/redo thất bại không mất action**: action được trả về ngăn cũ để thử lại.
- **Đổi tên chỉ khác hoa/thường hoạt động** (`a.txt` → `A.txt`) qua tên tạm.
- **Tên quá 255 ký tự bị từ chối** với thông báo nêu rõ giới hạn.
- **`create_folder`/`create_file` từ chối tên chứa đường dẫn** và tên thiết bị
  DOS.
- **Archive**: zip ghi qua tệp `.partial` rồi đổi tên nên lỗi không để lại
  archive dở; giải nén không ghi đè tệp đã có; nén không đi theo junction.
- **Watcher không bỏ sót sự kiện**: sự kiện tới trong lúc đang tải được ghi nhớ
  và xếp lại sau. Đổi khung duyệt nạp lại khung vừa kích hoạt.
- **Timer thuộc về cửa sổ** và dừng khi đóng, không còn `mem::forget` giữ `App`
  và luồng nền sống mãi.
- **Panel xem trước không chặn UI**: giải mã ảnh, đọc văn bản và đếm số mục
  chuyển sang luồng nền.
- **Kết quả xoá vào Thùng rác đếm theo thực tế**, không còn báo bằng số đầu vào.
- **Theme đồng bộ giữa hai cửa sổ**: Settings không còn hiện nền sáng khi cửa sổ
  chính đang tối.
- **Bộ đếm phần tử danh sách chính xác** khi đang ở chế độ hai khung.

### Thay đổi
- Phiên bản assembly và `VERSIONINFO` của `Files RS.exe` là `2.1.0.0`.
- `README.md` cập nhật kiến trúc, danh sách tính năng, giới hạn đã biết và
  hướng dẫn gỡ đăng ký trình duyệt mặc định.

### Đã biết
- Duyệt và khôi phục từng mục trong Thùng rác **chưa hoạt động**. Đã thử năm
  đường Shell API (`SHParseDisplayName`, `SHGetFolderLocation`,
  `BindToObject`, `BindToHandler`, luồng STA riêng); tất cả trả `S_OK` nhưng
  không trả mục nào, trong khi `Shell.Application` thấy 42 mục. Chỉ làm rỗng
  Thùng rác là dùng được.
- Danh sách chưa ảo hoá: thư mục hàng chục nghìn mục vẫn dựng toàn bộ hàng.
- Chưa có gói cài đặt và chưa ký mã.

## [2.0.0] — 2026-11-18

### Thêm
- **Hai khung duyệt** độc lập, mỗi khung có danh sách và lịch sử riêng.
- **Kéo-thả nội bộ** giữa hai khung (di chuyển, `Ctrl` để sao chép) và **nhận
  tệp kéo từ Explorer** qua OLE `IDropTarget`.
- **Hoàn tác/làm lại** cho đổi tên, tạo mục và dán, giới hạn 50 bước.
- **Ba chính sách khi trùng tên** lúc dán: bỏ qua, giữ cả hai, thay thế.
- **Thẻ tiến trình nổi** cho thao tác dài, có huỷ giữa chừng ở ranh giới tệp.
- **Nhiều cửa sổ**, mỗi cửa sổ có tab, worker và watcher riêng.
- **Menu "Mở bằng"** lấy danh sách ứng dụng thật từ Shell.
- **Cột Git** đọc trạng thái working tree.
- **Thumbnail ảnh** qua WIC, giữ đúng tỉ lệ.
- **Tìm kiếm đệ quy** có giới hạn độ sâu, số kết quả và huỷ được.
- **Xoay ảnh** 90° qua WIC.
- **Ghim thư mục** vào sidebar, lưu qua các lần chạy.
- **Cột động**: kéo giãn, đổi thứ tự, lưu vào `settings.json`.
- **Thẻ màu** cho tệp và thư mục.
- **Menu ngữ cảnh** chuột phải cho mục và nền.
- **Accessibility**: nhãn cho nút, tab và ô nhập; kiểm tra độ tương phản.

### Sửa
- Không còn `unwrap`/`expect` trong mã sản xuất.
- Trạng thái rỗng, tự xoá thông báo sau 4 giây, gợi ý khi bị từ chối quyền.

## [1.0.0] — 2026-11-17

Bản đầu tiên.

### Thêm
- Giao diện Fluent 2 với token sáng/tối và 133 icon Fluent System Icons.
- Tab strip, command bar, omnibar (`Ctrl+L`), sidebar có thư mục mặc định và ổ đĩa.
- Xem chi tiết và lưới, sắp xếp theo tên/ngày/loại/kích thước, thư mục trước.
- Lọc nhanh (`Ctrl+F`), ẩn/hiện mục ẩn (`Ctrl+H`).
- Panel xem trước cho ảnh và văn bản, kèm bảng thuộc tính.
- Thao tác tệp: tạo thư mục/tệp, đổi tên (`F2`), sao chép/cắt/dán, xoá vào
  Thùng rác thật qua `SHFileOperationW`.
- Nén và giải nén ZIP, mã băm MD5/SHA-1/SHA-256, tính kích thước thư mục.
- Tự làm mới khi thư mục thay đổi, mọi lời gọi đĩa chạy trên luồng nền.
- Đóng gói: icon 9 kích thước, manifest DPI PerMonitorV2, long path aware.

[Chưa phát hành]: https://github.com/iamhieuxz/files-rs-slint/compare/v2.1.0...HEAD
[2.1.0]: https://github.com/iamhieuxz/files-rs-slint/releases/tag/v2.1.0
[2.0.0]: https://github.com/iamhieuxz/files-rs-slint/releases/tag/v2.0.0
[1.0.0]: https://github.com/iamhieuxz/files-rs-slint/releases/tag/v1.0.0
