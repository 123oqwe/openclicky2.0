# Changelog

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
