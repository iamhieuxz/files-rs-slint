# files-rs-slint — kế hoạch bổ sung tính năng và cải tiến

Tài liệu này đối chiếu bản Rust (`files-rs-slint`) với bản gốc
(`files-community-files`, WinUI/C#) và liệt kê việc cần làm theo thứ tự.
Mỗi hạng mục có **mức độ khó**:

- **D** (dễ): dưới nửa ngày, chỉ chạm một vài file.
- **T** (trung bình): 1–3 ngày, cần thêm model + UI + test.
- **K** (khó): 1–2 tuần, có thay đổi kiến trúc hoặc Win32/COM phức tạp.
- **RK** (rất khó): nhiều tuần, cần thiết kế lại một phần.

## 0. Trạng thái đã kiểm chứng

Các điểm dưới đây đã đối chiếu với code và chạy thử ngày 18/11/2026.
Một số ghi chú trong `PLAN.md` cũ đã lỗi thời.

| Hạng mục | Trạng thái thực tế | Ghi chú |
|---|---|---|
| Dual pane | Có | `app/src/panes.rs`, `toggle-dual-pane` |
| Ghim thư mục vào sidebar | Có | `settings.pinned_folders`, `toggle_pin` |
| Kéo-thả | Có (một phần) | `drag.rs`, `native_drop.rs`, drop lên sidebar |
| Lịch sử Undo/Redo | Có, giới hạn 50 | `undo.rs`, `History::with_capacity(50)` |
| Git | Chỉ đọc trạng thái | `git status`, không có clone/pull/push |
| Duyệt Thùng rác từng mục | Chưa có | `can_browse()`/`can_restore()` trả `false` |
| Làm rỗng Thùng rác | Có | `SHEmptyRecycleBinW` |
| Trang Settings trong UI | Chưa có | Chỉ có `settings.json`, không có màn hình |
| Breadcrumb mục con | Chưa có | `wiring.rs:913`, `expandable: false` |
| Phím Alt+Enter (thuộc tính) | Chưa có | Không có phím Alt trong `app.slint` |
| Sắp xếp lại tab bằng kéo | Chưa có | Không tìm thấy logic reorder |
| Chọn đảo (invert) | Chưa có | Không tìm thấy trong `selection.rs` |
| Info pane / compact overlay | Chưa có | |
| Cloud, FTP, Network | Chưa có | |
| Số test | 228 test | theo `REFACTOR_PLAN.md` |

## 1. Thứ tự thực hiện

Làm theo từng giai đoạn. Mỗi giai đoạn kết thúc bằng test và chạy thử thật.

| Giai đoạn | Nội dung | Mức độ tổng |
|---|---|---|
| G1 | Trang Settings (đọc/ghi các trường có sẵn) | T |
| G2 | Thao tác chọn và sắp xếp nhanh (invert, auto-fit, folders-first mở rộng) | D |
| G3 | Alt+Enter thuộc tính, breadcrumb mục con | T |
| G4 | Sắp xếp lại tab bằng kéo, tab kéo ra cửa sổ mới | T |
| G5 | Trình duyệt Thùng rác (xem và khôi phục từng mục) | K |
| G6 | Accessibility (UIA) | T |
| G7 | Trang Tags và Actions/toolbar | T |
| G8 | Info pane, compact overlay | T |
| G9 | Git nâng cao (pull/push/fetch/clone) | K |
| G10 | Hiệu năng danh sách lớn, ảo hoá | K |
| G11 | Tách backend khỏi UI, đóng gói và ký | RK |

## 2. Chi tiết từng hạng mục

### G1. Trang Settings — T

**Thiếu so với gốc:** gốc có 9 trang (`SettingsPageKind.cs`: General,
Appearance, Layout, Folders, Actions, Tags, DevTools, Advanced, About). Rust
không có màn hình nào, người dùng phải sửa `settings.json` bằng tay.

**Phương án:**
1. Thêm `view` mới `settings` vào `app.slint`, mở bằng Ctrl+, và từ menu.
2. Trang 1 (Appearance): nền sáng/tối, kiểu xem mặc định. Gọi lại
   `view::push_chrome` sau khi đổi.
3. Trang 2 (Layout): sidebar, panel xem trước mở sẵn khi khởi động.
4. Trang 3 (Folders): folders first, hiện mục ẩn, hiện phần mở rộng.
5. Trang 4 (About): phiên bản, đường dẫn log và settings (`logging.rs`).
6. Mỗi thay đổi ghi qua `Settings::save`, đã có sẵn.

**Test:** round-trip mỗi trường (đã có một phần trong `settings.rs`), và test
rằng đổi giá trị trong UI được phản ánh ngay vào `App`.

**Rủi ro:** `save` ghi đè file khi hai cửa sổ cùng sửa. Đã có
`with_ui_from`/`with_pins_from` để giữ thay đổi của cửa sổ khác; cần kiểm lại
khi thêm trang Settings.

### G2. Chọn và sắp xếp nhanh — D

**Thiếu:** Invert selection, nhóm theo (group by), auto-fit cột, sắp xếp tệp
lên trước (gốc có `SortFilesFirstAction`, `SortFilesAndFoldersTogetherAction`).

**Phương án:**
- Invert: thêm `selection::invert` (chỉ đổi cờ chọn, không đụng dữ liệu),
  gắn vào palette và phím tắt.
- Auto-fit: đo độ rộng chữ dài nhất trong cột đang hiển thị, kẹp theo
  `column_bounds` đã có trong `settings.rs`.
- Sort kiểu "files first" và "together": thêm biến thể vào `sort` hiện có.

**Test:** invert trên danh sách rỗng, một mục, toàn bộ đã chọn; auto-fit không
vượt giới hạn; thứ tự sort ổn định.

### G3. Alt+Enter thuộc tính và breadcrumb mục con — T

**Thiếu:**
- Alt+Enter chưa mở thuộc tính (gốc: `OpenPropertiesAction`). Bản Rust đã có
  bảng thuộc tính trong preview pane, nên thiếu là phím tắt và cửa sổ riêng.
- Breadcrumb không bấm mũi tên để chọn thư mục con (gốc có). Hiện
  `expandable: false` ở `wiring.rs:913`.

**Phương án:**
- Alt+Enter: thêm phím trong `app.slint` gọi `show_properties`. Có thể mở
  panel thuộc tính thay vì cửa sổ mới ở giai đoạn đầu.
- Breadcrumb: đọc danh sách thư mục con (đã có `read_dir` trong `files-fs`),
  hiện popup khi bấm mũi tên. Đặt `expandable: true` cho mục có con.

**Test:** Alt+Enter khi không chọn gì (hiện thuộc tính thư mục hiện tại), khi
chọn một mục, khi chọn nhiều mục.

**Rủi ro:** đọc thư mục con trên ổ mạng có thể chậm; chạy nền và có trạng thái
"đang tải".

### G4. Tab: sắp xếp lại bằng kéo, kéo ra cửa sổ mới — T

**Thiếu:** không kéo được tab để đổi vị trí; không kéo tab ra thành cửa sổ mới
(gốc có `OpenInNewWindowAction`, drag tab).

**Phương án:**
1. Kéo trong dải tab: theo dõi vị trí chuột, đổi thứ tự `tabs` trong `App`,
   đồng bộ với `sync_tabs` đã có.
2. Kéo ra ngoài dải tab: gọi `open_new_window` với đường dẫn tab, rồi đóng tab
   ở cửa sổ cũ.
3. Dùng lại cơ chế drag có sẵn trong `drag.rs` để không viết lại.

**Test:** đổi thứ tự không làm mất tab đang chọn; kéo tab ra không làm mất dữ
liệu (tab đóng sau khi cửa sổ mới mở thành công).

### G5. Trình duyệt Thùng rác (xem, khôi phục) — K

**Thiếu:** chỉ làm rỗng được Thùng rác. Không liệt kê, không khôi phục từng
mục (gốc có `RestoreRecycleBinAction`, `RestoreAllRecycleBinAction`).

**Phương án:**
1. Liệt kê: dùng `SHGetKnownFolderPath(FOLDERID_RecycleBinFolder)` hoặc
   `IShellFolder` duyệt `CSIDL_BITBUCKET`. Lưu ý ghi trong `REFACTOR_PLAN.md`:
   `SHGetKnownFolderPath` trả `E_FAIL` trong cách gọi hiện tại, nên cần
   `SHGetDesktopFolder` + `IShellFolder::EnumObjects`.
2. Khôi phục: lấy verb `undelete` qua `IContextMenu` của từng mục, gọi
   `InvokeCommand`. Đây là cách Windows dùng nội bộ, không tự đọc file `$I`.
3. Hiện danh sách với tên gốc, ngày xoá, kích thước (lấy từ `IShellFolder`
   column).
4. Bật `can_browse()`/`can_restore()` chỉ khi đã có các bước trên.

**Test:** tạo tệp thử, xoá vào Thùng rác, kiểm tra xuất hiện trong danh sách,
khôi phục, kiểm tra tệp trở lại đúng vị trí. Test này chạy trên máy Windows
thật, không phải CI không có Thùng rác.

**Rủi ro:** COM và `IContextMenu` dễ rò bộ nhớ và treo; cần giải phóng đúng
thứ tự và chạy trên luồng STA.

### G6. Accessibility — T

**Thiếu:** không có thuộc tính `accessible` nào trong `app/ui` (kiểm tra
ngày 18/11/2026). Narrator không đọc được danh sách tệp, tab, và trạng thái
chọn.

**Phương án:**
1. Gán tên và vai trò cho nút command bar, tab, mục danh sách (Slint hỗ trợ
   `accessible-role`, `accessible-label`).
2. Trạng thái chọn và đã mở đọc được qua `accessible-checked` hoặc `accessible-description`.
3. Kiểm tra bằng Narrator (Win+Ctrl+Enter) và Accessibility Insights.

**Test:** ghi lại kịch bản Narrator đọc được tên và trạng thái các nút chính.
Đây là kiểm tra thủ công.

### G7. Trang Tags và Actions/toolbar — T

**Thiếu:**
- Tags: hiện bốn thẻ cố định trong code. Gốc có trang quản lý (đổi tên, đổi
  màu, xoá thẻ).
- Actions/toolbar: không tuỳ biến được command bar (gốc có `CustomizeToolbarAction`).

**Phương án:**
- Tags: đưa định nghĩa thẻ vào `tags.json` (đã có cấu trúc JSON). Trang Settings
  liệt kê, cho đổi tên và màu. Không đổi khoá nội bộ để giữ liên kết với tệp
  đã gắn thẻ.
- Toolbar: lưu danh sách nút ẩn trong `settings.json`. Command bar lọc theo
  danh sách này.

**Test:** đổi tên thẻ không làm mất gắn thẻ; ẩn nút rồi khởi động lại vẫn ẩn.

### G8. Info pane và compact overlay — T

**Thiếu:** panel thông tin (gốc có `ToggleInfoPaneAction`, `ToggleDetailsPaneAction`),
chế độ compact (`EnterCompactOverlayAction`, `ToggleCompactOverlayAction`).

**Phương án:**
- Info pane: tái sử dụng bảng thuộc tính đã có trong preview pane, thêm một
  vùng cố định bên phải. Lưu trạng thái vào settings.
- Compact overlay: dùng `winit` đặt cửa sổ luôn trên cùng và thu nhỏ, đã có
  nền tảng `window.rs`.

**Test:** đổi trạng thái panel được lưu qua khởi động lại; compact không làm
mất tab.

### G9. Git nâng cao — K

**Thiếu:** hiện chỉ có `git status` (chỉ đọc). Gốc có clone, init, pull, push,
fetch, sync (`GitCloneAction`, `GitPullAction`, `GitPushAction`, ...).

**Phương án:**
1. Dùng `git` CLI qua `std::process::Command`, không nhúng thư viện git. Đây là
   cách đơn giản và đúng với hành vi người dùng.
2. Chạy trong luồng nền, hiện tiến trình, ghi stderr vào thông báo.
3. Pull, push, fetch, sync đều hỏi xác nhận trước (push là thao tác không
   hoàn tác được).
4. Clone: hộp thoại nhập URL và đường dẫn đích.

**Test:** tạo repo thử trong thư mục tạm, chạy từng lệnh, kiểm tra kết quả và
lỗi (mất mạng, xung đột).

**Rủi ro:** `git` có thể không được cài; cần thông báo rõ thay vì lỗi chung.

### G10. Hiệu năng danh sách lớn — K

**Chưa đo:** thời gian dựng UI với 10.000 dòng trong model Slint. Các số đo
trước đó chưa được kiểm chứng.

**Phương án:**
1. Đo trước: tạo thư mục 10.000 tệp, ghi thời gian mở, cuộn, sắp xếp.
2. Nếu chậm: dùng `ListView` ảo hoá (Slint hỗ trợ) thay vì render toàn bộ.
3. Đo sau khi sửa, ghi số liệu vào tài liệu này.

**Test:** benchmark có số liệu cụ thể, chạy được trên máy thường.

### G11. Tách backend, đóng gói và ký — RK

**Thiếu:**
- Chưa ký mã (SmartScreen cảnh báo). Chưa có installer.
- Logic nằm chung với UI (`app/src`), khó thêm chế độ kiểm thử không giao diện.
  Gốc tách `Files.Storage`, `Files.App.Server`.

**Phương án:**
1. Ký mã bằng chứng chỉ code signing. Cần mua chứng chỉ, đây là quyết định của
   người dùng.
2. Installer: MSIX hoặc Inno Setup.
3. Tách: tiếp tục đẩy logic vào `files-fs` (đã có crate riêng), để `app` chỉ
   còn kết nối UI.

**Quyết định cần người dùng:** có ký mã không, có phát hành MSIX không.

## 3. Việc không làm trong giai đoạn này

- Cloud drive, FTP, Network: cần dịch vụ ngoài, để sau.
- Đặt hình nền, set lockscreen: phụ thuộc Win32 riêng, ưu tiên thấp.
- Format drive, cài font/chứng chỉ/INF, Storage Sense: rủi ro cao, cần quyền
  quản trị. Cân nhắc lại sau khi có ký mã.
- Thumbnail ảnh trong danh sách: chưa xác minh là có hay không. Cần kiểm tra
  trước khi lên kế hoạch.

## 4. Cách theo dõi tiến độ

Mỗi hạng mục đánh dấu `[ ]` khi bắt đầu và `[x]` khi xong. Hoàn thành phải có:
test pass, `cargo clippy` không cảnh báo, và một ảnh chụp chạy thật trong
`docs/` hoặc ghi chú trong bảng dưới.

| Ngày | Hạng mục | Kết quả |
|---|---|---|
| | | |
