# OnmyojiSupportTools

OnmyojiSupportTools 是一个面向 Windows 的 Android 模拟器辅助桌面应用。目前的 `0.1.0` 尚未公开发布，首个里程碑专注于安全、可检查地完成 ADB 连接、截图取点与点击。

应用采用 Rust、Tauri 2 和 Vue 3。界面打包在桌面程序内，通过 Tauri IPC 与 Rust 通信；运行时不会启动 HTTP 服务，也不支持浏览器部署。

## 当前能力

- 从 MuMu 模拟器 12、雷电模拟器 9、系统 `PATH` 和上次配置中发现 ADB。
- 验证并选择同一个 `adb.exe` 管理服务器、设备、截图与点击。
- 展示在线、离线及未授权设备；仅允许选择在线设备。
- 通过 IP 地址和端口请求 `adb connect`。
- 手动获取 PNG 截图，在缩放或留白后的画面中准确换算坐标。
- 单击画面只选择坐标；只有明确按下“执行点击”才向设备发送操作。

以下能力不属于 `0.1.0`：多设备并发、实时预览、图像识别、OCR、任务运行时、调度器、插件、远程控制、自动更新和 macOS 发布。

## 系统要求

| 项目 | 支持范围 |
| --- | --- |
| Windows | Windows 10 22H2 / Windows 11 x64；Windows 11 ARM64 为实验性构建目标 |
| 模拟器 | MuMu 模拟器 12、雷电模拟器 9 |
| WebView | NSIS 安装包可按需调用微软 WebView2 引导程序；便携版要求系统已安装 WebView2 Runtime |
| ADB | 由用户提供；可使用模拟器自带 ADB 或 Android SDK Platform-Tools |

macOS 目前不在构建和支持范围内。核心模块保留跨平台边界，待 Windows 版本稳定后再评估适配。

### 构建目标

| 平台 | 架构 | Rust target | CI runner | 状态 |
| --- | --- | --- | --- | --- |
| Windows | x64 | `x86_64-pc-windows-msvc` | `windows-2025` | 主要支持目标 |
| Windows | ARM64 | `aarch64-pc-windows-msvc` | `windows-11-arm` | 实验性，需继续完成真机和模拟器验收 |

ARM64 产物中的应用程序是原生 ARM64 PE；按照 [Tauri Windows 安装包说明](https://v2.tauri.app/distribute/windows-installer/)，NSIS 安装器本身仍以 x86 运行，并由 Windows ARM 仿真执行。模拟器及其 ADB 是否支持 Windows ARM64 取决于各自厂商，不能由应用构建成功替代验证。

## 使用流程

1. 启动受支持的模拟器并开启其 ADB 功能。
2. 打开应用，选择自动发现的 ADB；若未发现，使用“选择文件”定位 `adb.exe`。
3. 刷新设备列表，选择一台在线设备。只有一台在线设备时会自动选择。
4. 如设备未出现，可填写模拟器提供的 IP 和端口进行手动连接。
5. 点击“刷新截图”，在画面内取点或用键盘编辑 X/Y。
6. 检查坐标后点击“执行点击”。切换设备会清除旧截图和坐标。

## 本地开发

开发环境需要 Node.js 22.12 或更高版本、pnpm 11.19、Rust stable、Visual Studio C++ Build Tools 和 WebView2 开发环境。

```powershell
pnpm install --frozen-lockfile
pnpm dev
```

常用检查命令：

```powershell
pnpm lint
pnpm typecheck
pnpm test
pnpm check
pnpm frontend:build
pnpm build                 # 默认：Windows x64
pnpm build:windows:x64     # Windows x64
pnpm build:windows:arm64   # Windows ARM64
```

桌面打包命令只在 Windows 上运行；使用 macOS 开发时，请推送分支后由 GitHub Actions 生成 Windows x64 与 ARM64 产物。

`pnpm check` 会执行前端 lint、类型检查和测试，以及 Rust 格式、Clippy 和测试。Windows ARM64 本地构建还需要 Visual Studio 的 C++ ARM64 build tools。构建脚本会安装对应 Rust target、调用 Tauri、检查最终 PE Header，并生成带平台和架构的统一产物：

```text
artifacts/windows-x64/
  OnmyojiSupportTools_0.1.0_windows_x64_nsis-setup.exe
  OnmyojiSupportTools_0.1.0_windows_x64_portable.zip
artifacts/windows-arm64/
  OnmyojiSupportTools_0.1.0_windows_arm64_nsis-setup.exe
  OnmyojiSupportTools_0.1.0_windows_arm64_portable.zip
```

CI 在原生 x64 和 ARM64 Windows runner 上分别构建并上传这些产物；不会自动创建 GitHub Release、签名或启用自动更新。

## 架构

```text
src/                    Vue 3 桌面界面与坐标换算
src-tauri/src/device.rs DeviceManager：ADB、设备、截图、点击与配置
src-tauri/src/lib.rs    受控 Tauri IPC 命令
docs/adr/               架构决策记录
.github/                CI、Issue Form 与 PR 模板
```

`DeviceManager` 是设备域的唯一入口。Vue 只能调用 `get_app_app_state`、`set_adb_path`、`refresh_devices`、`connect_device`、`select_device`、`capture_screen` 和 `tap_screen`。截图以二进制 IPC 响应传输，不做 Base64 或重复编码；Rust 保存截图尺寸和所属设备，用于点击前验证。

领域术语见 [CONTEXT.md](CONTEXT.md)，关键取舍见 [ADR-0001](docs/adr/0001-rust-tauri-desktop.md)。

## 安全边界

- 外部命令始终以程序路径和结构化参数启动，不经过 shell。
- 每次 ADB 调用都有超时，超时后终止子进程。
- IP、端口、设备状态、PNG 头和点击边界均在 Rust 侧校验。
- 配置只保存已验证的 ADB 路径、最后活动设备和最后手动连接端点。
- 应用不捆绑、不下载 ADB，不提供网络监听或远程控制。
- Issue 和 PR 中的截图、设备序列号、用户名路径及日志必须脱敏。

## 与 OnmyojiAutoScript 的关系

项目参考 [OnmyojiAutoScript](https://github.com/runhey/OnmyojiAutoScript) 的模块化思路，但所有代码、界面与资产均独立实现。不得复制其 GPLv3 代码、游戏素材、识别模板或其他受许可约束的内容；本仓库继续使用 MIT 许可证。

## 路线图

Windows 基础链路稳定后，计划依次评估任务模型、图像识别/OCR、任务调度和可扩展执行模块。macOS 支持属于更后期工作，届时需补充平台发现、打包、签名和完整验收矩阵。

贡献前请阅读 [CONTRIBUTING.md](CONTRIBUTING.md)。
发布候选的手工验证项见 [Windows 验收矩阵](docs/windows-acceptance.md)。

## 许可证

[MIT](LICENSE)
