use std::io::{IsTerminal, Write};
use std::sync::Mutex;
use std::time::{Duration, Instant};

use bpfs_core::progress::{Monitor, Phase, Progress};

/// Prints log messages and a live progress line to stderr.
pub struct StderrMonitor {
    state: Mutex<State>,
    live: bool,
}

struct State {
    last_draw: Option<Instant>,
    phase: Option<Phase>,
    line_open: bool,
}

const REDRAW: Duration = Duration::from_millis(200);

fn phase_name(phase: Phase) -> &'static str {
    match phase {
        Phase::Opening => "Opening archive",
        Phase::Scanning => "Scanning",
        Phase::Hashing => "Checking contents",
        Phase::Packing => "Compressing & writing",
        Phase::Verifying => "Verifying",
        Phase::VerifyingContent => "Checking file contents",
        Phase::Extracting => "Restoring",
    }
}

fn mb(n: u64) -> f64 {
    n as f64 / (1024.0 * 1024.0)
}

impl StderrMonitor {
    pub fn new() -> Self {
        Self {
            state: Mutex::new(State {
                last_draw: None,
                phase: None,
                line_open: false,
            }),
            live: std::io::stderr().is_terminal(),
        }
    }

    fn end_line(state: &mut State) {
        if state.line_open {
            eprintln!();
            state.line_open = false;
        }
    }
}

impl Monitor for StderrMonitor {
    fn progress(&self, p: Progress<'_>) {
        let Ok(mut state) = self.state.try_lock() else {
            return;
        };
        let now = Instant::now();
        if state.phase != Some(p.phase) {
            Self::end_line(&mut state);
            state.phase = Some(p.phase);
            if !self.live {
                eprintln!("{}…", phase_name(p.phase));
            }
        } else if state.last_draw.is_some_and(|t| now - t < REDRAW) {
            return;
        }
        if !self.live {
            return;
        }
        state.last_draw = Some(now);

        let amount = match p.phase {
            Phase::Scanning => format!("{} items", p.done),
            _ if p.total > 0 => format!(
                "{:.0}% ({:.1} / {:.1} MB)",
                p.done as f64 * 100.0 / p.total as f64,
                mb(p.done),
                mb(p.total)
            ),
            _ => String::new(),
        };
        let item = p
            .item
            .map(|i| format!("  {}", i.path.display()))
            .unwrap_or_default();
        let mut line = format!("{}: {amount}{item}", phase_name(p.phase));
        line.truncate(line.floor_char_boundary(118));
        let mut err = std::io::stderr().lock();
        let _ = write!(err, "\r\x1b[2K{line}");
        let _ = err.flush();
        state.line_open = true;
    }

    fn log(&self, message: &str) {
        let mut state = self.state.lock().unwrap();
        if state.line_open {
            eprint!("\r\x1b[2K");
            state.line_open = false;
        }
        eprintln!("{message}");
    }
}

impl Drop for StderrMonitor {
    fn drop(&mut self) {
        Self::end_line(&mut self.state.lock().unwrap());
    }
}
