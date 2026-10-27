use toml_edit::{DocumentMut, Item, value};

use crate::cli::error::CliError;
use crate::cli::paths::config_path;

pub const METHOD_CLI_LOGIN: &str = "cli_login";
pub const METHOD_API_KEY: &str = "api_key";

/// Read `[auth].method` from config.toml. Unknown values are ignored so a
/// future method does not break this build. Secrets are never stored here.
pub fn load_auth_method() -> Result<Option<String>, CliError> {
    let path = config_path()?;
    if !path.is_file() {
        return Ok(None);
    }
    let text = std::fs::read_to_string(&path)?;
    parse_auth_method(&text)
}

pub fn parse_auth_method(text: &str) -> Result<Option<String>, CliError> {
    let document = text
        .parse::<DocumentMut>()
        .map_err(|error| CliError::msg(format!("invalid config.toml: {error}")))?;
    let Some(method) = document
        .get("auth")
        .and_then(Item::as_table)
        .and_then(|table| table.get("method"))
        .and_then(Item::as_str)
    else {
        return Ok(None);
    };
    match method {
        METHOD_CLI_LOGIN | METHOD_API_KEY => Ok(Some(method.to_string())),
        _ => Ok(None),
    }
}

pub fn save_auth_method(method: &str) -> Result<(), CliError> {
    if method != METHOD_CLI_LOGIN && method != METHOD_API_KEY {
        return Err(CliError::msg(format!(
            "unsupported [auth].method {method}; use {METHOD_CLI_LOGIN} or {METHOD_API_KEY}"
        )));
    }
    let path = config_path()?;
    if let Some(parent) = path.parent() {
        crate::privacy::ensure_private_dir(parent)?;
    }
    let mut document = if path.is_file() {
        std::fs::read_to_string(&path)?
            .parse::<DocumentMut>()
            .map_err(|error| CliError::msg(format!("invalid config.toml: {error}")))?
    } else {
        DocumentMut::new()
    };
    if document.get("schema_version").is_none() {
        document["schema_version"] = value(1);
    }
    if document.get("auth").and_then(Item::as_table).is_none() {
        document["auth"] = Item::Table(toml_edit::Table::new());
    }
    let Some(auth_table) = document["auth"].as_table_mut() else {
        return Err(CliError::msg("config.toml [auth] is not a table"));
    };
    auth_table["method"] = value(method);
    crate::privacy::write_private_file(&path, document.to_string())?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_known_methods() {
        assert_eq!(
            parse_auth_method("[auth]\nmethod = \"cli_login\"\n").unwrap(),
            Some(METHOD_CLI_LOGIN.to_string())
        );
        assert_eq!(
            parse_auth_method("[auth]\nmethod = \"api_key\"\n").unwrap(),
            Some(METHOD_API_KEY.to_string())
        );
        assert_eq!(
            parse_auth_method("[auth]\nmethod = \"pool\"\n").unwrap(),
            None
        );
        assert_eq!(parse_auth_method("[cli]\npath = \"\"\n").unwrap(), None);
    }

    #[test]
    fn rewrite_preserves_cli_and_unknown_keys() {
        let original = r#"schema_version = 1

[cli]
path = "/tmp/agent"
future_flag = "keep-me"

[auth]
method = "cli_login"
extra = 1
"#;
        let mut document = original.parse::<DocumentMut>().unwrap();
        document["auth"]["method"] = value(METHOD_API_KEY);
        let rewritten = document.to_string();
        assert!(rewritten.contains("future_flag = \"keep-me\""));
        assert!(rewritten.contains("path = \"/tmp/agent\""));
        assert!(rewritten.contains("method = \"api_key\""));
        assert!(rewritten.contains("extra = 1"));
    }
}
