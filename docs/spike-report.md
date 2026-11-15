# 可行性 spike 报告：KDE Plasma 6 Wayland 剪贴板中枢

- 日期：2026-11-16
- 目标：验证 clipdeck 在 KDE Plasma 6 Wayland 上做「剪贴板中枢」的技术可行性
- 测试环境：Debian 13 (trixie) + Plasma/KWin 6.3.6 + Wayland（`WAYLAND_DISPLAY=wayland-0`）+ fcitx5；Klipper 内嵌于 plasmashell（无独立进程）
- 结论先行：**GO**。方案 A（wl-clipboard-rs 常驻监听 data-control 协议）已在本机真实 Plasma 会话中完整跑通（文本 + 图片）；全局热键是最大短板，需走 portal/KGlobalAccel 自行实现。

---

## 问题 1：Rust 生态的 Wayland data-control crate

**结论：`wl-clipboard-rs` 是现成且生产可用的选择。**

| 项目 | 数据（2026-11-16 实查 GitHub / crates.io API） |
| --- | --- |
| 版本 | 0.9.5（max_version == newest_version） |
| 维护状态 | 活跃：最后 push 2026-10-26（两周前），非 archived，open issues 仅 8 |
| stars | 530 |
| 下载量 | 26,318,904 |
| 许可 | Apache-2.0 |

关键能力：
- 自 v0.9.2 起（2024-10）支持 **ext-data-control-v1**，且在 compositor 同时提供两种协议时**自动优先使用 ext-data-control-v1**，否则回落 wlr-data-control。常规剪贴板要求：ext-data-control 任意版本，或 wlr-data-control v1–2；primary selection 要求：ext-data-control 任意版本，或 wlr-data-control v2。
- 0.9.5 提供 `watch` 模块：`Watcher::new(ClipboardType, Seat)` + `next_event()` 返回 `Changed { clipboard, mime_types: Vec<String>, offer: Offer }` / `Cleared`，`Offer::receive(mime)` 返回可 `read_to_end` 的 `PipeReader`；支持 `cancel_handle()` 跨线程取消。这正是「常驻监听」所需的事件驱动 API，无需自己写协议层。
- 生态里的替代品 `wayland-clipboard-listener` 也默认走 ext-data-control-v1，但 wl-clipboard-rs 更成熟（下载量级更高、API 覆盖 copy/paste/watch 全套），选它。

**本机实测（重要发现）**：KWin 6.3.6 的 Wayland globals 里只有 `zwlr_data_control_manager_v1` version 2，**尚无 ext-data-control-v1**（KDE 在更高版本才引入）。wl-clipboard-rs 在此回落到 wlr-data-control v2，实测一切正常（见问题 2 / spike 输出）；将来 Plasma 升级提供 ext-data-control-v1 时，crate 自动切换，代码无需改动。

## 问题 2：wl-clipboard-rs 能否拿图片 MIME（image/png）

**结论：能。协议本身 MIME 无关，crate API 直接暴露任意 MIME；本机实测通过。**

- 写侧：`copy::Options::new().source(Source::Bytes(..)).mime_type(MimeType::Specific("image/png".into()))`。
- 读侧：`paste::get_contents(.., MimeType::Specific("image/png"))` 或 watch 的 `offer.receive("image/png")` → `PipeReader`（PNG 字节流）。
- **实测**：构造 8x8 纯红 PNG（75 字节）→ `wl-copy < /tmp/spike-test.png` → `wl-paste --list-types` 只显示 `image/png`；spike 程序捕获到 `changed, mimes = ["image/png"]`，`receive("image/png")` 读回 75 字节且 PNG magic 校验 `true`。
- 附带发现：KWin 的文本 offer 会带重复的 `text/plain` 以及 X11 兼容 MIME（`TEXT`/`STRING`/`UTF8_STRING`），MIME 选择逻辑需要按优先级过滤（spike 的 `pick_text_mime` 已处理）。

## 问题 3：全局热键在 KDE Wayland 的注册路径

**结论：`tauri-plugin-global-shortcut`（stable 2.4.1）在原生 KDE Wayland 下不可用；需绕道。**

- 该插件底层用 `global-hotkey` crate，后者在 Linux 仅支持 X11（XGrabKey）。Wayland 安全模型禁止应用私下全局抓键，`tauri-apps/global-hotkey#28` 与 `tauri#3578` 长期挂起。
- 可行路径（按推荐顺序）：
  1. **XDG Desktop Portal GlobalShortcuts（推荐）**：本机实测 `org.freedesktop.portal.Desktop /org/freedesktop/portal/desktop` 上存在 `org.freedesktop.portal.GlobalShortcuts`（`version = 1`，xdg-desktop-portal 1.20.3 + xdg-desktop-portal-kde）。流程：`CreateSession` → `BindShortcuts`（KDE 弹系统设置确认框，用户为快捷键选择按键）→ 监听 `Activated` 信号。缺点：快捷键由用户在系统设置中分配（首次 UX 多一步）；Tauri 插件未内置此能力，需用 `zbus` 自写 portal 客户端（工作量可控，接口仅 5 个方法 + 3 个信号）。
  2. **KDE 专有 `org.kde.KGlobalAccel` D-Bus**：本机实测接口存在（`/kglobalaccel`，含 `getComponent`/`setShortcut` 族）。可编程注册、不经用户确认，但属 KDE 私有 API，跨发行版不通用，且接口细节文档少。
  3. **XWayland 回退**：以 X11 模式跑 Tauri 窗口 + KDE 设置「旧式 X11 应用支持 → 允许捕获所有按键」，`tauri-plugin-global-shortcut` 即可用。不推荐：体验差、依赖用户改系统设置、GNOME Wayland 已无 X11 回退。
- 对 clipdeck 的含义：热键功能需自研（zbus + portal），或先期用 KDE 系统设置里由用户手动配置的「命令/DBus 调用」触发（KDE 自带快捷键可绑定任意命令），MVP 可先支持后者。

## 问题 4：Klipper D-Bus 接口实测

**结论：文本可用、图片不可用（验证了预期）；只适合做文本兜底。**

- 服务与对象：`org.kde.klipper`，对象 `/klipper`，接口 `org.kde.klipper.klipper`，由 **plasmashell（pid 内嵌，无独立 klipper 进程）** 提供。gdbus introspect 确认方法族：`getClipboardContents / setClipboardContents / clearClipboardContents / clearClipboardHistory / saveClipboardHistory / getClipboardHistoryMenu(as) / getClipboardHistoryItem(i)→s / showKlipperPopupMenu / reloadConfig`，信号 `clipboardHistoryUpdated()`。
- 文本实测：`gdbus call ... getClipboardContents` 返回当前剪贴板文本（当时为一段 URL，中文场景未测出问题，UTF-8 正常）。
- **图片实测（关键）**：`wl-copy < 8x8 红色 PNG` 后 `wl-paste --list-types` 仅 `image/png`，再次调用 `getClipboardContents` **仍返回旧文本**，Klipper 未收录该纯图片 offer → Klipper D-Bus 无法提供图片内容。
- 其它限制：无「当前内容变化」的逐条推送（仅有粗粒度 `clipboardHistoryUpdated` 信号，需拉取比对）；`getClipboardHistoryItem` 只能拿字符串（图片条目为空/不可读）；接口无版本属性，属 Plasma 私有实现，跨版本稳定性由 KDE 自负。
- 附加发现：Klipper 的 offer 里出现 `application/x-kde-onlyReplaceEmpty`，说明 Klipper 在监听 data-control——它自己是协议客户端，但不通过 D-Bus 转发图片。

## spike 实测记录

spike 项目：`spike/`（独立 cargo 项目 `clipdeck-spike`，不依赖 tauri；`src/mime_logic.rs` 纯逻辑 3 个单测，`src/main.rs` 常驻监听）。

- `cargo test --release`：**3 passed**（MIME 分类、文本 MIME 优先级、图片 MIME 优先级）。
- 真实 Plasma Wayland 会话运行 12 秒，期间依次复制「文本 → 图片 → 中文文本」（`wl-copy` 模拟用户复制），**4 个事件全部实时捕获、内容正确**：

```text
=== stdout ===
[regular] changed, mimes = ["image/png"]
[regular] image (image/png) = 75 bytes, png_magic=true
[regular] changed, mimes = ["text/plain", "text/plain", "text/plain;charset=utf-8", "TEXT", "STRING", "UTF8_STRING"]
[regular] text (text/plain;charset=utf-8) = "clipdeck spike 文本复制测试 - hello from wl-clipboard-rs"
[regular] changed, mimes = ["image/png"]
[regular] image (image/png) = 75 bytes, png_magic=true
[regular] changed, mimes = ["text/plain", "text/plain", "text/plain;charset=utf-8", "TEXT", "STRING", "UTF8_STRING"]
[regular] text (text/plain;charset=utf-8) = "second copy - 中文内容第二次复制"
```

（第一条为程序启动时对「当前剪贴板状态」的即时报告；文本 MIME 的重复项来自 KWin 对 XWayland/Wayland offer 的合并。）

## 推荐架构

- **方案 A（主干）：wl-clipboard-rs `watch::Watcher` 常驻监听 data-control 协议**
  - 事件驱动、全 MIME（文本 + 图片 + 富文本），不依赖 Klipper 是否运行；KDE 未来从 wlr-data-control 切到 ext-data-control-v1 时 crate 自动适配。
  - 集成方式：在 Tauri 的 Rust 侧起专用线程跑 `next_event()` 循环，经 channel 推给 UI/存储层；`cancel_handle` 用于应用退出。
- **方案 B（兜底）：Klipper D-Bus（getClipboardContents / getClipboardHistoryItem + clipboardHistoryUpdated 信号）**
  - 仅文本；用于 data-control 不可用的极端环境、或需要与 Klipper 历史对齐的场景。不承载图片，不承载变化推送。
- **热键子系统：zbus 自实现 XDG GlobalShortcuts portal 客户端**（KDE 专有 KGlobalAccel 作为备选），MVP 可接受「KDE 系统设置绑定命令」的降级路径。
- 复用 spike 产物的 `mime_logic.rs`（分类/优先级）作为正式代码起点。

## Go / No-Go

**GO**——核心链路（监听变化 + 读文本 + 读图片）在本机真实 Plasma 6.3.6 Wayland 会话零改造跑通，依赖 crate 活跃维护。前置条件：热键走 portal 自研；Plasma 大版本升级后跑一次 spike 冒烟。

## 风险清单

| # | 风险 | 等级 | 缓解 |
| --- | --- | --- | --- |
| 1 | KDE 协议演进：Plasma 6.5 beta 曾因协议变更使 CopyQ 整体失效；本机 KWin 6.3.6 只有 wlr-data-control v2，无 ext-data-control-v1 | 中 | wl-clipboard-rs 双协议自动选择且活跃维护；每次 Plasma 大版本升级跑 spike 冒烟测试；升级到带 ext-data-control 的版本后复测 primary/图片 |
| 2 | tauri-plugin-global-shortcut 在 Wayland 不可用（global-hotkey 仅 X11），热键需自研 portal 客户端 | 中高 | 用 zbus 实现 GlobalShortcuts portal（本机实测接口存在 v1）；首次绑定依赖用户在 KDE 系统设置确认，UX 需引导页 |
| 3 | GlobalShortcuts portal 实现较老（version=1），KDE 对 portal 行为仍在演进 | 低 | 封装隔离 portal 调用，便于跟进接口版本变化 |
| 4 | Klipper D-Bus 无图片、无逐条变化推送、私有接口 | 低 | 已定位为兜底路径，不承载关键能力 |
| 5 | KWin offer MIME 冗余/重复（两个 text/plain + X11 族 MIME），MIME 选择写错会导致乱码/读错 | 低 | 沿用 spike 的优先级选择逻辑并保留单测 |
| 6 | 大图/高频复制场景：每次 Changed 都 receive 读数据可能卡顿 | 中 | 正式版对非白名单 MIME 懒读取（先记 MIME 列表，按需读内容）、限制单条读取上限 |
| 7 | fcitx5/选中即变：primary selection 变化极高频 | 低 | 主监听只挂 Regular（spike 即如此）；如需 primary 再加节流 |
| 8 | 沙箱化（Flatpak）下 data-control 不可用 | 低 | 目标形态为 deb/原生包；若上 Flatpak 再评估 portal clipboard |

## 参考

- wl-clipboard-rs：<https://github.com/YaLTeR/wl-clipboard-rs>（CHANGELOG：v0.9.2 加入 ext-data-control 支持）· <https://crates.io/crates/wl-clipboard-rs> · <https://docs.rs/wl-clipboard-rs>
- global-hotkey Wayland 问题：<https://github.com/tauri-apps/global-hotkey/issues/28> · Tauri 追踪：<https://github.com/tauri-apps/tauri/issues/3578>
- XDG GlobalShortcuts portal：`org.freedesktop.portal.GlobalShortcuts`（本机 introspect 实测）
- Klipper D-Bus：`org.kde.klipper /klipper org.kde.klipper.klipper`（本机 gdbus introspect/实测）
