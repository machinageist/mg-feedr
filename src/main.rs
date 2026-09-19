// Author: Jeff
// Date: 2026-09-19
// Description: mg-feedr command line — run the ticker daemon, listen to it, choose its sources
// Notes: `run` is what the user unit starts. `stream` is what the shell listens with. The rest
//        read or change mg-brief's catalog directly and work without the daemon.
//        Sources are mg-brief sources; "following" one just turns its ticker flag on

use std::io::Write;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::AtomicBool;

use anyhow::{Context, Result, bail};
use chrono::{DateTime, Local};
use clap::{Parser, Subcommand};
use mg_brief::{ItemQuery, Store};

use mg_feedr::wire::{Line, WireItem};
use mg_feedr::{client, daemon, open, socket};

// how many headlines `items` shows unless asked
const DEFAULT_ITEMS: usize = 20;
// the video mark in plain-text output: nf-fa-video_camera
const VIDEO_GLYPH: char = '\u{f03d}';

#[derive(Parser)]
#[command(
    name = "mg-feedr",
    version,
    about = "Live headline ticker over mg-brief's catalog"
)]
struct Cli {
    /// Print JSON instead of text
    #[arg(long, global = true)]
    json: bool,
    /// The ticker socket (default $XDG_RUNTIME_DIR/mg-feedr.sock)
    #[arg(long, global = true)]
    socket: Option<PathBuf>,
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Run the daemon: fetch due ticker sources and push new headlines to listeners
    Run,
    /// Listen to the daemon and print each headline as it arrives
    Stream,
    /// Put an mg-brief source on the ticker
    Follow {
        source: String,
        /// Seconds between checks (30–86400; default 300)
        #[arg(long)]
        every: Option<i64>,
    },
    /// Take a source off the ticker (it stays in mg-brief)
    Unfollow { source: String },
    /// Register a new feed in mg-brief and put it on the ticker
    Add {
        name: String,
        url: String,
        #[arg(long)]
        every: Option<i64>,
    },
    /// Sources on the ticker
    Sources,
    /// Recent ticker headlines from the catalog (no daemon needed)
    Items {
        #[arg(long, default_value_t = DEFAULT_ITEMS)]
        limit: usize,
    },
    /// Open a headline: articles in the browser, videos in mpv
    Open { id: i64 },
    /// The ticker in the terminal
    Tui,
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    let socket_path = || cli.socket.clone().map_or_else(socket::default_path, Ok);
    match cli.command {
        Command::Run => {
            let listener = socket::prepare(&socket_path()?)?;
            // the user unit stops us with SIGTERM; the next start clears the stale socket
            daemon::run(
                catalog()?,
                listener,
                Arc::new(AtomicBool::new(false)),
                daemon::TICK,
            )
        }
        Command::Stream => stream(&socket_path()?, cli.json),
        Command::Follow { source, every } => {
            show(cli.json, &catalog()?.set_ticker(&source, true, every)?)
        }
        Command::Unfollow { source } => {
            show(cli.json, &catalog()?.set_ticker(&source, false, None)?)
        }
        Command::Add { name, url, every } => {
            let store = catalog()?;
            store.register(&name, &url, None)?;
            show(cli.json, &store.set_ticker(&name, true, every)?)
        }
        Command::Sources => {
            let followed: Vec<_> = catalog()?
                .list_sources()?
                .into_iter()
                .filter(|s| s.ticker)
                .collect();
            if cli.json {
                println!("{}", serde_json::to_string(&followed)?);
            } else {
                for s in &followed {
                    println!(
                        "{:<24} every {:>5}s  last {}",
                        s.name,
                        s.fetch_interval_seconds,
                        s.last_fetched_at.as_deref().unwrap_or("never")
                    );
                }
            }
            Ok(())
        }
        Command::Items { limit } => {
            let items: Vec<WireItem> = catalog()?
                .items(&ItemQuery {
                    ticker_only: true,
                    limit,
                    ..Default::default()
                })?
                .iter()
                .map(WireItem::from)
                .collect();
            let mut out = std::io::stdout().lock();
            for item in &items {
                if cli.json {
                    writeln!(out, "{}", serde_json::to_string(item)?)?;
                } else {
                    writeln!(out, "{}", headline(item))?;
                }
            }
            Ok(())
        }
        Command::Tui => mg_feedr::tui::run(socket_path()?),
        Command::Open { id } => {
            let item = find(&catalog()?, id)?;
            let target = open::open(&item)?;
            if cli.json {
                println!(
                    "{}",
                    serde_json::json!({ "ok": true, "id": id, "player": matches!(target, open::Target::Player(_)) })
                );
            }
            Ok(())
        }
    }
}

// mg-brief's catalog, where mg-brief itself keeps it
fn catalog() -> Result<Store> {
    let (db, artifacts) = mg_brief::default_paths();
    Store::open(db, artifacts).context("opening mg-brief's catalog")
}

// One item by id, from any source
fn find(store: &Store, id: i64) -> Result<WireItem> {
    let found = store.items(&ItemQuery {
        since: Some(id - 1),
        limit: 1,
        ..Default::default()
    })?;
    match found.first() {
        Some(item) if item.id == id => Ok(WireItem::from(item)),
        _ => bail!("no headline with id {id}"),
    }
}

// A source after a change: JSON, or one line
fn show(json: bool, source: &mg_brief::Source) -> Result<()> {
    if json {
        println!("{}", serde_json::to_string(source)?);
    } else {
        let state = if source.ticker {
            "on the ticker"
        } else {
            "off the ticker"
        };
        println!(
            "{} is {state} (every {}s)",
            source.name, source.fetch_interval_seconds
        );
    }
    Ok(())
}

// "14:02  wire  Headline text [video]" — first-seen time in local time
fn headline(item: &WireItem) -> String {
    let time = DateTime::parse_from_rfc3339(&item.first_seen_at)
        .map(|t| t.with_timezone(&Local).format("%H:%M").to_string())
        .unwrap_or_else(|_| "--:--".into());
    let video = if item.has_video {
        format!(" {VIDEO_GLYPH}")
    } else {
        String::new()
    };
    format!("{time}  {:<16} {}{video}", item.source, item.title)
}

// Relay the daemon's lines to stdout until it goes away (exit 1) or stdout closes (exit 0)
fn stream(path: &std::path::Path, json: bool) -> Result<()> {
    let mut out = std::io::stdout().lock();
    for line in client::listen(path)? {
        let text = match line? {
            // JSON mode passes every line on, hello included, so the shell knows the daemon is up
            ref l if json => serde_json::to_string(l)?,
            Line::Hello { followed, .. } => {
                format!("mg-feedr: listening, {followed} source(s) on the ticker")
            }
            Line::Item(item) => headline(&item),
        };
        // a closed pipe (the shell stopped listening) is a normal way to end
        if writeln!(out, "{text}").and_then(|_| out.flush()).is_err() {
            return Ok(());
        }
    }
    bail!("the mg-feedr daemon stopped")
}
