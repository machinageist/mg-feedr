// Author: Jeff
// Date: 2026-09-19
// Description: What travels over the ticker socket — one JSON object per line
// Notes: Two kinds of line: a hello when a listener connects, then items. The shape is
//        mg-feedr's own contract (the shell, the TUI and `stream` read it), copied field by
//        field from mg-brief's FeedItem so a change there cannot silently change the socket

use anyhow::{Context, Result};
use mg_brief::FeedItem;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct WireItem {
    pub id: i64,
    pub source: String,
    pub title: String,
    pub url: Option<String>,
    pub summary: Option<String>,
    pub published_at: Option<String>,
    pub first_seen_at: String,
    pub enclosure_url: Option<String>,
    pub enclosure_type: Option<String>,
    pub image_url: Option<String>,
    pub has_video: bool,
}

impl From<&FeedItem> for WireItem {
    fn from(item: &FeedItem) -> Self {
        WireItem {
            id: item.id,
            source: item.source.clone(),
            title: item.title.clone(),
            url: item.url.clone(),
            summary: item.summary.clone(),
            published_at: item.published_at.clone(),
            first_seen_at: item.first_seen_at.clone(),
            enclosure_url: item.enclosure_url.clone(),
            enclosure_type: item.enclosure_type.clone(),
            image_url: item.image_url.clone(),
            has_video: item.has_video,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum Line {
    // first line to every listener: who is talking and how many sources feed it
    Hello { version: String, followed: usize },
    Item(WireItem),
}

// One line, newline included
pub fn encode(line: &Line) -> Result<String> {
    let mut text = serde_json::to_string(line)?;
    text.push('\n');
    Ok(text)
}

// Read one line back; anything that is not a known line is an error, never a guess
pub fn decode(text: &str) -> Result<Line> {
    serde_json::from_str(text.trim_end()).context("not an mg-feedr line")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn item() -> WireItem {
        WireItem {
            id: 7,
            source: "wire".into(),
            title: "Hello".into(),
            url: Some("https://e.example/a".into()),
            summary: None,
            published_at: None,
            first_seen_at: "2026-09-19T00:00:00+00:00".into(),
            enclosure_url: None,
            enclosure_type: None,
            image_url: None,
            has_video: true,
        }
    }

    #[test]
    fn lines_round_trip_and_carry_their_kind() {
        let hello = Line::Hello {
            version: "0.1.0".into(),
            followed: 3,
        };
        let text = encode(&hello).unwrap();
        assert!(
            text.ends_with('\n') && text.matches('\n').count() == 1,
            "exactly one line"
        );
        assert!(text.contains(r#""kind":"hello""#));
        assert_eq!(decode(&text).unwrap(), hello);
        let line = Line::Item(item());
        let text = encode(&line).unwrap();
        assert!(text.contains(r#""kind":"item""#) && text.contains(r#""has_video":true"#));
        assert_eq!(decode(&text).unwrap(), line);
    }

    #[test]
    fn unknown_or_broken_lines_are_errors() {
        assert!(decode(r#"{"kind":"launch","url":"x"}"#).is_err());
        assert!(decode("{not json").is_err());
        assert!(decode("").is_err());
    }
}
