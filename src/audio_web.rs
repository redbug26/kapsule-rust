use anyhow::{Result, bail};
use std::{ffi::CString, path::Path};
unsafe extern "C" {
    fn kapsule_beep();
    fn kapsule_audio_play(path: *const std::ffi::c_char) -> i32;
    fn kapsule_audio_stop();
    fn kapsule_audio_playing() -> i32;
}
pub fn beep() {
    unsafe {
        kapsule_beep();
    }
}
pub struct AudioInfo {
    pub name: String,
    pub format: String,
    pub songs: u32,
}
pub fn play_audio_file(path: &Path) -> Result<AudioInfo> {
    if !path.is_file() {
        bail!("Audio file not found: {}", path.display());
    }
    let text = CString::new(path.to_string_lossy().as_bytes())?;
    if unsafe { kapsule_audio_play(text.as_ptr()) } == 0 {
        bail!("Browser audio playback is unavailable");
    }
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
    unsafe {
        kapsule_audio_stop();
    }
}
pub fn is_module_playing() -> bool {
    unsafe { kapsule_audio_playing() != 0 }
}
