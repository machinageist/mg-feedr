// Author: Jeff
// Date: 2026-09-19
// Description: Open a headline — articles in the browser, videos in mpv
// Notes: Only http(s) links are opened. mg-brief already refuses to hand out anything else;
//        this checks again, because this is where a link becomes a running program.
//        Both launchers get an argv, never a shell, and mpv gets `--` so a URL can never be
//        read as an option

use std::process::{Command, Stdio};

use anyhow::{Context, Result, bail};

use crate::wire::WireItem;

const BROWSER: &str = "xdg-open";
const PLAYER: &str = "mpv";
const PLAYER_ARGS: [&str; 2] = ["--ytdl=yes", "--force-window=immediate"];

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Target {
    Browser(String),
    Player(String),
}

// A link we are willing to hand to a program
fn web(link: Option<&str>) -> Option<String> {
    link.filter(|l| l.starts_with("https://") || l.starts_with("http://"))
        .map(str::to_owned)
}

// Where this item goes: a video plays (its video file if it has one, else its page through
// yt-dlp); a podcast-style item with only an audio file plays too; anything else is read
pub fn target(item: &WireItem) -> Option<Target> {
    let video_file = item
        .enclosure_type
        .as_deref()
        .is_some_and(|t| t.to_ascii_lowercase().starts_with("video/"));
    if item.has_video {
        let chosen = if video_file {
            web(item.enclosure_url.as_deref())
        } else {
            None
        };
        return chosen
            .or_else(|| web(item.url.as_deref()))
            .map(Target::Player);
    }
    web(item.url.as_deref())
        .map(Target::Browser)
        .or_else(|| web(item.enclosure_url.as_deref()).map(Target::Player))
}

// Start the browser or player and let it run on its own
pub fn launch(target: &Target) -> Result<()> {
    let mut command = match target {
        Target::Browser(url) => {
            let mut c = Command::new(BROWSER);
            c.arg(url);
            c
        }
        Target::Player(url) => {
            let mut c = Command::new(PLAYER);
            c.args(PLAYER_ARGS).arg("--").arg(url);
            c
        }
    };
    command
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .with_context(|| format!("starting {:?}", command.get_program()))?;
    Ok(())
}

// Open an item, or say plainly why it cannot be
pub fn open(item: &WireItem) -> Result<Target> {
    let Some(chosen) = target(item) else {
        bail!("item {} has no link that can be opened", item.id)
    };
    launch(&chosen)?;
    Ok(chosen)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn item(url: Option<&str>, enclosure: Option<(&str, &str)>, has_video: bool) -> WireItem {
        WireItem {
            id: 1,
            source: "s".into(),
            title: "t".into(),
            url: url.map(Into::into),
            summary: None,
            published_at: None,
            first_seen_at: String::new(),
            enclosure_url: enclosure.map(|e| e.0.into()),
            enclosure_type: enclosure.map(|e| e.1.into()),
            image_url: None,
            has_video,
        }
    }

    #[test]
    fn articles_go_to_the_browser_and_videos_to_the_player() {
        assert_eq!(
            target(&item(Some("https://e.example/a"), None, false)),
            Some(Target::Browser("https://e.example/a".into()))
        );
        assert_eq!(
            target(&item(Some("https://youtube.com/watch?v=1"), None, true)),
            Some(Target::Player("https://youtube.com/watch?v=1".into())),
            "a video page plays through yt-dlp"
        );
        assert_eq!(
            target(&item(
                Some("https://e.example/p"),
                Some(("https://cdn.example/v.mp4", "video/mp4")),
                true
            )),
            Some(Target::Player("https://cdn.example/v.mp4".into())),
            "a video file plays directly"
        );
        assert_eq!(
            target(&item(
                None,
                Some(("https://cdn.example/ep.mp3", "audio/mpeg")),
                false
            )),
            Some(Target::Player("https://cdn.example/ep.mp3".into())),
            "audio with no page plays"
        );
    }

    #[test]
    fn nothing_but_http_is_ever_opened() {
        assert_eq!(
            target(&item(Some("javascript:alert(1)"), None, false)),
            None
        );
        assert_eq!(target(&item(Some("file:///etc/passwd"), None, true)), None);
        assert_eq!(target(&item(None, None, false)), None);
        assert!(open(&item(Some("file:///etc/passwd"), None, false)).is_err());
    }
}
