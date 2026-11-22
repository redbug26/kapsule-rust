//! Blocking terminal dialogs for the kkc-dialog compatibility API.
use anyhow::Result;
use crossterm::event::{self, Event, KeyCode, KeyEventKind, KeyModifiers};
use mlua::{Lua, Table, Value};
use ratatui::{
    Terminal,
    backend::CrosstermBackend,
    layout::Rect,
    style::{Color, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, Paragraph, Wrap},
};
use std::io::{self, IsTerminal};

thread_local! { static REVISION: std::cell::Cell<u64> = const { std::cell::Cell::new(0) }; }
pub fn revision() -> u64 {
    REVISION.with(|value| value.get())
}
thread_local! { static HEADLESS: std::cell::Cell<bool> = const { std::cell::Cell::new(false) }; }
pub struct ModeGuard(bool);
impl ModeGuard {
    pub fn new(headless: bool) -> Self {
        Self(HEADLESS.with(|mode| mode.replace(headless)))
    }
}
impl Drop for ModeGuard {
    fn drop(&mut self) {
        HEADLESS.with(|mode| mode.set(self.0));
    }
}

type Tui = Terminal<CrosstermBackend<io::Stdout>>;
fn terminal() -> Result<Tui> {
    anyhow::ensure!(
        !HEADLESS.with(|mode| mode.get()),
        "Interactive dialogs are unavailable in headless mode"
    );
    anyhow::ensure!(
        io::stdin().is_terminal() && io::stdout().is_terminal(),
        "Interactive dialogs require a terminal"
    );
    Ok(Terminal::new(CrosstermBackend::new(io::stdout()))?)
}
fn key() -> Result<crossterm::event::KeyEvent> {
    loop {
        if let Event::Key(key) = event::read()?
            && key.kind != KeyEventKind::Release
        {
            anyhow::ensure!(
                !(key.code == KeyCode::Char('c') && key.modifiers.contains(KeyModifiers::CONTROL)),
                "Dialog interrupted by Ctrl-C"
            );
            return Ok(key);
        }
    }
}
fn paint(t: &mut Tui, title: &str, lines: Vec<Line<'_>>) -> Result<()> {
    REVISION.with(|value| value.set(value.get().wrapping_add(1)));
    t.draw(|f| {
        let screen = f.area();
        let w = screen.width.min(90);
        let h = screen
            .height
            .min((lines.len() as u16).saturating_add(4).max(8));
        let area = Rect::new(
            screen.x + (screen.width - w) / 2,
            screen.y + (screen.height - h) / 2,
            w,
            h,
        );
        f.render_widget(Clear, area);
        f.render_widget(
            Paragraph::new(lines)
                .wrap(Wrap { trim: false })
                .block(Block::default().title(title).borders(Borders::ALL))
                .style(
                    Style::default()
                        .fg(Color::Rgb(245, 225, 190))
                        .bg(Color::Rgb(65, 48, 35)),
                ),
            area,
        );
    })?;
    Ok(())
}
fn normalize(lua: &Lua, spec: Table) -> mlua::Result<Table> {
    if let Some(callback) = spec.get::<Option<mlua::Function>>("callback")?
        && let Some(buttons) = spec.get::<Option<Table>>("buttons")?
        && let Some(items) = buttons.get::<Option<Table>>("items")?
    {
        for item in items.sequence_values::<Table>() {
            let item = item?;
            let result: Value = callback.call(item.clone())?;
            let id = match result {
                Value::String(s) => s.to_str()?.to_string(),
                Value::Table(t) => t
                    .get::<Option<String>>("id")?
                    .or(t.get::<Option<String>>("callback")?)
                    .unwrap_or_default(),
                _ => String::new(),
            };
            if !id.is_empty() {
                item.set("callback", id)?;
            }
        }
    }
    spec.set("callback", lua.create_table()?)?;
    Ok(spec)
}
struct Checkbox {
    label: String,
    checked: bool,
}
fn select(
    prompt: &str,
    choices: Vec<String>,
    default: usize,
    checks: &mut [Checkbox],
) -> Result<Option<usize>> {
    if choices.is_empty() {
        return Ok(None);
    }
    let mut t = terminal()?;
    let mut selected = default.saturating_sub(1).min(choices.len() - 1);
    let mut focus_check = None;
    let mut filter = String::new();
    loop {
        let visible: Vec<usize> = choices
            .iter()
            .enumerate()
            .filter(|(_, s)| s.to_lowercase().contains(&filter.to_lowercase()))
            .map(|(i, _)| i)
            .collect();
        if !visible.contains(&selected)
            && let Some(&first) = visible.first()
        {
            selected = first;
        }
        let rows = usize::from(t.size()?.height.saturating_sub(8)).max(1);
        let pos = visible.iter().position(|&i| i == selected).unwrap_or(0);
        let start = pos.saturating_sub(rows - 1);
        let mut lines = vec![Line::raw(format!("Recherche : {filter}"))];
        for &i in visible.iter().skip(start).take(rows) {
            lines.push(Line::from(Span::styled(
                format!(
                    "{} {}",
                    if i == selected && focus_check.is_none() {
                        ">"
                    } else {
                        " "
                    },
                    choices[i]
                ),
                if i == selected && focus_check.is_none() {
                    Style::default()
                        .fg(Color::Black)
                        .bg(Color::Rgb(210, 180, 140))
                } else {
                    Style::default()
                },
            )));
        }
        for (i, c) in checks.iter().enumerate() {
            lines.push(Line::raw(format!(
                "{} [{}] {}",
                if focus_check == Some(i) { ">" } else { " " },
                if c.checked { "x" } else { " " },
                c.label
            )));
        }
        lines.push(Line::raw(
            "↑↓ Choisir · Entrée Valider · Esc Annuler · Tab Cases",
        ));
        paint(&mut t, prompt, lines)?;
        match key()?.code {
            KeyCode::Esc => return Ok(None),
            KeyCode::Enter if !visible.is_empty() => return Ok(Some(selected + 1)),
            KeyCode::Tab if !checks.is_empty() => {
                focus_check = match focus_check {
                    None => Some(0),
                    Some(i) if i + 1 < checks.len() => Some(i + 1),
                    _ => None,
                }
            }
            KeyCode::Char(' ') if focus_check.is_some() => {
                let c = &mut checks[focus_check.unwrap()];
                c.checked = !c.checked;
            }
            KeyCode::Up if !visible.is_empty() => {
                selected = visible[(pos + visible.len() - 1) % visible.len()]
            }
            KeyCode::Down if !visible.is_empty() => selected = visible[(pos + 1) % visible.len()],
            KeyCode::PageUp if !visible.is_empty() => selected = visible[pos.saturating_sub(rows)],
            KeyCode::PageDown if !visible.is_empty() => {
                selected = visible[(pos + rows).min(visible.len() - 1)]
            }
            KeyCode::Backspace => {
                filter.pop();
            }
            KeyCode::Char(c) => filter.push(c),
            _ => {}
        }
    }
}
pub fn install_lua_dialog_module(lua: &Lua, preload: &Table) -> Result<()> {
    preload.set(
        "kkc-dialog",
        lua.create_function(|lua, ()| {
            let t = lua.create_table()?;
            for name in ["confirm_box", "input_box"] {
                t.set(name, lua.create_function(normalize)?)?;
            }
            t.set(
                "message",
                lua.create_function(|_, text: String| {
                    (|| -> Result<()> {
                        let mut t = terminal()?;
                        loop {
                            paint(
                                &mut t,
                                "Message",
                                vec![Line::raw(text.as_str()), Line::raw("Entrée / Esc")],
                            )?;
                            if matches!(key()?.code, KeyCode::Enter | KeyCode::Esc) {
                                return Ok(());
                            }
                        }
                    })()
                    .map_err(mlua::Error::external)
                })?,
            )?;
            t.set(
                "input",
                lua.create_function(|_, (prompt, default): (String, Option<String>)| {
                    (|| -> Result<String> {
                        let default = default.unwrap_or_default();
                        let mut value = default.clone();
                        let mut t = terminal()?;
                        loop {
                            paint(
                                &mut t,
                                &prompt,
                                vec![
                                    Line::raw(format!("Valeur : {value}")),
                                    Line::raw("Entrée Valider · Esc Annuler"),
                                ],
                            )?;
                            match key()?.code {
                                KeyCode::Esc => return Ok(default),
                                KeyCode::Enter => {
                                    return Ok(if value.is_empty() { default } else { value });
                                }
                                KeyCode::Backspace => {
                                    value.pop();
                                }
                                KeyCode::Delete => value.clear(),
                                KeyCode::Char(c) => value.push(c),
                                _ => {}
                            }
                        }
                    })()
                    .map_err(mlua::Error::external)
                })?,
            )?;
            t.set(
                "confirm",
                lua.create_function(|_, (prompt, default): (String, Option<bool>)| {
                    (|| -> Result<bool> {
                        let default = default.unwrap_or(true);
                        let mut t = terminal()?;
                        loop {
                            paint(
                                &mut t,
                                &prompt,
                                vec![Line::raw(format!(
                                    "Y / N · Entrée / Esc : {}",
                                    if default { "Y" } else { "N" }
                                ))],
                            )?;
                            match key()?.code {
                                KeyCode::Char('y' | 'Y') => return Ok(true),
                                KeyCode::Char('n' | 'N') => return Ok(false),
                                KeyCode::Enter | KeyCode::Esc => return Ok(default),
                                _ => {}
                            }
                        }
                    })()
                    .map_err(mlua::Error::external)
                })?,
            )?;
            t.set(
                "select",
                lua.create_function(
                    |_, (prompt, options, default): (String, Table, Option<usize>)| {
                        select(
                            &prompt,
                            options
                                .sequence_values::<String>()
                                .collect::<mlua::Result<Vec<_>>>()?,
                            default.unwrap_or(1),
                            &mut [],
                        )
                        .map_err(mlua::Error::external)
                    },
                )?,
            )?;
            t.set(
                "select_with_checks",
                lua.create_function(
                    |lua,
                     (prompt, options, default, boxes, _theme): (
                        String,
                        Table,
                        Option<usize>,
                        Option<Table>,
                        Option<String>,
                    )| {
                        let mut checks = Vec::new();
                        if let Some(boxes) = boxes {
                            for value in boxes.sequence_values::<Value>() {
                                match value? {
                                    Value::String(s) => checks.push(Checkbox {
                                        label: s.to_str()?.to_string(),
                                        checked: false,
                                    }),
                                    Value::Table(t) => checks.push(Checkbox {
                                        label: t
                                            .get::<Option<String>>("label")?
                                            .or(t.get::<Option<String>>(1)?)
                                            .unwrap_or_default(),
                                        checked: t.get::<Option<bool>>("checked")?.unwrap_or(false),
                                    }),
                                    _ => {}
                                }
                            }
                        }
                        let index = select(
                            &prompt,
                            options
                                .sequence_values::<String>()
                                .collect::<mlua::Result<Vec<_>>>()?,
                            default.unwrap_or(1),
                            &mut checks,
                        )
                        .map_err(mlua::Error::external)?;
                        let out = lua.create_table()?;
                        out.set("index", index)?;
                        out.set(
                            "checks",
                            lua.create_sequence_from(checks.iter().map(|c| c.checked))?,
                        )?;
                        Ok(out)
                    },
                )?,
            )?;
            Ok(t)
        })?,
    )?;
    Ok(())
}
