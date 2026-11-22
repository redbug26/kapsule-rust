use anyhow::{Context, Result};
use std::{cell::RefCell, fs::File, path::Path};
struct Playback {
    _stream: rodio::MixerDeviceSink,
    player: rodio::Player,
}
thread_local! { static PLAYBACK: RefCell<Option<Playback>> = const { RefCell::new(None) }; }
pub struct AudioInfo {
    pub name: String,
    pub format: String,
    pub songs: u32,
}
pub fn play_audio_file(path: &Path) -> Result<AudioInfo> {
    let decoder = rodio::Decoder::try_from(File::open(path)?).context("Decoding audio (standalone supports WAV, MP3, FLAC, Ogg and other Symphonia formats; tracker plugins are not included)")?;
    let mut stream =
        rodio::DeviceSinkBuilder::open_default_sink().context("Opening audio output")?;
    stream.log_on_drop(false);
    let player = rodio::Player::connect_new(stream.mixer());
    player.append(decoder);
    PLAYBACK.with(|state| {
        *state.borrow_mut() = Some(Playback {
            _stream: stream,
            player,
        })
    });
    Ok(AudioInfo {
        name: path
            .file_name()
            .unwrap_or_default()
            .to_string_lossy()
            .into_owned(),
        format: path
            .extension()
            .unwrap_or_default()
            .to_string_lossy()
            .to_uppercase(),
        songs: 1,
    })
}
pub fn stop_module() {
    PLAYBACK.with(|state| *state.borrow_mut() = None);
}
pub fn is_module_playing() -> bool {
    PLAYBACK.with(|state| state.borrow().as_ref().is_some_and(|p| !p.player.empty()))
}
