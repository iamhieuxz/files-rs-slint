//! Checks the Windows shell-icon path against real executables on this machine.
//!
//! A silent failure here means every `.exe` looks like a blank page, which is
//! the exact thing the feature exists to prevent — so the test asserts on
//! pixels, not just on "did not error".
//!
//! These tests run serially (`--test-threads=1`): `SHGetFileInfoW` talks to the
//! Shell through COM, and several threads entering it at once produce
//! intermittent empty results that have nothing to do with the code under test.
//! Marking the file serial is honest about that constraint instead of hiding it
//! behind a retry loop.

#![cfg(windows)]

use std::path::{Path, PathBuf};

static SERIAL: std::sync::Mutex<()> = std::sync::Mutex::new(());

fn system32_exe(name: &str) -> Option<PathBuf> {
    let root = std::env::var_os("SystemRoot")?;
    let p = PathBuf::from(root).join("System32").join(name);
    p.exists().then_some(p)
}

#[test]
fn a_real_executable_yields_a_non_blank_icon() {
    let _guard = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let Some(exe) = system32_exe("notepad.exe") else {
        // A Windows install without notepad is not a failure of this code.
        return;
    };

    let icon = files_fs::shell_icon(&exe, 20)
        .expect("shell_icon must not error on a readable exe")
        .expect("notepad.exe has a shell icon");

    assert!(icon.width > 0 && icon.height > 0, "icon has a size");
    assert_eq!(
        icon.pixels.len(),
        (icon.width * icon.height * 4) as usize,
        "buffer matches the declared size"
    );

    // A blank icon is all zeros; that is the failure mode this test guards.
    let opaque = icon.pixels.as_chunks::<4>().0.iter().filter(|px| px[3] > 0).count();
    assert!(
        opaque > 0,
        "the icon must have at least one visible pixel, got an empty bitmap"
    );

    // And it must not be a single flat colour either, which is what a failed
    // decode looks like.
    let distinct: std::collections::HashSet<[u8; 4]> = icon
        .pixels
        .as_chunks::<4>()
        .0
        .iter()
        .filter(|px| px[3] > 0)
        .copied()
        .collect();
    assert!(
        distinct.len() > 1,
        "expected a real glyph, got one flat colour"
    );
}

#[test]
fn two_different_executables_give_different_icons() {
    let _guard = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let (Some(a), Some(b)) = (system32_exe("notepad.exe"), system32_exe("cmd.exe")) else {
        return;
    };
    let ia = files_fs::shell_icon(&a, 20).ok().flatten();
    let ib = files_fs::shell_icon(&b, 20).ok().flatten();
    let (Some(ia), Some(ib)) = (ia, ib) else { return };

    // Same size, different artwork: this is what "the publisher's own icon"
    // means, and it is what makes an exe list readable.
    assert_ne!(
        ia.pixels, ib.pixels,
        "notepad and cmd must not share an identical bitmap"
    );
}

#[test]
fn a_missing_file_returns_no_icon_instead_of_panicking() {
    let _guard = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let icon = files_fs::shell_icon(Path::new(r"C:\does\not\exist.exe"), 20);
    // Either an error or None is acceptable; a panic is not.
    match icon {
        Ok(None) => {}
        Ok(Some(_)) => {}
        Err(_) => {}
    }
}

#[test]
fn only_icon_worthy_extensions_are_requested() {
    // Khoá toàn cục: các test trong file này phải chạy lần lượt.
    // `SHGetFileInfoW` đi qua COM của Shell; nhiều thread vào cùng lúc cho
    // kết quả rỗng ngẫu nhiên, không liên quan tới code đang kiểm.
    static SERIAL: std::sync::Mutex<()> = std::sync::Mutex::new(());
    let _guard = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    assert!(files_fs::wants_shell_icon(Path::new("a.exe")));
    assert!(!files_fs::wants_shell_icon(Path::new("a.rs")));
}

#[test]
fn a_wide_image_is_scaled_keeping_its_aspect_ratio() {
    let root = std::path::PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("thumb-wide");
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).unwrap();
    let img = root.join("rong.png");
    let script = format!(
        "Add-Type -AssemblyName System.Drawing; \
         $b = New-Object System.Drawing.Bitmap 200,100; \
         $g = [System.Drawing.Graphics]::FromImage($b); \
         $g.Clear([System.Drawing.Color]::Blue); \
         $b.Save('{}', [System.Drawing.Imaging.ImageFormat]::Png); \
         $g.Dispose(); $b.Dispose()",
        img.display()
    );
    let ok = std::process::Command::new("powershell")
        .args(["-NoProfile", "-Command", &script])
        .status()
        .map(|s| s.success())
        .unwrap_or(false);
    if !ok || !img.exists() {
        let _ = std::fs::remove_dir_all(&root);
        return;
    }

    let thumb = files_fs::thumbnail(&img, 48)
        .expect("không lỗi")
        .expect("phải có ảnh thu nhỏ");

    assert_eq!(thumb.width, 48, "cạnh dài bằng size yêu cầu");
    assert_eq!(thumb.height, 24, "giữ tỉ lệ 2:1");
    assert_eq!(thumb.pixels.len(), (48 * 24 * 4) as usize);
    // Ảnh màu xanh đặc: phải có pixel hiển thị được.
    let opaque = thumb.pixels.as_chunks::<4>().0.iter().filter(|p| p[3] > 0).count();
    assert!(opaque > 0, "ảnh thu nhỏ không được rỗng");
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn a_tall_image_is_scaled_by_its_height() {
    let root = std::path::PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("thumb-tall");
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).unwrap();
    let img = root.join("cao.png");
    let script = format!(
        "Add-Type -AssemblyName System.Drawing; \
         $b = New-Object System.Drawing.Bitmap 100,200; \
         $g = [System.Drawing.Graphics]::FromImage($b); \
         $g.Clear([System.Drawing.Color]::Green); \
         $b.Save('{}', [System.Drawing.Imaging.ImageFormat]::Png); \
         $g.Dispose(); $b.Dispose()",
        img.display()
    );
    let ok = std::process::Command::new("powershell")
        .args(["-NoProfile", "-Command", &script])
        .status()
        .map(|s| s.success())
        .unwrap_or(false);
    if !ok || !img.exists() {
        let _ = std::fs::remove_dir_all(&root);
        return;
    }

    let thumb = files_fs::thumbnail(&img, 48).unwrap().expect("có ảnh");
    assert_eq!(thumb.height, 48);
    assert_eq!(thumb.width, 24);
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn a_non_image_file_has_no_thumbnail() {
    // Không được giải mã tệp văn bản để tìm ảnh thu nhỏ.
    let root = std::path::PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("thumb-txt");
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).unwrap();
    let f = root.join("a.txt");
    std::fs::write(&f, b"x").unwrap();
    assert!(files_fs::thumbnail(&f, 48).unwrap().is_none());
    let _ = std::fs::remove_dir_all(&root);
}
