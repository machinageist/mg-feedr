// Author: Jeff
// Date: 2026-09-19
// Description: mg-feedr — the live headline ticker over mg-brief's catalog
// Notes: wire is the socket's line format, socket takes the socket over safely, daemon fetches
//        and publishes, client listens, open launches a headline, tui draws the ticker

pub mod client;
pub mod daemon;
pub mod open;
pub mod socket;
pub mod tui;
pub mod wire;
