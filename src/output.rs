//! Output modes: human-readable (default), `--quiet`, `--json`, `--verbose`.
//!
//! Normal output goes to stdout; diagnostics and errors go to stderr. JSON
//! mode never mixes decorative text into stdout.

use std::io::Write;

use serde::Serialize;

use crate::error::Error;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    Normal,
    Quiet,
    Json,
}

#[derive(Debug, Clone, Copy)]
pub struct Output {
    pub mode: Mode,
    pub verbose: bool,
}

impl Output {
    pub fn new(json: bool, quiet: bool, verbose: bool) -> Self {
        let mode = if json {
            Mode::Json
        } else if quiet {
            Mode::Quiet
        } else {
            Mode::Normal
        };
        Self { mode, verbose }
    }

    /// Emit a command result: `human` is used in normal mode, `value` in JSON
    /// mode, nothing in quiet mode.
    pub fn emit<T: Serialize>(&self, human: impl FnOnce() -> String, value: &T) {
        match self.mode {
            Mode::Quiet => {}
            Mode::Normal => {
                let s = human();
                if !s.is_empty() {
                    println!("{s}");
                }
            }
            Mode::Json => {
                let mut out = std::io::stdout().lock();
                // Ignore EPIPE etc.: nothing sensible to do at this point.
                let _ = serde_json::to_writer_pretty(&mut out, value);
                let _ = out.write_all(b"\n");
            }
        }
    }

    /// Verbose diagnostic line (stderr), only with `--verbose`.
    pub fn debug(&self, msg: impl AsRef<str>) {
        if self.verbose {
            eprintln!("linkctl: {}", msg.as_ref());
        }
    }

    /// Non-fatal note (stderr) shown unless quiet or JSON.
    pub fn note(&self, msg: impl AsRef<str>) {
        if self.mode == Mode::Normal {
            eprintln!("{}", msg.as_ref());
        }
    }

    /// Report an error on stderr in the current mode.
    pub fn error(&self, err: &Error) {
        if self.mode == Mode::Json {
            let v = serde_json::json!({
                "error": err.kind(),
                "message": err.to_string(),
                "hint": err.hint(),
                "exit_code": err.exit_code() as i32,
            });
            eprintln!("{}", serde_json::to_string(&v).unwrap_or_default());
            return;
        }
        eprintln!("{err}");
        if let Some(hint) = err.hint() {
            eprintln!();
            eprintln!("{hint}");
        }
        if self.verbose {
            eprintln!("(exit code {})", err.exit_code() as i32);
        }
    }
}
