<!--
Author: Jeff
Date: 2026-09-19
Description: F4 slices, each committed with its gates green
-->

# mg-feedr plan

1. Core: wire format, socket preparation, the daemon hub (backlog plus live under one lock),
   the fetch loop with bounded concurrency, the stream client, open-target selection → tests.
2. CLI: run, stream, follow/unfollow/add/sources/items, open → live check against a scratch catalog.
3. TUI (ratatui) over the socket.
4. User unit `mg-feedr.service` in dotfiles, installed after Jeff picks the first ticker feeds.
5. Shell: Services/Feeds.qml + FeedState.js, TickerPill, FeedsPanel, TickerStrip, IPC, launcher.
