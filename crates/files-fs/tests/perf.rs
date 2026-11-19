//! Benchmark tối thiểu cho đường đọc thư mục ở kích thước lớn.
//!
//! Tạo 10.000 tệp mất chưa tới một giây trên máy phát triển, nên test chạy
//! trong bộ test thường; ngưỡng thời gian rất rộng để không flaky.

use files_core::{ListOptions, SortBy, SortDirection, SortSpec};
use std::path::PathBuf;
use std::time::{Duration, Instant};

fn scratch(name: &str) -> PathBuf {
    let root = PathBuf::from(env!("CARGO_TARGET_TMPDIR"));
    let dir = root.join(format!("perf-{name}"));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("create scratch dir");
    dir
}

#[test]
fn listing_and_sorting_ten_thousand_files_stays_bounded() {
    let dir = scratch("10k");

    for i in 0..10_000 {
        std::fs::File::create(dir.join(format!("f{i:05}.txt"))).unwrap();
    }
    for i in 0..100 {
        std::fs::create_dir(dir.join(format!("dir{i:03}"))).unwrap();
    }

    let spec = SortSpec {
        by: SortBy::Name,
        direction: SortDirection::Ascending,
        folder_order: files_core::sort::FolderOrder::FoldersFirst,
    };
    let opts = ListOptions {
        show_hidden: true,
        show_dotfiles: true,
    };
    let start = Instant::now();
    let listing = files_fs::read_dir(&dir, spec, opts).expect("list 10k");
    let elapsed = start.elapsed();

    assert_eq!(listing.entries.len(), 10_100);
    // 10 giây là ngưỡng rất rộng: mục đích là bắt thuật toán bậc hai hoặc
    // truy cập mạng, không phải đo hiệu năng tuyệt đối của máy.
    assert!(
        elapsed < Duration::from_secs(10),
        "listing 10k took {elapsed:?}"
    );

    // Sắp xếp thư mục trước, trong mỗi nhóm theo tên.
    let first = &listing.entries[0];
    assert!(first.is_dir(), "folders first must put a folder at the top");
    assert_eq!(first.name, "dir000");
    let last = listing.entries.last().unwrap();
    assert!(!last.is_dir());
    assert_eq!(last.name, "f09999.txt");

    let _ = std::fs::remove_dir_all(&dir);
}
