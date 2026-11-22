//! Browser-native modal functions preserve synchronous Lua signatures.
use anyhow::Result;
use mlua::{Lua, Table, Value};
use std::ffi::{CStr, CString, c_char};
unsafe extern "C" {
    fn kapsule_dialog(
        kind: *const c_char,
        prompt: *const c_char,
        fallback: *const c_char,
    ) -> *mut c_char;
    fn free(pointer: *mut std::ffi::c_void);
}
fn ask(kind: &str, prompt: &str, default: Value) -> mlua::Result<String> {
    let value = match default {
        Value::String(s) => serde_json::Value::String(s.to_str()?.to_string()),
        Value::Boolean(b) => serde_json::Value::Bool(b),
        _ => serde_json::Value::Null,
    };
    let kind = CString::new(kind).map_err(mlua::Error::external)?;
    let prompt = CString::new(prompt).map_err(mlua::Error::external)?;
    let fallback = CString::new(value.to_string()).map_err(mlua::Error::external)?;
    let ptr = unsafe { kapsule_dialog(kind.as_ptr(), prompt.as_ptr(), fallback.as_ptr()) };
    if ptr.is_null() {
        return Err(mlua::Error::external(
            "Browser dialog is unavailable or its response is too large",
        ));
    }
    let response = unsafe { CStr::from_ptr(ptr) }
        .to_string_lossy()
        .into_owned();
    unsafe {
        free(ptr.cast());
    }
    let decoded: serde_json::Value =
        serde_json::from_str(&response).map_err(mlua::Error::external)?;
    if let Some(error) = decoded.get("__kapsule_error").and_then(|v| v.as_str()) {
        return Err(mlua::Error::external(error.to_owned()));
    }
    Ok(response)
}
pub fn install_lua_dialog_module(lua: &Lua, preload: &Table) -> Result<()> {
    let module = lua.create_function(|lua, ()| {
        let t = lua.create_table()?;
        for name in ["confirm_box", "input_box"] {
            t.set(name, lua.create_function(crate::dialog_spec::normalize)?)?;
        }
        t.set(
            "message",
            lua.create_function(|_, text: String| {
                ask("message", &text, Value::Nil)?;
                Ok(())
            })?,
        )?;
        t.set(
            "input",
            lua.create_function(|lua, (prompt, default): (String, Option<String>)| {
                let default = default.unwrap_or_default();
                let answer: Option<String> = serde_json::from_str(&ask(
                    "input",
                    &prompt,
                    Value::String(lua.create_string(&default)?),
                )?)
                .map_err(mlua::Error::external)?;
                Ok(answer.filter(|s| !s.is_empty()).unwrap_or(default))
            })?,
        )?;
        t.set(
            "confirm",
            lua.create_function(|_, (prompt, default): (String, Option<bool>)| {
                serde_json::from_str::<bool>(&ask(
                    "confirm",
                    &prompt,
                    Value::Boolean(default.unwrap_or(true)),
                )?)
                .map_err(mlua::Error::external)
            })?,
        )?;
        t.set(
            "select",
            lua.create_function(
                |lua, (prompt, options, default): (String, Table, Option<usize>)| {
                    let choices = options
                        .sequence_values::<String>()
                        .collect::<mlua::Result<Vec<_>>>()?;
                    if choices.is_empty() {
                        return Ok(None);
                    }
                    let prompt = format!(
                        "{}\n{}",
                        prompt,
                        choices
                            .iter()
                            .enumerate()
                            .map(|(i, s)| format!("{}: {}", i + 1, s))
                            .collect::<Vec<_>>()
                            .join("\n")
                    );
                    let answer: Option<String> = serde_json::from_str(&ask(
                        "input",
                        &prompt,
                        Value::String(lua.create_string(
                            default.unwrap_or(1).clamp(1, choices.len()).to_string(),
                        )?),
                    )?)
                    .map_err(mlua::Error::external)?;
                    Ok(answer
                        .and_then(|s| s.parse::<usize>().ok())
                        .filter(|&i| i > 0 && i <= choices.len()))
                },
            )?,
        )?;
        let select: mlua::Function = t.get("select")?;
        t.set(
            "select_with_checks",
            lua.create_function(
                move |lua,
                      (prompt, options, default, checkboxes, _theme): (
                    String,
                    Table,
                    Option<usize>,
                    Option<Table>,
                    Option<String>,
                )| {
                    let mut checks = Vec::new();
                    if let Some(boxes) = checkboxes {
                        for entry in boxes.sequence_values::<Value>() {
                            match entry? {
                                Value::String(s) => checks.push((s.to_str()?.to_string(), false)),
                                Value::Table(t) => checks.push((
                                    t.get::<Option<String>>("label")?
                                        .or(t.get::<Option<String>>(1)?)
                                        .unwrap_or_default(),
                                    t.get::<Option<bool>>("checked")?.unwrap_or(false),
                                )),
                                _ => {}
                            }
                        }
                    }
                    let index: Option<usize> = select.call((prompt, options, default))?;
                    if index.is_some() {
                        for (label, checked) in &mut checks {
                            *checked = serde_json::from_str(&ask(
                                "confirm",
                                label,
                                Value::Boolean(*checked),
                            )?)
                            .map_err(mlua::Error::external)?;
                        }
                    }
                    let out = lua.create_table()?;
                    out.set("index", index)?;
                    out.set(
                        "checks",
                        lua.create_sequence_from(checks.into_iter().map(|(_, checked)| checked))?,
                    )?;
                    Ok(out)
                },
            )?,
        )?;
        Ok(t)
    })?;
    preload.set("kkc-dialog", module)?;
    Ok(())
}
