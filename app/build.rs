// build.rs — biên dịch UI và nhúng tài nguyên Windows.
//
// Hai việc:
//   1. Slint biên dịch `ui/app.slint`.
//   2. Nhúng icon + manifest vào .exe. Thiếu manifest thì Windows không biết
//      app cần quyền administrator; thiếu icon thì taskbar hiện icon mặc định.

fn main() {
    slint_build::compile("ui/app.slint").expect("failed to compile Slint UI");

    #[cfg(windows)]
    embed_windows_resources();
}

#[cfg(windows)]
fn embed_windows_resources() {
    // `winres` chỉ chạy khi build trên Windows; nó gọi rc.exe để gắn tài nguyên.
    let mut res = winres::WindowsResource::new();
    res.set_icon("assets/files-rs.ico");
    // Manifest khai báo quyền và độ phân giải cao; xem app.manifest.
    res.set_manifest_file("app.manifest");
    res.set("ProductName", "Files RS");
    res.set("FileDescription", "Trình quản lý tệp cho Windows");
    // VERSIONINFO dùng bốn số; Cargo chỉ có ba (1.0.0) nên thêm build 0.
    let version = format!("{}.0", env!("CARGO_PKG_VERSION"));
    res.set("FileVersion", &version);
    res.set("ProductVersion", &version);
    res.set("LegalCopyright", "MIT");
    if let Err(e) = res.compile() {
        // Thiếu rc.exe là chuyện của môi trường build, không phải của mã.
        // In cảnh báo để người build biết exe sẽ không có icon/manifest.
        println!("cargo:warning=không nhúng được tài nguyên Windows: {e}");
    }
}
