use livtet_importer::{ImporterClass, ImporterHandle};
use stanchion::mlua::{Lua, Result};
use stanchion::{LuaObject, load_class};

const INCOMPLETE_IMPORTER: &str = r#"
local Importer = {}
Importer.__index = Importer

function Importer.new(config, deps)
  return setmetatable({ config = config, deps = deps }, Importer)
end

function Importer:extensions()
  return { "epub" }
end

return Importer
"#;

#[test]
fn importer_instance_requires_read_metadata() -> Result<()> {
    let lua = Lua::new();
    let class = load_class::<ImporterClass>(&lua, INCOMPLETE_IMPORTER, "importer.lua")?;
    let error = class
        .new(lua.create_table()?, lua.create_table()?)
        .expect_err("metadata extraction is a required importer behavior");

    assert!(
        error.to_string().contains("read_metadata"),
        "unexpected contract error: {error}"
    );
    assert!(
        ImporterHandle::required_methods().contains(&"extensions")
            && ImporterHandle::required_methods().contains(&"read_metadata"),
        "ImporterClass must advertise the required instance methods"
    );
    Ok(())
}
