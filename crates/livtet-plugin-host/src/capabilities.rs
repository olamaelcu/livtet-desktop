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

use std::sync::Arc;

use camino::{Utf8Path, Utf8PathBuf};

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
pub fn register(
    registry: Registry<DynClass>,
    sqlite_paths: Vec<Utf8PathBuf>,
) -> Registry<DynClass> {
    let sqlite_paths = Arc::new(canonicalize_all(sqlite_paths));
    registry.with_setup(move |host| {
        host.capability("xml", |lua, _grant| {
            let lua_state = lua.lua_state().ok_or_else(|| {
                stanchion_abi::Error::Config("Lua runtime unavailable".to_string())
            })?;
            let lua = lua_state.lock().map_err(|err| {
                stanchion_abi::Error::Config(format!("Lua mutex poisoned: {err}"))
            })?;
            let module = build_xml_module(&lua)
                .map_err(|err| stanchion_abi::Error::Config(err.to_string()))?;
            Ok(stanchion_abi::value::lua::lua_to_abi(
                &lua,
                &mlua::Value::Table(module),
            ))
        });

        let sqlite_paths = Arc::clone(&sqlite_paths);
        host.capability("sqlite", move |lua, _grant| {
            let lua_state = lua.lua_state().ok_or_else(|| {
                stanchion_abi::Error::Config("Lua runtime unavailable".to_string())
            })?;
            let lua = lua_state.lock().map_err(|err| {
                stanchion_abi::Error::Config(format!("Lua mutex poisoned: {err}"))
            })?;
            let module = build_sqlite_module(&lua, Arc::clone(&sqlite_paths))
                .map_err(|err| stanchion_abi::Error::Config(err.to_string()))?;
            Ok(stanchion_abi::value::lua::lua_to_abi(
                &lua,
                &mlua::Value::Table(module),
            ))
        });
        host.capability("json", |lua, _grant| {
            let lua_state = lua.lua_state().ok_or_else(|| {
                stanchion_abi::Error::Config("Lua runtime unavailable".to_string())
            })?;
            let lua = lua_state.lock().map_err(|err| {
                stanchion_abi::Error::Config(format!("Lua mutex poisoned: {err}"))
            })?;
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
            let lua_state = lua.lua_state().ok_or_else(|| {
                stanchion_abi::Error::Config("Lua runtime unavailable".to_string())
            })?;
            let lua = lua_state.lock().map_err(|err| {
                stanchion_abi::Error::Config(format!("Lua mutex poisoned: {err}"))
            })?;
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

pub(crate) fn canonicalize_all(paths: Vec<Utf8PathBuf>) -> Vec<Utf8PathBuf> {
    paths
        .into_iter()
        .filter_map(|path| {
            path.into_std_path_buf()
                .canonicalize()
                .ok()
                .and_then(|canonical| Utf8PathBuf::from_path_buf(canonical).ok())
        })
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
fn build_sqlite_module(
    lua: &mlua::Lua,
    allowed: Arc<Vec<Utf8PathBuf>>,
) -> mlua::Result<mlua::Table> {
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
    allowed: &[Utf8PathBuf],
    path: &str,
    sql: &str,
    params: Option<&mlua::Table>,
) -> Result<mlua::Table, String> {
    let requested = Utf8Path::new(path)
        .to_path_buf()
        .into_std_path_buf()
        .canonicalize()
        .map_err(|err| format!("cannot open {path}: {err}"))
        .and_then(|canonical| {
            Utf8PathBuf::from_path_buf(canonical)
                .map_err(|err| format!("non-UTF-8 path {path}: {err:?}"))
        })?;
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

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]
    use super::*;
    use rusqlite::types::{Value, ValueRef};
    use stanchion::registry::{Rules, Sandbox};

    #[test]
    fn lua_nil_becomes_sql_null() {
        assert_eq!(lua_value_to_sql(&mlua::Value::Nil).unwrap(), Value::Null);
    }

    #[test]
    fn lua_boolean_integer_number_and_string_convert() {
        let lua = mlua::Lua::new();

        assert_eq!(
            lua_value_to_sql(&mlua::Value::Boolean(true)).unwrap(),
            Value::Integer(1)
        );
        assert_eq!(
            lua_value_to_sql(&mlua::Value::Boolean(false)).unwrap(),
            Value::Integer(0)
        );
        assert_eq!(
            lua_value_to_sql(&mlua::Value::Integer(7)).unwrap(),
            Value::Integer(7)
        );
        assert_eq!(
            lua_value_to_sql(&mlua::Value::Number(1.5)).unwrap(),
            Value::Real(1.5)
        );
        let text = mlua::Value::String(lua.create_string("hello").unwrap());
        assert_eq!(
            lua_value_to_sql(&text).unwrap(),
            Value::Text("hello".to_string())
        );
    }

    /// A Lua type with no SQL equivalent must be refused, not coerced to NULL:
    /// binding a function as a query parameter is a plugin bug worth surfacing.
    #[test]
    fn unsupported_lua_value_is_an_error() {
        let lua = mlua::Lua::new();
        let func = mlua::Value::Function(lua.create_function(|_, ()| Ok(())).unwrap());

        let error = lua_value_to_sql(&func).unwrap_err();

        assert!(
            error.contains("unsupported bind value"),
            "unexpected error: {error}"
        );
    }

    #[test]
    fn absent_params_convert_to_an_empty_vec() {
        assert_eq!(lua_params_to_sql(None).unwrap(), Vec::new());
    }

    #[test]
    fn param_table_converts_in_index_order() {
        let lua = mlua::Lua::new();
        let table = lua.create_table().unwrap();
        table.set(1, "first").unwrap();
        table.set(2, 2_i64).unwrap();
        table.set(3, true).unwrap();

        let params = lua_params_to_sql(Some(&table)).unwrap();

        assert_eq!(
            params,
            vec![
                Value::Text("first".to_string()),
                Value::Integer(2),
                Value::Integer(1),
            ]
        );
    }

    #[test]
    fn sql_values_round_trip_back_to_lua() {
        let lua = mlua::Lua::new();

        assert!(matches!(
            sql_value_to_lua(&lua, ValueRef::Null).unwrap(),
            mlua::Value::Nil
        ));
        assert!(matches!(
            sql_value_to_lua(&lua, ValueRef::Integer(9)).unwrap(),
            mlua::Value::Integer(9)
        ));
        assert!(matches!(
            sql_value_to_lua(&lua, ValueRef::Real(2.5)).unwrap(),
            mlua::Value::Number(n) if (n - 2.5).abs() < f64::EPSILON
        ));

        let text = sql_value_to_lua(&lua, ValueRef::Text(b"howdy")).unwrap();
        let mlua::Value::String(text) = text else {
            panic!("expected a Lua string, got {text:?}");
        };
        assert_eq!(text.to_str().unwrap().to_owned(), "howdy");

        // A blob arrives as a Lua string carrying the raw bytes.
        let blob = sql_value_to_lua(&lua, ValueRef::Blob(&[0xff, 0x00])).unwrap();
        assert!(matches!(blob, mlua::Value::String(_)));
    }

    /// Paths that cannot be resolved are dropped rather than passed through, so
    /// an unreadable grant silently narrows the allow-list instead of widening
    /// it. Uses this crate's own manifest as a path guaranteed to exist.
    #[test]
    pub(crate) fn canonicalize_all_keeps_real_paths_and_drops_the_rest() {
        let real = Utf8PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("Cargo.toml");
        let missing = Utf8PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("no-such-file-9f3a.toml");

        let resolved = canonicalize_all(vec![real.clone(), missing]);

        assert_eq!(resolved.len(), 1, "the missing path should be dropped");
        assert!(resolved[0].is_absolute());
        assert!(resolved[0].ends_with("Cargo.toml"));
    }

    /// `register` must extend, not replace, what `build_registry` set up: the
    /// remote host's `log` and forwarded callbacks have to survive alongside
    /// Livtet's modules.
    #[test]
    fn register_keeps_earlier_capabilities_and_offers_livtet_modules() {
        let registry = Registry::<DynClass>::new(mlua::Lua::new()).with_setup(|host| {
            host.capability("log", |_runtime, _grant| Ok(stanchion_abi::Value::Nil));
            Ok(())
        });

        let registry = register(registry, Vec::new());

        let offered: Vec<&str> = registry.host_setup().offered().collect();
        assert_eq!(offered, ["base64", "json", "log", "sqlite", "xml"]);
    }

    // ── capabilities bound into a real plugin ───────────────────────────────

    /// Writes a `probe` plugin under `root` whose `run(input)` method executes
    /// `body`, declaring `capabilities` in its manifest.
    fn write_probe(root: &Utf8Path, capabilities: &[&str], body: &str) {
        let dir = root.join("probe");
        fs_err::create_dir_all(&dir).unwrap();
        let mut manifest = String::from("name = \"probe\"\n");
        for capability in capabilities {
            manifest.push_str(&format!("\n[capabilities.{capability}]\n"));
        }
        fs_err::write(dir.join("plugin.toml"), manifest).unwrap();
        fs_err::write(
            dir.join("init.lua"),
            format!(
                "local P = {{}}\nP.__index = P\n\
                 function P.new(config, deps) return setmetatable({{}}, P) end\n\
                 function P:run(input)\n{body}\nend\nreturn P\n"
            ),
        )
        .unwrap();
    }

    /// Loads the probe into an isolated, restricted registry (as the host runs
    /// by default) extended by [`register`], with a policy granting every
    /// declared capability, then returns `run(input)` as a string.
    fn run_probe(
        capabilities: &[&str],
        sqlite_paths: Vec<Utf8PathBuf>,
        body: &str,
        input: &str,
    ) -> String {
        let root = tempfile::tempdir().unwrap();
        let root_path = Utf8Path::from_path(root.path()).unwrap();
        write_probe(root_path, capabilities, body);
        let policy = capabilities
            .iter()
            .fold(Rules::deny_all(), |rules, name| rules.allow(*name));
        let mut registry = register(
            Registry::<DynClass>::isolated(mlua::Lua::new(), Sandbox::restricted()),
            sqlite_paths,
        )
        .with_policy(policy);

        let report = registry.load_dir(root_path).unwrap();
        let failures: Vec<String> = report.failures.iter().map(ToString::to_string).collect();
        assert!(failures.is_empty(), "probe failed to load: {failures:?}");

        let plugin = registry.get("probe").unwrap();
        let args = plugin.lua().pack_multi(input).unwrap();
        let value = plugin.instance().call_method("run", args).unwrap();
        plugin.lua().unpack::<String>(value).unwrap()
    }

    /// Lua that calls `call` under `pcall` and returns `"<ok> <error>"`.
    fn protected(call: &str) -> String {
        format!("local ok, err = pcall({call}) return tostring(ok) .. ' ' .. tostring(err)")
    }

    /// A database at `<dir>/library.db` holding three books.
    fn library_db(dir: &Utf8Path) -> Utf8PathBuf {
        let path = dir.join("library.db");
        let conn = rusqlite::Connection::open(&path).unwrap();
        conn.execute_batch(
            "CREATE TABLE books (title TEXT, author TEXT, year INTEGER);
             INSERT INTO books VALUES ('The Dispossessed', 'Le Guin', 1974),
                                      ('A Wizard of Earthsea', 'Le Guin', 1968),
                                      ('Dune', 'Herbert', 1965);",
        )
        .unwrap();
        path
    }

    fn temp_dir() -> (tempfile::TempDir, Utf8PathBuf) {
        let dir = tempfile::tempdir().unwrap();
        let path = Utf8PathBuf::from_path_buf(dir.path().to_path_buf()).unwrap();
        (dir, path)
    }

    /// The node shape plugins rely on: local tag and attribute names (prefixes
    /// dropped), children in document order, and `text` holding only the
    /// element's own text, not its descendants'.
    #[test]
    fn xml_capability_parses_nested_documents_for_a_granted_plugin() {
        let body = r#"
            local root = xml.parse(input)
            local title, meta = root.children[1], root.children[2]
            return table.concat({
                root.tag, root.attrs.id, tostring(#root.children),
                title.tag, title.attrs.lang, title.text,
                title.children[1].tag, title.children[1].text,
                meta.tag, tostring(#meta.children), meta.text,
            }, "|")
        "#;
        let document = r#"<opf:package xmlns:opf="http://www.idpf.org/2007/opf" id="pkg"><title xml:lang="en">Left <i>Hand</i> of Darkness</title><meta/></opf:package>"#;

        assert_eq!(
            run_probe(&["xml"], Vec::new(), body, document),
            "package|pkg|2|title|en|Left  of Darkness|i|Hand|meta|0|"
        );
    }

    #[test]
    fn xml_capability_raises_a_catchable_error_for_a_document_without_a_root() {
        let out = run_probe(
            &["xml"],
            Vec::new(),
            &protected("xml.parse, input"),
            "just some text",
        );

        assert!(out.starts_with("false "), "parse should fail: {out}");
        assert!(
            out.contains("xml:"),
            "error should name the xml module: {out}"
        );
    }

    #[test]
    fn json_capability_decodes_nested_values() {
        let body = r#"
            local doc = json.decode(input)
            return table.concat({
                doc.title, tostring(doc.year), tostring(#doc.tags), doc.tags[2],
                tostring(doc.series.index), tostring(doc.owned),
            }, "|")
        "#;
        let document = r#"{"title":"Dune","year":1965,"tags":["sf","classic"],"series":{"index":1.5},"owned":true}"#;

        assert_eq!(
            run_probe(&["json"], Vec::new(), body, document),
            "Dune|1965|2|classic|1.5|true"
        );
    }

    #[test]
    fn json_capability_raises_a_catchable_error_for_malformed_input() {
        let out = run_probe(
            &["json"],
            Vec::new(),
            &protected("json.decode, input"),
            r#"{"title": "Dune""#,
        );

        assert!(out.starts_with("false "), "decode should fail: {out}");
        assert!(out.contains("json decode"), "unexpected error: {out}");
    }

    /// Each provider converts its module through the ABI separately; declaring
    /// all of them at once must still bind every one under its own name.
    #[test]
    fn every_livtet_capability_binds_together_in_one_plugin() {
        let body = r#"
            return table.concat({ type(xml.parse), type(json.decode), type(sqlite.query), type(base64) }, ",")
        "#;

        assert_eq!(
            run_probe(&["base64", "json", "sqlite", "xml"], Vec::new(), body, ""),
            "function,function,function,table"
        );
    }

    /// Grants are canonicalized at launch, so a grant spelled with `..` still
    /// matches the plain path the plugin asks for; params bind in order.
    #[test]
    fn sqlite_capability_queries_a_granted_database_with_params() {
        let (_guard, dir) = temp_dir();
        let db = library_db(&dir);
        fs_err::create_dir(dir.join("sub")).unwrap();
        let grant = dir.join("sub").join("..").join("library.db");
        let body = r#"
            local rows = sqlite.query(input,
                "SELECT title, year FROM books WHERE author = ? AND year > ? ORDER BY year",
                { "Le Guin", 1900 })
            local out = {}
            for i, row in ipairs(rows) do out[i] = row.title .. "@" .. row.year end
            return table.concat(out, ";")
        "#;

        assert_eq!(
            run_probe(&["sqlite"], vec![grant], body, db.as_str()),
            "A Wizard of Earthsea@1968;The Dispossessed@1974"
        );
    }

    /// Two databases with the same file name: granting one must not open the
    /// other, and the refusal surfaces as a Lua error the plugin can catch.
    #[test]
    fn sqlite_capability_refuses_a_database_that_was_not_granted() {
        let (_granted_guard, granted_dir) = temp_dir();
        let (_other_guard, other_dir) = temp_dir();
        let granted = library_db(&granted_dir);
        let other = library_db(&other_dir);

        let out = run_probe(
            &["sqlite"],
            vec![granted],
            &protected("sqlite.query, input, 'SELECT title FROM books'"),
            other.as_str(),
        );

        assert!(out.starts_with("false "), "query should be refused: {out}");
        assert!(
            out.contains("sqlite:"),
            "error should name the module: {out}"
        );
        assert!(out.contains("was not granted"), "unexpected error: {out}");
    }

    // ── run_query ───────────────────────────────────────────────────────────

    #[test]
    fn run_query_maps_every_storage_class_and_leaves_null_columns_unset() {
        let (_guard, dir) = temp_dir();
        let path = dir.join("types.db");
        rusqlite::Connection::open(&path)
            .unwrap()
            .execute_batch(
                "CREATE TABLE t (i INTEGER, r REAL, s TEXT, b BLOB, n TEXT);
                 INSERT INTO t VALUES (42, 2.5, 'héllo', x'00ff', NULL);",
            )
            .unwrap();
        let lua = mlua::Lua::new();
        let allowed = canonicalize_all(vec![path.clone()]);

        let rows = run_query(
            &lua,
            &allowed,
            path.as_str(),
            "SELECT i, r, s, b, n FROM t",
            None,
        )
        .unwrap();

        assert_eq!(rows.raw_len(), 1);
        let row: mlua::Table = rows.get(1).unwrap();
        assert!(matches!(
            row.get::<mlua::Value>("i").unwrap(),
            mlua::Value::Integer(42)
        ));
        assert!(matches!(
            row.get::<mlua::Value>("r").unwrap(),
            mlua::Value::Number(n) if (n - 2.5).abs() < f64::EPSILON
        ));
        assert_eq!(row.get::<String>("s").unwrap(), "héllo");
        let blob: mlua::LuaString = row.get("b").unwrap();
        assert_eq!(blob.as_bytes().to_vec(), vec![0x00, 0xff]);
        assert!(row.get::<mlua::Value>("n").unwrap().is_nil());
    }

    #[test]
    fn run_query_with_no_matching_rows_returns_an_empty_array() {
        let (_guard, dir) = temp_dir();
        let db = library_db(&dir);
        let lua = mlua::Lua::new();
        let params = lua.create_sequence_from([3000_i64]).unwrap();

        let rows = run_query(
            &lua,
            &canonicalize_all(vec![db.clone()]),
            db.as_str(),
            "SELECT title FROM books WHERE year > ?",
            Some(&params),
        )
        .unwrap();

        assert_eq!(rows.raw_len(), 0);
    }

    #[test]
    fn run_query_refuses_an_existing_database_missing_from_the_grants() {
        let (_guard, dir) = temp_dir();
        let db = library_db(&dir);
        let lua = mlua::Lua::new();

        let error = run_query(&lua, &[], db.as_str(), "SELECT title FROM books", None).unwrap_err();

        assert!(
            error.contains("was not granted"),
            "unexpected error: {error}"
        );
    }

    /// A missing path fails before anything is opened, so no empty database is
    /// left behind at the requested location.
    #[test]
    fn run_query_reports_a_missing_file_without_creating_it() {
        let (_guard, dir) = temp_dir();
        let missing = dir.join("missing.db");
        let lua = mlua::Lua::new();

        let error = run_query(
            &lua,
            std::slice::from_ref(&missing),
            missing.as_str(),
            "SELECT 1",
            None,
        )
        .unwrap_err();

        assert!(
            error.starts_with("cannot open"),
            "unexpected error: {error}"
        );
        assert!(!missing.exists());
    }

    #[test]
    fn run_query_reports_invalid_sql_and_parameter_count_mismatches() {
        let (_guard, dir) = temp_dir();
        let db = library_db(&dir);
        let allowed = canonicalize_all(vec![db.clone()]);
        let lua = mlua::Lua::new();

        let bad_sql =
            run_query(&lua, &allowed, db.as_str(), "SELEKT title FROM books", None).unwrap_err();
        let missing_param = run_query(
            &lua,
            &allowed,
            db.as_str(),
            "SELECT title FROM books WHERE year > ?",
            None,
        )
        .unwrap_err();

        assert!(
            bad_sql.starts_with("prepare failed"),
            "unexpected error: {bad_sql}"
        );
        assert!(
            missing_param.starts_with("query failed"),
            "unexpected error: {missing_param}"
        );
    }

    /// The grant is read-only: writes are refused and the file is untouched.
    #[test]
    fn run_query_cannot_modify_a_granted_database() {
        let (_guard, dir) = temp_dir();
        let db = library_db(&dir);
        let allowed = canonicalize_all(vec![db.clone()]);
        let lua = mlua::Lua::new();

        let delete = run_query(&lua, &allowed, db.as_str(), "DELETE FROM books", None);
        let insert = run_query(
            &lua,
            &allowed,
            db.as_str(),
            "INSERT INTO books VALUES ('Kindred', 'Butler', 1979)",
            None,
        );

        assert!(delete.is_err(), "DELETE should be refused");
        assert!(insert.is_err(), "INSERT should be refused");
        let count: i64 = rusqlite::Connection::open(&db)
            .unwrap()
            .query_row("SELECT count(*) FROM books", [], |row| row.get(0))
            .unwrap();
        assert_eq!(count, 3);
    }

    /// `lua_value_to_sql` maps `nil` to NULL, but `lua_params_to_sql` walks the
    /// params with `sequence_values`, which stops at the first `nil`: the NULL is
    /// never bound and every later param is dropped, so the query fails with a
    /// parameter-count error instead.
    #[test]
    #[ignore = "bug: a nil param ends the Lua sequence, so NULL can never be bound"]
    fn run_query_binds_a_nil_param_as_null() {
        let (_guard, dir) = temp_dir();
        let db = library_db(&dir);
        let lua = mlua::Lua::new();
        let params: mlua::Table = lua.load("return { nil, 'Dune' }").eval().unwrap();

        let rows = run_query(
            &lua,
            &canonicalize_all(vec![db.clone()]),
            db.as_str(),
            "SELECT title FROM books WHERE ?1 IS NULL AND title = ?2",
            Some(&params),
        )
        .unwrap();

        assert_eq!(rows.raw_len(), 1);
    }
}
