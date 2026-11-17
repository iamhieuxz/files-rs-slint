# files-rs-slint 实施计划

目标：用 Rust + Slint 做一个 Windows 文件管理器的可演示版本（demo）。参考对象是 `files-community/Files`（MIT），仅借鉴架构与交互，不照搬 C# 实现。

## 1. 范围

### 本期包含（MVP / demo）
- 左侧边栏：驱动器列表、常用文件夹（Home、Desktop、Documents、Downloads、Pictures、Music、Videos）。
- 顶部工具栏：后退、前进、上一级、刷新，以及地址栏（可直接输入路径回车跳转）。
- 主区域：文件列表，列为 名称 / 修改日期 / 类型 / 大小。
- 点击列头排序（升序/降序切换），文件夹始终排在文件前面。
- 双击文件夹进入；双击文件用系统默认程序打开。
- 显示/隐藏隐藏文件（以 `files-core` 的规则为准）。
- 目录变化自动刷新（基于 `files-fs::watch_directory`）。
- 底部状态栏：条目数、隐藏条目数、所选大小、错误信息。
- 无权限或路径不存在时显示错误，不崩溃。

### 本期不做（写入"已知限制"）
- 复制、移动、删除、重命名、新建（需要 `IFileOperation`/回收站，放到阶段 2）。
- 多标签页、拖放、右键菜单、预览窗格、缩略图、压缩包、FTP、Git。
- 回收站、网络位置、MTP 设备。
- 系统主题与 Mica 材质（Slint 不提供，使用自绘配色）。

## 2. 架构

```
files-rs-slint/
  Cargo.toml              workspace
  PLAN.md                 文件本身
  crates/
    files-core/           纯数据模型：Entry、Listing、排序、路径、错误（无 OS 依赖）
    files-fs/             唯一访问 OS 的层：读目录、驱动器、监视目录
  app/
    build.rs              编译 ui/app.slint
    ui/app.slint          界面声明（无业务逻辑）
    src/main.rs           装配：状态、回调、刷新线程
    src/navigation.rs     后退/前进历史栈（纯逻辑，可测试）
    src/format.rs         大小、日期、类型文本格式化（可测试）
```

依赖方向：`app -> files-fs -> files-core`，UI 只通过 `app/src/main.rs` 与模型交互。

## 3. 数据流

1. 用户操作（点击、双击、按钮）→ Slint 回调 → Rust 处理函数。
2. 处理函数调用 `files_fs::read_dir` 得到 `Listing`（已排序、已过滤）。
3. 把 `Listing` 转为 Slint 的 `VecModel<FileRow>`，写回 `AppWindow` 属性。
4. 目录监视在后台线程产生事件，通过 `slint::invoke_from_event_loop` 回到 UI 线程刷新。

所有 Slint 对象只在 UI 线程访问。

## 4. 里程碑与验收标准

| 阶段 | 内容 | 验收 |
|---|---|---|
| M1 | 工作区可编译，`files-core` / `files-fs` 测试通过 | `cargo test -p files-core -p files-fs` 全部通过 |
| M2 | 空白窗口 + 布局（侧边栏/工具栏/列表/状态栏）可启动 | `cargo run -p files-rs-app` 显示窗口 |
| M3 | 列表接入真实目录，显示 Home 内容 | 列表显示真实文件名、大小、日期 |
| M4 | 导航：双击进入、上一级、后退/前进、地址栏跳转、侧边栏点击 | 手动操作全部生效，历史栈有单元测试 |
| M5 | 排序（列头点击）、隐藏文件开关 | 排序结果与 `files-core` 测试一致 |
| M6 | 目录监视自动刷新 | 在目录新建文件后 1 秒内列表出现 |
| M7 | 错误处理：不存在路径、无权限目录 | 状态栏显示错误，窗口不崩溃 |
| M8 | 打磨与文档：配色、字体、README 中的运行说明 | 可演示 |

每个里程碑完成后执行 `cargo build` 检查，不通过则先修复再继续。

## 5. 风险

- Slint 的 Windows 构建需要 MSVC 工具链；若失败需先确认 `link.exe` 可用。
- `files-fs` 的 `watch_directory` 在部分网络盘上可能无事件，需要在界面上保留手动刷新按钮。
- 隐藏文件判断：`files-fs` 目前只用点前缀，未读取 Windows 隐藏属性，属于已知缺口。
- 日期使用本地时区格式化，需确认 `chrono` 的 `clock` 特性在 Windows 下可用。

## 6. 后续（不在本期）

阶段 2：复制/移动/删除（`IFileOperation` + 进度）。
阶段 3：多标签页、右键菜单、预览窗格。
阶段 4：压缩包、Git、FTP。
