#[cfg(not(target_os = "emscripten"))]
mod audio;
#[cfg(target_os = "emscripten")]
#[path = "audio_web.rs"]
mod audio;
mod dialog_spec;
#[cfg(not(target_os = "emscripten"))]
mod lua_dialog;
#[cfg(target_os = "emscripten")]
#[path = "dialog_web.rs"]
mod lua_dialog;
mod runtime;
#[cfg(not(target_os = "emscripten"))]
mod terminal;
mod ui;

#[cfg(target_os = "emscripten")]
pub use runtime::web_main;
#[cfg(not(target_os = "emscripten"))]
pub use runtime::{RunOptions, check, run};
