use std::collections::HashMap;

use regex::Regex;
use std::sync::LazyLock;

use crate::error::AppError;

static VARIABLE_RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\{\{([^}]+)\}\}").unwrap());

/// Globals set from JS handlers. Values keep their JSON type so `client.global.get`
/// round-trips the original type, while `{{var}}` substitution renders plain text.
pub type GlobalVars = HashMap<String, serde_json::Value>;

/// Render a global variable as the text pasted into a request.
/// Strings are inserted verbatim (no JSON quoting); whole numbers keep integer form.
pub fn render_global(value: &serde_json::Value) -> String {
    match value {
        serde_json::Value::String(s) => s.clone(),
        serde_json::Value::Null => "null".to_string(),
        serde_json::Value::Bool(b) => b.to_string(),
        serde_json::Value::Number(n) => match n.as_f64() {
            Some(f) if f.fract() == 0.0 && f.abs() < i64::MAX as f64 => format!("{}", f as i64),
            _ => n.to_string(),
        },
        other => other.to_string(),
    }
}

#[derive(Debug, Clone)]
pub struct VariableStore {
    env_vars: HashMap<String, String>,
    global_vars: GlobalVars,
    in_place_vars: HashMap<String, String>,
}

impl VariableStore {
    pub fn new(env_vars: HashMap<String, String>) -> Self {
        Self {
            env_vars,
            global_vars: GlobalVars::new(),
            in_place_vars: HashMap::new(),
        }
    }

    pub fn globals(&self) -> &GlobalVars {
        &self.global_vars
    }

    pub fn merge_globals(&mut self, globals: &GlobalVars) {
        for (k, v) in globals {
            self.global_vars.insert(k.clone(), v.clone());
        }
    }

    pub fn set_in_place(&mut self, name: String, value: String) {
        self.in_place_vars.insert(name, value);
    }

    /// Substitute all {{variable}} references in the input string.
    /// Precedence: in_place_vars > global_vars > env_vars
    pub fn substitute(&self, input: &str) -> Result<String, AppError> {
        let result = VARIABLE_RE
            .replace_all(input, |caps: &regex::Captures| {
                let var_name = caps[1].trim();

                // Dynamic variables
                if var_name == "$uuid" {
                    return uuid::Uuid::new_v4().to_string();
                }
                if var_name == "$timestamp" {
                    return std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .unwrap()
                        .as_secs()
                        .to_string();
                }
                if var_name == "$randomInt" {
                    return format!(
                        "{}",
                        std::time::SystemTime::now()
                            .duration_since(std::time::UNIX_EPOCH)
                            .unwrap()
                            .subsec_nanos()
                            % 1000
                    );
                }

                // Precedence: in-place > global > env
                if let Some(v) = self.in_place_vars.get(var_name) {
                    return v.clone();
                }
                if let Some(v) = self.global_vars.get(var_name) {
                    return render_global(v);
                }
                if let Some(v) = self.env_vars.get(var_name) {
                    return v.clone();
                }

                // Return original placeholder if not found
                caps[0].to_string()
            })
            .to_string();

        Ok(result)
    }
}
