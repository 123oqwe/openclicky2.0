# 05 部署方案（macOS / Windows / Linux）

| 项 | 值 |
|---|---|
| 编号 | OC2-DEPLOY |
| 版本 | 1.0（2026-11-23） |
| 适用 | v2.0.0 起 |
| 依据 | tech-prd §16/§17、ADR-0011、`06-acceptance.md` |
| 状态 | 构建与安装步骤可执行；Tauri 桌面外壳落地后补齐图形安装包（见 §7） |

本文件是**发布清单**，不是介绍。每一步都应能在目标机器上照做；做不到就是缺陷。

---

## 1. 产物矩阵

| 平台 | 产物 | 谁产出 | 现状 |
|---|---|---|---|
| macOS 14.2+（arm64；Intel 需 universal 构建，尚未产出） | CLI 可执行文件 + `OpenClicky.app` / `.dmg` | CI `release.yml`（desktop job） | 本机已产出 `.app`（6.95MiB）；`.dmg` 与 universal 未验证 |
| Windows 10 1809+ / 11（x64，**需 WebView2 运行时**） | CLI `.exe` + NSIS 安装器 | CI `release.yml`（desktop job） | CLI 可构建；NSIS 未运行 |
| Linux x86_64（X11） | CLI + `.AppImage` / `.deb` | CI `release.yml`（desktop job） | CLI 可构建；AppImage/deb 未运行 |

版本号规则：`vMAJOR.MINOR.PATCH`；tag 即发布触发条件，`main` 合并不等于发布。

---

## 2. 通用：配置与数据位置

| 内容 | macOS | Windows | Linux |
|---|---|---|---|
| 配置 | `~/Library/Application Support/openclicky/config/config.toml` | `%APPDATA%\openclicky\config\config.toml` | `~/.config/openclicky/config/config.toml` |
| 数据（DB/记忆/工作区/日志） | `~/Library/Application Support/openclicky/data/` | `%APPDATA%\openclicky\data\` | `~/.local/share/openclicky/data/` |
| 密钥 | 系统钥匙串（service `openclicky`） | 凭据管理器 | 无钥匙串时：`data/secrets.enc`（AES-256-GCM，0600，`doctor` 会标注 degraded） |

隔离运行（测试、多账户）：`openclicky --data-dir /tmp/oc2 doctor`。

---

## 2.1 平台前置条件（安装前就要满足）

| 平台 | 前置 | 不满足会怎样 |
|---|---|---|
| macOS | 14.2+；首次运行需给屏幕录制 / 辅助功能 / 麦克风权限 | 缺屏幕录制：只能无图回答；缺辅助功能：热键与注入失效 |
| macOS 15+ | 系统会周期性重新询问屏幕录制授权 | 表现为"权限自己丢了"，重开开关即可 |
| Windows | WebView2 运行时（Win11 自带；Win10 可能需单独安装） | 窗口白屏或不启动 |
| Linux | X11 会话（Wayland 仅能问答，不能注入/指点） | 听写与"指给你看"不可用 |

## 3. macOS

### 3.1 安装
1. 下载产物（`openclicky` 或 `.dmg`）。
2. CLI：`chmod +x openclicky && sudo mv openclicky /usr/local/bin/`。
3. `.dmg`：拖入 `Applications`。
4. 首次启动：未签名/未公证时系统会拦截。两种放行方式：
   - 右键 → 打开 → 再点"打开"；
   - 或 `xattr -dr com.apple.quarantine /Applications/OpenClicky.app`。
5. `openclicky doctor` 应显示 `screen recording: ok`、`accessibility: ok`。

### 3.2 权限（TCC，缺一不可）
| 权限 | 在哪开 | 不给会怎样 |
|---|---|---|
| 屏幕录制 | 系统设置 → 隐私与安全性 → 屏幕录制 | 无法截图，问答只能无图回答 |
| 辅助功能 | 同页 → 辅助功能 | 全局热键、听写注入、点击检测失效 |
| 麦克风 | 同页 → 麦克风 | 按住说话无音频 |

改完权限必须**重启应用**（TCC 缓存）。`doctor` 会给出具体缺哪项。

### 3.3 签名与公证（可选升级路径）
CI 预留：`APPLE_CERTIFICATE`、`APPLE_CERTIFICATE_PASSWORD`、`APPLE_SIGNING_IDENTITY`、`APPLE_ID`、`APPLE_TEAM_ID`、`APPLE_APP_SPECIFIC_PASSWORD`。
配齐后走 `codesign --deep --options runtime` + `xcrun notarytool submit --wait` + `xcrun stapler staple`。未配齐时按 3.1 的放行步骤，文档与 Release 说明必须写清。

### 3.4 升级 / 回滚 / 卸载
- 升级：替换二进制或 `.app`；升级前应用会把运行中任务落检查点并标 `Paused(upgrade)`。
- 回滚：下载上一个 Release 覆盖安装；数据库 `user_version` 更高时只读打开并提示（SDD A-20）。
- 卸载：删除 `.app`/二进制；数据不自动删除。彻底清除 = 删除 §2 的配置与数据目录 + 钥匙串项 `openclicky`。

### 3.5 故障排查
| 现象 | 原因 | 处理 |
|---|---|---|
| `doctor` 报 `E_PERM` | TCC 未授权或未重启 | 按 3.2 开启并重启 |
| 截图全黑 | 屏幕录制权限被系统更新重置 | 关闭再打开该开关，重启 |
| 热键无反应 | 与其他应用冲突 | `Settings → Shortcuts` 改键；`doctor` 打印当前绑定 |
| 注入到终端无效 | 未给辅助功能 | 同 3.2；终端目标只注入单行是设计行为 |

---

## 4. Windows

### 4.1 安装
1. 运行 `openclicky.exe`（或后续 `OpenClicky-Setup.exe` / `.msi`）。
2. 若出现"Windows 已保护你的电脑"：`More info` → `Run anyway`（未签名产物的预期行为）。
3. `openclicky doctor` 应显示 `screen recording: ok`（Windows 无 TCC，恒为 ok）。

### 4.2 权限与默认键
- 无系统级权限弹窗；麦克风由"设置 → 隐私 → 麦克风"控制。
- **没有 Fn 键**：桌面壳统一使用 **Ctrl+Alt+Space**（ADR-0014/0015 Q14）；听写键在壳内尚未接线。改键需要壳读配置，当前不支持。
- 全局热键与注入不需要管理员权限；若公司策略禁止，`doctor` 会报明确错误而不是静默失败。

### 4.3 升级 / 回滚 / 卸载
- 升级：运行新安装器覆盖；或替换 exe。
- 回滚：重新安装上一版；数据兼容同 3.4。
- 卸载：设置 → 应用 → 卸载；数据需手动删除 §2 路径。注册表只在安装器创建卸载项，不写业务数据。

### 4.4 故障排查
| 现象 | 原因 | 处理 |
|---|---|---|
| SmartScreen 拦截 | 未签名 | More info → Run anyway；发布说明需标注 |
| 热键被输入法占用 | IME 全局键 | 改键 |
| 注入中文失败 | 目标应用禁用粘贴 | 已自动回退剪贴板，粘贴后还原原剪贴板 |
| 双击热键无反应 | 设置里键位冲突 | `doctor` 打印绑定与冲突 |

---

## 5. Linux

### 5.1 安装
```bash
# AppImage
chmod +x OpenClicky-2.0.0-x86_64.AppImage && ./OpenClicky-2.0.0-x86_64.AppImage

# deb
sudo dpkg -i openclicky_2.0.0_amd64.deb || sudo apt-get -f install
```
依赖（deb 会声明）：`libwebkit2gtk-4.1-0`、`libgtk-3-0`、`libasound2`、`libxkbcommon0`（enigo 注入用）、`x11-utils`；构建时需要对应的 `-dev` 包（见 CI）。
`openclicky doctor` 应显示 `displays: N` 且 `screen recording: ok`。

### 5.2 X11 与 Wayland
| 能力 | X11 | Wayland |
|---|---|---|
| 截图 | ✅ | ⚠️ 需 portal，可能只拿到单屏 |
| 文本注入 | ✅（XTest） | ❌ |
| 点击检测（带做） | ✅（XRecord） | ❌ |
| 全局热键 | ✅ | ⚠️ 视合成器 |

**Wayland 下启动必须打印明确提示**（"需要 X11 会话，或改用剪贴板手动粘贴"），不得假装可用。

### 5.3 开机自启（用户级）
```bash
mkdir -p ~/.config/autostart
cat > ~/.config/autostart/openclicky.desktop <<'DESK'
[Desktop Entry]
Type=Application
Name=OpenClicky
Exec=/usr/bin/openclicky --tray
X-GNOME-Autostart-enabled=true
DESK
```

### 5.4 升级 / 回滚 / 卸载
- 升级：替换 AppImage / `dpkg -i` 新包；任务检查点同 3.4。
- 回滚：安装上一版包；DB 版本更高时只读。
- 卸载：`sudo dpkg -r openclicky`（deb）或删除 AppImage；数据手动删除 §2。
- 密钥：无 secret-service 时使用本地加密文件，删除账号会一并删除。

### 5.5 故障排查
| 现象 | 原因 | 处理 |
|---|---|---|
| `doctor` 无显示器 | 无 `DISPLAY`（SSH/无头） | 在图形会话中运行，或 `xvfb-run` 仅用于测试 |
| 注入不生效 | Wayland（XTest 不可用） | 切 X11 会话，或改用剪贴板手动粘贴 |
| 无 TTS | 未装 `spd-say`/`espeak-ng` | 安装任一，或只用字幕 |

---

## 6. 冒烟清单（每次发布必做，结果记 `06-acceptance.md`）

| # | 步骤 | 通过标准 |
|---|---|---|
| 1 | 安装产物 | 能在目标系统启动并打印版本+commit |
| 2 | `doctor` | 无 `E_*` 错误；权限与环境如实报告 |
| 3 | `selftest --out /tmp/shot.png` | 每块屏都有尺寸与字节数；图片能打开 |
| 4 | 配 key 后 `doctor --online` | 能列出模型；配置的模型在列表里 |
| 5 | `ask --text "hi"` | 有回答，用量 +1 |
| 6 | `voice --seconds 3` | 有转写、有回答 |
| 7 | `task "..."` | 状态、结果、`task_steps` 合理 |
| 8 | 卸载/清除 | 按 §2 清除后 `doctor` 显示无 key、无数据 |

---

## 7. 桌面壳（已落地，安装器待补）

当前可用形态（源码运行）：
```bash
cargo run -p openclicky-desktop        # 启动壳（main 窗口 + overlay 覆盖层）
# 按住 Ctrl+Alt+Space 说话，松开提问
```

仍需补齐（不改变已实现的行为）：
- 三平台安装器构建命令（`cargo tauri build`）与产物路径；
- 更新签名密钥（`TAURI_SIGNING_PRIVATE_KEY`）与公钥入库位置；
- 更新检查失败/回滚的用户可见行为；
- `Settings → General → Install & Relaunch` 的实际行为。

在此之前，**CLI 是唯一可交付入口**，Release 说明必须这样写，不能让用户以为有图形安装包。
