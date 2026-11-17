//! Real Windows shell icons.
//!
//! Files gets its icons from the Shell (`IShellItemImageFactory`), which is why
//! an `.exe` shows the publisher's own artwork. The same effect is available
//! through `SHGetFileInfoW` plus the system image list: the Shell resolves the
//! file type, follows shortcuts, and reads the embedded icon resource.
//!
//! Only formats where a publisher icon actually exists are asked for. Plain
//! documents keep the Fluent glyph from `format::icon_for`, because rendering a
//! 48×48 bitmap for every `.txt` would cost more than it tells the user.
//!
//! Everything here touches COM-free Win32 and is called from the worker thread.

use files_core::FsError;
use std::path::{Path, PathBuf};

/// Extensions worth asking the Shell for.
///
/// Executables, installers and shortcuts carry a publisher icon; so do the
/// document types Windows ships a handler for. Everything else falls back to the
/// Fluent glyph.
pub fn wants_shell_icon(path: &Path) -> bool {
    let ext = path
        .extension()
        .map(|e| e.to_string_lossy().to_ascii_lowercase())
        .unwrap_or_default();
    matches!(
        ext.as_str(),
        "exe" | "msi"
            | "lnk"
            | "ico"
            | "url"
            | "bat"
            | "cmd"
            | "ps1"
            | "appx"
            | "msix"
            | "pdf"
            | "doc"
            | "docx"
            | "xls"
            | "xlsx"
            | "ppt"
            | "pptx"
            | "zip"
            | "7z"
            | "rar"
            | "mp3"
            | "mp4"
            | "mkv"
            | "avi"
            | "png"
            | "jpg"
            | "jpeg"
            | "gif"
            | "webp"
    )
}

/// A decoded RGBA bitmap ready for Slint.
pub struct RgbaIcon {
    pub width: u32,
    pub height: u32,
    /// Premultiplied? No: straight RGBA, which is what Slint's
    /// `SharedPixelBuffer::clone_from_slice` expects.
    pub pixels: Vec<u8>,
}

/// Initialise COM on the calling thread, once.
///
/// `SHGetFileInfoW` needs an apartment. The app already initialises COM on its
/// main thread, but icon lookups run on the worker, which does not. `RPC_E_CHANGED_MODE`
/// means COM is already up in a different model; that is fine for this call.
#[cfg(windows)]
fn ensure_com() {
    use std::cell::Cell;
    use windows::Win32::System::Com::{CoInitializeEx, COINIT_APARTMENTTHREADED};

    thread_local! {
        static DONE: Cell<bool> = const { Cell::new(false) };
    }
    DONE.with(|done| {
        if done.get() {
            return;
        }
        // SAFETY: pairing every successful CoInitializeEx with an uninit would
        // be wrong here — the thread outlives the call and the worker keeps
        // using the Shell, so COM stays initialised for the thread's lifetime.
        let _ = unsafe { CoInitializeEx(None, COINIT_APARTMENTTHREADED) };
        done.set(true);
    });
}

/// Read the Shell's icon for `path` at `size` pixels.
///
/// Returns `None` when the Shell has no icon, which is not an error: the caller
/// keeps the Fluent glyph.
pub fn shell_icon(path: &Path, size: u32) -> Result<Option<RgbaIcon>, FsError> {
    #[cfg(windows)]
    {
        shell_icon_windows(path, size)
    }
    #[cfg(not(windows))]
    {
        let _ = (path, size);
        Ok(None)
    }
}

#[cfg(windows)]
fn shell_icon_windows(path: &Path, size: u32) -> Result<Option<RgbaIcon>, FsError> {
    use std::os::windows::ffi::OsStrExt;
    use windows::core::PCWSTR;
    use windows::Win32::Graphics::Gdi::{
        CreateCompatibleDC, DeleteDC, DeleteObject, GetDIBits, GetObjectW, BITMAP, BITMAPINFO,
        BITMAPINFOHEADER, BI_RGB, DIB_RGB_COLORS, HGDIOBJ,
    };
    use windows::Win32::UI::Shell::{
        SHGetFileInfoW, SHFILEINFOW, SHGFI_ICON, SHGFI_SMALLICON, SHGFI_LARGEICON,
    };
    use windows::Win32::UI::WindowsAndMessaging::{DestroyIcon, GetIconInfo, ICONINFO};

    // The Shell refuses to hand out icons on a thread that never initialised
    // COM. Skipping this makes `SHGetFileInfoW` return 0 *intermittently*,
    // depending on which thread happens to call it first — which is exactly the
    // kind of flake that hides until a user opens a folder of `.exe` files.
    ensure_com();

    // The Shell picks the closest available size; ask for the bucket that
    // matches what we render.
    let flag = if size <= 16 {
        SHGFI_SMALLICON
    } else {
        SHGFI_LARGEICON
    };

    let mut wide: Vec<u16> = path.as_os_str().encode_wide().collect();
    wide.push(0);

    let mut info = SHFILEINFOW::default();
    // SAFETY: `wide` is a NUL-terminated buffer and `info` is a valid out-param
    // of the documented size.
    let ok = unsafe {
        SHGetFileInfoW(
            PCWSTR(wide.as_ptr()),
            Default::default(),
            Some(&mut info),
            std::mem::size_of::<SHFILEINFOW>() as u32,
            SHGFI_ICON | flag,
        )
    };
    if ok == 0 || info.hIcon.is_invalid() {
        return Ok(None);
    }
    let hicon = info.hIcon;

    // An HICON must be turned into pixels; `GetIconInfo` gives us the colour
    // bitmap, and `GetDIBits` copies it out bottom-up as BGRA.
    let result = (|| -> Result<Option<RgbaIcon>, FsError> {
        let mut icon_info = ICONINFO::default();
        // SAFETY: `hicon` came from SHGetFileInfoW and is alive until DestroyIcon.
        unsafe { GetIconInfo(hicon, &mut icon_info) }
            .map_err(|e| FsError::new("icon_failed", format!("GetIconInfo: {e}")))?;

        let hbm = icon_info.hbmColor;
        if hbm.is_invalid() {
            if !icon_info.hbmMask.is_invalid() {
                let _ = unsafe { DeleteObject(HGDIOBJ(icon_info.hbmMask.0)) };
            }
            return Ok(None);
        }

        let mut bmp = BITMAP::default();
        // SAFETY: `hbm` is a valid bitmap handle from GetIconInfo.
        let got = unsafe {
            GetObjectW(
                HGDIOBJ(hbm.0),
                std::mem::size_of::<BITMAP>() as i32,
                Some(&mut bmp as *mut _ as *mut _),
            )
        };
        if got == 0 {
            let _ = unsafe { DeleteObject(HGDIOBJ(hbm.0)) };
            let _ = unsafe { DeleteObject(HGDIOBJ(icon_info.hbmMask.0)) };
            return Ok(None);
        }

        let w = bmp.bmWidth as u32;
        let h = bmp.bmHeight as u32;
        if w == 0 || h == 0 {
            let _ = unsafe { DeleteObject(HGDIOBJ(hbm.0)) };
            let _ = unsafe { DeleteObject(HGDIOBJ(icon_info.hbmMask.0)) };
            return Ok(None);
        }

        let mut bi = BITMAPINFO {
            bmiHeader: BITMAPINFOHEADER {
                biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
                biWidth: w as i32,
                // Negative height asks for a top-down buffer, which avoids a flip.
                biHeight: -(h as i32),
                biPlanes: 1,
                biBitCount: 32,
                biCompression: BI_RGB.0,
                ..Default::default()
            },
            ..Default::default()
        };

        let mut buf = vec![0u8; (w * h * 4) as usize];
        // SAFETY: the DC is only used for the DIB copy, and `buf` is sized for
        // the requested dimensions.
        let copied = unsafe {
            let dc = CreateCompatibleDC(None);
            let n = GetDIBits(
                dc,
                hbm,
                0,
                h,
                Some(buf.as_mut_ptr() as *mut _),
                &mut bi,
                DIB_RGB_COLORS,
            );
            let _ = DeleteDC(dc);
            n
        };

        let _ = unsafe { DeleteObject(HGDIOBJ(hbm.0)) };
        let _ = unsafe { DeleteObject(HGDIOBJ(icon_info.hbmMask.0)) };

        if copied == 0 {
            return Ok(None);
        }

        // GetDIBits gives BGRA; Slint wants RGBA. Straight swap, alpha kept.
        for px in buf.as_chunks_mut::<4>().0 {
            px.swap(0, 2);
        }

        Ok(Some(RgbaIcon { width: w, height: h, pixels: buf }))
    })();

    let _ = unsafe { DestroyIcon(hicon) };
    result
}

/// Everything the caller needs to cache an icon lookup.
pub fn cache_key(path: &Path) -> PathBuf {
    path.to_path_buf()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn executables_and_installers_want_a_shell_icon() {
        assert!(wants_shell_icon(Path::new(r"C:\x\app.exe")));
        assert!(wants_shell_icon(Path::new(r"C:\x\setup.msi")));
        assert!(wants_shell_icon(Path::new(r"C:\x\shortcut.lnk")));
    }

    #[test]
    fn plain_text_does_not_pay_for_a_bitmap() {
        // A folder of 5000 .txt files must not decode 5000 bitmaps.
        assert!(!wants_shell_icon(Path::new(r"C:\x\notes.txt")));
        assert!(!wants_shell_icon(Path::new(r"C:\x\data.json")));
        assert!(!wants_shell_icon(Path::new(r"C:\x\main.rs")));
    }

    #[test]
    fn the_extension_match_ignores_case() {
        assert!(wants_shell_icon(Path::new(r"C:\x\APP.EXE")));
        assert!(wants_shell_icon(Path::new(r"C:\x\Photo.PNG")));
    }

    #[test]
    fn a_file_without_extension_is_not_requested() {
        assert!(!wants_shell_icon(Path::new(r"C:\x\LICENSE")));
    }
}
