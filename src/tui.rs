// Author: Jeff
// Date: 2026-09-19
// Description: `mg-feedr tui` — the ticker in a terminal: newest headline on top, Enter opens it
// Notes: A listener thread reads the socket and hands lines to the loop through a channel;
//        when the daemon goes away it waits and tries again, so the TUI survives a restart.
//        State is pure (keys and lines in, effects out) and tested without a terminal.
//        Named ANSI colours only, so the terminal's theme decides the shades

use std::path::PathBuf;
use std::sync::mpsc::{self, Receiver, Sender};
use std::time::Duration;

use anyhow::Result;
use chrono::{DateTime, Local};
use ratatui::crossterm::event::{self, Event, KeyCode, KeyEvent, KeyEventKind};
use ratatui::layout::{Constraint, Layout};
use ratatui::style::{Color, Modifier, Style, Stylize};
use ratatui::text::{Line as TextLine, Span};
use ratatui::widgets::{List, ListItem, ListState, Paragraph};
use ratatui::{DefaultTerminal, Frame};

use crate::client;
use crate::open;
use crate::wire::{Line, WireItem};

// headlines kept in memory; older ones fall off the bottom
pub const KEEP: usize = 500;
const RECONNECT_AFTER: Duration = Duration::from_secs(3);
const FRAME: Duration = Duration::from_millis(250);
// nf-fa-video_camera
const VIDEO_GLYPH: &str = "\u{f03d}";
const ACCENT: Color = Color::Cyan;
const DIM: Color = Color::DarkGray;

// What the listener thread reports (an item is boxed: it dwarfs the other variant)
pub enum Feed {
    Line(Box<Line>),
    Gone(String),
}

#[derive(Debug, PartialEq)]
pub enum Effect {
    None,
    Quit,
    Open(Box<WireItem>),
}

#[derive(Default)]
pub struct State {
    // newest first
    pub items: Vec<WireItem>,
    pub selected: usize,
    pub connected: bool,
    pub followed: usize,
    pub message: Option<String>,
}

impl State {
    // Take one report from the listener
    pub fn take(&mut self, feed: Feed) {
        match feed {
            Feed::Line(line) => self.take_line(*line),
            Feed::Gone(reason) => {
                self.connected = false;
                self.message = Some(reason);
            }
        }
    }

    // One line from the daemon
    fn take_line(&mut self, line: Line) {
        match line {
            Line::Hello { followed, .. } => {
                self.connected = true;
                self.followed = followed;
                // a reconnect replays the backlog; start clean so nothing shows twice
                self.items.clear();
                self.selected = 0;
            }
            Line::Item(item) => {
                // keep the cursor on the same headline as new ones arrive above it
                if !self.items.is_empty() {
                    self.selected += 1;
                }
                self.items.insert(0, item);
                self.items.truncate(KEEP);
                self.selected = self.selected.min(self.items.len().saturating_sub(1));
            }
        }
    }

    // One keypress → a change and maybe something for the loop to do
    pub fn key(&mut self, key: KeyEvent) -> Effect {
        let last = self.items.len().saturating_sub(1);
        match key.code {
            KeyCode::Char('q') | KeyCode::Esc => return Effect::Quit,
            KeyCode::Down | KeyCode::Char('j') => self.selected = (self.selected + 1).min(last),
            KeyCode::Up | KeyCode::Char('k') => self.selected = self.selected.saturating_sub(1),
            KeyCode::Home | KeyCode::Char('g') => self.selected = 0,
            KeyCode::End | KeyCode::Char('G') => self.selected = last,
            KeyCode::Enter | KeyCode::Char('o') => {
                if let Some(item) = self.items.get(self.selected) {
                    return Effect::Open(Box::new(item.clone()));
                }
            }
            _ => {}
        }
        Effect::None
    }
}

// Listen forever, reconnecting after a pause whenever the daemon is gone
fn listen(path: PathBuf, to_loop: Sender<Feed>) {
    loop {
        let reason = match client::listen(&path) {
            Ok(lines) => {
                let mut reason = "the mg-feedr daemon stopped".to_string();
                for line in lines {
                    match line {
                        Ok(line) => {
                            if to_loop.send(Feed::Line(Box::new(line))).is_err() {
                                return;
                            }
                        }
                        Err(e) => {
                            reason = format!("{e:#}");
                            break;
                        }
                    }
                }
                reason
            }
            Err(e) => format!("{e:#}"),
        };
        if to_loop.send(Feed::Gone(reason)).is_err() {
            return;
        }
        std::thread::sleep(RECONNECT_AFTER);
    }
}

// Take over the terminal until quit, and always give it back
pub fn run(socket: PathBuf) -> Result<()> {
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || listen(socket, tx));
    let mut terminal = ratatui::init();
    let result = event_loop(&mut terminal, &rx);
    ratatui::restore();
    result
}

fn event_loop(terminal: &mut DefaultTerminal, rx: &Receiver<Feed>) -> Result<()> {
    let mut state = State::default();
    loop {
        while let Ok(feed) = rx.try_recv() {
            state.take(feed);
        }
        terminal.draw(|frame| draw(frame, &state))?;
        if !event::poll(FRAME)? {
            continue;
        }
        let Event::Key(key) = event::read()? else {
            continue;
        };
        if key.kind != KeyEventKind::Press {
            continue;
        }
        match state.key(key) {
            Effect::None => {}
            Effect::Quit => return Ok(()),
            Effect::Open(item) => {
                state.message = Some(match open::open(&item) {
                    Ok(open::Target::Player(_)) => format!("playing: {}", item.title),
                    Ok(open::Target::Browser(_)) => format!("opened: {}", item.title),
                    Err(e) => format!("{e:#}"),
                });
            }
        }
    }
}

// Local HH:MM a headline was first seen
fn clock(item: &WireItem) -> String {
    DateTime::parse_from_rfc3339(&item.first_seen_at)
        .map(|t| t.with_timezone(&Local).format("%H:%M").to_string())
        .unwrap_or_else(|_| "--:--".into())
}

// Header, headlines, then the selected one's summary and the key line
pub fn draw(frame: &mut Frame, state: &State) {
    let [top, list, detail, bottom] = Layout::vertical([
        Constraint::Length(1),
        Constraint::Min(3),
        Constraint::Length(2),
        Constraint::Length(1),
    ])
    .areas(frame.area());

    let status = if state.connected {
        Span::styled(
            format!("live · {} source(s) on the ticker", state.followed),
            Style::new().fg(Color::Green),
        )
    } else {
        Span::styled(
            "not connected — systemctl --user start mg-feedr",
            Style::new().fg(Color::Yellow),
        )
    };
    frame.render_widget(
        Paragraph::new(TextLine::from(vec![
            Span::styled("mg-feedr  ", Style::new().fg(ACCENT).bold()),
            status,
        ])),
        top,
    );

    let rows: Vec<ListItem> = state
        .items
        .iter()
        .map(|item| {
            let mut spans = vec![
                Span::styled(format!("{}  ", clock(item)), Style::new().fg(DIM)),
                Span::styled(format!("{:<14} ", item.source), Style::new().fg(ACCENT)),
                Span::raw(item.title.clone()),
            ];
            if item.has_video {
                spans.push(Span::styled(
                    format!("  {VIDEO_GLYPH}"),
                    Style::new().fg(Color::Magenta),
                ));
            }
            ListItem::new(TextLine::from(spans))
        })
        .collect();
    let mut list_state =
        ListState::default().with_selected((!state.items.is_empty()).then_some(state.selected));
    frame.render_stateful_widget(
        List::new(rows).highlight_style(Style::new().add_modifier(Modifier::REVERSED)),
        list,
        &mut list_state,
    );

    let summary = state
        .items
        .get(state.selected)
        .and_then(|i| i.summary.clone())
        .unwrap_or_default();
    frame.render_widget(Paragraph::new(summary).fg(DIM), detail);

    let keys = state
        .message
        .clone()
        .unwrap_or_else(|| "enter open · j/k move · g/G top/bottom · q quit".into());
    frame.render_widget(Paragraph::new(keys).fg(DIM), bottom);
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;
    use ratatui::crossterm::event::{KeyEventState, KeyModifiers};

    fn key(code: KeyCode) -> KeyEvent {
        KeyEvent {
            code,
            modifiers: KeyModifiers::NONE,
            kind: KeyEventKind::Press,
            state: KeyEventState::NONE,
        }
    }

    fn item(id: i64, title: &str, video: bool) -> WireItem {
        WireItem {
            id,
            source: "wire".into(),
            title: title.into(),
            url: Some(format!("https://e.example/{id}")),
            summary: Some(format!("about {title}")),
            published_at: None,
            first_seen_at: "2026-09-19T12:00:00+00:00".into(),
            enclosure_url: None,
            enclosure_type: None,
            image_url: None,
            has_video: video,
        }
    }

    #[test]
    fn new_headlines_go_on_top_and_the_cursor_stays_on_its_headline() {
        let mut s = State::default();
        s.take(Feed::Line(Box::new(Line::Hello {
            version: "x".into(),
            followed: 2,
        })));
        s.take(Feed::Line(Box::new(Line::Item(item(1, "old", false)))));
        s.take(Feed::Line(Box::new(Line::Item(item(2, "new", true)))));
        assert_eq!(s.items[0].title, "new");
        s.key(key(KeyCode::Char('j')));
        assert_eq!(s.items[s.selected].title, "old");
        s.take(Feed::Line(Box::new(Line::Item(item(3, "newer", false)))));
        assert_eq!(
            s.items[s.selected].title, "old",
            "still on the same headline"
        );
        assert_eq!(
            s.key(key(KeyCode::Enter)),
            Effect::Open(Box::new(item(1, "old", false)))
        );
        assert_eq!(s.key(key(KeyCode::Char('q'))), Effect::Quit);
    }

    #[test]
    fn a_reconnect_starts_clean_and_a_drop_is_shown() {
        let mut s = State::default();
        s.take(Feed::Line(Box::new(Line::Item(item(1, "a", false)))));
        s.take(Feed::Gone("gone".into()));
        assert!(!s.connected);
        s.take(Feed::Line(Box::new(Line::Hello {
            version: "x".into(),
            followed: 1,
        })));
        assert!(
            s.connected && s.items.is_empty(),
            "the backlog is about to be replayed"
        );
    }

    #[test]
    fn the_screen_shows_headlines_the_video_mark_and_the_status() {
        let mut s = State::default();
        s.take(Feed::Line(Box::new(Line::Hello {
            version: "x".into(),
            followed: 1,
        })));
        s.take(Feed::Line(Box::new(Line::Item(item(
            1,
            "Launch day",
            true,
        )))));
        let mut terminal = Terminal::new(TestBackend::new(80, 8)).unwrap();
        terminal.draw(|f| draw(f, &s)).unwrap();
        let screen: String = terminal
            .backend()
            .buffer()
            .content()
            .iter()
            .map(|c| c.symbol())
            .collect();
        assert!(
            screen.contains("Launch day")
                && screen.contains(VIDEO_GLYPH)
                && screen.contains("live")
        );
        assert!(
            screen.contains("about Launch day"),
            "the selected summary shows"
        );
        // an empty, disconnected ticker at a tiny size must not panic
        let mut tiny = Terminal::new(TestBackend::new(10, 3)).unwrap();
        tiny.draw(|f| draw(f, &State::default())).unwrap();
    }
}
