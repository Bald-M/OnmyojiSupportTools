# 内置 Android Debug Bridge 来源

Windows 安装包携带 Android SDK Platform-Tools 中的开源 ADB 组件。应用不反编译这些文件，也不会在运行时下载它们。

## 固定来源

- 组件：Android Debug Bridge
- 版本：`37.0.1`
- 官方归档：`https://dl.google.com/android/repository/platform-tools_r37.0.1-win.zip`
- 官方仓库元数据 SHA-1：`e03e78b1d80b396f1c3358e31251cb31740e1110`
- 固定归档 SHA-256：`45f4d63113e895ebde0c90f194099a4676b6ac653bd28d54314a9e022bbc1a99`
- AOSP 源码：`https://android.googlesource.com/platform/packages/modules/adb/`
- 官方仓库元数据：`https://dl.google.com/android/repository/repository2-3.xml`
- 官方版本说明：`https://developer.android.com/tools/releases/platform-tools`

## 随安装包分发的文件

| 文件 | SHA-256 |
| --- | --- |
| `adb.exe` | `b4a6b455702684652cccf7b46258b29e653538904359a58fd4931cf3ef286b3f` |
| `AdbWinApi.dll` | `c1d653030b4bde65d3e07e4d0b0979e17be56df1436cdd15528630f27808050d` |
| `AdbWinUsbApi.dll` | `0710e894d9b40f71a670c13c694079d564c92c1279da382cfe4850983aaebe1b` |
| `NOTICE.txt` | `38ec8c6f5b7799c223ffeab1f9e81c2d5fc67b5e56d6424f649630ca1ee1a811` |
| `source.properties` | `2dccd788c0234d8cf7f7457377e57f57527a86a629c6ed54feb8af0f549dac38` |

ADB 的 AOSP 源码使用 Apache License 2.0；官方归档中的完整 `NOTICE.txt` 会原样进入安装目录，覆盖 ADB 构建所含的第三方声明。构建还会生成 `PROVENANCE.json`，记录版本、下载地址、源码地址和全部固定哈希。机器可读来源以 `src-tauri/adb-distribution.json` 为准，并同时用于构建校验和应用启动时的完整性复核。

截至 2026-09-10，上游 ADB 仓库没有发布名为 `platform-tools-37.0.1` 的公开 tag，因此本项目不把任意源码 commit 冒充官方二进制的精确构建输入。当前可复核链路锚定 Google 官方版本说明、仓库元数据中的版本与 SHA-1、固定归档 SHA-256、`source.properties` 及每个分发文件的 SHA-256；若上游补发对应 tag，升级审计时应把它补入清单。

上述 Windows 文件是 PE32 x86 程序。Windows x64 是主要验收目标；Windows ARM64 产物仍为实验性，必须在 ARM64 真机上验证 ADB、模拟器和卸载流程后才能声明兼容。
