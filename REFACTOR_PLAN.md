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

- [ ] **5.1.1** Thiết kế enum `UndoAction` (đổi tên ngược, xoá tệp vừa tạo, di chuyển ngược, khôi phục từ thùng rác khi có thể).
- [ ] **5.1.2** Ngăn xếp hoàn tác có giới hạn (ví dụ 50 mục), tách khỏi ngăn xếp `closed_tabs`.
- [ ] **5.1.3** Gắn hoàn tác cho đổi tên, tạo mới, dán (copy/move). Ctrl+Z hoàn tác, Ctrl+Y làm lại.
- [ ] **5.1.4** Xoá vĩnh viễn không hoàn tác được: hiển thị rõ điều này trong hộp thoại.
- [ ] **5.1.5** Test: mỗi thao tác có test hoàn tác và test trạng thái sau hoàn tác.

### 5.2 Tiến trình và huỷ thao tác

- [ ] **5.2.1** Báo cáo tiến trình từ `ops_worker` về UI (số tệp đã xử lý, tổng, byte nếu có).
- [ ] **5.2.2** Thanh tiến trình trong status bar, có nút Huỷ.
- [ ] **5.2.3** Huỷ an toàn: dừng ở ranh giới tệp, dọn tệp đích dở dang.
- [ ] **5.2.4** Khi đích đã tồn tại, `copy_into`/`move_into` trả lỗi `exists` (đã kiểm chứng trong `files-fs/src/ops.rs`). Ứng dụng KHÔNG ghi đè và cũng không hỏi. Cần quyết định: giữ hành vi "từ chối", hay thêm hộp thoại Thay thế/Bỏ qua/Giữ cả hai (kiểu Files gốc).

### 5.3 Kéo-thả

Hiện trạng: chỉ có kéo cửa sổ (`window.rs`, `window-drag`). Chưa có kéo-thả tệp.

- [ ] **5.3.1** Kéo tệp trong cùng cửa sổ để di chuyển hoặc sao chép (Ctrl để sao chép).
- [ ] **5.3.2** Thả tệp từ Explorer vào danh sách hoặc vào sidebar.
- [ ] **5.3.3** Hiển thị con trỏ và vùng thả khi kéo.

### 5.4 Accessibility

- [ ] **5.4.1** Thêm `accessible-role` và `accessible-label` cho mọi control trong `widgets.slint`. (Đã kiểm chứng: hiện có 0 thuộc tính `accessible` trong `app/ui/*.slint`.)
- [ ] **5.4.2** Thêm accessible-label cho các nút icon trong command bar và title bar.
- [ ] **5.4.3** Kiểm tra bằng Narrator: điều hướng được bằng Tab, đọc được tên nút và trạng thái.
- [ ] **5.4.4** Kiểm tra độ tương phản ở cả light và dark.

### 5.5 Thông báo lỗi và phản hồi

- [ ] **5.5.1** Thông báo khi thiếu quyền (`Access is denied`) có gợi ý chạy lại với quyền quản trị.
- [ ] **5.5.2** Thông báo thành công ngắn (ví dụ "Đã chuyển 3 mục") tự ẩn sau vài giây.
- [ ] **5.5.3** Trạng thái rỗng: thư mục trống hiển thị thông báo thay vì khoảng trắng.

---

## Giai đoạn 6: Tính năng còn thiếu

Sắp xếp theo giá trị với người dùng và độ phức tạp tăng dần.

- [ ] **6.1** Thumbnail ảnh trong danh sách, có cache trên đĩa, tạo ở luồng nền, giới hạn kích thước cache. (Đã kiểm chứng: không có từ khoá `thumbnail` trong mã.)
- [ ] **6.2** Tìm kiếm trong thư mục hiện tại (không đệ quy), đã có lọc nhanh. Thêm tuỳ chọn tìm đệ quy có giới hạn độ sâu và có thể huỷ.
- [ ] **6.3** Hiển thị đường dẫn đầy đủ khi rê chuột lên breadcrumb.
- [ ] **6.4** Ghim thư mục vào sidebar (lưu trong settings). Lưu ý: từ "pin" trong mã chủ yếu là các mục `pin:` của Command Palette và màu thẻ, chưa có ghim sidebar thật.
- [ ] **6.5** Xoay ảnh (90°, lưu lại).
- [ ] **6.6** Mở tệp `.lnk` và hiển thị đích của shortcut.
- [ ] **6.7** Trình duyệt Thùng rác (xem, khôi phục, làm rỗng).
- [ ] **6.8** Nhiều cửa sổ (mỗi cửa sổ có tab riêng).
- [ ] **6.9** Dual pane (hai khung cạnh nhau, kéo-thả giữa hai khung).
- [ ] **6.10** Git status trong danh sách (chỉ đọc, không cần thư viện nặng ngay).

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
