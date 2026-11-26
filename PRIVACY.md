# Privacy

OpenClicky looks at your screen, listens to your microphone, and types into other
applications. This page says exactly what that means, where each thing goes, and how to
remove it. Every claim below is traceable to the code in this repository at this commit.
Where the code and an older document disagree, this page follows the code.

No account. No analytics. No crash reporting. The only network traffic is to the model
endpoint you configured yourself.

## What gets captured

**The screen.** `crates/core/src/privacy.rs` defines a gate that every capture must pass
through. `ScreenSource::capture_all` takes a `CaptureGuard` and there is no other way to
call it, so a capture cannot happen unless something opened the gate first. There are five
reasons a gate may open, and each one follows a thing you did:

| Reason | Trigger |
|---|---|
| `hotkey` | you held the talk key and asked a question |
| `typed` | you sent a question from the input box |
| `calibration` | you ran the self-test, from the CLI or from settings |

The enum also declares `onboarding` and `retry`. Nothing calls either one, so neither
captures anything today. If that changes, this table changes with it.

Nothing captures on a timer, on a schedule, or in the background. There is no such code
path.

Without screen recording permission the two paths behave differently. The talk key returns
an error and the question is never sent. A typed question is still answered, just without a
screenshot and without a pointer (`crates/core/src/voice.rs`).

**The microphone.** Only while you hold the dictation key, and only long enough to
transcribe. The audio is converted to a WAV in memory and posted once.

**Your clipboard.** It is touched in two cases. When speech is unavailable, the answer is
copied so you can paste it yourself. When an application refuses synthetic keystrokes,
OpenClicky puts the text on the clipboard, sends a paste keystroke, and restores what was
there before (`crates/platform/src/input.rs`). The restore reads your clipboard's text and
can only put text back. A clipboard holding an image or a file is cleared rather than
restored. That is a known bug, open as issue #107.

**The name of the frontmost application.** Read with `osascript` / `powershell` /
`xdotool` on each question so the answer can be written for that kind of app. It is part of
the prompt, listed below.

## Where it stays on disk

Paths come from `crates/platform/src/paths.rs`. `openclicky doctor` prints both directories
for your machine.

| What | Where |
|---|---|
| Settings | `config.toml` in the config directory |
| Conversation history, tasks, usage | `app.db` (SQLite) in the data directory |
| Things you asked it to remember | `memory/profile.md`, `memory/projects.md` |
| Files a background task produced | `workspace/` |
| The API key | the OS key store, or `secrets.enc` at mode 0600 |

`app.db` holds:

| Table | Contents |
|---|---|
| `sessions`, `messages` | the text of every question and every answer, plus which model answered |
| `tasks`, `task_steps` | the task goal, each step's tool name, a redacted and truncated argument string, and the tool's output |
| `approvals` | a step waiting for your approval, and the prompt it will show |
| `usage_events` | token units per turn |
| `audit_log` | tool name, risk level, outcome, and a digest of the arguments rather than the arguments |
| `dict_entries` | pairs of "what it heard" and "what you corrected it to", from dictation corrections |
| `metrics_daily`, `routines` | counters and scheduled runs |

Screenshots are never written to disk. They exist as JPEG bytes in memory for the length of
one request. The single exception is `openclicky selftest --out <path>`, which writes the
screenshot you asked it to write.

The `logs/` directory is created when the data directory is created, and `openclicky delete`
removes it, but nothing writes into it today. There is no logging framework in this build.

Keys are never written into `config.toml`, never into the database, and never into a
commit. Anything that does get logged goes through `crates/core/src/security.rs`, which
masks bearer tokens and key-shaped values.

## What leaves your machine

One destination: `provider.base_url`, plus `provider.stt_base_url` for dictation when it is
set. Both are yours to choose. `docs/07-compatible-endpoints.md` covers what changes when
you point them somewhere else.

Each question sends:

- the question itself, typed or transcribed;
- one JPEG per display;
- a system prompt that contains the frontmost application's name, a short teaching note for
  known apps, every line in `memory/profile.md` and `memory/projects.md`, the number and
  resolution of your displays and which one your cursor is on, and your language setting;
- the earlier turns of the same conversation, when it is a follow-up.

A dictation sends the WAV of what you said and nothing else.

A background task additionally sends the list of tools and the output of each tool it was
allowed to run. That output can contain file contents, because reading files is what those
tools do. The approval prompt is the place that decision is yours.

Your API key travels in the `Authorization` header of every one of those requests.

What is never sent: your clipboard contents, your keystrokes, the database, the memory
files as files, and any file a tool did not open.

## Deleting it

```bash
openclicky delete --yes
```

Eight numbered steps, each printed with its own result. A failure is printed as a failure,
not as success. In order: database rows and the open handle, memory files, workspace and
task artifacts, the logs directory, the database file, its `-wal` and `-shm` side files, the
migration backup, and the stored API key.

Two things it deliberately does not cover, and says so:

- system-level backups, meaning Time Machine and Windows backup;
- anything still on disk in freed blocks.

For memory alone there is a smaller hammer:

```bash
openclicky memory            # show what it remembers
openclicky forget 关键词      # drop every line containing that text
```

Deleting the API key by itself means running the eight-step delete, since the key lives in
the same store that command clears.

## What this page cannot promise

- A third-party endpoint is outside our control. Whatever you point `base_url` at keeps what
  it receives under its own policy, not this one.
- The operating system, your backups, and your filesystem keep copies this program does not
  delete.
- This is a description of the code at this commit, not an audit. If it disagrees with
  `crates/core/src/privacy.rs` or `crates/platform/src/paths.rs`, the code is right and this
  page is a bug.
