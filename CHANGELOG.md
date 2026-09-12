# Changelog

本文件记录 OnmyojiSupportTools 的重要变更。格式参考 [Keep a Changelog](https://keepachangelog.com/zh-CN/1.1.0/)，版本遵循 [Semantic Versioning](https://semver.org/lang/zh-CN/)。

## [Unreleased]

### Summary

将原有 Koa/Vue 工作区整体改造为 Windows-first 的 Rust、Tauri 2、Vue 3 单体桌面应用。当前版本号为 `0.1.0`，但尚未打标签或公开发布，以下内容均保留在 Unreleased。

### Added

- 新增 `DeviceManager`，集中管理 ADB 发现、验证、设备状态、配置、截图和点击。
- 新增 MuMu 模拟器 12、雷电模拟器 9、系统 `PATH` 和历史路径的 ADB 候选发现。
- 新增在线、离线、未授权设备展示，单活动设备选择和 IP/端口连接。
- 新增二进制 PNG 截图 IPC、截图尺寸校验、缩放/留白坐标换算和显式点击确认。
- 新增明暗主题、键盘坐标输入、可见焦点、禁用状态和可恢复错误反馈。
- 新增 Rust 与 Vue 自动化测试、Windows CI、NSIS 构建产物。
- 新增 Windows x64 的显式构建命令、原生 CI runner 和 PE 架构校验。
- 新增跨平台 Windows 构建调度器，在 Windows 使用原生 MSVC，并允许 macOS 通过 `cargo-xwin` 构建 x64 NSIS 安装包。
- 新增 Windows 安装包内置的 AOSP ADB 37.0.1、完整第三方 NOTICE、来源记录和构建前哈希校验。
- 新增内置 ADB 候选及外部 ADB 回退入口；没有已保存的用户选择时默认使用内置 ADB。
- 新增 README、贡献指南、领域词汇、ADR、PR 模板和 Bug/Feature Issue Forms。
- 新增可显式开始、停止和重新开始的 H.264 实时预览原型；预览仍保持只选点、显式点击和单活动设备约束。
- 新增单点与矩形点击目标、可持久化随机延时/偏移/按压配置、可取消等待及结构化点击回执。
- 新增本机活动校准、不可还原视觉特征、唯一状态评分和画面尺寸/方向约束。
- 新增由 `DeviceManager` 管理的单活动自动挑战会话、成功结算计数、有限退避、截止时间、已知弹窗白名单和安全暂停/确认恢复。
- 新增活动校准向导、识别配置摘要与任务控制面板。

### Changed

- 仓库从 Lerna/pnpm 多包结构扁平化为根目录 Vue `src/` 与 Rust `src-tauri/`。
- 移除 Koa HTTP 后端和浏览器部署路径，所有设备能力改由受控 Tauri IPC 暴露。
- 工程版本统一为 `0.1.0`，根包设为私有，许可证元数据统一为 MIT。
- 桌面支持范围明确为 Windows 10 22H2 与 Windows 11 x64；macOS 暂不提供应用产物或运行支持。
- 构建仅保留 Windows x64 目标，产物统一使用包含版本、平台和架构的文件名。
- 面向用户的 Windows 发布物统一为带标准卸载入口的 NSIS `.exe` 安装包；Windows 与 macOS 构建完成后都清空 `dist/` 并只保留该安装包。

### Fixed

- 修复截图在 `contain` 缩放和四周留白下的坐标偏移。
- 修复切换设备后旧截图仍可能参与点击校验的问题。
- 修复 ADB 连接退出码成功但输出文本报告连接失败时未返回错误的问题。
- 修复重复截图时旧 Object URL 未及时释放造成的内存增长风险。
- 修复 macOS 执行 `pnpm build` 时因硬编码调用 PowerShell 而立即失败的问题。

### Performance

- 每次截图只启动一个 ADB 进程，并将 PNG 原始字节直接返回前端。
- ADB 操作在 Rust 异步任务中执行，避免阻塞桌面界面。

### Security

- ADB 通过程序路径和结构化参数启动，禁止 shell 拼接。
- 为外部命令增加超时与子进程终止策略。
- 在 Rust 侧校验 ADB、IP、端口、设备在线状态、截图归属和坐标边界。
- ADB 只在构建阶段从固定的 Google 官方地址获取，并在进入安装包前校验归档和每个分发文件的 SHA-256；应用运行时不下载 ADB。
- 应用启动时会复核内置 ADB 分发文件的 SHA-256 和固定版本；缺失、损坏或版本异常时提示重新安装或改选外部 ADB。
- 内置 ADB server 使用应用专用端口 5038，并由 Rust 直接读取 smart-socket `server-status` 核对版本与程序路径；可回收异常退出留下的同版本自有实例，拒绝接管或停止其他 ADB。
- 不引入 OnmyojiAutoScript 的 GPLv3 代码、游戏素材或模板。

[Unreleased]: https://github.com/Bald-M/OnmyojiSupportTools/commits/main
