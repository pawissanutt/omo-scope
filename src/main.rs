mod app;
mod config;
mod diff;
mod herdr;
mod live;
mod locate;
mod pinned;
mod settings;
mod stats;
mod store;
mod text;
mod tools;
mod transcript;
mod ui;

use std::io::Write;
use std::path::PathBuf;
use std::time::{Duration, Instant};

use anyhow::{Context, bail};
use ratatui::crossterm::event::{self, Event};

use crate::app::App;
use crate::config::Config;

const USAGE: &str = "\
omo-scope - view-only TUI for OmO subagent tasks, DAG runs and live child transcripts

Usage:
  omo-scope [--cwd DIR] [--session ID] [--stats LIST] [--close-pane]
  omo-scope open [--cwd DIR] [--session ID] [--stats LIST] [--ratio 0.6]

  open           split the current Herdr pane and run the viewer beside it (no focus change)
  --cwd DIR      project directory (default: nearest ancestor with an OmO task store)
  --session ID   parent OmO session (default: $PI_SESSION_ID, else the newest session)
  --stats LIST   comma-separated title-bar stats for this run, or `none`
                 (turns,tools,tps,ctx,cache,cost,io,reasoning,compact,tok)
  --ratio R      share of the width kept by the current pane when opening (default 0.6)
  --close-pane   close this Herdr pane when the viewer quits

Keys: click/wheel anywhere, s sessions, t tasks/DAG, z zoom log, x expand all,
      p pinned plan/goal, f follow, c settings, Tab focus, j/k move, q quit";

#[derive(Default)]
struct Opts {
    open: bool,
    cwd: Option<PathBuf>,
    session: Option<String>,
    ratio: Option<f32>,
    stats: Option<String>,
    close_pane: bool,
}

fn value(args: &mut impl Iterator<Item = String>, flag: &str) -> anyhow::Result<String> {
    args.next().with_context(|| format!("{flag} needs a value"))
}

fn parse(mut args: impl Iterator<Item = String>) -> anyhow::Result<Option<Opts>> {
    let mut o = Opts::default();
    while let Some(a) = args.next() {
        match a.as_str() {
            "open" => o.open = true,
            "--cwd" => o.cwd = Some(value(&mut args, &a)?.into()),
            "--session" => o.session = Some(value(&mut args, &a)?),
            "--stats" => o.stats = Some(value(&mut args, &a)?),
            "--ratio" => {
                let r: f32 = value(&mut args, &a)?.parse().context("--ratio must be a number")?;
                if !(0.1..=0.9).contains(&r) {
                    bail!("--ratio must be between 0.1 and 0.9");
                }
                o.ratio = Some(r);
            }
            "--close-pane" => o.close_pane = true,
            "-h" | "--help" => {
                println!("{USAGE}");
                return Ok(None);
            }
            "-V" | "--version" => {
                println!("omo-scope {}", env!("CARGO_PKG_VERSION"));
                return Ok(None);
            }
            other => bail!("unknown argument `{other}`\n\n{USAGE}"),
        }
    }
    Ok(Some(o))
}

struct MouseGuard;

impl MouseGuard {
    fn enable() -> std::io::Result<Self> {
        let mut out = std::io::stdout();
        out.write_all(b"\x1b[?1000h\x1b[?1002h\x1b[?1006h")?;
        out.flush()?;
        Ok(Self)
    }
}

impl Drop for MouseGuard {
    fn drop(&mut self) {
        let mut out = std::io::stdout();
        let _ = out.write_all(b"\x1b[?1006l\x1b[?1002l\x1b[?1000l");
        let _ = out.flush();
    }
}

fn run(app: &mut App) -> anyhow::Result<()> {
    let mut terminal = ratatui::init();
    let _mouse = MouseGuard::enable()?;
    let tick = Duration::from_millis(250);
    let mut last = Instant::now();
    loop {
        if app.dirty {
            app.dirty = false;
            terminal.draw(|f| ui::draw(f, app))?;
        }
        if app.quit {
            return Ok(());
        }
        if event::poll(tick.saturating_sub(last.elapsed()))? {
            match event::read()? {
                Event::Key(k) => app.on_key(k),
                Event::Mouse(m) => app.on_mouse(m),
                Event::Resize(..) => app.dirty = true,
                _ => {}
            }
        }
        if last.elapsed() >= tick {
            last = Instant::now();
            app.dirty |= app.refresh();
        }
    }
}

fn main() -> anyhow::Result<()> {
    let Some(opts) = parse(std::env::args().skip(1))? else {
        return Ok(());
    };
    let mut config = Config::load();
    if let Some(list) = &opts.stats {
        config.set_bar_list(list)?;
    }
    if opts.open {
        let start = match opts.cwd {
            Some(c) => c,
            None => std::env::current_dir()?,
        };
        let root = locate::locate(&start).root;
        return herdr::open(&root, opts.session, opts.stats, opts.ratio.unwrap_or(0.6));
    }
    let mut app = App::new(opts.cwd, opts.session, config)?;
    let result = run(&mut app);
    ratatui::restore();
    result?;
    if opts.close_pane {
        herdr::close_own_pane();
    }
    Ok(())
}
