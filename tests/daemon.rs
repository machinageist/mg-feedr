// Author: Jeff
// Date: 2026-09-19
// Description: The daemon end to end on a scratch catalog — greet, backlog, fetch, push
// Notes: Feeds are file:// fixtures in trusted fixture mode; the socket lives in the temp dir

use std::fs;
use std::io::{BufRead, BufReader};
use std::os::unix::net::UnixStream;
use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use mg_brief::Store;
use mg_feedr::wire::{self, Line};
use mg_feedr::{daemon, socket};

// A feed with the given headlines, newest last
fn feed(titles: &[&str]) -> String {
    let items: String = titles
        .iter()
        .map(|t| {
            format!(
                "<item><title>{t}</title><guid>{t}</guid><link>https://e.example/{t}</link></item>"
            )
        })
        .collect();
    format!(
        r#"<?xml version="1.0"?><rss version="2.0"><channel><title>W</title>{items}</channel></rss>"#
    )
}

// A store on the scratch catalog that may read the fixtures folder
fn store(dir: &Path) -> Store {
    Store::open_with_trusted_file_root(
        dir.join("catalog.sqlite"),
        dir.join("artifacts"),
        dir.join("fixtures"),
    )
    .unwrap()
}

// Read the next line, waiting at most the stream's read timeout
fn next(reader: &mut BufReader<UnixStream>) -> Line {
    let mut text = String::new();
    reader
        .read_line(&mut text)
        .expect("a line before the timeout");
    wire::decode(&text).unwrap()
}

#[test]
fn a_listener_gets_hello_backlog_then_only_new_headlines() {
    let dir = tempfile::tempdir().unwrap();
    fs::create_dir_all(dir.path().join("fixtures")).unwrap();
    let fixture = dir.path().join("fixtures/wire.xml");
    fs::write(&fixture, feed(&["one", "two"])).unwrap();
    let url = url::Url::from_file_path(fs::canonicalize(&fixture).unwrap()).unwrap();

    let setup = store(dir.path());
    setup.register("wire", url.as_str(), None).unwrap();
    setup.set_ticker("wire", true, Some(30)).unwrap();
    setup.fetch("wire", 1 << 20, 5).unwrap();

    // the daemon, on its own store handle, ticking fast
    let socket_path = dir.path().join(socket::SOCKET_NAME);
    let listener = socket::prepare(&socket_path).unwrap();
    let stop = Arc::new(AtomicBool::new(false));
    let running = {
        let stop = Arc::clone(&stop);
        let daemon_store = store(dir.path());
        std::thread::spawn(move || {
            daemon::run(daemon_store, listener, stop, Duration::from_millis(200))
        })
    };

    let stream = UnixStream::connect(&socket_path).unwrap();
    stream
        .set_read_timeout(Some(Duration::from_secs(5)))
        .unwrap();
    let mut reader = BufReader::new(stream);
    assert!(matches!(next(&mut reader), Line::Hello { followed: 1, .. }));
    let backlog: Vec<String> = (0..2)
        .map(|_| match next(&mut reader) {
            Line::Item(i) => i.title,
            other => panic!("expected an item, got {other:?}"),
        })
        .collect();
    assert_eq!(backlog, ["one", "two"]);

    // a new headline appears and the source falls due: the daemon fetches and pushes it
    fs::write(&fixture, feed(&["one", "two", "three"])).unwrap();
    let c = rusqlite::Connection::open(dir.path().join("catalog.sqlite")).unwrap();
    c.execute(
        "UPDATE sources SET last_fetched_at='2000-01-01T00:00:00+00:00'",
        [],
    )
    .unwrap();
    match next(&mut reader) {
        Line::Item(i) => assert_eq!(i.title, "three", "only the new one, no repeats"),
        other => panic!("expected an item, got {other:?}"),
    }

    stop.store(true, Ordering::Relaxed);
    running.join().unwrap().unwrap();
}
