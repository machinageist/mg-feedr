<!--
Author: Jeff
Date: 2026-09-19
Description: mg-feedr MVP — a live headline ticker over mg-brief's catalog
Notes: Geistos cycle 02, slice F4. Decided with Jeff 2026-09-18
-->

# mg-feedr MVP

mg-brief collects feeds to read later; mg-feedr is its live side: a ticker of headlines
that arrive while you work. Click one to read it, or watch it in mpv if it is a video.

## Decisions (Jeff, 2026-09-18)

- The same back end as mg-brief, linked as a library (`mg-brief = { path = "../mg-briefr" }`).
  mg-feedr keeps no store of its own; it reads and writes mg-brief's catalog.
- Only sources marked for the ticker (`ticker` flag, M5) appear.
- A daemon plus a local socket: `mg-feedr run` fetches due ticker sources and pushes each new
  headline to everyone listening on `$XDG_RUNTIME_DIR/mg-feedr.sock`. Nothing polls the database
  for updates.
- Shell: a ticker pill in the centre beside the clock, a Feeds panel, and a crawl strip you can
  turn on or off (dotfiles side, same slice).
- Clicking opens the browser; a video item plays in mpv. A glyph marks video items.

## Behaviour

- `mg-feedr run`: every 15 s, fetch the ticker sources that are due (their own interval,
  default 300 s), at most 4 at a time, through mg-brief's guarded fetch. After each round,
  push every ticker item newer than the last one pushed.
- The socket is push-only. Clients never send anything the daemon reads, so there is no
  request parsing to attack. It is `0600` inside the per-user runtime folder. A stale socket
  is removed, a live one (another daemon) is refused, and a non-socket file at that path is
  never deleted.
- A new listener first gets a hello line, then the latest 30 ticker items, then live items,
  with no gap or duplicate between backlog and live (both are written under the same lock).
- One JSON object per line: `{"kind":"hello",…}` or `{"kind":"item",…mg-brief FeedItem…}`.
- `mg-feedr stream [--json]` relays the socket to stdout, one line per item, and exits
  non-zero when the daemon goes away (the shell restarts it with backoff).
- `mg-feedr open <id>`: an article goes to `xdg-open`; a video goes to
  `mpv --ytdl=yes -- <url>` (its video enclosure if there is one). Only http(s) links, as
  mg-brief hands out, can be opened.
- `mg-feedr follow|unfollow <source> [--every N]`, `add <name> <url> [--every N]`, `sources`,
  `items [--limit]` (reads the catalog, no daemon needed), and `tui` (a ratatui ticker over the
  socket).

## Acceptance

- Gates: `cargo fmt --check`, `cargo clippy --all-targets -- -D warnings`, `cargo test`.
- Tests: wire lines, stale/live/non-socket socket handling, a daemon round trip on a temp
  catalog with file:// fixtures (backlog, then a pushed item after the fixture changes),
  open-target selection.
- Live: the user unit runs; `stream` shows items; the shell pill, panel and strip show them.
