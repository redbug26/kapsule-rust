// Adapted from kkc-rust/src/lua_apps.rs; this standalone runtime is now the Kapsule reference.
use crate::ui::{ShortcutBarItem, render_shortcut_bar};
use anyhow::{Context, Result, anyhow, bail};
use crossterm::{
    event::{
        self, Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers, MouseButton, MouseEvent,
        MouseEventKind,
    },
    terminal::{self},
};
use mlua::{Lua, String as LuaString, Table, Value};
use ratatui::{
    Terminal,
    backend::CrosstermBackend,
    layout::Rect,
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, BorderType, Borders, Clear as RatatuiClear, Paragraph},
};
use serde::Deserialize;
use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::rc::Rc;
use std::time::{Duration, Instant};

#[derive(Debug, Clone, Deserialize)]
struct LuaAppManifest {
    application: LuaAppManifestMeta,
    #[serde(default)]
    runtime: LuaAppRuntime,
}

#[derive(Debug, Clone, Deserialize)]
struct LuaAppManifestMeta {
    id: String,
    name: Option<String>,
    version: Option<String>,
    description: Option<String>,
}

#[derive(Debug, Clone, Default, Deserialize)]
struct LuaAppRuntime {
    main: Option<String>,
    fps: Option<u32>,
    width: Option<u16>,
    height: Option<u16>,
}

#[derive(Debug, Clone)]
struct LuaAppDescriptor {
    app_dir: PathBuf,
    manifest: LuaAppManifest,
}

#[derive(Debug, Clone, Copy)]
struct CellStyle {
    fg: u32, // RGB color as 0xRRGGBB, 0xFFFFFF = white
    bg: u32, // RGB color as 0xRRGGBB, 0x000000 = black
}

impl CellStyle {
    fn default() -> Self {
        Self {
            fg: 0xFFFFFF, // white
            bg: 0x000000, // black
        }
    }
}

#[derive(Debug, Clone)]
struct GraphicsBuffer {
    width: u16,
    height: u16,
    cells: Vec<char>,
    styles: Vec<CellStyle>,
    current_style: CellStyle,
}

impl GraphicsBuffer {
    fn new(width: u16, height: u16) -> Self {
        let w = width.max(1);
        let h = height.max(1);
        let size = w as usize * h as usize;
        Self {
            width: w,
            height: h,
            cells: vec![' '; size],
            styles: vec![CellStyle::default(); size],
            current_style: CellStyle::default(),
        }
    }

    fn resize(&mut self, width: u16, height: u16) {
        let w = width.max(1);
        let h = height.max(1);
        let size = w as usize * h as usize;
        self.width = w;
        self.height = h;
        self.cells = vec![' '; size];
        self.styles = vec![CellStyle::default(); size];
    }

    fn clear(&mut self, ch: char) {
        self.cells.fill(ch);
        self.styles.fill(self.current_style);
    }

    fn put(&mut self, x: i64, y: i64, ch: char) {
        if x < 1 || y < 1 {
            return;
        }
        if x > i64::from(self.width) || y > i64::from(self.height) {
            return;
        }
        let xu = (x - 1) as u16;
        let yu = (y - 1) as u16;
        if xu >= self.width || yu >= self.height {
            return;
        }
        let idx = yu as usize * self.width as usize + xu as usize;
        self.cells[idx] = ch;
        self.styles[idx] = self.current_style;
    }

    fn set_color(&mut self, fg: u32, bg: u32) {
        self.current_style = CellStyle { fg, bg };
    }

    fn reset_color(&mut self) {
        self.current_style = CellStyle::default();
    }

    fn text(&mut self, x: i64, y: i64, text: &str) {
        if y < 1 {
            return;
        }
        let mut col = x;
        for ch in text.chars() {
            self.put(col, y, ch);
            col = col.saturating_add(1);
        }
    }

    fn box_rect(&mut self, x: i64, y: i64, w: i64, h: i64, ch: char) {
        if w <= 0 || h <= 0 {
            return;
        }
        for row in y.max(1)..=y.saturating_add(h - 1).min(i64::from(self.height)) {
            for col in x.max(1)..=x.saturating_add(w - 1).min(i64::from(self.width)) {
                self.put(col, row, ch);
            }
        }
    }

    fn render_lines(&self) -> Vec<(String, Vec<CellStyle>)> {
        let mut out = Vec::with_capacity(self.height as usize);
        for y in 0..self.height {
            let start = y as usize * self.width as usize;
            let end = start + self.width as usize;
            let line_str: String = self.cells[start..end].iter().collect();
            let line_styles: Vec<CellStyle> = self.styles[start..end].to_vec();
            out.push((line_str, line_styles));
        }
        out
    }
}

fn load_manifest(path: &Path) -> Result<LuaAppManifest> {
    let text = fs::read_to_string(path).with_context(|| format!("Reading {}", path.display()))?;
    let manifest: LuaAppManifest =
        toml::from_str(&text).with_context(|| format!("Parsing {}", path.display()))?;
    Ok(manifest)
}

fn run_lua_app(
    descriptor: &LuaAppDescriptor,
    script_path: &Path,
    args: &[String],
    cwd: Option<&Path>,
    options: &RunOptions,
) -> Result<()> {
    let _dialog_mode = crate::lua_dialog::ModeGuard::new(options.headless_frames.is_some());
    let session = if options.headless_frames.is_none() {
        Some(crate::terminal::Session::enter()?)
    } else {
        None
    };
    let has_release_events = session.as_ref().is_some_and(|s| s.enhanced);
    let lua = Lua::new();
    let (width, height) = if options.headless_frames.is_some() {
        (80, 24)
    } else {
        terminal::size()?
    };
    let graphics = Rc::new(RefCell::new(GraphicsBuffer::new(width, height)));
    let should_quit = Rc::new(Cell::new(false));
    let key_state: Rc<RefCell<HashMap<String, Instant>>> = Rc::new(RefCell::new(HashMap::new()));
    let start_time = Instant::now();
    let launch_cwd = cwd
        .map(Path::to_path_buf)
        .unwrap_or_else(|| std::env::current_dir().unwrap_or_else(|_| descriptor.app_dir.clone()));

    install_lua_app_modules(
        &lua,
        &descriptor.manifest,
        &descriptor.app_dir,
        args,
        ModuleContext {
            graphics: Rc::clone(&graphics),
            should_quit: Rc::clone(&should_quit),
            key_state: Rc::clone(&key_state),
            has_release_events,
            start_time,
            launch_cwd,
        },
    )?;

    if session.as_ref().is_some_and(|s| !s.enhanced) {
        lua.load(
            r#"require('kkc-dialog').message(
            "Ce terminal ne transmet pas le relâchement des touches (key up).\n" ..
            "Les déplacements dans les jeux peuvent être moins fluides.\n" ..
            "Pour une meilleure expérience, utilisez un terminal qui prend en charge key up.")"#,
        )
        .exec()?;
        crossterm::execute!(io::stdout(), terminal::Clear(terminal::ClearType::All))?;
    }

    let script = fs::read_to_string(script_path)
        .with_context(|| format!("Reading {}", script_path.display()))?;
    let value: Value = lua
        .load(&script)
        .set_name(script_path.to_string_lossy().as_ref())
        .eval()
        .with_context(|| format!("Running {}", script_path.display()))?;

    let app_table = match value {
        Value::Table(t) => t,
        _ => lua
            .globals()
            .get::<Table>("app")
            .map_err(|_| anyhow!("Lua app must return a table with callbacks"))?,
    };

    let init_ctx = lua.create_table()?;
    init_ctx.set("width", width)?;
    init_ctx.set("height", height)?;
    init_ctx.set("id", descriptor.manifest.application.id.clone())?;
    init_ctx.set(
        "name",
        descriptor
            .manifest
            .application
            .name
            .clone()
            .unwrap_or_else(|| descriptor.manifest.application.id.clone()),
    )?;
    init_ctx.set(
        "version",
        descriptor
            .manifest
            .application
            .version
            .clone()
            .unwrap_or_else(|| "0.1.0".to_string()),
    )?;
    init_ctx.set(
        "description",
        descriptor
            .manifest
            .application
            .description
            .clone()
            .unwrap_or_default(),
    )?;
    let lua_args = lua.create_table()?;
    for (idx, arg) in args.iter().enumerate() {
        lua_args.set(idx + 1, arg.clone())?;
    }
    init_ctx.set("args", lua_args)?;

    call_table_function_if_exists(&app_table, "init", init_ctx)?;

    let fps = descriptor.manifest.runtime.fps.unwrap_or(30).clamp(10, 120);
    let frame_time = Duration::from_secs_f64(1.0 / fps as f64);

    if let Some(frames) = options.headless_frames {
        let inner = lua_app_inner_area(
            Rect::new(0, 0, width, height),
            !options.windowed,
            descriptor.manifest.runtime.width.unwrap_or(40),
            descriptor.manifest.runtime.height.unwrap_or(20),
        );
        graphics.borrow_mut().resize(inner.width, inner.height);
        call_table_function_if_exists(&app_table, "resize", (inner.width, inner.height))?;
        for _ in 0..frames {
            if should_quit.get() {
                break;
            }
            call_table_function_if_exists(&app_table, "update", frame_time.as_secs_f64())?;
            call_table_function_if_exists(&app_table, "draw", ())?;
            lua_shortcut_items(&app_table)?;
        }
        if options.dump_screen {
            for (line, _) in graphics.borrow().render_lines() {
                println!("{line}");
            }
        }
        return Ok(());
    }

    let backend = CrosstermBackend::new(io::stdout());
    let mut terminal = Terminal::new(backend)?;

    let app_title = descriptor
        .manifest
        .application
        .name
        .as_deref()
        .unwrap_or(&descriptor.manifest.application.id)
        .to_string();

    // zoomed: false = centered manifest window, true = full-screen
    let mut zoomed: bool = !options.windowed;

    // App-requested window size (from manifest; defaults if not specified)
    let app_width = descriptor.manifest.runtime.width.unwrap_or(40);
    let app_height = descriptor.manifest.runtime.height.unwrap_or(20);

    let mut dialog_revision = crate::lua_dialog::revision();
    let mut last_frame = Instant::now();
    let mut resized = false;

    // Notify Lua of its initial inner content area
    {
        let sz = terminal.size()?;
        let term_size = Rect::new(0, 0, sz.width, sz.height);
        let inner = lua_app_inner_area(term_size, zoomed, app_width, app_height);
        graphics.borrow_mut().resize(inner.width, inner.height);
        call_table_function_if_exists(
            &app_table,
            "resize",
            (inner.width as i64, inner.height as i64),
        )?;
    }

    while !should_quit.get() {
        let now = Instant::now();
        let elapsed = now.saturating_duration_since(last_frame);
        if elapsed < frame_time {
            let timeout = frame_time - elapsed;
            if event::poll(timeout)? {
                match event::read()? {
                    Event::Key(key) => {
                        // F5 to toggle zoom (full-screen vs floating window)
                        if matches!(key.code, KeyCode::F(5)) && key.kind != KeyEventKind::Release {
                            zoomed = !zoomed;
                            resized = true;
                        } else if is_quit_key(&key) && key.kind != KeyEventKind::Release {
                            key_state.borrow_mut().clear();
                            should_quit.set(true);
                            continue;
                        } else if let Some(name) = key_name(&key) {
                            let now_ts = Instant::now();
                            match key.kind {
                                KeyEventKind::Press => {
                                    let is_new = !key_is_held(
                                        &key_state.borrow(),
                                        &name,
                                        now_ts,
                                        has_release_events,
                                    );
                                    key_state.borrow_mut().insert(name.clone(), now_ts);
                                    if is_new {
                                        call_table_function_if_exists(
                                            &app_table,
                                            "keydown",
                                            name.clone(),
                                        )?;
                                    }
                                    call_table_function_if_exists(&app_table, "keypressed", name)?;
                                }
                                KeyEventKind::Release => {
                                    key_state.borrow_mut().remove(&name);
                                    call_table_function_if_exists(&app_table, "keyup", name)?;
                                }
                                KeyEventKind::Repeat => {
                                    // True repeat (enhanced protocol terminals):
                                    // refresh timestamp to keep key marked as held.
                                    key_state.borrow_mut().insert(name.clone(), now_ts);
                                    call_table_function_if_exists(&app_table, "keypressed", name)?;
                                }
                            }
                        }
                    }
                    Event::Mouse(mouse) => {
                        dispatch_mouse_event(&app_table, mouse)?;
                    }
                    Event::Resize(new_w, new_h) => {
                        let inner = lua_app_inner_area(
                            Rect::new(0, 0, new_w, new_h),
                            zoomed,
                            app_width,
                            app_height,
                        );
                        graphics.borrow_mut().resize(inner.width, inner.height);
                        resized = true;
                    }
                    Event::FocusLost => {
                        // Clear all held keys to avoid "stuck" keys when losing focus.
                        key_state.borrow_mut().clear();
                        call_table_function_if_exists(&app_table, "focuslost", ())?;
                    }
                    Event::FocusGained => {
                        call_table_function_if_exists(&app_table, "focusgained", ())?;
                    }
                    _ => {}
                }
            }
            continue;
        }

        let dt = elapsed.as_secs_f64();
        last_frame = now;

        if resized {
            let sz = terminal.size()?;
            let term_size = Rect::new(0, 0, sz.width, sz.height);
            let inner = lua_app_inner_area(term_size, zoomed, app_width, app_height);
            graphics.borrow_mut().resize(inner.width, inner.height);
            call_table_function_if_exists(
                &app_table,
                "resize",
                (inner.width as i64, inner.height as i64),
            )?;
            resized = false;
        }

        call_table_function_if_exists(&app_table, "update", dt)?;
        call_table_function_if_exists(&app_table, "draw", ())?;

        let lua_lines = graphics.borrow().render_lines();
        let shortcut_items = lua_shortcut_items(&app_table)?;
        let title_str = app_title.clone();

        let revision = crate::lua_dialog::revision();
        if revision != dialog_revision {
            // Resize invalidates the cached frame without querying cursor position.
            let size = terminal.size()?;
            terminal.resize(Rect::new(0, 0, size.width, size.height))?;
            dialog_revision = revision;
        }
        terminal.draw(|f| {
            let term_area = f.area();

            let win_area = lua_app_window_area(term_area, zoomed, app_width, app_height);

            // Window border + title  (viewer style)
            let border_style = Style::default()
                .fg(Color::Rgb(210, 180, 140))
                .add_modifier(Modifier::BOLD);
            let title_line = Line::from(vec![
                Span::styled(
                    format!(" {} ", title_str),
                    Style::default()
                        .fg(Color::Rgb(255, 240, 210))
                        .bg(Color::Rgb(70, 50, 35))
                        .add_modifier(Modifier::BOLD),
                ),
                Span::styled(
                    "  F5 zoom  Ctrl-C quit ",
                    Style::default()
                        .fg(Color::Rgb(160, 140, 110))
                        .bg(Color::Rgb(70, 50, 35)),
                ),
            ]);
            let block = Block::default()
                .title(title_line)
                .borders(Borders::ALL)
                .border_type(BorderType::Thick)
                .border_style(border_style)
                .style(Style::default().bg(Color::Black));

            let inner = block.inner(win_area);
            f.render_widget(RatatuiClear, win_area);
            f.render_widget(block, win_area);

            // Render Lua buffer lines into the inner area
            for (row_idx, (line_str, line_styles)) in lua_lines.iter().enumerate() {
                let y = inner.y + row_idx as u16;
                if y >= inner.y + inner.height {
                    break;
                }
                // Clip to inner width
                let chars: Vec<char> = line_str.chars().collect();
                let visible_w = inner.width as usize;
                let mut spans = Vec::new();
                for (col, ch) in chars.iter().take(visible_w).enumerate() {
                    let style = if col < line_styles.len() {
                        let s = line_styles[col];
                        let fg = Color::Rgb(
                            ((s.fg >> 16) & 0xFF) as u8,
                            ((s.fg >> 8) & 0xFF) as u8,
                            (s.fg & 0xFF) as u8,
                        );
                        let bg = Color::Rgb(
                            ((s.bg >> 16) & 0xFF) as u8,
                            ((s.bg >> 8) & 0xFF) as u8,
                            (s.bg & 0xFF) as u8,
                        );
                        Style::default().fg(fg).bg(bg)
                    } else {
                        Style::default().fg(Color::White).bg(Color::Black)
                    };
                    spans.push(Span::styled(ch.to_string(), style));
                }
                f.render_widget(
                    Paragraph::new(Line::from(spans)),
                    Rect {
                        x: inner.x,
                        y,
                        width: inner.width,
                        height: 1,
                    },
                );
            }

            // Generic Lua footer shortcuts rendered by the standalone host.
            if !shortcut_items.is_empty() && inner.height > 0 {
                let footer_area = Rect {
                    x: inner.x,
                    y: inner.y + inner.height.saturating_sub(1),
                    width: inner.width,
                    height: 1,
                };
                render_shortcut_bar(f, footer_area, &shortcut_items);
            }
        })?;
    }

    key_state.borrow_mut().clear();
    Ok(())
}

/// Compute the window area for a Lua app (floating when !zoomed, full-screen when zoomed).
fn lua_app_window_area(term: Rect, zoomed: bool, req_width: u16, req_height: u16) -> Rect {
    if zoomed {
        return term; // Full screen
    }
    // Floating window: use app-requested size, constrained to terminal
    let w = req_width.min(term.width);
    let h = req_height.min(term.height);
    let x = term.x + term.width.saturating_sub(w) / 2;
    let y = term.y + term.height.saturating_sub(h) / 2;
    Rect {
        x,
        y,
        width: w,
        height: h,
    }
}

/// Inner content area (after borders) inside the window.
fn lua_app_inner_area(term: Rect, zoomed: bool, req_width: u16, req_height: u16) -> Rect {
    let win = lua_app_window_area(term, zoomed, req_width, req_height);
    Rect {
        x: win.x + 1,
        y: win.y + 1,
        width: win.width.saturating_sub(2),
        height: win.height.saturating_sub(2),
    }
}

// With real key-up events, the terminal owns the held state. Only legacy
// input needs the historical timestamp approximation.
fn key_is_held(
    keys: &HashMap<String, Instant>,
    name: &str,
    now: Instant,
    has_release_events: bool,
) -> bool {
    keys.get(name).is_some_and(|last| {
        has_release_events || now.saturating_duration_since(*last) < Duration::from_millis(200)
    })
}

struct ModuleContext {
    graphics: Rc<RefCell<GraphicsBuffer>>,
    should_quit: Rc<Cell<bool>>,
    key_state: Rc<RefCell<HashMap<String, Instant>>>,
    has_release_events: bool,
    start_time: Instant,
    launch_cwd: PathBuf,
}

fn install_lua_app_modules(
    lua: &Lua,
    manifest: &LuaAppManifest,
    app_dir: &Path,
    args: &[String],
    context: ModuleContext,
) -> Result<()> {
    let ModuleContext {
        graphics,
        should_quit,
        key_state,
        has_release_events,
        start_time,
        launch_cwd,
    } = context;
    let globals = lua.globals();
    let package: Table = globals.get("package")?;
    let current_path: String = package.get("path")?;
    let app_path = format!(
        "{}/?.lua;{}/?/init.lua;{}",
        app_dir.display(),
        app_dir.display(),
        current_path
    );
    package.set("path", app_path)?;

    let preload: Table = package.get("preload")?;
    let app_root = app_dir.to_path_buf();

    let quit_cell = Rc::clone(&should_quit);
    let app_id = manifest.application.id.clone();
    let app_name = manifest
        .application
        .name
        .clone()
        .unwrap_or_else(|| app_id.clone());
    let app_version = manifest
        .application
        .version
        .clone()
        .unwrap_or_else(|| "0.1.0".to_string());
    let app_args = args.to_vec();
    let launch_cwd_text = launch_cwd.to_string_lossy().into_owned();

    let kkc_mod = lua.create_function(move |lua, ()| {
        let t = lua.create_table()?;
        let quit_cell = Rc::clone(&quit_cell);
        t.set(
            "quit",
            lua.create_function(move |_, ()| {
                quit_cell.set(true);
                Ok(())
            })?,
        )?;

        let start_time = start_time;
        t.set(
            "time",
            lua.create_function(move |_, ()| Ok(start_time.elapsed().as_secs_f64()))?,
        )?;

        let args_for_fn = app_args.clone();
        t.set(
            "args",
            lua.create_function(move |lua, ()| {
                let tbl = lua.create_table()?;
                for (idx, arg) in args_for_fn.iter().enumerate() {
                    tbl.set(idx + 1, arg.clone())?;
                }
                Ok(tbl)
            })?,
        )?;

        t.set("id", app_id.clone())?;
        t.set("name", app_name.clone())?;
        t.set("version", app_version.clone())?;
        t.set("cwd", launch_cwd_text.clone())?;
        let cwd_fn_value = launch_cwd_text.clone();
        t.set(
            "get_cwd",
            lua.create_function(move |_, ()| Ok(cwd_fn_value.clone()))?,
        )?;
        Ok(t)
    })?;
    preload.set("kkc", kkc_mod)?;

    let gfx = Rc::clone(&graphics);
    let graphics_mod = lua.create_function(move |lua, ()| {
        let t = lua.create_table()?;

        let g_size = Rc::clone(&gfx);
        t.set(
            "size",
            lua.create_function(move |_, ()| {
                let g = g_size.borrow();
                Ok((g.width, g.height))
            })?,
        )?;

        let g_clear = Rc::clone(&gfx);
        t.set(
            "clear",
            lua.create_function(move |_, ch: Option<String>| {
                let c = ch.as_deref().and_then(|s| s.chars().next()).unwrap_or(' ');
                g_clear.borrow_mut().clear(c);
                Ok(())
            })?,
        )?;

        let g_put = Rc::clone(&gfx);
        t.set(
            "put",
            lua.create_function(move |_, (x, y, ch): (i64, i64, LuaString)| {
                let bytes = ch.as_bytes();
                let text = String::from_utf8_lossy(&bytes);
                let c = text.chars().next().unwrap_or(' ');
                g_put.borrow_mut().put(x, y, c);
                Ok(())
            })?,
        )?;

        let g_text = Rc::clone(&gfx);
        t.set(
            "text",
            lua.create_function(move |_, (x, y, text): (i64, i64, LuaString)| {
                let bytes = text.as_bytes();
                let rendered = String::from_utf8_lossy(&bytes).into_owned();
                g_text.borrow_mut().text(x, y, &rendered);
                Ok(())
            })?,
        )?;

        // print(row, col, text) — row-first convenience alias for text()
        let g_print = Rc::clone(&gfx);
        t.set(
            "print",
            lua.create_function(move |_, (row, col, text): (i64, i64, LuaString)| {
                let bytes = text.as_bytes();
                let rendered = String::from_utf8_lossy(&bytes).into_owned();
                g_print.borrow_mut().text(col, row, &rendered);
                Ok(())
            })?,
        )?;

        let g_box = Rc::clone(&gfx);
        t.set(
            "box",
            lua.create_function(
                move |_, (x, y, w, h, ch): (i64, i64, i64, i64, Option<String>)| {
                    let c = ch.as_deref().and_then(|s| s.chars().next()).unwrap_or(' ');
                    g_box.borrow_mut().box_rect(x, y, w, h, c);
                    Ok(())
                },
            )?,
        )?;

        // color(fg, bg) - set foreground and background colors in hex (0xRRGGBB)
        let g_color = Rc::clone(&gfx);
        t.set(
            "color",
            lua.create_function(move |_, (fg, bg): (i64, i64)| {
                g_color.borrow_mut().set_color(fg as u32, bg as u32);
                Ok(())
            })?,
        )?;

        // set_fg(color) - set foreground color, keep background
        let g_set_fg = Rc::clone(&gfx);
        t.set(
            "set_fg",
            lua.create_function(move |_, fg: i64| {
                let mut g = g_set_fg.borrow_mut();
                let bg = g.current_style.bg;
                g.set_color(fg as u32, bg);
                Ok(())
            })?,
        )?;

        // set_bg(color) - set background color, keep foreground
        let g_set_bg = Rc::clone(&gfx);
        t.set(
            "set_bg",
            lua.create_function(move |_, bg: i64| {
                let mut g = g_set_bg.borrow_mut();
                let fg = g.current_style.fg;
                g.set_color(fg, bg as u32);
                Ok(())
            })?,
        )?;

        // reset() - reset to default colors (white text on black background)
        let g_reset = Rc::clone(&gfx);
        t.set(
            "reset",
            lua.create_function(move |_, ()| {
                g_reset.borrow_mut().reset_color();
                Ok(())
            })?,
        )?;

        Ok(t)
    })?;
    preload.set("kkc-graphics", graphics_mod)?;

    let key_state_for_mod = Rc::clone(&key_state);
    let key_mod = lua.create_function(move |lua, ()| {
        let t = lua.create_table()?;
        t.set("LEFT", "left")?;
        t.set("RIGHT", "right")?;
        t.set("UP", "up")?;
        t.set("DOWN", "down")?;
        t.set("SPACE", "space")?;
        t.set("ENTER", "enter")?;
        t.set("ESC", "esc")?;
        t.set("HAS_RELEASE_EVENTS", has_release_events)?;
        // is_down(name) — returns true if the named key is currently held.
        // Works with the same key names as keydown / keyup callbacks.
        let ks = Rc::clone(&key_state_for_mod);
        t.set(
            "is_down",
            lua.create_function(move |_, name: String| {
                let held = key_is_held(&ks.borrow(), &name, Instant::now(), has_release_events);
                Ok(held)
            })?,
        )?;
        Ok(t)
    })?;
    preload.set("kkc-key", key_mod)?;

    let mouse_mod = lua.create_function(move |lua, ()| {
        let t = lua.create_table()?;
        t.set("LEFT", "left")?;
        t.set("RIGHT", "right")?;
        t.set("MIDDLE", "middle")?;
        t.set("UP", "up")?;
        t.set("DOWN", "down")?;
        t.set("DRAG", "drag")?;
        t.set("MOVE", "move")?;
        t.set("SCROLL_UP", "scroll_up")?;
        t.set("SCROLL_DOWN", "scroll_down")?;
        t.set("SCROLL_LEFT", "scroll_left")?;
        t.set("SCROLL_RIGHT", "scroll_right")?;
        Ok(t)
    })?;
    preload.set("kkc-mouse", mouse_mod)?;

    // Lightweight pseudo-random helper module for terminal games.
    let rand_state = Rc::new(RefCell::new({
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos() as u64;
        nanos ^ 0x9E37_79B9_7F4A_7C15
    }));

    let rand_mod = lua.create_function(move |lua, ()| {
        let t = lua.create_table()?;

        let state_int = Rc::clone(&rand_state);
        t.set(
            "int",
            lua.create_function(move |_, (min, max): (i64, i64)| {
                let low = min.min(max);
                let high = min.max(max);
                let span = (i128::from(high) - i128::from(low) + 1) as u128;
                let mut s = state_int.borrow_mut();
                *s = s.wrapping_mul(6364136223846793005).wrapping_add(1);
                let val = (i128::from(low) + (u128::from(*s) % span) as i128) as i64;
                Ok(val)
            })?,
        )?;

        let state_float = Rc::clone(&rand_state);
        t.set(
            "float",
            lua.create_function(move |_, ()| {
                let mut s = state_float.borrow_mut();
                *s = s.wrapping_mul(6364136223846793005).wrapping_add(1);
                let top = (*s >> 11) as f64;
                Ok(top / ((1u64 << 53) as f64))
            })?,
        )?;

        let state_seed = Rc::clone(&rand_state);
        t.set(
            "seed",
            lua.create_function(move |_, seed: i64| {
                let mut s = state_seed.borrow_mut();
                *s = seed as u64;
                Ok(())
            })?,
        )?;

        Ok(t)
    })?;
    preload.set("kkc-rand", rand_mod)?;

    // FS module anchored to the app directory for relative paths (not a sandbox).
    let fs_mod = lua.create_function(move |lua, ()| {
        let t = lua.create_table()?;

        let app_root_resolve = app_root.clone();
        let resolve = move |path: &str| -> PathBuf {
            let candidate = PathBuf::from(path);
            if candidate.is_absolute() {
                candidate
            } else {
                app_root_resolve.join(candidate)
            }
        };

        let app_root_exists = app_root.clone();
        let resolve_exists = resolve.clone();
        t.set(
            "exists",
            lua.create_function(move |_, path: String| Ok(resolve_exists(&path).exists()))?,
        )?;

        let resolve_is_dir = resolve.clone();
        t.set(
            "is_dir",
            lua.create_function(move |_, path: String| Ok(resolve_is_dir(&path).is_dir()))?,
        )?;

        let resolve_read = resolve.clone();
        t.set(
            "read_text",
            lua.create_function(move |_, path: String| {
                let p = resolve_read(&path);
                let text = fs::read_to_string(&p).map_err(|e| {
                    mlua::Error::external(anyhow!("Reading {}: {}", p.display(), e))
                })?;
                Ok(text)
            })?,
        )?;

        let resolve_write = resolve.clone();
        t.set(
            "write_text",
            lua.create_function(move |_, (path, text): (String, String)| {
                let p = resolve_write(&path);
                if let Some(parent) = p.parent() {
                    fs::create_dir_all(parent).map_err(|e| {
                        mlua::Error::external(anyhow!("Creating {}: {}", parent.display(), e))
                    })?;
                }
                fs::write(&p, text).map_err(|e| {
                    mlua::Error::external(anyhow!("Writing {}: {}", p.display(), e))
                })?;
                Ok(())
            })?,
        )?;

        let resolve_list = resolve.clone();
        t.set(
            "list_dir",
            lua.create_function(move |lua, path: Option<String>| {
                let p = path
                    .as_deref()
                    .map(resolve_list.clone())
                    .unwrap_or_else(|| app_root_exists.clone());
                let entries = fs::read_dir(&p).map_err(|e| {
                    mlua::Error::external(anyhow!("Reading {}: {}", p.display(), e))
                })?;
                let out = lua.create_table()?;
                for (idx, entry) in entries.flatten().enumerate() {
                    if let Some(name) = entry.file_name().to_str() {
                        out.set(idx + 1, name.to_string())?;
                    }
                }
                Ok(out)
            })?,
        )?;

        let resolve_mkdir = resolve.clone();
        t.set(
            "mkdir_all",
            lua.create_function(move |_, path: String| {
                let p = resolve_mkdir(&path);
                fs::create_dir_all(&p).map_err(|e| {
                    mlua::Error::external(anyhow!("Creating {}: {}", p.display(), e))
                })?;
                Ok(())
            })?,
        )?;

        t.set(
            "join",
            lua.create_function(move |_, (a, b): (String, String)| {
                Ok(Path::new(&a).join(&b).to_string_lossy().into_owned())
            })?,
        )?;

        Ok(t)
    })?;
    preload.set("kkc-fs", fs_mod)?;

    // Shell facade for terminal apps. Useful for Git-oriented tools.
    let shell_default_cwd = launch_cwd.clone();
    let shell_mod =
        lua.create_function(move |lua, ()| {
            let t = lua.create_table()?;

            let run_default_cwd = shell_default_cwd.clone();
            t.set(
            "run",
            lua.create_function(
                move |lua, (program, args_tbl, cwd): (String, Option<Table>, Option<String>)| {
                    let mut args = Vec::new();
                    if let Some(tbl) = args_tbl {
                        for value in tbl.sequence_values::<String>() {
                            args.push(value?);
                        }
                    }

                    let mut cmd = Command::new(&program);
                    cmd.args(&args);
                    cmd.stdin(Stdio::null());
                    cmd.env("GIT_TERMINAL_PROMPT", "0");
                    match cwd {
                        Some(path) if !path.trim().is_empty() => {
                            cmd.current_dir(path);
                        }
                        _ => {
                            cmd.current_dir(&run_default_cwd);
                        }
                    }

                    let output = cmd.output().map_err(|e| {
                        mlua::Error::external(anyhow!("Running '{}': {}", program, e))
                    })?;

                    let result = lua.create_table()?;
                    result.set("ok", output.status.success())?;
                    result.set("code", output.status.code().unwrap_or(-1))?;
                    result.set("stdout", String::from_utf8_lossy(&output.stdout).into_owned())?;
                    result.set("stderr", String::from_utf8_lossy(&output.stderr).into_owned())?;
                    Ok(result)
                },
            )?,
        )?;

            let cwd_value = shell_default_cwd.to_string_lossy().into_owned();
            t.set(
                "cwd",
                lua.create_function(move |_, ()| Ok(cwd_value.clone()))?,
            )?;

            Ok(t)
        })?;
    preload.set("kkc-shell", shell_mod)?;
    crate::lua_dialog::install_lua_dialog_module(lua, &preload)?;

    // Audio facade for terminal apps.
    let audio_mod = lua.create_function(move |lua, ()| {
        let t = lua.create_table()?;
        t.set(
            "beep",
            lua.create_function(move |_, ()| {
                print!("\x07");
                io::stdout()
                    .flush()
                    .map_err(|e| mlua::Error::external(anyhow!("Flushing stdout: {}", e)))?;
                Ok(())
            })?,
        )?;
        t.set(
            "play_audio",
            lua.create_function(move |lua, path: String| {
                let info = crate::audio::play_audio_file(Path::new(&path))
                    .map_err(mlua::Error::external)?;
                let tbl = lua.create_table()?;
                tbl.set("name", info.name)?;
                tbl.set("format", info.format)?;
                tbl.set("songs", info.songs)?;
                Ok(tbl)
            })?,
        )?;
        t.set(
            "stop_audio",
            lua.create_function(move |_, ()| {
                crate::audio::stop_module();
                Ok(())
            })?,
        )?;
        t.set(
            "is_audio_playing",
            lua.create_function(move |_, ()| Ok(crate::audio::is_module_playing()))?,
        )?;
        Ok(t)
    })?;
    preload.set("kkc-audio", audio_mod)?;

    Ok(())
}

fn call_table_function_if_exists<A>(table: &Table, name: &str, args: A) -> Result<()>
where
    A: mlua::IntoLuaMulti,
{
    let value: Value = table.get(name)?;
    if let Value::Function(func) = value {
        func.call::<()>(args)?;
    }
    Ok(())
}

fn lua_shortcut_items(app_table: &Table) -> Result<Vec<ShortcutBarItem>> {
    let value: Value = app_table.get("shortcuts")?;
    let Value::Function(func) = value else {
        return Ok(Vec::new());
    };

    let returned: Value = func.call(())?;
    let Value::Table(tbl) = returned else {
        return Ok(Vec::new());
    };

    let mut out = Vec::new();
    for value in tbl.sequence_values::<String>() {
        let raw = value?;
        let trimmed = raw.trim();
        if trimmed.is_empty() {
            continue;
        }
        if let Some((key, label)) = trimmed.split_once(':') {
            out.push(ShortcutBarItem {
                key: key.trim().to_string(),
                label: label.trim().to_string(),
            });
        } else {
            out.push(ShortcutBarItem {
                key: trimmed.to_string(),
                label: String::new(),
            });
        }
    }
    Ok(out)
}

fn is_quit_key(key: &KeyEvent) -> bool {
    key.code == KeyCode::Char('c') && key.modifiers.contains(KeyModifiers::CONTROL)
}

fn key_name(key: &KeyEvent) -> Option<String> {
    match key.code {
        KeyCode::Up => Some("up".to_string()),
        KeyCode::Down => Some("down".to_string()),
        KeyCode::Left => Some("left".to_string()),
        KeyCode::Right => Some("right".to_string()),
        KeyCode::Enter => Some("enter".to_string()),
        KeyCode::Esc => Some("esc".to_string()),
        KeyCode::Backspace => Some("backspace".to_string()),
        KeyCode::Delete => Some("delete".to_string()),
        KeyCode::Tab => Some("tab".to_string()),
        KeyCode::PageUp => Some("pageup".to_string()),
        KeyCode::PageDown => Some("pagedown".to_string()),
        KeyCode::Home => Some("home".to_string()),
        KeyCode::End => Some("end".to_string()),
        KeyCode::Insert => Some("insert".to_string()),
        KeyCode::F(n) => Some(format!("f{}", n)),
        KeyCode::Char(' ') => Some("space".to_string()),
        KeyCode::Char(c) => Some(format!("char:{}", c)),
        _ => None,
    }
}

fn dispatch_mouse_event(app_table: &Table, mouse: MouseEvent) -> Result<()> {
    let x = mouse.column as i64 + 1;
    let y = mouse.row as i64 + 1;
    match mouse.kind {
        MouseEventKind::Down(button) => {
            let name = mouse_button_name(button);
            call_table_function_if_exists(app_table, "mousepressed", (name, x, y))?;
        }
        MouseEventKind::Up(button) => {
            let name = mouse_button_name(button);
            call_table_function_if_exists(app_table, "mousereleased", (name, x, y))?;
        }
        MouseEventKind::Drag(button) => {
            let name = mouse_button_name(button);
            call_table_function_if_exists(app_table, "mousedragged", (name, x, y))?;
        }
        MouseEventKind::Moved => {
            call_table_function_if_exists(app_table, "mousemoved", (x, y))?;
        }
        MouseEventKind::ScrollUp => {
            call_table_function_if_exists(app_table, "mousewheel", (0i64, 1i64, x, y))?;
        }
        MouseEventKind::ScrollDown => {
            call_table_function_if_exists(app_table, "mousewheel", (0i64, -1i64, x, y))?;
        }
        MouseEventKind::ScrollLeft => {
            call_table_function_if_exists(app_table, "mousewheel", (-1i64, 0i64, x, y))?;
        }
        MouseEventKind::ScrollRight => {
            call_table_function_if_exists(app_table, "mousewheel", (1i64, 0i64, x, y))?;
        }
    }
    Ok(())
}

fn mouse_button_name(button: MouseButton) -> &'static str {
    match button {
        MouseButton::Left => "left",
        MouseButton::Right => "right",
        MouseButton::Middle => "middle",
    }
}

/// Standalone host settings. Headless mode runs deterministic 80×24 frames.
#[derive(Debug, Default)]
pub struct RunOptions {
    pub windowed: bool,
    pub headless_frames: Option<u32>,
    pub dump_screen: bool,
}

fn descriptor(path: &Path) -> Result<(LuaAppDescriptor, PathBuf)> {
    let manifest_path = if path.is_dir() {
        path.join("apps.toml")
    } else {
        path.to_path_buf()
    };
    let manifest_path = manifest_path
        .canonicalize()
        .with_context(|| format!("Opening {}", manifest_path.display()))?;
    let manifest = load_manifest(&manifest_path)?;
    if manifest.application.id.trim().is_empty() {
        bail!("application.id must not be empty");
    }
    let app_dir = manifest_path
        .parent()
        .context("Manifest has no parent directory")?
        .to_path_buf();
    let main = manifest
        .runtime
        .main
        .as_deref()
        .filter(|s| !s.trim().is_empty())
        .unwrap_or("app.lua");
    let main = Path::new(main);
    if main.is_absolute()
        || main
            .components()
            .any(|c| matches!(c, std::path::Component::ParentDir))
    {
        bail!("runtime.main must be a relative path inside the Kapsule directory");
    }
    let script = app_dir
        .join(main)
        .canonicalize()
        .context("Opening runtime.main")?;
    if !script.starts_with(&app_dir) || !script.is_file() {
        bail!("runtime.main must be a file inside the Kapsule directory");
    }
    Ok((LuaAppDescriptor { app_dir, manifest }, script))
}

/// Validate the descriptor and entry script syntax without executing Lua code.
pub fn check(path: &Path) -> Result<()> {
    let (_, script) = descriptor(path)?;
    Lua::new()
        .load(fs::read_to_string(&script)?)
        .set_name(script.to_string_lossy())
        .into_function()?;
    Ok(())
}

/// Run a Kapsule directory or apps.toml. Arguments are passed unchanged to Lua.
pub fn run(path: &Path, args: &[String], cwd: Option<&Path>, options: &RunOptions) -> Result<()> {
    let (descriptor, script) = descriptor(path)?;
    run_lua_app(&descriptor, &script, args, cwd, options)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn held_key_expiry_applies_only_to_terminals_without_key_up() {
        let now = Instant::now();
        let mut keys = HashMap::from([("right".to_owned(), now)]);
        assert!(key_is_held(
            &keys,
            "right",
            now + Duration::from_millis(199),
            false
        ));
        assert!(!key_is_held(
            &keys,
            "right",
            now + Duration::from_millis(200),
            false
        ));
        assert!(key_is_held(
            &keys,
            "right",
            now + Duration::from_millis(600),
            true
        ));
        keys.remove("right");
        assert!(!key_is_held(
            &keys,
            "right",
            now + Duration::from_millis(601),
            true
        ));
    }
    #[test]
    fn graphics_clips_large_coordinates_and_rectangles() {
        let mut buffer = GraphicsBuffer::new(4, 2);
        buffer.put(65_537, 1, 'X');
        assert_eq!(buffer.cells[0], ' ');
        buffer.box_rect(i64::MIN, 1, i64::MAX, i64::MAX, 'X');
        assert!(buffer.cells.iter().all(|&c| c == ' '));
        buffer.box_rect(1, 1, i64::MAX, i64::MAX, '#');
        assert!(buffer.cells.iter().all(|&c| c == '#'));
        buffer.text(i64::MAX, 1, "outside");
        assert!(buffer.cells.iter().all(|&c| c == '#'));
    }
}
