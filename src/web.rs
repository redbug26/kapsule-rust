//! Emscripten browser host. The manifest, Lua modules and drawing buffer are shared.
use super::*;
use serde::Deserialize;
use serde_json::{Value as Json, json};
use std::ffi::{CStr, CString, c_char};

struct BrowserApp {
    _lua: Lua,
    app: Table,
    graphics: Rc<RefCell<GraphicsBuffer>>,
    quit: Rc<Cell<bool>>,
    keys: Rc<RefCell<HashMap<String, Instant>>>,
    name: String,
    fps: u32,
    shortcuts: Vec<ShortcutBarItem>,
    window_width: u16,
    window_height: u16,
}
impl BrowserApp {
    fn start(
        path: &str,
        args: &[String],
        width: u16,
        height: u16,
        expected_id: Option<&str>,
    ) -> Result<Self> {
        validate_size(width, height)?;
        let (descriptor, script) = descriptor(Path::new(path))?;
        if let Some(expected) = expected_id {
            anyhow::ensure!(
                descriptor.manifest.application.id == expected,
                "The store Kapsule ID does not match apps.toml"
            );
        }
        let lua = Lua::new();
        lua.set_memory_limit(64 * 1024 * 1024)?;
        let os: Table = lua.globals().get("os")?;
        os.set(
            "exit",
            lua.create_function(|_, _args: mlua::MultiValue| {
                Err::<(), _>(mlua::Error::external(
                    "os.exit is unavailable in the browser; use kkc.quit",
                ))
            })?,
        )?;
        // Browser files are scoped to Emscripten's in-memory filesystem.
        let graphics = Rc::new(RefCell::new(GraphicsBuffer::new(width, height)));
        let quit = Rc::new(Cell::new(false));
        let keys = Rc::new(RefCell::new(HashMap::new()));
        install_lua_app_modules(
            &lua,
            &descriptor.manifest,
            &descriptor.app_dir,
            args,
            ModuleContext {
                graphics: graphics.clone(),
                should_quit: quit.clone(),
                key_state: keys.clone(),
                has_release_events: true,
                start_time: Instant::now(),
                launch_cwd: descriptor.app_dir.clone(),
            },
        )?;
        let app = load_application(&lua, &descriptor, &script, args, width, height)?;
        call_table_function_if_exists(&app, "resize", (width, height))?;
        call_table_function_if_exists(&app, "draw", ())?;
        let shortcuts = lua_shortcut_items(&app)?;
        Ok(Self {
            _lua: lua,
            app,
            graphics,
            quit,
            keys,
            name: descriptor
                .manifest
                .application
                .name
                .unwrap_or(descriptor.manifest.application.id),
            fps: descriptor.manifest.runtime.fps.unwrap_or(30).clamp(10, 120),
            shortcuts,
            window_width: descriptor
                .manifest
                .runtime
                .width
                .unwrap_or(40)
                .saturating_sub(2)
                .clamp(1, 240),
            window_height: descriptor
                .manifest
                .runtime
                .height
                .unwrap_or(20)
                .saturating_sub(2)
                .clamp(1, 120),
        })
    }
    fn snapshot(&self) -> Result<Json> {
        let g = self.graphics.borrow();
        let shortcuts = self
            .shortcuts
            .iter()
            .map(|item| json!({"key":item.key,"label":item.label}))
            .collect::<Vec<_>>();
        Ok(
            json!({"width":g.width,"height":g.height,"name":self.name,"fps":self.fps,
            "running":!self.quit.get(),"window_width":self.window_width,"window_height":self.window_height, "cells":g.cells.iter().map(|&c|c as u32).collect::<Vec<_>>(),
            "fg":g.styles.iter().map(|s|s.fg).collect::<Vec<_>>(),
            "bg":g.styles.iter().map(|s|s.bg).collect::<Vec<_>>(),"shortcuts":shortcuts}),
        )
    }
}
fn validate_size(width: u16, height: u16) -> Result<()> {
    anyhow::ensure!(
        (1..=240).contains(&width) && (1..=120).contains(&height),
        "Canvas size must be within 1..240 columns and 1..120 rows"
    );
    Ok(())
}
#[derive(Deserialize)]
#[serde(tag = "op", rename_all = "snake_case")]
enum Request {
    Start {
        path: String,
        #[serde(default)]
        expected_id: Option<String>,
        #[serde(default)]
        args: Vec<String>,
        width: u16,
        height: u16,
    },
    Tick {
        dt: f64,
    },
    Key {
        name: String,
        kind: u8,
    },
    Resize {
        width: u16,
        height: u16,
    },
    Focus {
        active: bool,
    },
    Mouse {
        event: String,
        button: Option<String>,
        x: i64,
        y: i64,
        dx: Option<i64>,
        dy: Option<i64>,
    },
    Stop,
}
thread_local! {
    static APP: RefCell<Option<BrowserApp>> = const { RefCell::new(None) };
    static RESPONSE: RefCell<CString> = RefCell::new(CString::new("{}").expect("static JSON"));
}
fn dispatch(request: Request) -> Result<Json> {
    APP.with(|state| {
        let mut state = state.borrow_mut();
        match request {
            Request::Start {
                path,
                expected_id,
                args,
                width,
                height,
            } => {
                // Release the prior VM/audio before loading a new application.
                state.take();
                crate::audio::stop_module();
                *state = Some(BrowserApp::start(
                    &path,
                    &args,
                    width,
                    height,
                    expected_id.as_deref(),
                )?);
            }
            Request::Stop => {
                state.take();
                crate::audio::stop_module();
                return Ok(json!({"running":false}));
            }
            request => {
                let host = state.as_mut().context("No Kapsule is running")?;
                match request {
                    Request::Tick { dt } => {
                        anyhow::ensure!(
                            dt.is_finite() && (0.0..=1.0).contains(&dt),
                            "Invalid frame delta"
                        );
                        if !host.quit.get() {
                            call_table_function_if_exists(&host.app, "update", dt)?;
                            call_table_function_if_exists(&host.app, "draw", ())?;
                            host.shortcuts = lua_shortcut_items(&host.app)?;
                        }
                    }
                    Request::Key { name, kind } => match kind {
                        0 => {
                            let is_new = !host.keys.borrow().contains_key(&name);
                            host.keys.borrow_mut().insert(name.clone(), Instant::now());
                            if is_new {
                                call_table_function_if_exists(&host.app, "keydown", name.clone())?;
                            }
                            call_table_function_if_exists(&host.app, "keypressed", name)?;
                        }
                        1 => {
                            host.keys.borrow_mut().insert(name.clone(), Instant::now());
                            call_table_function_if_exists(&host.app, "keypressed", name)?;
                        }
                        2 => {
                            host.keys.borrow_mut().remove(&name);
                            call_table_function_if_exists(&host.app, "keyup", name)?;
                        }
                        _ => bail!("Unknown keyboard event type"),
                    },
                    Request::Resize { width, height } => {
                        validate_size(width, height)?;
                        host.graphics.borrow_mut().resize(width, height);
                        call_table_function_if_exists(&host.app, "resize", (width, height))?;
                        call_table_function_if_exists(&host.app, "draw", ())?;
                        host.shortcuts = lua_shortcut_items(&host.app)?;
                    }
                    Request::Focus { active } => {
                        if !active {
                            host.keys.borrow_mut().clear();
                        }
                        call_table_function_if_exists(
                            &host.app,
                            if active { "focusgained" } else { "focuslost" },
                            (),
                        )?;
                    }
                    Request::Mouse {
                        event,
                        button,
                        x,
                        y,
                        dx,
                        dy,
                    } => match event.as_str() {
                        "mousepressed" | "mousereleased" | "mousedragged" => {
                            call_table_function_if_exists(
                                &host.app,
                                &event,
                                (button.unwrap_or_else(|| "left".into()), x, y),
                            )?
                        }
                        "mousemoved" => call_table_function_if_exists(&host.app, &event, (x, y))?,
                        "mousewheel" => call_table_function_if_exists(
                            &host.app,
                            &event,
                            (dx.unwrap_or(0), dy.unwrap_or(0), x, y),
                        )?,
                        _ => bail!("Unknown mouse event"),
                    },
                    _ => unreachable!(),
                }
            }
        }
        state.as_ref().context("No Kapsule is running")?.snapshot()
    })
}
/// One synchronous call per browser event/frame; returned string is valid until the next call.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn kapsule_call(request: *const c_char) -> *const c_char {
    let result = (|| -> Result<Json> {
        anyhow::ensure!(!request.is_null(), "Missing request");
        let text = unsafe { CStr::from_ptr(request) }.to_str()?;
        dispatch(serde_json::from_str(text)?)
    })();
    let response = match result {
        Ok(value) => value,
        Err(error) => json!({"error":format!("{error:#}")}),
    };
    RESPONSE.with(|slot| {
        *slot.borrow_mut() = CString::new(response.to_string()).expect("JSON contains no raw NUL");
        slot.borrow().as_ptr()
    })
}
pub fn initialize() { /* JavaScript starts the application after its files have loaded. */
}
