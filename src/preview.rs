//! `linkctl preview`: spawn an external video player on the streaming node.
//!
//! This is the one place where spawning a subprocess is intended: the player
//! is a user-facing frontend, and opening the stream is exactly what makes
//! the camera "active" for every other command.

use std::path::Path;
use std::process::{Command, Stdio};

use crate::cli::Player;
use crate::error::{Error, Result};

/// Build the player command line. Kept separate from spawning so it can be
/// unit-tested.
pub fn build_command(
    player: Player,
    stream_node: &Path,
    title: &str,
    resolution: &str,
) -> Vec<String> {
    let node = stream_node.display().to_string();
    match player {
        // Low-latency flags: no input buffering, drop late frames, request
        // MJPEG at a modest size so decoding starts immediately.
        Player::Ffplay => vec![
            "ffplay".into(),
            "-hide_banner".into(),
            "-loglevel".into(),
            "error".into(),
            "-f".into(),
            "v4l2".into(),
            "-input_format".into(),
            "mjpeg".into(),
            "-video_size".into(),
            resolution.into(),
            "-fflags".into(),
            "nobuffer".into(),
            "-flags".into(),
            "low_delay".into(),
            "-framedrop".into(),
            "-window_title".into(),
            title.into(),
            "-i".into(),
            node,
        ],
        Player::Mpv => vec![
            "mpv".into(),
            "--profile=low-latency".into(),
            "--untimed".into(),
            "--demuxer-lavf-format=v4l2".into(),
            format!("--demuxer-lavf-o=input_format=mjpeg,video_size={resolution}"),
            format!("--title={title}"),
            format!("av://v4l2:{node}"),
        ],
    }
}

/// Locate `program` on `PATH`. No shell involved.
pub fn find_in_path(program: &str) -> Option<std::path::PathBuf> {
    let path = std::env::var_os("PATH")?;
    std::env::split_paths(&path)
        .map(|dir| dir.join(program))
        .find(|candidate| candidate.is_file())
}

/// Run the preview and return the player's exit status code.
pub fn run(player: Player, stream_node: &Path, title: &str, resolution: &str) -> Result<i32> {
    let argv = build_command(player, stream_node, title, resolution);
    let program = &argv[0];
    if find_in_path(program).is_none() {
        let hint = match player {
            Player::Ffplay => "Install FFmpeg or use another application to open the camera.",
            Player::Mpv => "Install mpv or use another application to open the camera.",
        };
        return Err(Error::Preview(format!(
            "{program} was not found.\n\n{hint}"
        )));
    }
    let status = Command::new(program)
        .args(&argv[1..])
        .stdin(Stdio::inherit())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit())
        .status()
        .map_err(|e| Error::Preview(format!("failed to start {program}: {e}")))?;
    Ok(status.code().unwrap_or(1))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ffplay_command_shape() {
        let argv = build_command(
            Player::Ffplay,
            Path::new("/dev/video0"),
            "Insta360 Link 2",
            "1280x720",
        );
        assert_eq!(argv[0], "ffplay");
        assert!(argv.windows(2).any(|w| w == ["-f", "v4l2"]));
        assert!(argv.windows(2).any(|w| w == ["-fflags", "nobuffer"]));
        assert!(argv
            .windows(2)
            .any(|w| w == ["-window_title", "Insta360 Link 2"]));
        assert!(argv.windows(2).any(|w| w == ["-video_size", "1280x720"]));
        // Input must come last so option ordering is unambiguous for ffplay.
        assert_eq!(&argv[argv.len() - 2..], ["-i", "/dev/video0"]);
    }

    #[test]
    fn mpv_command_shape() {
        let argv = build_command(Player::Mpv, Path::new("/dev/video4"), "T", "640x480");
        assert_eq!(argv[0], "mpv");
        assert_eq!(argv.last().unwrap(), "av://v4l2:/dev/video4");
        assert!(argv.iter().any(|a| a == "--title=T"));
    }

    #[test]
    fn find_in_path_behaviour() {
        assert!(find_in_path("definitely-not-a-real-binary-xyz").is_none());
        // `sh` exists on every Unix we support.
        assert!(find_in_path("sh").is_some());
    }
}
