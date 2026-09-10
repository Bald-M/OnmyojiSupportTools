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
- 新增 Rust 与 Vue 自动化测试、Windows CI、NSIS 与便携 ZIP 构建产物。
- 新增 Windows x64 与 Windows ARM64 的显式构建命令、原生 CI runner 和 PE 架构校验。
- 新增跨平台 Windows 构建调度器，在 Windows 使用原生 MSVC，并允许 macOS 通过 `cargo-xwin` 构建 x64 与 ARM64 便携产物。
- 新增 README、贡献指南、领域词汇、ADR、PR 模板和 Bug/Feature Issue Forms。

### Changed

- 仓库从 Lerna/pnpm 多包结构扁平化为根目录 Vue `src/` 与 Rust `src-tauri/`。
- 移除 Koa HTTP 后端和浏览器部署路径，所有设备能力改由受控 Tauri IPC 暴露。
- 工程版本统一为 `0.1.0`，根包设为私有，许可证元数据统一为 MIT。
- 桌面支持范围明确为 Windows 10 22H2 与 Windows 11 x64；macOS 暂不提供应用产物或运行支持。
- 构建产物统一使用包含版本、平台和架构的文件名；Windows ARM64 暂列为实验性目标。

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
- 不捆绑或下载 ADB，不引入 OnmyojiAutoScript 的 GPLv3 代码或游戏素材。

[Unreleased]: https://github.com/Bald-M/OnmyojiSupportTools/commits/main
