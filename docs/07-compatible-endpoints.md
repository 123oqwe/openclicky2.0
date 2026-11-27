# 07 · 把端点换成别家

这一页讲怎么让 OpenClicky 去打一个 OpenAI 兼容端点，以及换完之后哪些能力会形变。

**这是选项，不是推荐路径。** 默认配置打 `https://api.openai.com/v1`，那条路是我们自己每天跑的。换成 LiteLLM、OpenRouter、vLLM、LM Studio 是你的选择，代价在下面写清楚了，要不要付是你判断。

## 什么情况下值得看

- 你已经有 LiteLLM 这类网关，公司所有 key 都从那儿发。
- 你想用本地模型，截图不出机器。
- 你在 OpenAI 上遇到地区或支付限制，想换一个入口。

如果你只是想让它能用，不需要读这一页，默认配置就是最短路径。

## 改哪几个键

配置文件在 `openclicky doctor` 打印的 `config:` 那一行。改完不用重装，重启应用或再按一次热键即可。

```toml
config_version = 1

[provider]
kind = "openai_compatible"
base_url = "http://127.0.0.1:4000/v1"
chat_model = "gpt-4o"

# 听写走另一条地址时填这里。留空表示跟 base_url 一样。
stt_base_url = ""
stt_model = "whisper-1"
```

| 键 | 作用 | 空着会怎样 |
|---|---|---|
| `provider.kind` | `openai` / `anthropic` / `openai_compatible` | 默认 `openai` |
| `provider.base_url` | 聊天与列模型的根地址，必须以 http 开头 | 校验会报错 |
| `provider.chat_model` | 主模型 | 语音那条路直接返回「还没选择模型」 |
| `provider.fast_model` | 快档，路由给短问题 | 回落到 `chat_model` |
| `provider.deep_model` | 深档，路由给需要思考的问题 | 回落到 `chat_model` |
| `provider.stt_base_url` | 听写单独一条地址 | 跟 `base_url` 一样 |
| `provider.stt_model` | 听写模型名 | 默认 `whisper-1` |

指向 Anthropic 原生端点时 `kind = "anthropic"`，走的是另一套请求体，本文不覆盖。

## 对方至少要能给的东西

| 能力 | 用在哪 | 缺了会怎样 |
|---|---|---|
| `GET /models` | 保存 key 之前探一次，确认 key 活着、模型存在 | 设置页保存被拒；`openclicky set-key --no-check` 可绕过 |
| `POST /chat/completions` | 全部问答 | 一切都不工作 |
| 同一路由的 SSE 流式 | 按住热键提问 | 语音那条路要流式增量，只有非流式会失败 |
| `tools` 字段 | 后台任务 | 任务跑不了，问答不受影响 |
| `POST /audio/transcriptions` | 听写 | 听写 404，问答与指向不受影响 |

流式和非流式两条都在用。语音提问走流式，后台任务走一次性返回。所以对方的兼容性要把两条都算上。

## 四个例子

地址都填到 `/v1` 为止，不要带具体路由。模型名以对方 `/models` 的输出为准，不要照抄这里的形状。

**LiteLLM 网关**

```toml
kind = "openai_compatible"
base_url = "http://127.0.0.1:4000/v1"
chat_model = "gpt-4o"
```

key 填 LiteLLM 发给你的虚拟 key，不是上游的 OpenAI key。

**OpenRouter**

```toml
kind = "openai_compatible"
base_url = "https://openrouter.ai/api/v1"
chat_model = "openai/gpt-4o"
```

OpenRouter 的模型 id 带厂商前缀，形如 `openai/gpt-4o`、`anthropic/claude-sonnet-4`。填错会被保存前的校验挡住，错误信息里会写出你填的那个名字。

**vLLM**

```toml
kind = "openai_compatible"
base_url = "http://127.0.0.1:8000/v1"
chat_model = "<你 serve 的那个模型名>"
```

`chat_model` 必须和 `vllm serve` 里的模型名一致，不是 HuggingFace 仓库路径。

**LM Studio**

```toml
kind = "openai_compatible"
base_url = "http://localhost:1234/v1"
chat_model = "<载入模型在 /v1/models 里的 id>"
```

LM Studio 就在 App 里点开 Local Server，然后：

```bash
curl -s http://localhost:1234/v1/models | head
```

把返回的 id 抄进 `chat_model`。

先确认对方活着，再改配置：

```bash
curl -s -o /dev/null -w '%{http_code}\n' http://127.0.0.1:4000/v1/models
```

200 才是通的。连接被拒说明服务没起；401 说明 key 还没配。

## 代价

这一节是本文的重点。上面每一条都能做到，但都不是白拿的。

**1. 你的屏幕会经过第三方**

提问时它会截屏，把每块屏幕编码成 JPEG 塞进请求体。同一次请求里还有问的那句话、当前前台应用名、每块屏幕的尺寸与坐标，以及你让它记住的那些偏好（`openclicky memory` 能列出来）。

指向 `api.openai.com` 时这些数据去 OpenAI。指向别人的网关时去那个网关，以及网关后面接的那家。本地 vLLM 或 LM Studio 时不出机器。

没有「只发文字不发图」的开关，但两条提问路径的降级方式不一样。按住热键那条没有屏幕录制权限就直接报错，问题不发出；打字问那条没有权限时照常回答，只是不带图、也不指向。

**2. 视觉能力变成你要自己确认的事**

指着屏幕说话这件事依赖模型看图。纯文本模型不会报错，它会给你一个跟你屏幕无关的回答，然后指向一个它猜的位置。切过去之后先用 `openclicky ask` 问一句「我屏幕上写了什么」，答案对得上再信它。

**3. 听写要另配一条地址**

`stt_base_url` 留空时听写跟 `base_url` 走。多数本地推理服务不提供 `/audio/transcriptions`，于是听写 404 而问答正常。这时候把 `stt_base_url` 指向一个真能转写的地方，或者干脆不用听写。

这正好和 `#91` 那一族问题相邻，区别是现在地址可以配，不用改代码。

**4. 语音不走端点，但本机没声音时它会退化**

朗读用的是操作系统自己的声音，macOS 上是 `say`，Windows 上是 `System.Speech`，Linux 上是 `spd-say` 或 `espeak-ng`。所以换端点**不会**让朗读变成对方的 TTS，本机有声音就照常朗读。

反过来，本机没有可用的声音时（Linux 少装 `speech-dispatcher`，或者 `say` 调不起来），这一轮会：

- 弹一条 `E_TTS` 提示，说明改为字幕与剪贴板；
- 把答案复制到剪贴板；
- 用字幕把答案显示出来（`voice.captions` 为 true，默认就是 true）；
- 指针不再等朗读结束，而是固定活 4000 毫秒（`UNSPOKEN_POINTER_MS`）。

所以「没有语音合成时指针跟着字幕走」这句话的成因是**本机没有声音**，不是你换了端点。本地模型本身不会造成这个。

**5. 音色是本机的，不是端点的**

`provider.tts_voice` 会被读，喂给 `say -v`。填一个本机没装的音色时会被忽略并用默认音色。端点给不了音色，也换不了声音引擎。

**6. 保存 key 会先探一次 `/models`**

设置页保存之前会请求一次 `/models`，用它证明 key 活着、你选的模型名在那个列表里。对方没实现这条路由时保存会被拒，报错会点名是哪一次请求失败。命令行可以 `openclicky set-key --no-check` 跳过这次探测，但跳过之后没有任何东西替你确认 key 是对的。

## 改完怎么确认

```bash
openclicky doctor --online
```

除了配置、权限、屏幕，它会列一次对方的模型，并在 `chat_model` 不在列表里时打一行 `warning`。

```bash
openclicky ask --text "我屏幕上写了什么"
```

问一句只有看见屏幕才答得上的话。答得出，说明视觉这条通了。答不出但也不报错，通常是模型没有看图能力，回到第 2 条。

听写单独验：

```bash
openclicky dictate --seconds 3
```

它会把转写出的文字打进你的光标位置。先在一个空窗口里试，不要去浏览器地址栏试。

## 回滚

把三行改回默认就行：

```toml
kind = "openai"
base_url = "https://api.openai.com/v1"
chat_model = "<原来的模型>"
```

key 是按 `provider/default` 这一个名字存的，换端点不会覆盖它。所以从本地模型切回 OpenAI 时，如果 key 还是本地服务的假 key，记得重新 `openclicky set-key`。
