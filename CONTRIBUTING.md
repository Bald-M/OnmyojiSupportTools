# 贡献指南

感谢参与 OnmyojiSupportTools。项目目前处于未发布的 `0.1.0` 阶段，优先保证 Windows 设备链路小而可靠。

## 开始之前

除错别字等极小修改外，请先创建或认领 Issue，说明问题、范围和验收方式。安全问题不要公开提交可直接利用的细节，请先联系维护者确定报告渠道。

不得复制 OnmyojiAutoScript 或其他项目中许可证不兼容的代码、游戏素材、识别模板及数据。可以借鉴公开的模块思想，但实现必须独立且可追溯。

## 分支与提交

从最新 `main` 创建短生命周期分支，建议使用 `feat/123-short-name`、`fix/123-short-name`、`docs/short-name`。

提交信息和 PR 标题都使用 Conventional Commits：

```text
feat(device): discover emulator adb
fix(capture): reject a stale screenshot
docs: explain installer WebView2 requirement
```

允许的类型为 `feat`、`fix`、`docs`、`refactor`、`perf`、`test`、`build`、`ci`、`chore`。破坏性变更使用 `!` 并在 PR 中写明迁移方式。

## 开发约束

- Windows-first；不要把未经验证的平台行为标记为受支持。
- 桌面程序不得启动公开或本地 HTTP 服务，设备能力只能通过受控 Tauri IPC 暴露。
- 外部程序必须使用结构化参数启动，不得调用 shell 或拼接命令字符串。
- ADB 操作必须有超时，并保留单活动设备、截图归属和坐标边界校验。
- 截图使用原始二进制传输，不增加 Base64 或重复编码。
- 新行为需要测试；优先通过假的命令执行适配器测试，不依赖真实设备。
- 内置 ADB 的版本、来源和哈希以 `src-tauri/adb-distribution.json` 为唯一机器可读清单，只能由 `scripts/prepare-bundled-adb.mjs` 准备；应用运行时不得下载 ADB，升级时必须同步来源记录和 NOTICE。

## 提交前检查

```shell
pnpm install --frozen-lockfile
pnpm check
pnpm frontend:build
pnpm build:windows:x64
```

Windows x64 桌面构建使用原生 MSVC 并生成面向用户的 NSIS 安装包。macOS 可先执行 `brew install llvm nsis` 和 `cargo install --locked cargo-xwin`，再通过相同命令交叉编译 Windows x64 NSIS 安装包。构建完成后 `dist/` 只保留一个带版本、平台和架构名称的安装包；若未执行 Windows 构建，请在 PR 中说明，CI 会在原生 Windows runner 上复核。

涉及设备流程时，按影响范围执行 [Windows 验收矩阵](docs/windows-acceptance.md)：自动/手动 ADB、单/多设备、IP 连接、截图、缩放取点、点击，以及离线、未授权和超时恢复。

## 文档与 Changelog

用户行为、支持范围、依赖或命令变化时同步更新 `README.md`。领域术语变化更新 `CONTEXT.md`；重要架构取舍新增 ADR。所有用户可感知或安全相关变更都应写入 `CHANGELOG.md` 的 `Unreleased` 对应分类，不要预先创建发布日期。

## PR 要求

PR 应保持单一目标，并包含：

- 关联 Issue 和变更动机。
- 实现范围、明确不做的内容及风险。
- 自动化测试和手工验证结果。
- 界面变更的前后截图。
- 文档与 Changelog 更新，或说明不适用的原因。

提交日志和截图前，请移除设备序列号、IP、账户信息、用户名目录、令牌及其他隐私数据。不要上传完整个人配置文件。
