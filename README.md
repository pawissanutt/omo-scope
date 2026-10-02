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
`OMO_SCOPE_INSTALL_DIR` or `OMO_SCOPE_VERSION` (for example `v0.1.1`) to change that.
From source: `cargo install --git https://github.com/pawissanutt/omo-scope`.

## Usage

```bash
omo-scope open    # inside Herdr: open the viewer in a pane to the right
omo-scope         # run in the current terminal
```

From an OmO prompt, `!omo-scope open` follows the current session (`$PI_SESSION_ID`). Running
`open` again reuses the existing pane, and `q` closes it. Options: `--cwd DIR`, `--session ID`,
`--ratio 0.6`; see `omo-scope --help`.

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
| | `r` | reasoning: preview, full, off |
| | `Tab` | move focus between list and log |
| | `q` | quit |

## How it works

omo-scope polls OmO's on-disk state four times a second and reads only what changed:

- `<project>/.omo/senpi-task/tasks/*.json`: task records (status, model, timing, tokens)
- `<project>/.omo/senpi-task/dag/runs/*.json`: DAG checkpoints
- `<project>/.omo/senpi-task/children/<task>/sessions/...jsonl`: child transcripts, tailed incrementally
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
