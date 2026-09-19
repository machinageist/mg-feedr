// Author: Jeff
// Date: 2026-09-19
// Description: Listen to the ticker socket — the `stream` command and the TUI both use this
// Notes: A listener only reads. A missing daemon is a clear error that says how to start it

use std::io::{BufRead, BufReader};
use std::os::unix::net::UnixStream;
use std::path::Path;

use anyhow::{Context, Result};

use crate::wire::{self, Line};

const NOT_RUNNING: &str = "mg-feedr is not running (systemctl --user start mg-feedr)";

// Connect and yield each line as it arrives; the iterator ends when the daemon goes away
pub fn listen(path: &Path) -> Result<impl Iterator<Item = Result<Line>>> {
    let stream = UnixStream::connect(path).context(NOT_RUNNING)?;
    Ok(BufReader::new(stream)
        .lines()
        .map(|line| wire::decode(&line?)))
}
