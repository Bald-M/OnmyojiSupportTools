# OnmyojiSupportTools

OnmyojiSupportTools 是一个面向 Windows 的 Android 模拟器辅助桌面应用。目前的 `0.1.0` 尚未公开发布，首个里程碑专注于安全、可检查地完成 ADB 连接、截图取点与点击。

应用采用 Rust、Tauri 2 和 Vue 3。界面打包在桌面程序内，通过 Tauri IPC 与 Rust 通信；运行时不会启动 HTTP 服务，也不支持浏览器部署。

## 当前能力

- 默认使用安装包内置且经过哈希验证的开源 ADB，也可发现 MuMu 模拟器 12、雷电模拟器 9、系统 `PATH` 和上次配置中的外部 ADB。
- 验证并选择同一个 `adb.exe` 管理服务器、设备、截图与点击。
- 展示在线、离线及未授权设备；仅允许选择在线设备。
- 通过 IP 地址和端口请求 `adb connect`。
- 手动获取 PNG 截图，在缩放或留白后的画面中准确换算坐标。
- 单击画面只选择坐标；只有明确按下“执行点击”才向设备发送操作。

以下能力不属于 `0.1.0`：多设备并发、实时预览、图像识别、OCR、任务运行时、调度器、插件、远程控制、自动更新和 macOS 发布。

## 系统要求

| 项目 | 支持范围 |
| --- | --- |
| Windows | Windows 10 22H2 / Windows 11 x64 |
| 模拟器 | MuMu 模拟器 12、雷电模拟器 9 |
| WebView | NSIS 安装包可按需调用微软 WebView2 引导程序 |
| ADB | Windows 安装包内置 AOSP ADB 37.0.1；也可改用模拟器或 Android SDK 提供的外部 ADB |

macOS 目前不提供原生应用或运行支持，但开发机可以交叉编译 Windows x64 NSIS 安装包；核心模块继续保留平台边界，待 Windows 版本稳定后再评估原生 macOS 适配。

### 构建目标

| 平台 | 架构 | Rust target | CI runner | 状态 |
| --- | --- | --- | --- | --- |
| Windows | x64 | `x86_64-pc-windows-msvc` | `windows-2025` | 主要支持目标 |

## 使用流程

1. 使用 NSIS `.exe` 安装应用，再启动受支持的模拟器并开启其 ADB 功能。
2. 打开应用后默认使用内置 ADB；如需兼容特定模拟器，也可选择自动发现或手动定位的外部 `adb.exe`。
3. 刷新设备列表，选择一台在线设备。只有一台在线设备时会自动选择。
4. 如设备未出现，可填写模拟器提供的 IP 和端口进行手动连接。
5. 点击“刷新截图”，在画面内取点或用键盘编辑 X/Y。
6. 检查坐标后点击“执行点击”。切换设备会清除旧截图和坐标。

## 本地开发

通用开发环境需要 Node.js 22.12 或更高版本、pnpm 11.19 和 Rust stable。Windows 原生打包需要 Visual Studio C++ Build Tools 和 WebView2 开发环境。

```shell
pnpm install --frozen-lockfile
pnpm dev
```

常用检查命令：

```shell
pnpm lint
pnpm typecheck
pnpm test
pnpm check
pnpm frontend:build
pnpm build                 # 默认：Windows x64
pnpm build:windows         # Windows x64
pnpm build:windows:x64     # Windows x64
```

构建入口会根据宿主系统选择工具链：Windows 使用原生 MSVC，macOS 使用 `cargo-xwin` 交叉编译 Windows MSVC 目标。首次在 macOS 构建前安装依赖：

```shell
brew install llvm nsis
cargo install --locked cargo-xwin
```

脚本会自动发现 Homebrew 的 LLVM 路径，无需修改 `~/.zshrc`。构建会先取得固定的官方 Platform-Tools 37.0.1 归档，校验归档和安装文件的 SHA-256 后再打包；应用运行时不会下载 ADB。Windows 原生构建与 macOS 交叉构建都会校验 PE 架构并生成集成 ADB 的 NSIS 安装包。这只生成 Windows 应用，不代表支持在 macOS 上运行该应用。

`pnpm check` 会执行构建调度器测试、前端 lint、类型检查和测试，以及 Rust 格式、Clippy 和测试。构建脚本会安装 x64 Rust target、调用 Tauri、检查最终 PE Header，并在完成后清空 `dist/`，只保留带版本、平台和架构的安装包：

```text
dist/
  OnmyojiSupportTools_0.1.0_windows_x64_nsis-setup.exe
```

CI 在原生 x64 Windows runner 上构建并上传同一个 NSIS 安装包；安装后可通过 Windows“已安装的应用”卸载。CI 不会自动创建 GitHub Release、签名或启用自动更新。

## 架构

```text
src/                    Vue 3 桌面界面与坐标换算
src-tauri/src/device.rs DeviceManager：ADB、设备、截图、点击与配置
src-tauri/src/lib.rs    受控 Tauri IPC 命令
docs/adr/               架构决策记录
.github/                CI、Issue Form 与 PR 模板
```

`DeviceManager` 是设备域的唯一入口。Vue 只能调用 `get_app_app_state`、`set_adb_path`、`refresh_devices`、`connect_device`、`select_device`、`capture_screen` 和 `tap_screen`。截图以二进制 IPC 响应传输，不做 Base64 或重复编码；Rust 保存截图尺寸和所属设备，用于点击前验证。

领域术语见 [CONTEXT.md](CONTEXT.md)，桌面架构见 [ADR-0001](docs/adr/0001-rust-tauri-desktop.md)，内置 ADB 的来源、隔离和升级取舍见 [ADR-0002](docs/adr/0002-bundle-auditable-adb.md)。

## 安全边界

- 外部命令始终以程序路径和结构化参数启动，不经过 shell。
- 每次 ADB 调用都有超时，超时后终止子进程。
- 内置 ADB 使用独立的本机 server 端口 `5038`；由 Rust 直接读取 smart-socket 的 `server-status`，无副作用地核对服务端版本和程序路径，只回收同版本、同安装目录的遗留实例，拒绝接管其他 ADB，并在退出前再次核对身份。
- 启动内置 ADB 前会根据编译进应用的固定清单复核安装目录内各分发文件的 SHA-256 和 ADB 版本；失败时可改选外部 ADB。
- IP、端口、设备状态、PNG 头和点击边界均在 Rust 侧校验。
- 配置只保存已验证的 ADB 路径、最后活动设备和最后手动连接端点。
- 构建阶段只接收并校验固定版本的官方 ADB；应用运行时不下载 ADB，不启动 HTTP 服务，也不提供远程控制。
- Issue 和 PR 中的截图、设备序列号、用户名路径及日志必须脱敏。

## 与 OnmyojiAutoScript 的关系

项目参考 [OnmyojiAutoScript](https://github.com/runhey/OnmyojiAutoScript) 的模块化思路，但所有代码、界面与资产均独立实现。不得复制其 GPLv3 代码、游戏素材、识别模板或其他受许可约束的内容；本仓库继续使用 MIT 许可证。

## 路线图

Windows 基础链路稳定后，计划依次评估任务模型、图像识别/OCR、任务调度和可扩展执行模块。macOS 支持属于更后期工作，届时需补充平台发现、打包、签名和完整验收矩阵。

贡献前请阅读 [CONTRIBUTING.md](CONTRIBUTING.md)。
发布候选的手工验证项见 [Windows 验收矩阵](docs/windows-acceptance.md)。

## 许可证

应用代码使用 [MIT](LICENSE)。Windows 安装包中的 ADB 及第三方声明见 [ADB 来源与许可证记录](docs/third-party/adb.md)，完整 `NOTICE.txt` 随安装包分发。
