use crate::log;
use crate::util::normalize_path;
use color_eyre::eyre::{Result, bail};
use globset::{GlobBuilder, GlobSet};
use serde::Deserialize;
use std::path::Path;

#[derive(Debug, Clone, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AppConfig {
    #[serde(deserialize_with = "deserialize_globset")]
    pub paths: GlobSet
}

fn deserialize_globset<'de, D>(
    deserializer: D,
) -> core::result::Result<GlobSet, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let patterns = Vec::<String>::deserialize(deserializer)?;
    let normalized_patterns = patterns.iter()
        .map(|p| {
            GlobBuilder::new(&normalize_path(p))
                .literal_separator(true)
                .build()
                .map_err(|e| e.into())
        })
        .collect::<Result<Vec<_>>>()
        .map_err(serde::de::Error::custom)?;

    GlobSet::new(normalized_patterns)
        .map_err(serde::de::Error::custom)
}

pub fn read_app_config(config_path: &Path) -> Result<AppConfig> {
    let result = std::fs::read_to_string(config_path);
    let Some(file_contents) = result
        .as_ref()
        .ok()
        .filter(|contents| !contents.trim().is_empty())
    else {
        match result {
            Ok(_) => bail!("Config file is empty, please fill it with valid JSON"),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => bail!("Config file not found"),
            Err(e) => bail!("Error reading config file: {}", e),
        };
    };

    let parsed_config = match serde_json::from_str::<AppConfig>(file_contents) {
        Ok(config) => config,
        Err(e) => bail!("Error parsing config file: {}", e),
    };
    log!("Config loaded successfully");
    Ok(parsed_config)
}
