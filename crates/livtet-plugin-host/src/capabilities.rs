//! Livtet's host-provided sandbox modules.
//!
//! These register as in-host stanchion capabilities on top of what
//! [`stanchion::remote::build_registry`] sets up, relying on
//! `Registry::with_setup` being additive so the remote host's `log` and
//! forwarded callbacks survive. A plugin that declares a capability here gets it
//! as a global, so it can do common work without vendoring a native module from
//! LuaRocks.
//!
//! - `xml` — a table with `parse(source) -> node`, backed by the tolerant parser
//!   in [`livtet_epub::xml`]. A node is `{ tag, attrs = { local = value }, ... ,
//!   children = { node, ... }, text }`.

use stanchion::registry::{DynClass, Registry};
use stanchion_lua::mlua;

/// Adds Livtet's in-host module capabilities to `registry`.
///
/// Chains onto whatever `build_registry` already registered. The capabilities
/// are still gated by policy: a plugin only receives one it declares and the
/// host config allows.
pub fn register(registry: Registry<DynClass>) -> Registry<DynClass> {
    registry.with_setup(|host| {
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
        Ok(())
    })
}

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
///
/// `{ tag = <local name>, attrs = { <local name> = <value> }, children = { ... },
/// text = <this element's own text> }`.
fn element_to_lua(lua: &mlua::Lua, element: &livtet_epub::xml::Element) -> mlua::Result<mlua::Table> {
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
