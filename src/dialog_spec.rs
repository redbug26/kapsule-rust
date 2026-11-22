use mlua::{Lua, Table, Value};
pub(crate) fn normalize(lua: &Lua, spec: Table) -> mlua::Result<Table> {
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
