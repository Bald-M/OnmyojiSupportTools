# ADR-0003：先用 screenrecord 验证 WebView2 H.264 预览链路

- 状态：试验性
- 日期：2026-09-11

## 背景

Issue #4 希望增加 scrcpy 式实时设备预览，但正式采集组件、WebView2 解码可靠性、端到端延迟和资源稳定性尚未在 Windows x64 + MuMu 12 上验证。同时引入设备端服务器或 Windows 原生解码会扩大许可证、打包和维护范围。

## 决策

先实现隔离的采集与解码原型。`DeviceManager` 使用当前已验证 ADB 的 `exec-out screenrecord` 获取固定 1280 × 720 H.264 Annex-B 流，经 Tauri 二进制 Channel 传给 Vue，并由 WebView2 WebCodecs 解码到 Canvas。

预览会话仍由 `DeviceManager` 独占管理。更换 ADB、切换或失去活动设备、显式停止及应用退出都会清理预览；预览帧只提供坐标选择边界，点击仍需用户明确确认。静态 PNG 截图保持为受支持的降级路径。

该决策只批准可行性原型，不批准把 Android `screenrecord` 作为正式采集实现，也不构成 Windows 性能支持声明。

## 进入正式实现的门槛

必须在真实 Windows x64 + MuMu 12 环境完成并记录：稳定 30 FPS、P95 延迟不高于 150 ms、连续 30 分钟资源稳定、重复开始停止无遗留、点击安全语义以及设备/进程异常清理。测量方法与结果记录在 `docs/prototypes/issue-4-live-preview.md`。

若 WebView2 路径达标，再单独决定固定版本的设备端持续编码组件、许可证和打包方式；若不达标，再评估 Windows 原生解码。两条正式解码路径不会并行建设。

## 取舍

该方案用最少新增依赖回答最高风险的解码问题，并保持无 HTTP 服务、结构化 ADB 参数、8 秒码流停滞超时和单活动设备边界。代价是 `screenrecord` 具有设备相关的录制时长与能力限制，原型只提供统一的结束事件，尚无细分断线原因、动态方向元数据或正式的持续编码协议。
