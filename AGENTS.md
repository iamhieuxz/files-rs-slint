# AGENTS.md

## Mục tiêu và phạm vi

- File này áp dụng cho toàn bộ repository `files-rs-slint`.
- Trả lời người dùng bằng tiếng Việt. Giữ nguyên ngôn ngữ hiện có của UI, comment và thông báo lỗi.
- Đây là trình quản lý tệp Windows viết bằng Rust + Slint. Mọi thay đổi phải ưu tiên tính đúng đắn và an toàn dữ liệu hơn tốc độ phát triển tính năng.
- Chỉ sửa phần liên quan trực tiếp đến yêu cầu. Không refactor cơ hội, đổi format hàng loạt hoặc nâng dependency ngoài phạm vi.
- Working tree có thể đang chứa nhiều thay đổi chưa commit của người dùng. Không tự ý `git reset`, `stash`, `clean`, revert hoặc ghi đè các thay đổi đó.
- Nếu workspace có thư mục tham chiếu `../files-community-files`, chỉ đọc để đối chiếu hành vi. Không sửa bản C# và không sao chép mã C# vào dự án Rust.

## Bản đồ mã nguồn

```text
crates/files-core/   Model thuần: Entry, Listing, sort, path, lỗi. Không I/O.
crates/files-fs/     Biên hệ điều hành: listing, file ops, archive, search,
                     watcher, Shell icon, recycle bin, Git status, thumbnail.
app/src/             Trạng thái, điều hướng, command, dialog, worker và event pump.
app/ui/              Theme, widget và layout Slint.
docs/manual-test.md  Checklist kiểm thử thủ công trước phát hành.
scripts/             Công cụ hỗ trợ, gồm cảnh báo kích thước module.
target/              Output của Cargo. Không chỉnh sửa trực tiếp.
```

Luồng phụ thuộc chính:

```text
app -> files-fs -> files-core
```

- Giữ `files-core` thuần dữ liệu và không gọi hệ điều hành.
- Đặt thao tác filesystem và Win32 dùng chung trong `files-fs` khi có thể.
- `app` được phép chứa Win32 gắn với cửa sổ, OLE drop target và vòng đời UI.
- `app/build.rs` biên dịch `app/ui/app.slint` và nhúng icon/manifest. Không sửa mã sinh trong `target/`.

## Lệnh làm việc

Chạy từ thư mục gốc repository bằng PowerShell trên Windows 10/11 với MSVC toolchain.

```powershell
cargo run -p files-rs-app
cargo build --workspace
cargo test --workspace
cargo clippy --workspace --all-targets
cargo fmt --all -- --check
git diff --check
```

- Không có npm, pnpm hay Node trong build path.
- `cargo fmt --all` chỉ dùng khi người dùng yêu cầu sửa format; `--check` là chế độ xác minh.
- CI đặt `RUSTFLAGS=-D warnings`. Khi cần tái hiện nghiêm ngặt, chạy cùng biến môi trường đó trước lệnh Clippy.
- `cargo build --workspace --release` chỉ cần cho xác minh phát hành hoặc khi thay đổi có rủi ro khác biệt giữa debug và release.

### Test tập trung

```powershell
cargo test -p files-core
cargo test -p files-fs
cargo test -p files-fs --test ops
cargo test -p files-fs --test archive
cargo test -p files-fs --test workflow
cargo test -p files-fs --test shellicon -- --nocapture
cargo test -p files-rs-app
cargo test -p files-rs-app ops_worker
```

- Dùng test hẹp nhất bao phủ thay đổi trước, sau đó chạy toàn workspace trước khi bàn giao.
- `--test perf` tạo nhiều tệp và chỉ đo listing/sort. Không dùng riêng test này để kết luận UI nhanh.
- Script `scripts/check-module-size.ps1` chỉ cảnh báo khi file vượt khoảng 800 dòng, không chặn CI. Chạy qua `pwsh` theo cùng cách CI đang dùng; execution policy của máy local có thể chặn script.

## Quy tắc an toàn dữ liệu

Đây là nhóm quy tắc bắt buộc cho mọi thay đổi liên quan đến tệp, thư mục, archive, recycle bin hoặc undo.

### Đường dẫn và đích

- Từ chối đích trùng nguồn hoặc nằm bên trong nguồn khi copy/move thư mục. Kiểm tra trước khi tạo thư mục đích hoặc xóa bất cứ thứ gì.
- Canonicalize khi cần so sánh đường dẫn trên Windows; so sánh chuỗi đơn thuần không đủ vì khác hoa/thường, separator và `..`.
- Không đi theo junction, symlink hoặc reparse point trong thao tác đệ quy nếu chưa có kiểm soát chu trình.
- Chỉ fallback từ `rename` sang copy-rồi-delete khi lỗi thực sự là khác volume. Lỗi quyền, khóa tệp hoặc đường dẫn không hợp lệ phải được báo nguyên nhân.
- Validate tên mới cho cả create và rename: không separator, không rooted path, không thành phần `..`, không tên thiết bị DOS, không trailing dot/space. Không dùng chung logic nếu logic đó chỉ kiểm tra một phần.

### Thay thế, xóa và hoàn tác

- Không xóa đích cũ trước khi nguồn đã đọc được và thao tác mới có khả năng hoàn tất. Với replace, ghi vào tệp tạm cùng volume rồi trao đổi/rollback.
- Không bỏ qua lỗi xóa, rename, flush hoặc commit của thao tác phá hủy.
- Redo phải tạo đúng tên và đúng đường dẫn đã ghi trong lịch sử. Không tự thêm extension, không tự chọn tên `(2)` và không tiếp tục nếu đích đã bị chiếm.
- Chỉ đưa action ra khỏi undo/redo stack sau khi đã có kết quả xử lý phù hợp. Khi thất bại, action phải được khôi phục để người dùng có thể thử lại hoặc xử lý phần đã hoàn tất.
- Batch nhiều mục phải giữ chính xác cặp nguồn-đích đã xử lý. Không báo toàn bộ thành công khi chỉ một phần hoàn tất.
- Không dựa vào số phần tử đầu vào để báo số mục đã xóa. Kết quả Shell/Win32 phải phản ánh số mục thực sự thành công hoặc trả lỗi từng mục.
- Ưu tiên `IFileOperation` cho thao tác Shell cần tiến trình, undo và báo lỗi từng mục. Nếu tiếp tục dùng API cũ, phải xác minh hậu điều kiện thay vì suy đoán.
- Không thực hiện xóa vĩnh viễn, làm rỗng Thùng rác hoặc thay đổi file association trên dữ liệu thật để kiểm thử.

### Archive

- Giữ kiểm tra zip-slip bằng `enclosed_name` và kiểm tra thêm đường dẫn tuyệt đối.
- Không ghi đè archive hoặc tệp đích nếu chưa có chính sách conflict rõ ràng.
- Archive tạo dở phải được dọn hoặc không được coi là thành công.
- Không đi theo junction/symlink khi nén cây thư mục.

## UI, luồng nền và vòng đời

- Không gọi I/O đồng bộ trên UI thread, kể cả `is_dir`, `exists`, `read_dir`, decode ảnh, đọc shortcut hoặc liệt kê handler Shell. Đưa việc đó vào worker và trả kết quả qua event pump.
- Giữ token của listing/pane để kết quả cũ không ghi đè thư mục mới.
- Watcher phải có cờ dirty khi đang bận. Không consume rồi bỏ mất sự kiện đang tới.
- Watch từng pane đang hiển thị và reload khi pane được kích hoạt lại; không giả định chỉ active pane thay đổi.
- Timer và worker phải có chủ sở hữu và được dừng khi cửa sổ đóng. Không dùng `mem::forget` cho timer đang giữ `Rc<App>` hoặc kênh worker.
- Không giữ borrow `RefCell` chồng mutable và immutable. Sao chép giá trị cần thiết ra ngoài borrow trước khi cập nhật UI.
- Danh sách lớn phải hướng tới virtualization. Không dựng toàn bộ row cho thư mục hàng chục nghìn mục.
- Grid và details phải dùng cùng dữ liệu hiển thị; nếu tải thumbnail thì phải có nhánh render tương ứng hoặc không tải.
- UI accessibility không chỉ dành cho nút. Hàng tệp, trạng thái chọn, tab và thao tác mặc định phải có tên/vai trò đọc được khi phạm vi cho phép.

## Lưu cấu hình và trạng thái

- Ghi settings/tags qua tệp tạm cùng thư mục rồi rename nguyên tử.
- Khi nhiều cửa sổ cùng ghi, đọc bản mới nhất và chỉ thay trường thuộc sở hữu của thao tác hiện tại.
- Lỗi parse không được âm thầm xóa dữ liệu cũ. Giữ backup hoặc file corrupt có tên rõ ràng trước khi ghi đè bằng mặc định.
- `let _ =` chỉ chấp nhận cho cleanup best-effort, logging không thể tự báo lỗi hoặc channel đã đóng khi shutdown. Mọi lỗi ghi bền phải được log hoặc trả về UI.

## Bảo mật và quyền

- Không tăng quyền mặc định. Khi chạm manifest, ưu tiên `asInvoker`; chỉ nâng quyền cho action cụ thể nếu người dùng yêu cầu rõ.
- Không đổi registry, file association, ứng dụng mặc định, cài driver/font/chứng chỉ hoặc format ổ đĩa nếu chưa có yêu cầu và xác nhận rõ.
- Mọi khối `unsafe` phải có comment `// SAFETY:` giải thích tính hợp lệ của con trỏ, lifetime, buffer và quyền sở hữu.
- Kiểm tra kết quả Win32/COM. `CoInitializeEx` và `CoUninitialize` phải cân bằng trên cùng thread hoặc dùng RAII.
- Không đưa secret, token, đường dẫn cá nhân hoặc dữ liệu người dùng vào log, test fixture hay tài liệu.

## Kiểm thử

- Mỗi bug sửa phải có regression test ở tầng thấp nhất có thể tái hiện lỗi.
- Test thao tác tệp dùng thư mục tạm riêng, tên duy nhất và cleanup sau test.
- Test tích hợp phụ thuộc Windows/Shell/Git/junction phải dùng `common::skip` trong `crates/files-fs/tests/common/mod.rs`. Không thay bằng `return` im lặng.
- Trên CI, `common::skip` phải panic nếu thiếu môi trường. Không làm yếu cơ chế này.
- Test UI phải kiểm tra keyboard, focus, accessibility, DPI, dual pane và cửa sổ nhiều phiên bản khi thay đổi liên quan.
- Trước khi kết luận hiệu năng UI, đo thao tác thật với thư mục lớn. Benchmark sort/listing không thay thế phép đo render.
- Checklist `docs/manual-test.md` là một phần của kiểm chứng phát hành, không phải bằng chứng thay cho test tự động.

## Quy ước mã

- Rust edition 2021. Production code không dùng `unwrap()` hoặc `expect()`; giữ các lint deny hiện có trong crate.
- Lỗi đi qua `FsError` hoặc `Result`, có mã lỗi và thông báo hành động được bằng tiếng Việt.
- Không panic vì input người dùng, metadata lỗi, tệp biến mất hoặc dữ liệu cấu hình hỏng.
- Giữ comment ngắn, giải thích lý do và bất biến. Không lặp lại điều mã đã thể hiện.
- Dùng widget/theme/token Slint hiện có trong `app/ui/widgets.slint` và `app/ui/theme.slint`; không hardcode màu, kích thước hoặc chuỗi UI mới nếu token phù hợp đã tồn tại.
- `app/ui/app.slint` đang lớn và có kế hoạch tách. Khi sửa UI, thay đổi tối thiểu và giữ hành vi; không tách file hàng loạt trong cùng thay đổi chức năng.
- Không thêm dependency nếu standard library hoặc crate hiện có giải quyết được. Nếu thêm, cập nhật `Cargo.lock` bằng Cargo và giải thích lý do.
- Giữ dependency một chiều `app -> files-fs -> files-core`. Không đưa kiến thức UI vào crate dữ liệu.
- Không chỉnh sửa file sinh trong `target/`.

## Tài liệu và trạng thái kế hoạch

- Code và test là nguồn sự thật cuối cùng. Nếu tài liệu mâu thuẫn với code, dừng lại và cập nhật tài liệu trong cùng thay đổi.
- `ROADMAP.md` là nguồn sự thật duy nhất về việc còn thiếu, xếp theo giai đoạn A–L.
- `docs/archive/` chứa các kế hoạch đã hoàn thành (`PLAN`, `REFACTOR_PLAN`, `CODE_REVIEW_PLAN`, `FEATURE_GAP_PLAN`). Chỉ tra cứu lịch sử, không cập nhật.
- Không đánh dấu `[x]` nếu chưa chạy kiểm chứng tương ứng. Mục UI cần kiểm tra thao tác thật hoặc UIA; mục filesystem cần test và môi trường Windows.
- Chỉ ghi số test, số cảnh báo hoặc kích thước build khi vừa đo lại.

## Ưu tiên hiện tại

Thứ tự dưới đây lấy từ `ROADMAP.md`. Kiểm tra lại code trước khi sửa vì mục có thể đã được xử lý.

- Giai đoạn **A** — dọn nợ kỹ thuật: `// SAFETY:` cho `native_drop.rs`, watcher không bỏ sót sự kiện, `git diff --check` trong CI, đồng bộ README.
- Giai đoạn **B** — duyệt và khôi phục Thùng rác (`IShellFolder` + `IContextMenu`).
- Giai đoạn **C** — trang Settings trong UI.
- **D1** — ảo hoá danh sách, đo trước và sau với 10.000–100.000 mục.
- Sau đó theo phản hồi người dùng: D2–D6, E, F, G, H, I, J, K, L.

## Định nghĩa hoàn thành

Một thay đổi chỉ được coi là hoàn tất khi:

1. Có regression test hoặc lý do rõ ràng vì sao không thể tự động hóa.
2. `git diff --check` sạch và thay đổi không tạo thêm lỗi format. Nếu `cargo fmt --all -- --check` đã đỏ ở file ngoài phạm vi từ trước, báo rõ thay vì format hàng loạt file không liên quan.
3. Test tập trung liên quan pass.
4. `cargo test --workspace` pass.
5. `cargo clippy --workspace --all-targets` không có cảnh báo mới.
6. Phần UI/Windows liên quan đã được kiểm tra theo `docs/manual-test.md` hoặc nêu rõ chưa kiểm chứng.
7. Tài liệu, checkbox và mô tả tính năng được cập nhật đúng với kết quả đo.
8. Báo cáo cuối cùng nêu file đã đổi, lệnh đã chạy, kết quả thực tế và phần còn chưa xác minh.

Không tự commit, push, ký, đóng gói hay phát hành nếu người dùng không yêu cầu rõ.
