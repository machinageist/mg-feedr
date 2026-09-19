// Author: Jeff
// Date: 2026-09-19
// Description: The ticker daemon — fetch due sources, push new headlines to everyone listening
// Notes: Two threads. The accept thread greets each new listener; the main loop fetches and
//        publishes. Both go through the Hub, and both take its one lock, so a listener never
//        gets an item twice or misses one between its backlog and the live stream.
//        The cursor is the newest item id already pushed; mg-brief ids only grow, so "newer
//        than the cursor" is exactly "not pushed yet", whoever fetched it (this daemon, or a
//        manual `mg-brief fetch` of a ticker source).
//        Listeners only listen: nothing they send is ever read

use std::io::Write;
use std::os::unix::net::{UnixListener, UnixStream};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use anyhow::Result;
use chrono::Utc;
use mg_brief::{FetchResult, ItemQuery, Store};

use crate::wire::{self, Line, WireItem};

// how many recent headlines a new listener gets before the live ones
pub const BACKLOG: usize = 30;
// how often the daemon looks for due sources; each source still keeps its own interval
pub const TICK: Duration = Duration::from_secs(15);
// sources fetched at the same time, so one slow site cannot hold up the rest
pub const MAX_PARALLEL: usize = 4;
// the most one publish round sends; anything more waits for the next round
const PUBLISH_LIMIT: usize = 500;
const FETCH_MAX_BYTES: u64 = 5 * 1024 * 1024;
const FETCH_TIMEOUT_SECONDS: u64 = 20;
// a listener that cannot take a line within this is dropped rather than stalling everyone
const WRITE_TIMEOUT: Duration = Duration::from_secs(2);
// how often the loops wake to check for stop
const POLL: Duration = Duration::from_millis(100);

struct HubState {
    listeners: Vec<UnixStream>,
    cursor: i64,
}

pub struct Hub {
    store: Arc<Store>,
    state: Mutex<HubState>,
}

// Newest ticker items, at most `limit`, oldest first
fn recent(store: &Store, limit: usize) -> Result<Vec<mg_brief::FeedItem>> {
    store.items(&ItemQuery {
        ticker_only: true,
        limit,
        ..Default::default()
    })
}

// Write every line or fail; a failed listener is dropped by the caller
fn send(stream: &mut UnixStream, lines: &[Line]) -> Result<()> {
    let mut text = String::new();
    for line in lines {
        text.push_str(&wire::encode(line)?);
    }
    stream.write_all(text.as_bytes())?;
    Ok(())
}

impl Hub {
    // Start with the cursor at the newest ticker item, so nothing old is pushed as new
    pub fn new(store: Arc<Store>) -> Result<Self> {
        let cursor = recent(&store, 1)?.last().map_or(0, |i| i.id);
        Ok(Hub {
            store,
            state: Mutex::new(HubState {
                listeners: Vec::new(),
                cursor,
            }),
        })
    }

    // Greet a new listener: hello, then recent items up to the cursor; from then on it hears
    // every publish. Done under the lock so no publish can slip in between
    pub fn welcome(&self, mut stream: UnixStream) -> Result<()> {
        stream.set_write_timeout(Some(WRITE_TIMEOUT))?;
        let mut state = self.state.lock().expect("hub lock");
        let followed = self
            .store
            .list_sources()?
            .iter()
            .filter(|s| s.ticker)
            .count();
        let mut lines = vec![Line::Hello {
            version: env!("CARGO_PKG_VERSION").into(),
            followed,
        }];
        let cursor = state.cursor;
        lines.extend(
            recent(&self.store, BACKLOG)?
                .iter()
                .filter(|i| i.id <= cursor)
                .map(|i| Line::Item(WireItem::from(i))),
        );
        send(&mut stream, &lines)?;
        state.listeners.push(stream);
        Ok(())
    }

    // Push every ticker item newer than the cursor; returns how many went out
    pub fn publish(&self) -> Result<usize> {
        let mut state = self.state.lock().expect("hub lock");
        let fresh = self.store.items(&ItemQuery {
            since: Some(state.cursor),
            ticker_only: true,
            limit: PUBLISH_LIMIT,
            ..Default::default()
        })?;
        let Some(last) = fresh.last() else {
            return Ok(0);
        };
        let lines: Vec<Line> = fresh
            .iter()
            .map(|i| Line::Item(WireItem::from(i)))
            .collect();
        // a listener that errors (gone, or too slow) is dropped
        state.listeners.retain_mut(|l| send(l, &lines).is_ok());
        state.cursor = last.id;
        Ok(lines.len())
    }

    pub fn listener_count(&self) -> usize {
        self.state.lock().expect("hub lock").listeners.len()
    }
}

// Fetch the ticker sources that are due, a few at a time
pub fn fetch_due(store: &Store) -> Result<Vec<(String, Result<FetchResult>)>> {
    let due = store.due_ticker_sources(Utc::now())?;
    let mut results = Vec::with_capacity(due.len());
    for batch in due.chunks(MAX_PARALLEL) {
        std::thread::scope(|scope| {
            let handles: Vec<_> = batch
                .iter()
                .map(|s| {
                    let name = s.name.clone();
                    scope.spawn(move || {
                        (
                            name.clone(),
                            store.fetch(&name, FETCH_MAX_BYTES, FETCH_TIMEOUT_SECONDS),
                        )
                    })
                })
                .collect();
            for handle in handles {
                if let Ok(result) = handle.join() {
                    results.push(result);
                }
            }
        });
    }
    Ok(results)
}

// Run until `stop` is set: accept listeners on one thread, fetch and publish on this one
pub fn run(
    store: Store,
    listener: UnixListener,
    stop: Arc<AtomicBool>,
    tick: Duration,
) -> Result<()> {
    let store = Arc::new(store);
    let hub = Arc::new(Hub::new(Arc::clone(&store))?);
    // non-blocking accept, so the thread notices stop
    listener.set_nonblocking(true)?;
    let accept = {
        let hub = Arc::clone(&hub);
        let stop = Arc::clone(&stop);
        std::thread::spawn(move || {
            while !stop.load(Ordering::Relaxed) {
                match listener.accept() {
                    Ok((stream, _)) => {
                        // the accepted socket inherits non-blocking; listeners get plain writes
                        let greeted = stream
                            .set_nonblocking(false)
                            .map_err(anyhow::Error::from)
                            .and_then(|_| hub.welcome(stream));
                        if let Err(e) = greeted {
                            eprintln!("mg-feedr: a listener could not be greeted: {e:#}");
                        }
                    }
                    Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                        std::thread::sleep(POLL)
                    }
                    Err(e) => eprintln!("mg-feedr: accept failed: {e}"),
                }
            }
        })
    };

    while !stop.load(Ordering::Relaxed) {
        for (name, result) in fetch_due(&store)? {
            match result {
                Ok(r) if r.status == "failed" => {
                    eprintln!("mg-feedr: {name}: {}", r.error.unwrap_or_default())
                }
                Ok(_) => {}
                Err(e) => eprintln!("mg-feedr: {name}: {e:#}"),
            }
        }
        let sent = hub.publish()?;
        if sent > 0 {
            eprintln!(
                "mg-feedr: {sent} new headline(s) to {} listener(s)",
                hub.listener_count()
            );
        }
        // sleep in short steps so stop is noticed quickly
        let until = Instant::now() + tick;
        while Instant::now() < until && !stop.load(Ordering::Relaxed) {
            std::thread::sleep(POLL);
        }
    }
    let _ = accept.join();
    Ok(())
}
