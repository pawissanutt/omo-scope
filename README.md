# omo-scope

[![CI](https://github.com/pawissanutt/omo-scope/actions/workflows/ci.yml/badge.svg)](https://github.com/pawissanutt/omo-scope/actions/workflows/ci.yml)
[![Release](https://img.shields.io/github/v/release/pawissanutt/omo-scope)](https://github.com/pawissanutt/omo-scope/releases/latest)
[![License: MIT](https://img.shields.io/badge/license-MIT-blue.svg)](LICENSE)

**Watch your [OmO (oh-my-openagent)](https://github.com/code-yeongyu/oh-my-openagent) subagents work, live, in a terminal pane.**

omo-scope is a view-only, mouse-driven TUI for OmO subagent tasks, DAG runs and child
transcripts. It only reads files OmO already writes, so there is no extension, patch or
transport to install.

![omo-scope following a DAG run: clicking nodes, expanding tool calls, a live test log, dragging the splitter and the session picker](demo/omo-scope.gif)

<sub>Recorded from a fabricated project with <code>demo/record.py</code>, using real keystrokes and mouse events.</sub>

## Features

- **Tasks and DAGs**: the session's subagent tree (nested children indented) or its DAG runs,
  with state, dependencies and elapsed time.
- **Live transcript**: follows the selected subagent as it works; scroll up to pause, click
  `follow` to resume.
- **Live command output**: while a command runs, tails the file it writes (`> log`, `tee`,
  `*.log`), including `tool.bash` calls inside `eval`.
- **File edits as diffs**: `apply_patch`, `write` and `edit` calls render as colored diffs with
  `+N -M` counts.
- **Reasoning**: thinking blocks in preview, full or hidden mode.
- **Stats**: turns, tools, tok/s, context use, cache hits and cost in the log title bar and
  task rows, picked and ordered from a settings menu.
- **Mouse first**: click rows, tabs, log entries and the session title; wheel scrolling; a
  draggable splitter between the list and the log.
- **Herdr side pane**: `omo-scope open` splits your current [Herdr](https://herdr.dev) pane
  without stealing focus.

## Install

Prebuilt binary for Linux and macOS (x86_64 and arm64), no Rust required:

```bash
curl -fsSL https://raw.githubusercontent.com/pawissanutt/omo-scope/main/install.sh | sh
```

It installs to `~/.local/bin` after verifying the release checksum. Set
`OMO_SCOPE_INSTALL_DIR` or `OMO_SCOPE_VERSION` (for example `v0.2.0`) to change that.
From source: `cargo install --git https://github.com/pawissanutt/omo-scope`.

## Usage

```bash
omo-scope open    # inside Herdr: open the viewer in a pane to the right
omo-scope         # run in the current terminal
```

From an OmO prompt, `!omo-scope open` follows the current session (`$PI_SESSION_ID`). Running
`open` again reuses the existing pane, and `q` closes it. Options: `--cwd DIR`, `--session ID`,
`--ratio 0.6`, `--stats LIST`; see `omo-scope --help`.

## Controls

| Mouse | Key | Action |
|---|---|---|
| click session title | `s` | pick a session |
| click tab | `t` | switch Tasks / DAG |
| click row | `j` `k` | select a task or DAG node |
| click log entry | `x` (all) | expand or collapse a prompt, tool call, diff or reasoning block |
| wheel | `PgUp` `PgDn` | scroll the pane under the pointer (scrolling up pauses follow) |
| click `follow` | `f` / `End` | resume auto-scroll |
| drag the `≡` bar | `+` `-` `=` | resize the list / log split (`=` resets) |
| click `[zoom]` | `z` | show only the log |
| click the gear in the log title bar | `c` | open the stats settings menu |
| | `r` | reasoning: preview, full, off |
| | `Tab` | move focus between list and log |
| | `q` | quit |

## Stats and settings

The log title bar and each task-list row show their own ordered list of stats:

| Key | Example | Meaning |
|---|---|---|
| `turns` | `74 turns` | model turns |
| `tools` | `85 tools` | tool calls |
| `tps` | `23 tok/s` | output tokens over generation time per message, time-to-first-token included |
| `ctx` | `106k ctx`, `106k/400k ctx 26%` | context used, with the limit when known |
| `cache` | `99% cache` | cache hit rate |
| `cost` | `~$9.26` | list-price cost |
| `io` | `312k in · 16k out` | input and output tokens |
| `reasoning` | `2.1k think` | reasoning tokens |
| `compact` | `⇣2` | compactions |
| `tok` | `21M tok` | total tokens |

Defaults: the bar shows `turns, tools, tps, ctx, cache, cost` and rows show `cost`, on task rows and
DAG node rows alike. When space runs out, rows drop category and model first, then stats from the
end of the list, keeping elapsed time. A bar too narrow for one line keeps the status on the title line and wraps
the rest onto extra lines below it, so a narrow side pane still shows every stat.

Finished tasks use OmO's `run_stats` from the task file. Running tasks are computed live from
the transcript's per-message usage, so they update once per completed model message, not per
token. Context and compactions always come from the transcript, so they show only for the task
whose log is open. Other running tasks' rows show only what `run_stats` has, which is nothing
until the task finishes.

Cost is list price. For subscription providers (the provider name contains `subscription`) it's
an estimate shown with `~`. The cost mode is `auto`, `always` or `never`; `never` hides it for
subscriptions only. The context limit comes from a per-model override in the config, else from
`contextWindow` in `~/.omo/agent/models.json` and `models-store.json` (the agent dir honors
`$OMO_CODING_AGENT_DIR` and `$SENPI_CODING_AGENT_DIR`). With no known limit, only the count shows.

Press `c` or click the gear in the log title bar to open the settings menu. Each stat has `[x]`
checkboxes for bar and row, arrows to reorder and a live preview of the selected task's values.
Below them sit the cost mode and the context limit for the selected task's model (128k, 200k,
256k, 400k, 1M; stepping below 128k returns to auto), then Save, Reset defaults and Close.

| Key | Action |
|---|---|
| `j` `k` / arrows | select |
| `Space` / `b` | toggle in bar |
| `w` | toggle in row |
| `J` `K` | move the stat up or down |
| `m` | cycle cost mode |
| `[` `]` | context limit down / up |
| `Enter` / `S` | save |
| `R` | reset defaults |
| `Esc` `c` `q` | close |

Changes apply live; only Save persists them, atomically, to `$XDG_CONFIG_HOME/omo-scope/config`
(else `~/.config/omo-scope/config`). Unknown lines are ignored:

```
bar = turns, tools, tps, ctx, cache, cost
row = cost
cost = auto
ctx-limit.gpt-6-astra = 400000
```

`--stats LIST` overrides the bar list for one run (comma-separated keys, or `none`);
`omo-scope open` forwards it to the new pane.

## How it works

omo-scope polls OmO's on-disk state four times a second and reads only what changed. The task
store `<store>` is found automatically for the nearest ancestor of the start directory:
`~/.omo/agent/projects/<name>-<sha256(path)[..12]>/senpi-task` (current OmO) or
`<project>/.omo/senpi-task` (older OmO); if both exist, the one updated last wins.

- `<store>/tasks/*.json`: task records (status, model, timing, tokens)
- `<store>/dag/runs/*.json`: DAG checkpoints
- `<store>/children/<task>/sessions/...jsonl`: child transcripts, tailed incrementally
- `~/.omo/agent/sessions/`: session titles for the picker

Updates arrive per transcript entry. OmO keeps token deltas in memory, so text appears when
each message completes; the footer shows the running tool and its elapsed time. A command
that only prints to its terminal shows its output when it finishes.

## Development

```bash
cargo test
cargo clippy --all-targets -- -D warnings
python3 demo/record.py target/release/omo-scope demo/omo-scope.cast   # re-record the demo
```

Pushing a `v*` tag builds static release binaries with checksums.

## Credits

- [OmO / oh-my-openagent](https://github.com/code-yeongyu/oh-my-openagent) and
  [Senpi](https://github.com/code-yeongyu/senpi), whose task store and session files this reads.
- [Herdr](https://herdr.dev), the terminal workspace the side pane runs in.
- [omo-herdr-dag](https://github.com/jc01rho/omo-herdr-dag), which inspired the side-pane idea.

omo-scope is an independent community tool, not an official OmO component.

## License

[MIT](LICENSE)
