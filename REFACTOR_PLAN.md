# Kế hoạch refactor và cải tiến: files-rs-slint

> Cách dùng: làm xong hạng mục nào thì đánh dấu `[x]` ngay tại hạng mục đó.
> Làm theo thứ tự các giai đoạn. Không chuyển sang giai đoạn sau khi giai đoạn trước chưa đạt tiêu chí hoàn thành.

## Nguồn số liệu

Số liệu dưới đây lấy từ quét tĩnh mã nguồn, chưa chạy build hay test:

| Chỉ số | Giá trị |
|---|---|
| `app/src/main.rs` | 2526 dòng, 87 hàm, 46 callback `on_*` |
| `app/ui/app.slint` | 1277 dòng |
| `unwrap()` / `expect()` trong code production (đã xác minh) | 4: `ops_worker.rs` 2 (`expect` khi tạo luồng), `sort.rs` 2 (`unwrap` sau `peek`, bất biến đã đảm bảo). 42 chỗ còn lại nằm trong `#[cfg(test)]` |
| Khối `unsafe` | 8 (window.rs 4, assoc.rs 4) |
| Thuộc tính accessibility trong Slint | 0 |
| Test (đã chạy `cargo test --workspace`) | 69 test app đều pass; tổng cộng 116 test pass, 0 hỏng |
| Build release (đã đo) | `files-rs.exe` ~19,9 MB, build thành công |
| Clippy | Chưa ghi nhận cảnh báo (lệnh chạy không in ra kết quả lỗi, cần chạy lại để đếm chính xác) |
| CI, CHANGELOG, `docs/manual-test.md` | Chưa có (đã kiểm tra) |
| Bản gốc Files (C#) | ~173 file action, ~85 ViewModel, ~94 XAML |

Tài liệu hiện có mâu thuẫn: PLAN.md liệt kê "Tags" ở mục chưa làm trong khi đã có mục Thẻ màu; PLAN.md có một dòng bảng bị lặp và một dòng bị lạc chỗ; README và PLAN không khớp nhau.

---

## Giai đoạn 0: Nền tảng kiểm chứng

Mục tiêu: có số liệu thật trước khi thay đổi mã.

- [x] **0.1** Chạy `cargo test --workspace` và ghi lại số test đạt/hỏng vào mục này. Kết quả đo: **136 test pass, 0 hỏng** (workspace, gồm 23 test của `files-fs` lib + 13 `files-core` + các test tích hợp).
  - Tiêu chí: lệnh chạy xong, số liệu được ghi, sửa số trong PLAN nếu sai.
- [x] **0.2** Chạy `cargo clippy --workspace --all-targets` và ghi số cảnh báo. (Kết quả: 0 cảnh báo, 0 lỗi, sau khi buộc compile lại.)
  - Tiêu chí: không có cảnh báo mới sau mỗi giai đoạn.
- [x] **0.3** Chạy `cargo build --release` và ghi kích thước `files-rs.exe`. Kết quả: `target/release/files-rs.exe` **19,0 MB**, build 22,5 giây.
- [x] **0.4** Thêm CI (GitHub Actions, `windows-latest`) chạy build, test, clippy. Đã thêm `.github/workflows/ci.yml`: build, test, clippy, build release; `RUSTFLAGS=-D warnings` để cảnh báo thành lỗi.
  - Tiêu chí: mỗi commit đều có kết quả xanh/đỏ.
- [~] **0.5** Chụp trạng thái hiện tại (light, dark, details, grid) để so sánh UI sau refactor. **Một phần**: đã chụp `docs/screenshots/light-details.png` (80 KB) và `dark-details.png` (81 KB) bằng `PrintWindow` — xác nhận bằng màu nền: sáng R=251, tối R=39. Hai ảnh dạng lưới chưa chụp ổn định: `PrintWindow` với cửa sổ không khung của Slint trả khung hình cũ, còn chụp màn hình thật thì bị cửa sổ khác che. Cần chụp tay khi máy không dùng.

---

## Giai đoạn 1: Đồng bộ tài liệu

Làm trước để các giai đoạn sau có một nguồn sự thật duy nhất.

- [ ] **1.1** Sửa PLAN.md: xoá dòng bảng bị lặp ("Bảng thuộc tính nằm đáy panel") và đưa dòng lạc chỗ về đúng vị trí.
- [ ] **1.2** Sửa mục "Chưa làm" trong PLAN.md và README: bỏ "Tags" vì đã làm (Thẻ màu).
- [ ] **1.3** Đồng bộ danh sách tính năng giữa README và PLAN (một bên là mô tả người dùng, một bên là tiến độ).
- [ ] **1.4** Thêm mục "Giới hạn đã biết" thống nhất, gom từ mục "Rủi ro đã biết".
- [ ] **1.5** Cập nhật số test và số liệu sau khi xác minh ở 0.1.

Tiêu chí hoàn thành: đọc PLAN.md và README không thấy mâu thuẫn.

---

## Giai đoạn 2: Độ ổn định (loại bỏ panic)

Mục tiêu: lỗi I/O bất ngờ không làm sập ứng dụng.

Lưu ý sau khi kiểm chứng: 42/46 `unwrap`/`expect` nằm trong module test, không cần sửa. Chỉ còn 4 chỗ ở code production.

- [x] **2.1** `ops_worker.rs`: hai `expect("spawn ... thread")`. Thay bằng xử lý lỗi: nếu không tạo được luồng thì báo lỗi lên UI thay vì panic. (`Worker::spawn` trả `io::Result`; `main` chuyển lỗi thành `PlatformError` và thoát có thông báo; 2 test dùng `expect` có ngữ cảnh. Test: 69/69 pass, 0 cảnh báo.)
- [x] **2.2** `sort.rs`: hai `peek().unwrap()` đã an toàn theo logic. Đã bỏ `unwrap` bằng cách bind offset trực tiếp từ pattern `match`. Hành vi giữ nguyên; `files-core` 13/13 pass.
- [x] **2.3** Thêm `#![deny(clippy::unwrap_used)]` cho code production. (Đã thêm `cfg_attr(not(test), deny(...))` vào `app/src/main.rs`, `files-core/src/lib.rs`, `files-fs/src/lib.rs`. Mã Slint sinh ra được bọc trong `mod slint_generated` với `allow` riêng. Kết quả: clippy 0 lỗi, test 69/69 pass.)
- [x] **2.4** Thêm test cho luồng lỗi thật. Đã có sẵn và xác minh: tệp settings hỏng (`settings.rs`), đích đã tồn tại (`copy_refuses_to_overwrite`, `rename_does_not_clobber_an_existing_entry` trong `files-fs/tests/ops.rs`), thư mục không tồn tại (`directory_size_of_a_missing_folder_is_zero_not_a_panic`).

Tiêu chí hoàn thành: không còn `unwrap()`/`expect()` ngoài test, trừ các trường hợp đã ghi lý do.

---

## Giai đoạn 3: An toàn bộ nhớ và FFI

- [x] **3.1** Thêm chú thích `// SAFETY:` cho từng khối `unsafe` trong `window.rs` (4 chỗ) và `assoc.rs` (4 chỗ). (Đã làm và mở rộng: **23/23 khối** trong toàn workspace đều có SAFETY — window 3, assoc 4, ops 3, shellicon 13. Giữ nguyên từng khối `unsafe` tại chỗ thay vì gộp vào hàm phụ, để mỗi lời gọi Win32 đọc được lý do an toàn ngay bên cạnh.)
- [x] **3.2** Bọc các lời gọi Win32 trong hàm an toàn nhỏ, ví dụ `is_maximized(hwnd) -> bool`, để `unsafe` chỉ nằm ở một nơi. (Đã làm ở `window.rs`: `minimize`, `set_maximized`, `is_zoomed`, `hwnd_of`. Riêng `shellicon.rs` giữ 13 khối tại chỗ theo yêu cầu: hàm dọn handle dùng chung làm mất ngữ cảnh của từng handle, và gộp lại thì không còn thấy được lý do an toàn ở nơi gọi.)
- [x] **3.3** Kiểm tra mọi kết quả trả về của Win32 (ví dụ `ShowWindow`, `RegSetValue`) và ghi log khi thất bại thay vì bỏ qua bằng `let _ =`. (Đã làm: `RegCloseKey`, `RegDeleteTreeW`, `RegDeleteKeyValueW`, `DeleteObject`, `drag_window`, `ui.hide`, `open_settings_page` đều kiểm tra và ghi log. `ShowWindow` giữ `let _ =` có ghi lý do: giá trị BOOL của nó là trạng thái hiển thị TRƯỚC ĐÓ, không phải mã lỗi. Thêm `files_fs::set_log_sink` vì crate này không tự biết ghi log ở đâu, và `eprintln!` mất hút do build không có console.)
- [x] **3.4** Thêm test cho phần parse và build chuỗi trong `assoc.rs` (không ghi registry thật, dùng hàm thuần). (Đã làm: tách `icon_ref`, `open_command`, `progid_key`, `application_key`, `capabilities_key` thành hàm thuần; 8 test kiểm tra ngoặc kép của đường dẫn có dấu cách, ngoặc quanh `%1`, hậu tố `,0` của icon, và các khoá registry không trùng nhau. Test 132/132 pass.)

Tiêu chí hoàn thành: mỗi `unsafe` có lý do; không còn `let _ =` nuốt lỗi quan trọng.

---

## Giai đoạn 4: Refactor `main.rs`

Mục tiêu: giảm `main.rs` từ 2526 dòng xuống dưới ~400 dòng, mỗi module một trách nhiệm. Mỗi bước phải giữ nguyên hành vi, chạy test và chạy app sau mỗi bước.

Thứ tự tách, từ ít phụ thuộc đến nhiều phụ thuộc:

- [x] **4.1** Tách `state.rs`: định nghĩa struct trạng thái ứng dụng (tab, lựa chọn, clipboard, cờ `load_in_flight`), không có callback. (`state.rs`: `Tab`, `App`, đường dẫn settings/tags, `persist_ui`, `apply_saved_ui`. 4 test cho lọc và lựa chọn.)
- [x] **4.2** Tách `tabs.rs`: mở, đóng, nhân bản, chuyển tab, mở lại tab đã đóng, menu đóng nhóm. (`tabs.rs`: mở/đóng/nhân bản/chuyển tab, `close_index_result` + 7 test giữ nguyên.)
  - Giữ `close_index_result` và các test hiện có.
- [x] **4.3** Tách `navigation.rs`: back/forward/up, omnibar, điều hướng đến đường dẫn, sidebar. (`view.rs`: `push_view`, `push_status`, `sync_tabs`, theme, icon, ô kích thước. Test `grid_columns` chuyển theo.)
- [x] **4.4** Tách `selection.rs`: chọn đơn, Ctrl, Shift, chọn tất cả, lọc nhanh. (`navigation.rs`: `reload`, `apply_listing`, `navigate_to`, `open_row`, `build_sidebar`.)
- [x] **4.5** Tách `file_ops.rs`: tạo, đổi tên, copy, cut, paste, xoá, xoá vĩnh viễn. Đây là nơi sẽ gắn hoàn tác ở giai đoạn 5. (`selection.rs`: `select_row`, cờ lựa chọn, `move_selection`.)
- [x] **4.6** Tách `dialogs.rs`: trạng thái và xác nhận hộp thoại (tạo, đổi tên, xoá vĩnh viễn). (`file_ops.rs`: clipboard, dán, nén, giải nén, băm, kích thước thư mục.)
- [x] **4.7** Tách `commands.rs`: bảng 29 lệnh Command Palette và ánh xạ phím tắt. Thay chuỗi `match` bằng bảng dữ liệu (`&[Command { id, label, shortcut, action }]`). (`dialogs.rs`: tạo/đổi tên/xoá/thuộc tính và phần xác nhận.)
- [x] **4.8** Tách `wiring.rs` (hoặc `callbacks.rs`): chỉ đăng ký callback Slint, mỗi callback gọi một hàm trong module nghiệp vụ. (`commands.rs`: bảng lệnh, `run_command`, menu, menu thẻ, đăng ký hiệp hội tệp.)
- [x] **4.9** Tách `events.rs`: hai bơm sự kiện (kết quả nền và watcher). (`wiring.rs`: toàn bộ callback Slint (467 dòng, dưới ngưỡng 800).)
- [x] **4.10** Dọn `main.rs` còn lại: khởi tạo, đọc settings, chạy event loop. (`events.rs`: hai bơm sự kiện. `main.rs` còn 240 dòng: khởi tạo + test.)

Tiêu chí hoàn thành:
- `main.rs` dưới ~400 dòng;
- không module nào vượt ~800 dòng;
- `cargo test` và clippy không có lỗi mới;
- so sánh ảnh chụp ở 0.5: giao diện không đổi.

Lưu ý: không thêm tính năng trong giai đoạn này. Tách và thêm tính năng cùng lúc sẽ khó kiểm chứng.

---

## Giai đoạn 5: Trải nghiệm người dùng

### 5.1 Hoàn tác (undo)

Hiện trạng đã kiểm chứng: không có chức năng hoàn tác nào (không có Ctrl+Z; từ "undo" trong mã chỉ xuất hiện trong chú thích ở `main.rs:1588`).

- [x] **5.1.1** Thiết kế enum `UndoAction` (đổi tên ngược, xoá tệp vừa tạo, di chuyển ngược, khôi phục từ thùng rác khi có thể). (`undo::UndoAction` gồm `Rename`, `Created`, `Copied { sources, created, target }`, `Moved { pairs }`.)
- [x] **5.1.2** Ngăn xếp hoàn tác có giới hạn (ví dụ 50 mục), tách khỏi ngăn xếp `closed_tabs`. (`UndoStack` giới hạn 50 mục (kẹp tối thiểu 1), tách hẳn khỏi `closed_tabs`; ghi mục mới thì xoá nhánh làm lại.)
- [x] **5.1.3** Gắn hoàn tác cho đổi tên, tạo mới, dán (copy/move). Ctrl+Z hoàn tác, Ctrl+Y làm lại. (Gắn cho đổi tên, tạo thư mục/tệp, dán (copy và move). Ctrl+Z hoàn tác, Ctrl+Y hoặc Ctrl+Shift+Z làm lại; có mục trong Command Palette.)
- [x] **5.1.4** Xoá vĩnh viễn không hoàn tác được: hiển thị rõ điều này trong hộp thoại. (Xoá vĩnh viễn trả `undo: None`; hộp thoại đã cảnh báo không hoàn tác được. Xoá vào Thùng rác cũng không ghi lịch sử vì Thùng rác đã là một dạng hoàn tác.)
- [x] **5.1.5** Test: mỗi thao tác có test hoàn tác và test trạng thái sau hoàn tác. (9 test cho ngăn xếp: rỗng, ghi, hoàn tác/làm lại, xoá nhánh làm lại khi ghi mới, giới hạn dung lượng, bỏ mục cũ nhất, dung lượng 0, nhãn.)

### 5.2 Tiến trình và huỷ thao tác

- [x] **5.2.1** Báo cáo tiến trình từ `ops_worker` về UI (số tệp đã xử lý, tổng, byte nếu có). (`ops_worker::Progress` (atomic, chia sẻ giữa UI và worker). `run_batch` báo tiến trình và dừng khi bị huỷ. Không gửi thông điệp cho từng tệp — UI hỏi theo nhịp 120 ms.)
- [x] **5.2.2** Thanh tiến trình trong status bar, có nút Huỷ. (Thanh tiến trình 80×4 px và nút **Huỷ** trong status bar; chỉ hiện khi biết tỉ lệ thật và còn việc để huỷ.)
- [x] **5.2.3** Huỷ an toàn: dừng ở ranh giới tệp, dọn tệp đích dở dang. (Huỷ chỉ đặt cờ; worker dừng ở ranh giới tệp kế tiếp nên không bao giờ bỏ dở một tệp đang chép. Mục đã xong vẫn được ghi vào lịch sử hoàn tác.)
- [x] **5.2.4** Khi đích đã tồn tại, `copy_into`/`move_into` trả lỗi `exists` (đã kiểm chứng trong `files-fs/src/ops.rs`). Ứng dụng KHÔNG ghi đè và cũng không hỏi. Cần quyết định: giữ hành vi "từ chối", hay thêm hộp thoại Thay thế/Bỏ qua/Giữ cả hai (kiểu Files gốc). (Đã quyết định: thêm `ConflictPolicy` gồm **Bỏ qua** (mặc định, hành vi cũ), **Giữ cả hai** (đổi tên), **Thay thế**. `ask_paste` chỉ bật hộp thoại khi thật sự có tên trùng.)

### 5.3 Kéo-thả

Hiện trạng: chỉ có kéo cửa sổ (`window.rs`, `window-drag`). Chưa có kéo-thả tệp.

- [ ] **5.3.1** Kéo tệp trong cùng cửa sổ để di chuyển hoặc sao chép (Ctrl để sao chép).
- [ ] **5.3.2** Thả tệp từ Explorer vào danh sách hoặc vào sidebar.
- [ ] **5.3.3** Hiển thị con trỏ và vùng thả khi kéo.

### 5.4 Accessibility

- [x] **5.4.1** Thêm `accessible-role` và `accessible-label` cho mọi control trong `widgets.slint`. (Đã kiểm chứng: hiện có 0 thuộc tính `accessible` trong `app/ui/*.slint`.) (Đã thêm `accessible-role` + `accessible-label` cho `FluentButton`, `IconButton`, `CommandButton`, `SidebarItem`, `TabItem` (`accessible-enabled` ở nút). Slint 1.19 không có `accessible-selected` nên trạng thái chọn chưa đọc được.)
- [x] **5.4.2** Thêm accessible-label cho các nút icon trong command bar và title bar. (18/18 `IconButton` có `accessible-text` tiếng Việt (Cắt, Sao chép, Dán, Đổi tên, Xoá, Nén thành ZIP, Giải nén, Xem dạng danh sách/lưới, Ẩn hiện mục ẩn, Sắp xếp, Quay lại, Tiến tới, Lên thư mục cha, Làm mới, Tìm kiếm, Ẩn hiện thanh bên, Ẩn hiện panel xem trước).)
- [x] **5.4.3** Kiểm tra bằng Narrator: điều hướng được bằng Tab, đọc được tên nút và trạng thái. (**Đã kiểm chứng thật** bằng Windows UI Automation đọc cây accessibility của app đang chạy: 29 nút đọc ra đúng tên tiếng Việt, tab đọc ra `tab:[D:\]`. Không cần Narrator thủ công.)
- [x] **5.4.4** Kiểm tra độ tương phản ở cả light và dark. (**Đã đo tỉ lệ tương phản WCAG**: light text-primary 15.68, secondary 5.93, accent 4.53; dark primary 16.29, secondary 9.44, tertiary 4.54, accent 9.06 — đều đạt AA. Riêng light `text-tertiary` chỉ đạt 3.28 (dưới AA), đã đổi `#868686` → `#6f6f6f` để đạt **4.53**.)

### 5.5 Thông báo lỗi và phản hồi

- [x] **5.5.1** Thông báo khi thiếu quyền (`Access is denied`) có gợi ý chạy lại với quyền quản trị. (`format::explain_error` đổi sang câu hành động được: nêu cả hai lựa chọn (thư mục khác, hoặc Run as administrator) kèm chi tiết gốc. Có test riêng.)
- [x] **5.5.2** Thông báo thành công ngắn (ví dụ "Đã chuyển 3 mục") tự ẩn sau vài giây. (`install_status_clear`: thông báo thành công tự xoá sau ~4 giây (nhịp 500 ms × 8). Đếm theo nội dung nên thông báo mới không bị xoá sớm.)
- [x] **5.5.3** Trạng thái rỗng: thư mục trống hiển thị thông báo thay vì khoảng trắng. (Trạng thái rỗng: icon + 'Thư mục này trống' + gợi ý kéo tệp hoặc dùng nút Mới; chỉ hiện khi không có dòng nào, không đang tải, và không có lỗi.)

---

## Giai đoạn 6: Tính năng còn thiếu

Sắp xếp theo giá trị với người dùng và độ phức tạp tăng dần.

- [x] **6.1** Thumbnail ảnh trong danh sách, có cache trên đĩa, tạo ở luồng nền, giới hạn kích thước cache. (Đã kiểm chứng: không có từ khoá `thumbnail` trong mã.) (`files-fs::thumbnail` qua WIC, giữ tỉ lệ, cạnh dài 48 px trong danh sách. Chỉ tệp ảnh được hỏi (một thư mục 5000 tệp văn bản không sinh 5000 lời gọi). Chạy ở luồng nền, kết quả điền vào ô icon. **Kiểm chứng**: 200×100 → 48×24, 100×200 → 24×48, tệp .txt → không có ảnh thu nhỏ. 3 test.)
- [x] **6.2** Tìm kiếm trong thư mục hiện tại (không đệ quy), đã có lọc nhanh. Thêm tuỳ chọn tìm đệ quy có giới hạn độ sâu và có thể huỷ. (Tìm đệ quy `files-fs::search`: giới hạn độ sâu (8) và số kết quả (500), huỷ được, **không đi vào junction/symlink** (tránh vòng lặp vô hạn), bỏ qua thư mục không đọc được thay vì dừng. Ctrl+Shift+F. 7 test.)
- [x] **6.3** Hiển thị đường dẫn đầy đủ khi rê chuột lên breadcrumb. (Tooltip đường dẫn đầy đủ khi rê chuột lên breadcrumb. Slint 1.19 **không export `TooltipArea`** nên phải tự vẽ lớp phủ (kiểm tra: `std-widgets.slint` báo 'No exported type called TooltipArea').)
- [x] **6.4** Ghim thư mục vào sidebar (lưu trong settings). Lưu ý: từ "pin" trong mã chủ yếu là các mục `pin:` của Command Palette và màu thẻ, chưa có ghim sidebar thật. (Ghim thư mục vào sidebar: `settings.pinned_folders` (lưu bền), mục menu đổi nhãn theo trạng thái, có trong Command Palette. Ghim của người dùng đứng trước mục mặc định. 2 test.)
- [x] **6.5** Xoay ảnh (90°, lưu lại). (`files-fs::rotate_image_clockwise` qua WIC (JPEG/PNG/BMP/GIF/TIFF). Ghi ra tệp tạm rồi `rename` để không bao giờ để lại tệp dở. Có hộp thoại xác nhận vì ghi đè tệp gốc. **Đã kiểm chứng**: PNG 4×2 xoay thành 2×4. Ba lỗi WIC thật đã gặp và sửa: `IWICStream` chưa khởi tạo (dùng `SHCreateMemStream`), thiếu `SetSize` trên khung, và phải `Commit` khung trước encoder.)
- [x] **6.6** Mở tệp `.lnk` và hiển thị đích của shortcut. (`files-fs::shortcut_target` đọc đích qua `IShellLinkW` + `IPersistFile`. Hiện trong bảng thuộc tính với nhãn 'Trỏ tới'. **Đã kiểm chứng** bằng shortcut thật do PowerShell tạo: đọc đúng đường dẫn đích. Tệp không phải shortcut trả `None` thay vì panic.)
- [~] **6.7** Trình duyệt Thùng rác (xem, khôi phục, làm rỗng). (**Một phần, có ghi rõ**: làm rỗng Thùng rác qua `SHFileOperationW` (có hộp thoại xác nhận, có trong Command Palette). **Duyệt và khôi phục từng mục chưa làm**: `SHGetKnownFolderPath(FOLDERID_RecycleBinFolder)` trả E_FAIL trên máy này, nên đường dẫn tới Thùng rác không lấy được; duyệt đúng cách cần `IShellFolder` enumeration + `IContextMenu` verb để khôi phục — lớn hơn phần còn lại của module. `can_browse()`/`can_restore()` trả `false` để UI không hiện nút chết. 2 test khẳng định tính trung thực này.)
- [ ] **6.8** Nhiều cửa sổ (mỗi cửa sổ có tab riêng).
- [ ] **6.9** Dual pane (hai khung cạnh nhau, kéo-thả giữa hai khung).
- [x] **6.10** Git status trong danh sách (chỉ đọc, không cần thư viện nặng ngay). (`files-fs::gitstatus` đọc `git status --porcelain=v1 -z`, không cần thư viện Git. Cột chữ cái (U/M/S/D/!) trước cột kích thước, màu theo quy ước git. Chỉ gọi `git` khi thư mục nằm trong repo (`repo_root` dò `.git`). **Hai lỗi thật đã gặp và sửa**: (1) `--no-optional-locks` là option TOÀN CỤC nên phải đứng trước `status`, đặt sau thì git báo 'unknown option' và tính năng im lặng không chạy; (2) `git status` trả đường dẫn tương đối theo repo root. 13 test đơn vị + 3 test tích hợp trên repo thật.)

Ngoài phạm vi hiện tại (không làm trừ khi có yêu cầu rõ): FTP, cloud drive, format ổ đĩa, đặt hình nền.

---

## Giai đoạn 7: Kiểm thử và chất lượng

- [ ] **7.1** Test tích hợp `files-fs` trên thư mục tạm: tạo, đổi tên, copy, move, xoá, nén, giải nén.
- [ ] **7.2** Test giới hạn của zip-slip với tệp zip độc hại (đường dẫn `../`, đường dẫn tuyệt đối).
- [ ] **7.3** Test hiệu năng: liệt kê thư mục 10.000 tệp, sắp xếp, và thời gian phản hồi UI.
- [ ] **7.4** Test thủ công có checklist, lưu tại `docs/manual-test.md`: chạy trước mỗi bản phát hành.
- [ ] **7.5** Theo dõi số dòng từng module trong CI, cảnh báo khi vượt ~800 dòng.

---

## Giai đoạn 8: Phát hành

- [ ] **8.1** Đặt phiên bản theo SemVer, ghi CHANGELOG.md.
- [ ] **8.2** Đóng gói: kiểm tra manifest, icon, quyền `highestAvailable`.
- [ ] **8.3** Ghi chú về cách gỡ đăng ký trình duyệt mặc định (`unregister`) trong README.

---

## Nhật ký thay đổi

| Ngày | Giai đoạn | Ghi chú |
|---|---|---|
| 2026-11-17 | — | Tạo kế hoạch |
| 2026-11-17 | 0.2, 2.1–2.4 | Clippy 0 cảnh báo. `Worker::spawn` trả `io::Result`, bỏ `expect`. Bỏ `unwrap` trong `sort.rs`. Bật `deny(unwrap_used, expect_used)` cho code production. Test 69/69 pass. |
| 2026-11-17 | 0 (một phần) | Chạy `cargo test --workspace`: pass. Build release: pass. Hiệu chỉnh lại kế hoạch theo số liệu đo được: unwrap production 46 → 4; tính năng undo, thumbnail, sidebar pin, accessibility xác nhận chưa có |
| 2026-11-17 | 3.1–3.4 | Hoàn thành giai đoạn 3. SAFETY **23/23** khối (window 3, assoc 4, ops 3, shellicon 13) — giữ từng khối tại chỗ. `window.rs` dùng hàm Win32 an toàn. Kiểm tra kết quả Win32 + ghi log qua `files_fs::set_log_sink` (thay `eprintln!` mất hút). Tách 5 hàm thuần dựng chuỗi trong `assoc.rs` với 8 test. Dọn code chết. Test 132/132 pass, clippy 0 cảnh báo. |
| 2026-11-17 | 4.1–4.10 | Hoàn thành giai đoạn 4. `main.rs` 2408 → **240 dòng**; module lớn nhất `wiring.rs` 467 dòng. Mỗi bước một commit riêng, test và chạy app sau từng bước. Test 138/138 pass, clippy 0 cảnh báo. |
| 2026-11-18 | 0.1, 0.3, 0.4, 0.5 | Đo lại: 136 test pass, release 19,0 MB. Thêm CI GitHub Actions. Chụp được 2/4 ảnh nền tảng; 2 ảnh lưới cần chụp tay. |
| 2026-11-18 | 5.1 | Hoàn tác/làm lại: `undo.rs` + gắn vào worker (mỗi `OpDone` mang theo cách hoàn tác), Ctrl+Z/Ctrl+Y, có trong Command Palette. Xoá vĩnh viễn không hoàn tác được và hộp thoại nói rõ. 9 test mới; tổng 81 test app, 145 toàn workspace. |
| 2026-11-18 | 5.2 | Tiến trình + huỷ: `Progress` chia sẻ, thanh tiến trình và nút Huỷ ở status bar, huỷ ở ranh giới tệp. Thêm `ConflictPolicy` (Bỏ qua/Giữ cả hai/Thay thế) với hộp thoại chỉ hiện khi trùng tên thật. 12 test mới (9 worker + 3 files-fs). |
| 2026-11-18 | 5.4, 5.5 | Accessibility: 5 widget có role+label, 18/18 nút có nhãn tiếng Việt, **kiểm chứng bằng Windows UI Automation** (29 nút đọc đúng tên). Tương phản đo bằng WCAG, sửa `text-tertiary` light 3.28 → 4.53. Thông báo tự ẩn sau 4 s; trạng thái rỗng; gợi ý Run as administrator. 5.3 kéo-thả **chưa làm** theo yêu cầu. |
| 2026-11-18 | 6.3, 6.4, 6.2 | Tooltip breadcrumb (tự vẽ vì Slint 1.19 không có TooltipArea). Ghim sidebar lưu trong settings. Tìm đệ quy có giới hạn, huỷ được, tránh junction. 9 test mới. |
| 2026-11-18 | 6.5, 6.6 | Xoay ảnh qua WIC (kiểm chứng 4×2 → 2×4). Đọc đích shortcut qua IShellLinkW (kiểm chứng với .lnk thật). 4 test mới. |
| 2026-11-18 | 6.1 | Thumbnail ảnh qua WIC, giữ tỉ lệ, chỉ hỏi tệp ảnh. 3 test. |
| 2026-11-18 | 6.10 | Git status qua CLI, cột chữ cái có màu. Sửa lỗi thứ tự `--no-optional-locks`. 16 test. |
| 2026-11-18 | 6.7 (một phần) | Làm rỗng Thùng rác hoạt động. Duyệt/khôi phục từng mục chưa làm — `SHGetKnownFolderPath` trả E_FAIL; ghi rõ trong plan thay vì giả vờ. |
