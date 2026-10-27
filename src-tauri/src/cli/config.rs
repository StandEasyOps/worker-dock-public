use std::fs;
use std::path::Path;

use toml_edit::{DocumentMut, Item, value};

use crate::cli::error::CliError;
use crate::cli::paths::config_path;
use crate::cli::version::PINNED_VERSION;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CliConfig {
    pub path: String,
    pub install_method: String,
    pub pinned_version: String,
    pub disable_auto_update: bool,
}

impl Default for CliConfig {
    fn default() -> Self {
        Self {
            path: String::new(),
            install_method: "official_archive".to_string(),
            pinned_version: PINNED_VERSION.to_string(),
            disable_auto_update: true,
        }
    }
}

pub fn load() -> Result<CliConfig, CliError> {
    let path = config_path()?;
    if !path.is_file() {
        return Ok(CliConfig::default());
    }
    let text = fs::read_to_string(&path)?;
    parse_cli_table(&text)
}

pub fn save_cli_fields(cli: &CliConfig) -> Result<(), CliError> {
    let path = config_path()?;
    if let Some(parent) = path.parent() {
        crate::privacy::ensure_private_dir(parent)?;
    }
    let mut document = if path.is_file() {
        fs::read_to_string(&path)?.parse::<DocumentMut>().map_err(|error| {
            CliError::msg(format!("invalid config.toml: {error}"))
        })?
    } else {
        DocumentMut::new()
    };
    if document.get("schema_version").is_none() {
        document["schema_version"] = value(1);
    }
    if document.get("cli").and_then(Item::as_table).is_none() {
        document["cli"] = Item::Table(toml_edit::Table::new());
    }
    let Some(cli_table) = document["cli"].as_table_mut() else {
        return Err(CliError::msg("config.toml [cli] is not a table"));
    };
    cli_table["path"] = value(cli.path.as_str());
    cli_table["install_method"] = value(cli.install_method.as_str());
    cli_table["pinned_version"] = value(cli.pinned_version.as_str());
    cli_table["disable_auto_update"] = value(cli.disable_auto_update);
    crate::privacy::write_private_file(&path, document.to_string())?;
    Ok(())
}

pub fn parse_cli_table(text: &str) -> Result<CliConfig, CliError> {
    let document = text
        .parse::<DocumentMut>()
        .map_err(|error| CliError::msg(format!("invalid config.toml: {error}")))?;
    let mut config = CliConfig::default();
    if let Some(table) = document.get("cli").and_then(Item::as_table) {
        if let Some(path) = table.get("path").and_then(Item::as_str) {
            config.path = path.to_string();
        }
        if let Some(method) = table.get("install_method").and_then(Item::as_str) {
            config.install_method = method.to_string();
        }
        if let Some(pinned) = table.get("pinned_version").and_then(Item::as_str) {
            config.pinned_version = pinned.to_string();
        }
        if let Some(flag) = table
            .get("disable_auto_update")
            .and_then(Item::as_value)
            .and_then(toml_edit::Value::as_bool)
        {
            config.disable_auto_update = flag;
        }
    }
    Ok(config)
}

pub fn configured_binary(config: &CliConfig) -> Option<&Path> {
    let path = Path::new(config.path.trim());
    if config.path.trim().is_empty() {
        None
    } else {
        Some(path)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_pins_and_disables_auto_update() {
        let config = CliConfig::default();
        assert_eq!(config.pinned_version, PINNED_VERSION);
        assert!(config.disable_auto_update);
        assert_eq!(config.install_method, "official_archive");
    }

    #[test]
    fn preserves_unknown_keys_on_rewrite() {
        let original = r#"schema_version = 1

[app]
start_minimized = true

[cli]
path = ""
install_method = "manual"
future_flag = "keep-me"

[auth]
method = "cli_login"
"#;
        let mut document = original.parse::<DocumentMut>().unwrap();
        document["cli"]["path"] = value("/tmp/agent");
        document["cli"]["disable_auto_update"] = value(true);
        let rewritten = document.to_string();
        assert!(rewritten.contains("future_flag = \"keep-me\""));
        assert!(rewritten.contains("start_minimized = true"));
        assert!(rewritten.contains("method = \"cli_login\""));
        assert!(rewritten.contains("path = \"/tmp/agent\""));
    }

    #[test]
    fn parses_cli_section() {
        let parsed = parse_cli_table(
            r#"
[cli]
path = "/home/dev/.local/bin/agent"
install_method = "official_archive"
pinned_version = "2026.10.19-6bbf2c4"
disable_auto_update = true
"#,
        )
        .unwrap();
        assert_eq!(parsed.path, "/home/dev/.local/bin/agent");
        assert!(parsed.disable_auto_update);
    }
}
