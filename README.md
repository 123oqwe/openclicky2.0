# OpenClicky 2.0

An AI buddy that lives on your computer. Hold a key, ask a question out loud, and it looks
at your screen, answers out loud, and draws an arrow or a circle on the exact thing it is
talking about. It also dictates, and runs background tasks.

macOS · Windows · Linux

![OpenClicky pointing at the screen it is explaining](docs/screenshots/main.png)

## Install

macOS and Linux, one line:

```bash
curl -fsSL https://raw.githubusercontent.com/123oqwe/openclicky2.0/main/scripts/install.sh | sh
```

Windows, in PowerShell:

```powershell
irm https://raw.githubusercontent.com/123oqwe/openclicky2.0/main/scripts/install.ps1 | iex
```

That puts an `openclicky` command on your PATH. Where the release carries a desktop shell
for your machine, the script installs it too: `OpenClicky.app` on macOS, the AppImage plus a
menu entry on Linux. On Windows it installs the CLI only, so take
[`OpenClicky_2.0.3_x64-setup.exe`](https://github.com/123oqwe/openclicky2.0/releases/latest)
from the release page for the window.

The artifacts are ad-hoc signed, not notarized. macOS blocks the first launch and you have to
allow it under System Settings → Privacy & Security. That is a known gap, not something you
did wrong.

`PREFIX` changes the destination (default `~/.local/bin`). `VERSION` pins a release tag.

The rest of this page is in Chinese. The next section is about building from source, which
you do not need if the one-liner above worked.

## Requirements

- **Rust 1.95.0**。`rust-toolchain.toml` 已锁定版本，装了 [rustup](https://rustup.rs) 就会自动用对。
- **macOS**：`xcode-select --install`。
- **Linux**：`bash scripts/linux-deps.sh build`。系统依赖只有这一份清单，CI 也调它。
- **Windows**：MSVC 生成工具（Visual Studio Build Tools 的 "Desktop development with C++"）。WebView2 在 Windows 11 自带。
- **系统权限**（只影响提问与操作，不影响编译）：
  - 屏幕录制：提问要用，必须给。
  - 辅助功能：听写与工具要注入按键，必须给。

## Build and run

三个平台同一套命令，只有最后一步的路径不同：

```bash
git clone https://github.com/123oqwe/openclicky2.0.git && cd openclicky2.0
cargo build --release
./target/release/openclicky doctor          # 先确认环境看对了
```

起界面：

```bash
cargo run -p openclicky-desktop --release   # macOS / Linux
.\target\release\openclicky-desktop.exe     # Windows，先跑上面那条 cargo build --release
```

`doctor` 正常时大致这样，其中 `commit` 是你的构建：

```text
openclicky 2.0.0 (macos) commit 53af262aaab1
config: /Users/you/Library/Application Support/openclicky/config.toml
data:   /Users/you/Library/Application Support/openclicky
config: 还没有选择模型（Settings → 模型）
api key: missing
key store: system keychain
screen recording: ok
accessibility:    ok
displays: 1
  screen0 3024x1964 @ 2.00 origin (0,0) primary
```

## First run

三步。每步都写了失败时你看到什么。

**1. 设 key。** key 从标准输入读，不进命令行历史。

```bash
./target/release/openclicky set-key
# 粘贴 key，回车，再按 Ctrl-D
```

- 失败：`Error: empty key on stdin`，说明输入是空的。

**2. 选模型 + 给权限。** 桌面端首次启动有向导，选模型、存 key、给权限都在里面。如果只用 CLI，模型要写进 `config.toml` 的 `[provider]`（`chat_model` 默认是空的，空着会被拒）。

给了权限之后 `doctor` 应显示 `screen recording: ok` / `accessibility: ok`。

- 失败：`缺少「屏幕录制」权限。在系统设置里打开它，然后回到 OpenClicky。`
  处理：`系统设置 → 隐私与安全性 → 屏幕录制 → 勾选 OpenClicky，然后重启应用。`
- 失败：`缺少「辅助功能」权限。…`
  处理：`系统设置 → 隐私与安全性 → 辅助功能 → 勾选 OpenClicky（听写与工具需要）。`

**3. 起界面，按一次热键。**

```bash
openclicky app          # 起桌面壳；加 --wait 留在前台看输出
```

窗口起来后按 `Ctrl+Alt+Space`。按住说话，松开提问，答案会播报并在屏幕上指出来。`Ctrl+Alt+O` 召回窗口。

- 失败：`the desktop shell is not installed yet. Run this, then try again:` 后面跟着安装命令，说明桌面壳没装到位，回到安装那一步。

- 失败：`no api key; run: openclicky set-key`，回到第 1 步。
- 失败：`还没有选择模型（Settings → 模型）`，回到第 2 步。
- 失败：`请求太频繁了，请过一会儿再试。`，上游限流，没有自动重试。

一次问答后可以用 `./target/release/openclicky usage` 看到用量。

## Data and limits

**数据在哪**

| 内容 | macOS | Linux | Windows |
|---|---|---|---|
| 配置 | `~/Library/Application Support/openclicky/` | `~/.config/openclicky/` | `%APPDATA%\openclicky\` |
| 数据 | `~/Library/Application Support/openclicky/` | `~/.local/share/openclicky/` | `%LOCALAPPDATA%\openclicky\` |

**怎么删干净**

```bash
./target/release/openclicky delete --yes
```

八步逐项打印，失败会写成 `FAIL` 而不是假装成功：

```text
ok   1/8 sqlite rows and handle
ok   2/8 memory files
…
ok   8/8 keychain entries
out of scope: 系统级备份（Time Machine / Windows 备份）
out of scope: 磁盘上已释放区块的残留
```

**能不能用别家端点**

`provider.base_url` 可以指向任何 OpenAI 兼容端点，LiteLLM、OpenRouter、vLLM、LM Studio 都行。代价（截图会经过第三方、听写要另配地址、本机没声音时指针改跟字幕）写在 [`docs/07-compatible-endpoints.md`](docs/07-compatible-endpoints.md)。那是一个选项，不是推荐路径。

**现在不支持什么**

- 没有签名安装包，只能源码运行；macOS 上会带未签名警告。
- 没有自动更新，也没有更新包验签。
- 没有常驻看门狗，卡住的长任务要重启应用才会被回收。
- 依赖失败后不会自动重试。
- 多屏只覆盖鼠标所在那一块屏幕。
- Wayland 下不注入按键，需要切 X11。
- 截图只存在内存里，30 秒就丢，不落盘。
- 记忆只记你明说的：`openclicky remember "说话短一点"`，`openclicky memory` 查看，`openclicky forget 关键词` 删除。它不会自己从对话里学。

## License

见 `LICENSE`。
