# Kế hoạch cải tiến sau code review: files-rs-slint

> Cách dùng: làm tuần tự từ trên xuống. Xong mục nào thì đánh `[x]` ngay và ghi kết quả đo được.
> Mỗi mục phải giữ `cargo test --workspace` và `cargo clippy --workspace --all-targets` xanh.
> Không commit trong kế hoạch này. Commit do người dùng quyết định.

## Mục 1: Test không được "xanh giả" khi thiếu môi trường

Hiện trạng: nhiều test trong `crates/files-fs/tests/shellicon.rs` và `tests/ops.rs` gọi `return;` khi thiếu `SystemRoot`, PowerShell hoặc file mẫu. Test vẫn báo pass nên không ai biết nó không kiểm tra gì.

- [x] **1.1** Thêm helper `skip` trong `crates/files-fs/tests/common/mod.rs`: nếu biến môi trường `CI` được đặt thì panic, còn ở máy local thì in `SKIPPED: <lý do>` ra stderr và trả về. (Đổi tên `skip_or_fail` thành `skip` cho gọn.)
- [x] **1.2** Thay 16 chỗ `return;` bỏ qua trong `tests/ops.rs` (8) và `tests/shellicon.rs` (8) bằng `common::skip(...)` kèm lý do cụ thể. Không còn `return;` nào im lặng.
- [x] **1.3** Chạy `cargo test -p files-fs --test ops --test shellicon -- --nocapture`: 29 + 8 test pass, 0 dòng `SKIPPED` trên máy này.

Tiêu chí: không còn `return;` nuốt test; CI không bao giờ pass khi thiếu điều kiện.

## Mục 2: Dọn tham số không dùng

Lưu ý: 3 chỗ `let _ = x;` trong bản trước không phải code chết. Đó là nhánh `#[cfg(not(windows))]` nhận tham số nhưng không dùng. Chỉ `recycle::restore` là tham số thật sự không dùng trên mọi nền tảng.

- [x] **2.1** `recycle::restore`: đổi tham số `item` thành `_item`, bỏ dòng `let _ = item;`.
- [x] **2.2** Giữ nguyên `ops.rs` (clipboard) và `openwith.rs` vì là nhánh stub không Windows. Lý do: tham số được dùng ở nhánh Windows, dòng `let _ =` chỉ có ở nhánh `#[cfg(not(windows))]` để tránh cảnh báo `unused`. Xoá sẽ làm build Linux/macOS báo lỗi.

Tiêu chí: `restore` không còn dòng `let _ =`; các nhánh stub giữ nguyên có lý do.

## Mục 3: Tách `ops_worker.rs` (982 dòng)

- [x] **3.1** Tách `Progress` (và `impl`) ra `app/src/ops_worker/progress.rs`, re-export lại từ `ops_worker` (`pub use progress::Progress`).
- [x] **3.2** Tách `mod tests` ra `app/src/ops_worker/tests.rs`.
- [x] **3.3** Đo lại: `ops_worker.rs` còn 631 dòng (từ 982). `progress.rs` 77 dòng, `tests.rs` 285 dòng. 113 test app pass, clippy 0 cảnh báo.

Tiêu chí: không thay đổi hành vi; chỉ di chuyển code.

## Mục 4: Rà `let _ =` ở chỗ ghi dữ liệu bền

Chỉ xét chỗ có hậu quả nếu lỗi bị nuốt: ghi file settings/tags, ghi thumbnail cache, ghi log. Bỏ qua `send` vào channel đã biết có thể đóng khi thoát app.

- [x] **4.1** Đã rà. Kết luận: `settings.rs`, `tags.rs` trả `io::Result` từ `save`; `state.rs` `write_settings` ghi log khi lỗi; `navigation.rs` và `commands.rs` ghi log khi `store.save` lỗi; `thumbcache.rs` ghi cache trả lỗi và chỉ ghi log. Các `let _ =` còn lại là dọn file tạm, dọn thư mục test, hoặc logger (`logging::write` không thể tự báo lỗi cho chính nó) nên chấp nhận được.
- [x] **4.2** Có một chỗ nuốt lỗi thật: `thumbcache.rs` tạo thư mục cache dự phòng với `let _ =`. Đã đổi thành ghi log có ngữ cảnh.
- [x] **4.3** Thêm `a_cache_write_into_a_blocked_folder_reports_an_error` (ghi cache vào chỗ thư mục bị chặn phải trả `Err`). 5/5 test thumbcache pass.

Tiêu chí: mọi lỗi ghi file bền đều được báo lại hoặc ghi log.

## Mục 5: Tách `app.slint` (2166 dòng)

- [x] **5.1** Đã liệt kê. `app.slint` (2166 dòng) chỉ có một `export component AppWindow` lớn, cùng 4 struct. Khối palette (dòng ~1978–2119) và dialog (~2121–2188) đọc trực tiếp `root.*` (property, callback), gọi `key-root.focus()` và `root.close-palette()`. Chúng không phụ thuộc lẫn nhau nhưng đều phụ thuộc root.
- [ ] **5.2** Chưa làm. Để tách palette/dialog ra `.slint` riêng phải chuyển ~15 property và ~8 callback thành `in`/`callback` của component con, và đẩy `key-root.focus()` về Rust hoặc callback. Đây là thay đổi lớn về API nội bộ UI, không đúng với "chỉ di chuyển code". Cần quyết định trước (xem mục 5.4).
- [ ] **5.3** Chưa làm. Phụ thuộc 5.2.
- [ ] **5.4** Đề xuất (chờ bạn chọn): (a) giữ nguyên `app.slint`, coi mục 5 là hoãn; (b) tách bằng cách chuyển palette/dialog thành component nhận `in property` và `callback` rõ ràng, chấp nhận nhiều dòng thay đổi và kiểm tra UI thủ công; (c) tách trước các khối đọc ít property (ví dụ `PropRow`/`NavEntry` đã là struct, `Tooltip` breadcrumb, menu con "Mở bằng") rồi đánh giá lại.

Tiêu chí: UI không đổi hành vi; build và test xanh.

## Mục 6: Đồng bộ tài liệu

- [x] **6.1** `REFACTOR_PLAN.md`: mục "Nguồn số liệu" sửa clippy thành 0 cảnh báo, và số test thành 228 đo lại hôm nay (thay cho 69/116 cũ).
- [x] **6.2** Mục 1.x (đồng bộ PLAN/README) chưa thuộc đợt này nên không đánh dấu; ghi rõ trong nhật ký đợt. Các việc của đợt đã ghi vào nhật ký.
- [x] **6.3** Số liệu cuối đợt: **228 test pass, 0 hỏng; clippy 0 cảnh báo; `ops_worker.rs` 631 dòng; `main.rs` 240; `wiring.rs` 918; `app.slint` 2290**. (`app.slint` tăng từ 2166 do các thay đổi UI trước đó, không phải do đợt này.)

Tiêu chí: số liệu trong tài liệu khớp với lệnh đo thật.

## Mục 7: Sửa theo review an toàn dữ liệu (2026-11-19)

Thứ tự theo mức độ ưu tiên. Mỗi mục có test hồi quy riêng.

- [x] **7.1** `cargo fmt` đỏ ở `ops_worker.rs`, `ops_worker/tests.rs`: đã chạy `cargo fmt`, `cargo fmt --check` trả 0.
- [x] **7.2** CI thiếu kiểm tra format: thêm bước `cargo fmt --all -- --check` vào `.github/workflows/ci.yml`.
- [x] **7.3** Chép/chuyển thư mục vào chính thư mục con của nó: thêm `reject_into_descendant` (mã lỗi `into_self`). Test: `copying_a_folder_into_its_own_child_is_refused`, `moving_a_folder_into_its_own_child_is_refused`.
- [x] **7.4** `move_into` coi mọi lỗi `rename` là khác volume: chỉ fallback copy+xoá khi mã lỗi là `ERROR_NOT_SAME_DEVICE` (17) hoặc `EXDEV` (18); lỗi khác trả về nguyên vẹn.
- [x] **7.5** `Replace` xoá đích trước rồi nuốt lỗi: thay bằng `replace_staged` (đổi bản cũ sang tên tạm, ghi, xoá bản cũ; ghi lỗi thì trả bản cũ về). Test: `a_failed_replace_keeps_the_old_destination`.
- [x] **7.6** Undo/redo mất action khi thất bại: `events.rs` trả action về ngăn cũ ở nhánh `Err`.
- [x] **7.7** Redo `Created` tạo sai tên (`a.md` thành `a.md.txt`): tạo đúng đường dẫn đã ghi. Test: `redoing_a_created_file_keeps_its_exact_name`.
- [x] **7.8** `delete_to_recycle_bin` đếm theo thực tế (mục không còn trên đĩa) thay vì `paths.len()`.
- [x] **7.9** Kiểm tra tên: thêm `validate_name` (rỗng, `.`/`..`, ký tự cấm, tên thiết bị DOS, dấu chấm/khoảng trắng cuối), dùng trong `rename`, `create_folder`, `create_file`. Test: `unsafe_names_are_rejected_and_normal_ones_pass`, `creating_with_a_device_name_is_refused`.
- [x] **7.10** Ghi settings/tags không nguyên tử và mất bản hỏng: thêm `app/src/persist.rs` (`write_atomic`, `quarantine`); file parse lỗi được đổi tên thành `.corrupt` trước khi dùng mặc định. 3 test trong `persist`.
- [ ] **7.11** `create_zip` để lại zip dở khi lỗi, đi theo symlink/junction; `extract_zip` ghi đè file có sẵn. Chưa làm.
- [ ] **7.12** 7 chỗ `mem::forget(timer)` (không dừng khi đóng cửa sổ). Chưa làm.
- [ ] **7.13** I/O đồng bộ trên UI thread (`is_dir`, `read_dir` trong `navigation.rs`, `file_ops.rs`, `drag.rs`, `tabs.rs`, `wiring.rs`, `preview.rs`). Chưa làm.
- [ ] **7.14** `native_drop.rs`: 17 khối `unsafe`, chỉ 6 có `// SAFETY:`. Chưa làm.

Kết quả đo sau 7.1 đến 7.10: `cargo fmt --check` 0; clippy với `-D warnings` 0 cảnh báo; `cargo test --workspace` toàn bộ pass (files-fs `ops` 34, app 117).

## Mục 8: Lỗi phát hiện ở vòng review sau (2026-11-19)

Mỗi mục đã được tái hiện bằng chương trình kiểm chứng riêng trước khi sửa.

- [x] **8.1** File JSON rỗng bị coi là hỏng: `state::settings_path()`/`tag_store_path()` tạo sẵn file 0 byte để thử quyền ghi, rồi `Settings::load`/`TagStore::load` đổi tên nó thành `.corrupt` ở mọi lần khởi động, làm mất đường dẫn thật. Sửa: nội dung rỗng thì trả mặc định, không đụng đĩa. Test: `an_empty_file_is_not_treated_as_corrupt`, `an_empty_file_keeps_the_path_in_place`.
- [x] **8.2** Rename chỉ đổi hoa/thường bị từ chối `exists` (`a.txt` → `A.txt`). Sửa: `same_entry` → `rename_changing_case` qua tên tạm, khôi phục tên cũ nếu bước hai lỗi. Test: `renaming_only_the_letter_case_is_allowed`.
- [x] **8.3** `validate_name` không giới hạn độ dài. Sửa: chặn trên 255 ký tự, thông báo nêu rõ giới hạn. Test: `a_name_longer_than_the_filesystem_limit_is_rejected`.
- [x] **8.4** `replace_staged` báo thất bại dù bản mới đã ghi xong khi chỉ việc dọn bản cũ lỗi. Sửa: dọn lỗi chỉ ghi log; nếu không trả được bản cũ về chỗ cũ thì báo mã `replace_rollback_failed` kèm đường dẫn bản cũ. *Chưa có test hồi quy: Windows chặn ngay ở bước dời bản cũ nên probe không vào được nhánh này; sửa ở mức phòng thủ.*
- [x] **8.5** `is_cross_device` hardcode 17/18: đổi thành hằng `CROSS_DEVICE_ERRNO` theo `cfg` và ghi rõ nghĩa.
- [x] **8.6** `validate_name` chưa được export: thêm vào `lib.rs` để tầng trên dùng chung.

Kết quả đo sau 8.1–8.6: `cargo fmt --check` 0; clippy `-D warnings` 0 cảnh báo; `cargo test --workspace` pass (files-fs `ops` 36, app 119, tổng 241).

## Ngoài phạm vi đợt này

- CHANGELOG.md, ghi chú gỡ đăng ký (mục 8.1, 8.3 của REFACTOR_PLAN): để sau khi người dùng quyết định phát hành.
- Commit các thay đổi: để người dùng quyết định cách gom commit.
- Duyệt và khôi phục từng mục Thùng rác: cần thiết kế riêng.
