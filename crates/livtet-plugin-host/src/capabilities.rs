//! Livtet's host-provided sandbox modules.
//!
//! These register as in-host stanchion capabilities on top of what
//! [`stanchion::remote::build_registry`] sets up, relying on
//! `Registry::with_setup` being additive so the remote host's `log` and
//! forwarded callbacks survive. A plugin that declares a capability here gets it
//! as a global, so it can do common work without vendoring a native module from
//! LuaRocks.
//!
//! - `xml` — `parse(source) -> node`, backed by the tolerant parser in
//!   [`livtet_epub::xml`]. A node is `{ tag, attrs = { local = value },
//!   children = { node, ... }, text }`.
//! - `sqlite` — `query(path, sql, params) -> rows`, read-only and restricted to
//!   the database paths the application granted when it launched the host
//!   (`--sqlite <path>`). Rows are an array of `{ column = value }` tables.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use mlua::LuaSerdeExt;
use stanchion::registry::{DynClass, Registry};
use stanchion_lua::mlua;

/// Adds Livtet's in-host module capabilities to `registry`.
///
/// `sqlite_paths` are the database files the `sqlite` capability may open; an
/// empty list means the capability is offered but every `query` is refused.
/// Chains onto whatever `build_registry` already registered; the capabilities
/// remain gated by policy (a plugin only receives one it declares and the host
/// config allows).
pub fn register(registry: Registry<DynClass>, sqlite_paths: Vec<PathBuf>) -> Registry<DynClass> {
    let sqlite_paths = Arc::new(canonicalize_all(sqlite_paths));
    registry.with_setup(move |host| {
        host.capability("xml", |lua, _grant| {
            let lua_state = lua.lua_state().expect("Lua runtime available");
            let lua = lua_state.lock().expect("Lua mutex not poisoned");
            let module = build_xml_module(&lua)
                .map_err(|err| stanchion_abi::Error::Config(err.to_string()))?;
            Ok(stanchion_abi::value::lua::lua_to_abi(
                &lua,
                &mlua::Value::Table(module),
            ))
        });

        let sqlite_paths = Arc::clone(&sqlite_paths);
        host.capability("sqlite", move |lua, _grant| {
            let lua_state = lua.lua_state().expect("Lua runtime available");
            let lua = lua_state.lock().expect("Lua mutex not poisoned");
            let module = build_sqlite_module(&lua, Arc::clone(&sqlite_paths))
                .map_err(|err| stanchion_abi::Error::Config(err.to_string()))?;
            Ok(stanchion_abi::value::lua::lua_to_abi(
                &lua,
                &mlua::Value::Table(module),
            ))
        });
        host.capability("json", |lua, _grant| {
            let lua_state = lua.lua_state().expect("Lua runtime available");
            let lua = lua_state.lock().expect("Lua mutex not poisoned");
            let module = lua
                .create_table()
                .map_err(|err| stanchion_abi::Error::Config(err.to_string()))?;
            let decode = lua
                .create_function(|lua, source: String| {
                    let val: serde_json::Value = serde_json::from_str(&source)
                        .map_err(|e| mlua::Error::RuntimeError(format!("json decode: {e}")))?;
                    lua.to_value(&val)
                })
                .map_err(|err| stanchion_abi::Error::Config(err.to_string()))?;
            module
                .set("decode", decode)
                .map_err(|err| stanchion_abi::Error::Config(err.to_string()))?;
            Ok(stanchion_abi::value::lua::lua_to_abi(
                &lua,
                &mlua::Value::Table(module),
            ))
        });
        host.capability("base64", |lua, _grant| {
            let lua_state = lua.lua_state().expect("Lua runtime available");
            let lua = lua_state.lock().expect("Lua mutex not poisoned");
            let module = lua
                .create_table()
                .map_err(|err| stanchion_abi::Error::Config(err.to_string()))?;
            Ok(stanchion_abi::value::lua::lua_to_abi(
                &lua,
                &mlua::Value::Table(module),
            ))
        });
        Ok(())
    })
}

fn canonicalize_all(paths: Vec<PathBuf>) -> Vec<PathBuf> {
    paths
        .into_iter()
        .filter_map(|path| path.canonicalize().ok())
        .collect()
}

// ── xml ─────────────────────────────────────────────────────────────────────

/// Builds the `xml` module table: `{ parse = function(source) -> node }`.
fn build_xml_module(lua: &mlua::Lua) -> mlua::Result<mlua::Table> {
    let module = lua.create_table()?;
    let parse = lua.create_function(|lua, source: String| {
        let element = livtet_epub::xml::parse(source.as_bytes())
            .map_err(|err| mlua::Error::RuntimeError(format!("xml: {err}")))?;
        element_to_lua(lua, &element)
    })?;
    module.set("parse", parse)?;
    Ok(module)
}

/// Converts a parsed element into a Lua node table, recursively.
fn element_to_lua(
    lua: &mlua::Lua,
    element: &livtet_epub::xml::Element,
) -> mlua::Result<mlua::Table> {
    let node = lua.create_table()?;
    node.set("tag", element.local.clone())?;

    let attrs = lua.create_table()?;
    for attr in &element.attrs {
        attrs.set(attr.local.clone(), attr.value.clone())?;
    }
    node.set("attrs", attrs)?;

    let children = lua.create_table()?;
    for (index, child) in element.children.iter().enumerate() {
        children.set(index + 1, element_to_lua(lua, child)?)?;
    }
    node.set("children", children)?;

    node.set("text", element.direct_text())?;
    Ok(node)
}

// ── sqlite ──────────────────────────────────────────────────────────────────

/// Builds the `sqlite` module table: `{ query = function(path, sql, params) }`.
fn build_sqlite_module(lua: &mlua::Lua, allowed: Arc<Vec<PathBuf>>) -> mlua::Result<mlua::Table> {
    let module = lua.create_table()?;
    let query = lua.create_function(
        move |lua, (path, sql, params): (String, String, Option<mlua::Table>)| {
            run_query(lua, &allowed, &path, &sql, params.as_ref())
                .map_err(|err| mlua::Error::RuntimeError(format!("sqlite: {err}")))
        },
    )?;
    module.set("query", query)?;
    Ok(module)
}

/// Opens `path` read-only (only if it was granted), runs `sql` with `params`,
/// and returns the rows as a Lua array of `{ column = value }` tables.
fn run_query(
    lua: &mlua::Lua,
    allowed: &[PathBuf],
    path: &str,
    sql: &str,
    params: Option<&mlua::Table>,
) -> Result<mlua::Table, String> {
    let requested = Path::new(path)
        .canonicalize()
        .map_err(|err| format!("cannot open {path}: {err}"))?;
    if !allowed.iter().any(|granted| granted == &requested) {
        return Err(format!("{path} was not granted to this plugin"));
    }

    let conn = rusqlite::Connection::open_with_flags(
        &requested,
        rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY | rusqlite::OpenFlags::SQLITE_OPEN_URI,
    )
    .map_err(|err| format!("open failed: {err}"))?;
    let mut statement = conn
        .prepare(sql)
        .map_err(|err| format!("prepare failed: {err}"))?;

    let columns: Vec<String> = statement
        .column_names()
        .into_iter()
        .map(str::to_string)
        .collect();
    let bindings = lua_params_to_sql(params)?;

    let rows = lua.create_table().map_err(|err| err.to_string())?;
    let mut cursor = statement
        .query(rusqlite::params_from_iter(bindings.iter()))
        .map_err(|err| format!("query failed: {err}"))?;
    let mut index = 0;
    while let Some(row) = cursor.next().map_err(|err| format!("row failed: {err}"))? {
        let record = lua.create_table().map_err(|err| err.to_string())?;
        for (column_index, column) in columns.iter().enumerate() {
            let value = row
                .get_ref(column_index)
                .map_err(|err| format!("column read failed: {err}"))?;
            record
                .set(column.as_str(), sql_value_to_lua(lua, value)?)
                .map_err(|err| err.to_string())?;
        }
        index += 1;
        rows.set(index, record).map_err(|err| err.to_string())?;
    }
    Ok(rows)
}

/// Converts a Lua params array into owned rusqlite values.
fn lua_params_to_sql(params: Option<&mlua::Table>) -> Result<Vec<rusqlite::types::Value>, String> {
    let Some(params) = params else {
        return Ok(Vec::new());
    };
    let mut out = Vec::new();
    for pair in params.clone().sequence_values::<mlua::Value>() {
        let value = pair.map_err(|err| err.to_string())?;
        out.push(lua_value_to_sql(&value)?);
    }
    Ok(out)
}

fn lua_value_to_sql(value: &mlua::Value) -> Result<rusqlite::types::Value, String> {
    use rusqlite::types::Value;
    Ok(match value {
        mlua::Value::Nil => Value::Null,
        mlua::Value::Boolean(b) => Value::Integer(i64::from(*b)),
        mlua::Value::Integer(i) => Value::Integer(*i),
        mlua::Value::Number(n) => Value::Real(*n),
        mlua::Value::String(s) => {
            Value::Text(s.to_str().map_err(|err| err.to_string())?.to_string())
        }
        other => return Err(format!("unsupported bind value: {other:?}")),
    })
}

fn sql_value_to_lua(
    lua: &mlua::Lua,
    value: rusqlite::types::ValueRef,
) -> Result<mlua::Value, String> {
    use rusqlite::types::ValueRef;
    Ok(match value {
        ValueRef::Null => mlua::Value::Nil,
        ValueRef::Integer(i) => mlua::Value::Integer(i),
        ValueRef::Real(f) => mlua::Value::Number(f),
        ValueRef::Text(bytes) => {
            let text = String::from_utf8_lossy(bytes).into_owned();
            mlua::Value::String(lua.create_string(&text).map_err(|err| err.to_string())?)
        }
        ValueRef::Blob(bytes) => {
            mlua::Value::String(lua.create_string(bytes).map_err(|err| err.to_string())?)
        }
    })
}
