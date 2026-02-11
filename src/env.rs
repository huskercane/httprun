use std::collections::HashMap;
use std::path::Path;

use crate::error::AppError;

/// Load environment variables from http-client.env.json and optional private override file.
/// Returns a flat HashMap<String, String> for the selected environment.
pub fn load_environment(
    env_file: &Path,
    env_name: &str,
) -> Result<HashMap<String, String>, AppError> {
    let mut vars = HashMap::new();

    // Load public env file
    if env_file.exists() {
        let content = std::fs::read_to_string(env_file).map_err(AppError::Io)?;
        let all_envs: HashMap<String, HashMap<String, serde_json::Value>> =
            serde_json::from_str(&content).map_err(|e| {
                AppError::Environment(format!("Failed to parse {}: {}", env_file.display(), e))
            })?;

        if let Some(env) = all_envs.get(env_name) {
            for (key, value) in env {
                vars.insert(key.clone(), value_to_string(value));
            }
        } else {
            let available: Vec<&String> = all_envs.keys().collect();
            return Err(AppError::Environment(format!(
                "Environment '{}' not found. Available: {:?}",
                env_name, available
            )));
        }
    } else {
        return Err(AppError::Environment(format!(
            "Environment file not found: {}",
            env_file.display()
        )));
    }

    // Load private env file (override) if it exists
    let private_file = private_env_path(env_file);
    if private_file.exists() {
        let content = std::fs::read_to_string(&private_file).map_err(AppError::Io)?;
        let all_envs: HashMap<String, HashMap<String, serde_json::Value>> =
            serde_json::from_str(&content).map_err(|e| {
                AppError::Environment(format!(
                    "Failed to parse {}: {}",
                    private_file.display(),
                    e
                ))
            })?;

        if let Some(env) = all_envs.get(env_name) {
            for (key, value) in env {
                vars.insert(key.clone(), value_to_string(value));
            }
        }
    }

    Ok(vars)
}

/// List available environment names from the env file.
#[allow(dead_code)]
pub fn list_environments(env_file: &Path) -> Result<Vec<String>, AppError> {
    if !env_file.exists() {
        return Ok(Vec::new());
    }

    let content = std::fs::read_to_string(env_file).map_err(AppError::Io)?;
    let all_envs: HashMap<String, serde_json::Value> =
        serde_json::from_str(&content).map_err(|e| {
            AppError::Environment(format!("Failed to parse {}: {}", env_file.display(), e))
        })?;

    let mut names: Vec<String> = all_envs.keys().cloned().collect();
    names.sort();
    Ok(names)
}

fn private_env_path(env_file: &Path) -> std::path::PathBuf {
    let stem = env_file
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("http-client.env");
    let ext = env_file
        .extension()
        .and_then(|s| s.to_str())
        .unwrap_or("json");

    // http-client.env.json -> http-client.private.env.json
    // Handle double extension: if stem ends with ".env", insert ".private" before ".env"
    let private_name = if stem.ends_with(".env") {
        let base = &stem[..stem.len() - 4];
        format!("{}.private.env.{}", base, ext)
    } else {
        format!("{}.private.{}", stem, ext)
    };

    env_file.with_file_name(private_name)
}

fn value_to_string(value: &serde_json::Value) -> String {
    match value {
        serde_json::Value::String(s) => s.clone(),
        serde_json::Value::Number(n) => n.to_string(),
        serde_json::Value::Bool(b) => b.to_string(),
        serde_json::Value::Null => String::new(),
        other => other.to_string(),
    }
}
