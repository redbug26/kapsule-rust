#[cfg(not(target_os = "emscripten"))]
mod cli;

#[cfg(not(target_os = "emscripten"))]
fn main() {
    cli::main();
}

#[cfg(target_os = "emscripten")]
fn main() {
    kapsule::web_main();
}
