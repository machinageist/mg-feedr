<!--
Author: Jeff
Date: 2026-09-19
Description: mg-feedr — the live headline ticker for the Geist suite
-->

# mg-feedr

mg-brief collects feeds to read later. mg-feedr is its live side: a daemon fetches the sources
marked for the ticker and pushes each new headline, as it arrives, to everyone listening on
`$XDG_RUNTIME_DIR/mg-feedr.sock`. It links mg-brief as a library: the same catalog, and the
same guarded fetch (SSRF checks, pinned DNS, byte and redirect limits). It keeps no store of
its own.

```sh
mg-feedr add wire https://example.com/feed.xml --every 300   # register in mg-brief + follow
mg-feedr follow <source> [--every N] | unfollow <source>
mg-feedr sources [--json]
mg-feedr run                     # the daemon (user unit mg-feedr.service)
mg-feedr stream [--json]         # listen: hello, the latest 30, then live headlines
mg-feedr items [--limit N]       # recent ticker headlines from the catalog, no daemon needed
mg-feedr open <id>               # article → xdg-open; video → mpv --ytdl=yes
mg-feedr tui                     # the ticker in a terminal
```

The socket is push-only (listeners are never read from) and owner-only. A stale socket is
replaced at start, a live one refused, and a file that is not a socket is never deleted. Only
http(s) links are ever opened.

The shell side lives in dotfiles: `Services/Feeds.qml`, the ticker pill right of the weather,
the Feeds panel, and the crawl strip (`qs -c mgeist ipc call ticker toggle`).

## Gates

```sh
cargo fmt --check && cargo clippy --all-targets -- -D warnings && cargo test
```
