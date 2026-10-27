use toml_edit::{Array, ArrayOfTables, DocumentMut, Item, Table, value};

use crate::cli::paths::config_path;
use crate::worker::error::WorkerError;
use crate::worker::paths::worker_data_dir;
use crate::worker::validate::{
    dir_status, suggested_name, validate_name, validate_worker_dirs,
};

pub const DEFAULT_MANAGEMENT_PORT: u16 = 18741;
pub const DEFAULT_IDLE_RELEASE_TIMEOUT: u32 = 0;
pub const SHARE_DESKTOP_VIEW: &str = "view";
pub const SHARE_DESKTOP_VIEW_AND_CONTROL: &str = "view_and_control";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkerRecord {
    pub id: String,
    pub name: String,
    pub enabled: bool,
    pub autostart: bool,
    pub worker_dirs: Vec<String>,
    pub computer_use: bool,
    pub display: String,
    pub connect_workspace: bool,
    pub identity_socket: bool,
    pub share_desktop: bool,
    pub share_desktop_mode: String,
    pub management_port: u16,
    pub idle_release_timeout: u32,
}

impl WorkerRecord {
    pub fn defaults(id: String, name: String, worker_dirs: Vec<String>) -> Self {
        Self {
            id,
            name,
            enabled: true,
            autostart: true,
            worker_dirs,
            computer_use: false,
            display: String::new(),
            connect_workspace: false,
            identity_socket: false,
            share_desktop: false,
            share_desktop_mode: SHARE_DESKTOP_VIEW.to_string(),
            management_port: DEFAULT_MANAGEMENT_PORT,
            idle_release_timeout: DEFAULT_IDLE_RELEASE_TIMEOUT,
        }
    }
}

pub fn load_records() -> Result<Vec<WorkerRecord>, WorkerError> {
    let path = config_path().map_err(|error| WorkerError::msg(error.to_string()))?;
    if !path.is_file() {
        return Ok(Vec::new());
    }
    let text = std::fs::read_to_string(&path)?;
    parse_workers(&text)
}

pub fn parse_workers(text: &str) -> Result<Vec<WorkerRecord>, WorkerError> {
    let document = text
        .parse::<DocumentMut>()
        .map_err(|error| WorkerError::msg(format!("invalid config.toml: {error}")))?;
    let Some(array) = document.get("workers").and_then(Item::as_array_of_tables) else {
        return Ok(Vec::new());
    };
    Ok(array.iter().map(parse_worker_table).collect())
}

pub fn parse_worker_table(table: &Table) -> WorkerRecord {
    let id = table
        .get("id")
        .and_then(Item::as_str)
        .unwrap_or("")
        .to_string();
    let name = table
        .get("name")
        .and_then(Item::as_str)
        .unwrap_or("")
        .to_string();
    let worker_dirs = table
        .get("worker_dirs")
        .and_then(Item::as_array)
        .map(|array| {
            array
                .iter()
                .filter_map(toml_edit::Value::as_str)
                .map(str::to_string)
                .collect()
        })
        .unwrap_or_default();
    WorkerRecord {
        id,
        name,
        enabled: bool_field(table, "enabled", true),
        autostart: bool_field(table, "autostart", true),
        worker_dirs,
        computer_use: bool_field(table, "computer_use", false),
        display: table
            .get("display")
            .and_then(Item::as_str)
            .unwrap_or("")
            .to_string(),
        connect_workspace: bool_field(table, "connect_workspace", false),
        identity_socket: bool_field(table, "identity_socket", false),
        share_desktop: bool_field(table, "share_desktop", false),
        share_desktop_mode: match table.get("share_desktop_mode").and_then(Item::as_str) {
            Some(SHARE_DESKTOP_VIEW_AND_CONTROL) => SHARE_DESKTOP_VIEW_AND_CONTROL.to_string(),
            _ => SHARE_DESKTOP_VIEW.to_string(),
        },
        management_port: table
            .get("management_port")
            .and_then(Item::as_value)
            .and_then(toml_edit::Value::as_integer)
            .and_then(|port| u16::try_from(port).ok())
            .filter(|port| *port >= 1024)
            .unwrap_or(DEFAULT_MANAGEMENT_PORT),
        idle_release_timeout: table
            .get("idle_release_timeout")
            .and_then(Item::as_value)
            .and_then(toml_edit::Value::as_integer)
            .and_then(|value| u32::try_from(value).ok())
            .unwrap_or(DEFAULT_IDLE_RELEASE_TIMEOUT),
    }
}

fn bool_field(table: &Table, key: &str, default: bool) -> bool {
    table
        .get(key)
        .and_then(Item::as_value)
        .and_then(toml_edit::Value::as_bool)
        .unwrap_or(default)
}

/// Update the first `[[workers]]` table (create it if needed). Extra tables stay.
/// Only name and folders are overwritten on an existing table so hand-edited
/// advanced flags survive. New tables get DESIGN.md defaults. No pool keys.
pub fn apply_primary_worker(
    document: &mut DocumentMut,
    name: &str,
    dirs: &[String],
) -> Result<WorkerRecord, WorkerError> {
    let name = validate_name(name)?;
    let dirs = validate_worker_dirs(dirs)?;
    if document.get("schema_version").is_none() {
        document["schema_version"] = value(1);
    }
    if document.get("workers").and_then(Item::as_array_of_tables).is_none() {
        document["workers"] = Item::ArrayOfTables(ArrayOfTables::new());
    }
    let Some(array) = document["workers"].as_array_of_tables_mut() else {
        return Err(WorkerError::msg("config.toml workers is not an array of tables"));
    };
    for (index, table) in array.iter().enumerate().skip(1) {
        let other = parse_worker_table(table);
        if other.name == name {
            return Err(WorkerError::msg(format!(
                "Another [[workers]] entry already uses the name {name} (index {index}). This build edits one worker."
            )));
        }
    }
    let creating = array.is_empty();
    if creating {
        array.push(Table::new());
    }
    let table = array
        .get_mut(0)
        .ok_or_else(|| WorkerError::msg("failed to create [[workers]][0]"))?;
    let mut record = parse_worker_table(table);
    if record.id.is_empty() || uuid::Uuid::parse_str(&record.id).is_err() {
        record.id = uuid::Uuid::new_v4().to_string();
    }
    record.name = name;
    record.worker_dirs = dirs;
    if creating {
        record = WorkerRecord::defaults(record.id.clone(), record.name.clone(), record.worker_dirs.clone());
        write_full_defaults(table, &record);
    } else {
        table["id"] = value(record.id.as_str());
        table["name"] = value(record.name.as_str());
        table["worker_dirs"] = Item::Value(toml_edit::Value::Array(dirs_array(&record.worker_dirs)));
    }
    let _ = worker_data_dir(&record.id)?;
    Ok(record)
}

fn write_full_defaults(table: &mut Table, record: &WorkerRecord) {
    table["id"] = value(record.id.as_str());
    table["name"] = value(record.name.as_str());
    table["enabled"] = value(record.enabled);
    table["autostart"] = value(record.autostart);
    table["worker_dirs"] = Item::Value(toml_edit::Value::Array(dirs_array(&record.worker_dirs)));
    table["computer_use"] = value(record.computer_use);
    table["display"] = value(record.display.as_str());
    table["connect_workspace"] = value(record.connect_workspace);
    table["identity_socket"] = value(record.identity_socket);
    table["share_desktop"] = value(record.share_desktop);
    table["share_desktop_mode"] = value(record.share_desktop_mode.as_str());
    table["management_port"] = value(i64::from(record.management_port));
    table["idle_release_timeout"] = value(i64::from(record.idle_release_timeout));
}

fn dirs_array(dirs: &[String]) -> Array {
    let mut array = Array::new();
    for dir in dirs {
        array.push(dir.as_str());
    }
    array
}

pub fn save_autostart(autostart: bool) -> Result<WorkerRecord, WorkerError> {
    let path = config_path().map_err(|error| WorkerError::msg(error.to_string()))?;
    if !path.is_file() {
        return Err(WorkerError::msg("Save the personal worker first."));
    }
    let mut document = std::fs::read_to_string(&path)?
        .parse::<DocumentMut>()
        .map_err(|error| WorkerError::msg(format!("invalid config.toml: {error}")))?;
    let Some(array) = document
        .get_mut("workers")
        .and_then(Item::as_array_of_tables_mut)
    else {
        return Err(WorkerError::msg("Save the personal worker first."));
    };
    let Some(table) = array.get_mut(0) else {
        return Err(WorkerError::msg("Save the personal worker first."));
    };
    table["autostart"] = value(autostart);
    let record = parse_worker_table(table);
    crate::privacy::write_private_file(&path, document.to_string())?;
    Ok(record)
}

pub fn save_primary_worker(name: &str, dirs: &[String]) -> Result<WorkerRecord, WorkerError> {
    let path = config_path().map_err(|error| WorkerError::msg(error.to_string()))?;
    if let Some(parent) = path.parent() {
        crate::privacy::ensure_private_dir(parent)?;
    }
    let mut document = if path.is_file() {
        std::fs::read_to_string(&path)?
            .parse::<DocumentMut>()
            .map_err(|error| WorkerError::msg(format!("invalid config.toml: {error}")))?
    } else {
        DocumentMut::new()
    };
    let record = apply_primary_worker(&mut document, name, dirs)?;
    crate::privacy::write_private_file(&path, document.to_string())?;
    Ok(record)
}

pub fn default_suggested_name() -> String {
    suggested_name()
}

pub fn describe_dir(path: &str) -> (bool, bool) {
    dir_status(path)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    fn sample_dir() -> (tempfile::TempDir, String) {
        let dir = tempdir().unwrap();
        let path = dir.path().canonicalize().unwrap().to_string_lossy().to_string();
        (dir, path)
    }

    #[test]
    fn parses_design_worker() {
        let parsed = parse_workers(
            r#"
[[workers]]
id = "8f3c2a10-2b1e-4d3a-9c0a-1b2c3d4e5f60"
name = "dev-laptop"
enabled = true
autostart = true
worker_dirs = ["/home/dev/src/app", "/home/dev/src/infra"]
computer_use = false
display = ""
connect_workspace = false
identity_socket = false
share_desktop = false
share_desktop_mode = "view"
management_port = 18741
idle_release_timeout = 0
"#,
        )
        .unwrap();
        assert_eq!(parsed.len(), 1);
        assert_eq!(parsed[0].name, "dev-laptop");
        assert_eq!(parsed[0].worker_dirs.len(), 2);
        assert_eq!(parsed[0].management_port, 18741);
        assert_eq!(parsed[0].idle_release_timeout, 0);
        assert!(!parsed[0].computer_use);
    }

    #[test]
    fn create_writes_defaults_and_no_pool_keys() {
        let (_keep, path) = sample_dir();
        let mut document = "schema_version = 1\n\n[cli]\npath = \"/tmp/agent\"\n\n[auth]\nmethod = \"cli_login\"\n"
            .parse::<DocumentMut>()
            .unwrap();
        let record = apply_primary_worker(&mut document, "dev-laptop", &[path]).unwrap();
        uuid::Uuid::parse_str(&record.id).unwrap();
        let text = document.to_string();
        assert!(text.contains("name = \"dev-laptop\""));
        assert!(text.contains("idle_release_timeout = 0"));
        assert!(text.contains("management_port = 18741"));
        assert!(text.contains("computer_use = false"));
        assert!(text.contains("path = \"/tmp/agent\""));
        assert!(text.contains("method = \"cli_login\""));
        assert!(!text.contains("pool"));
        assert!(!text.contains("mint-github"));
        assert!(!text.contains("service_account"));
    }

    #[test]
    fn update_preserves_id_advanced_flags_and_extra_worker() {
        let (_keep, path) = sample_dir();
        let original = r#"schema_version = 1

[[workers]]
id = "8f3c2a10-2b1e-4d3a-9c0a-1b2c3d4e5f60"
name = "old-name"
worker_dirs = ["/old"]
computer_use = true
future_flag = "keep-me"
management_port = 19000

[[workers]]
id = "aaaaaaaa-bbbb-4ccc-8ddd-eeeeeeeeeeee"
name = "other-machine"
worker_dirs = ["/other"]
"#;
        let mut document = original.parse::<DocumentMut>().unwrap();
        let record = apply_primary_worker(&mut document, "new-name", &[path]).unwrap();
        assert_eq!(record.id, "8f3c2a10-2b1e-4d3a-9c0a-1b2c3d4e5f60");
        assert_eq!(record.name, "new-name");
        let text = document.to_string();
        assert!(text.contains("computer_use = true"));
        assert!(text.contains("future_flag = \"keep-me\""));
        assert!(text.contains("management_port = 19000"));
        assert!(text.contains("name = \"other-machine\""));
        assert!(text.contains("name = \"new-name\""));
    }

    #[test]
    fn save_autostart_preserves_name_and_flags() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("config.toml");
        std::fs::write(
            &path,
            r#"
[[workers]]
id = "8f3c2a10-2b1e-4d3a-9c0a-1b2c3d4e5f60"
name = "dev-laptop"
autostart = true
computer_use = true
"#,
        )
        .unwrap();
        let mut document = std::fs::read_to_string(&path)
            .unwrap()
            .parse::<DocumentMut>()
            .unwrap();
        document["workers"]
            .as_array_of_tables_mut()
            .unwrap()
            .get_mut(0)
            .unwrap()["autostart"] = value(false);
        let text = document.to_string();
        assert!(text.contains("autostart = false"));
        assert!(text.contains("name = \"dev-laptop\""));
        assert!(text.contains("computer_use = true"));
    }

    #[test]
    fn rejects_name_collision_with_extra_worker() {
        let (_keep, path) = sample_dir();
        let original = r#"
[[workers]]
id = "8f3c2a10-2b1e-4d3a-9c0a-1b2c3d4e5f60"
name = "one"
worker_dirs = ["/old"]

[[workers]]
id = "aaaaaaaa-bbbb-4ccc-8ddd-eeeeeeeeeeee"
name = "taken"
worker_dirs = ["/other"]
"#;
        let mut document = original.parse::<DocumentMut>().unwrap();
        assert!(apply_primary_worker(&mut document, "taken", &[path]).is_err());
    }
}
