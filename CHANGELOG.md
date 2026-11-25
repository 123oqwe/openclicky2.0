# Changelog

## 2.0.1

修的是"下载下来跑不起来"，不是新功能。

- **macOS 的 .app 现在自带签名封印。** 2.0.0 的 dmg 和 zip 里的 bundle 都缺 `_CodeSignature`，`codesign --verify` 报 `code has no resources but signature indicates they must be present`。两个归档同病，换解包器无用；构建时补了 ad-hoc 签名。
- **Info.plist 声明了麦克风用途。** 缺 `NSMicrophoneUsageDescription` 时 macOS 在首次请求麦克风时直接结束进程，用户看到的是"一用就崩"。
- **`openclicky app` 子命令。** 装完敲它起界面；`--wait` 留在前台。
- **两个安装脚本都装桌面壳了。** 之前 `openclicky app` 提示"跑安装脚本"，而安装脚本只装 CLI，照着提示做修不好问题。macOS 用 `ditto` 解 zip（不用 unzip，避免 bundle 结构被拆散），Linux 装 AppImage 并写 `.desktop`，Windows 静默跑 NSIS 并 `Unblock-File`。
- **`--uninstall`** 删掉 CLI、桌面壳与桌面项，并说明怎么连数据一起删。
- **安装器的资产名从发布 API 取**，不再用 tag 拼。产物名跟的是 crate 版本，tag 与版本不一致时会 404。
- **发布改用 `gh` 三段式**：建 draft、上传、确认正文后再公开。softprops 会上传时与自己更新资产元数据竞争，2.0.0 因此两次只发出 8/15 个资产。
- **已公开的 release 不会被重跑覆盖**：`publish` 只对 draft 删重建，遇到公开版本直接 `exit 1`。

## 2.0.0

第一个公开版本。源码可构建，产物可下载，未签名。

### 能做什么

- **拿着真东西问**：按住 `Ctrl+Alt+Space` 说话，松开提问。它截屏、回答、朗读，并在屏幕上指出来。
- **打字也能问**：`openclicky ask --text "..."`，同样会看屏并指出位置。
- **交办**：`openclicky task "..."` 跑一个带工具的任务，写文件前会要你批准。
- **听写**：`openclicky dictate --seconds 5`，只注入新增的部分，疑似密码与卡号不注入。
- **记得住**：`openclicky remember "说话短一点"`、`openclicky memory`、`openclicky forget 关键词`。只记你明说的，不会自己从对话里学。
- **删干净**：`openclicky delete --yes`，八步逐项报告，失败会写成 `FAIL`。

### 产物

五类，三平台：

| 平台 | 桌面 | CLI |
|---|---|---|
| macOS | `OpenClicky_*_aarch64.dmg`、`OpenClicky.app.zip` | `openclicky` |
| Windows | `OpenClicky_*_x64-setup.exe`（NSIS） | `openclicky.exe` |
| Linux | `OpenClicky_*_amd64.AppImage`、`OpenClicky_*.deb` | `openclicky` |

另附一份 `SHA256SUMS`，覆盖全部分发文件，在下载目录里用 `scripts/verify-checksums.sh` 校验，三平台通用。

### 升级与兼容

- 数据库 schema v3。首次启动自动迁移，迁移前写 `app.db.bak`。
- 用旧版本打开更新的库时进入只读模式，不会假装能写。

### 已知问题

- **未签名**。macOS 首次打开要在「系统设置 → 隐私与安全性」放行，Windows 要点「更多信息 → 仍要运行」。
- **没有自动更新**。等签名方案定了再说。
- **没有安装脚本**。这一版要手动下载安装包，`install.sh` 与 `install.ps1` 在 0.1.x 里补。
- **macOS 首启的 bundle 自洽性未验证**（`Info.plist` 的麦克风用途说明与 `_CodeSignature`）。见 #62。
- **麦克风权限探测是硬编码的**，首启不会提示缺权限。见 #63。
- **桌面端启动时不校验配置**，坏值会静默生效。见 #65。
- **快捷键写死在代码里**，改 `config.toml` 不生效。见 #50。
- **Linux 朗读依赖未声明**，Wayland 下不注入按键。见 #66。
- **没有看门狗**：卡住的长任务要重启应用才会被回收。
- **失败不自动重试**。
- **多屏只覆盖鼠标所在那一块**。
- **界面只有中文**，无 i18n 框架。
- **截图只存在内存里**，30 秒丢弃，不落盘。

### 验证范围

自动化覆盖：三平台编译与测试（212 个测试）、clippy、文档五门禁、安装包在三平台虚拟机上的安装与启动（含窗口出现的断言）。

未覆盖：真实模型调用、真机麦克风与屏幕权限弹窗、签名验签、多显示器、睡眠唤醒。

详见 `docs/06-acceptance.md`。
