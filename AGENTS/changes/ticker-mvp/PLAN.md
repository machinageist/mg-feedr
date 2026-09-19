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

## Status (2026-09-19)

Done. Core 6896252, CLI 824e207, TUI 38a3161. The user unit was installed and enabled at login
with 14 feeds Jeff chose (security, tech, world news, STEM). The shell side is dotfiles
bdc3430..57cf3b7. mg-brief additions it needed: default_paths 8eae2f4, decoded titles 26bb494.
