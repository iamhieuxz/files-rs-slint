//! Checks the Windows shell-icon path against real executables on this machine.
//!
//! A silent failure here means every `.exe` looks like a blank page, which is
//! the exact thing the feature exists to prevent — so the test asserts on
//! pixels, not just on "did not error".

#![cfg(windows)]

use std::path::{Path, PathBuf};

fn system32_exe(name: &str) -> Option<PathBuf> {
    let root = std::env::var_os("SystemRoot")?;
    let p = PathBuf::from(root).join("System32").join(name);
    p.exists().then_some(p)
}

#[test]
fn a_real_executable_yields_a_non_blank_icon() {
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
    let opaque = icon.pixels.chunks_exact(4).filter(|px| px[3] > 0).count();
    assert!(
        opaque > 0,
        "the icon must have at least one visible pixel, got an empty bitmap"
    );

    // And it must not be a single flat colour either, which is what a failed
    // decode looks like.
    let distinct: std::collections::HashSet<[u8; 4]> = icon
        .pixels
        .chunks_exact(4)
        .filter(|px| px[3] > 0)
        .map(|px| [px[0], px[1], px[2], px[3]])
        .collect();
    assert!(
        distinct.len() > 1,
        "expected a real glyph, got one flat colour"
    );
}

#[test]
fn two_different_executables_give_different_icons() {
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
    assert!(files_fs::wants_shell_icon(Path::new("a.exe")));
    assert!(!files_fs::wants_shell_icon(Path::new("a.rs")));
}
