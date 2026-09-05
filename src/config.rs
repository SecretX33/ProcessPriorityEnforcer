use crate::log;
use crate::process::{CpuPriority, IoPriority, PowerQos};
use crate::util::normalize_path;
use color_eyre::eyre::{Result, bail};
use globset::{GlobBuilder, GlobSet};
use serde::Deserialize;
use std::path::Path;

#[derive(Debug, Clone, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AppConfig {
    pub groups: Vec<AppGroupConfig>,
}

#[derive(Debug, Clone, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AppGroupConfig {
    #[serde(deserialize_with = "deserialize_globset")]
    pub paths: GlobSet,
    pub priorities: AppPriorityConfig,
}

#[derive(Debug, Clone, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AppPriorityConfig {
    #[serde(default, deserialize_with = "deserialize_optional_case_insensitive")]
    pub cpu: Option<CpuPriority>,
    #[serde(default, deserialize_with = "deserialize_optional_case_insensitive")]
    pub io: Option<IoPriority>,
    #[serde(default, deserialize_with = "deserialize_optional_case_insensitive")]
    pub power: Option<PowerQos>,
}

trait CaseInsensitiveConfigValue: Sized {
    const VARIANTS: &'static [&'static str];

    fn from_config_str(value: &str) -> Option<Self>;
}

fn deserialize_optional_case_insensitive<'de, D, T>(
    deserializer: D,
) -> core::result::Result<Option<T>, D::Error>
where
    D: serde::Deserializer<'de>,
    T: CaseInsensitiveConfigValue,
{
    Option::<String>::deserialize(deserializer)?
        .map(|value| {
            T::from_config_str(&value)
                .ok_or_else(|| <D::Error as serde::de::Error>::unknown_variant(&value, T::VARIANTS))
        })
        .transpose()
}

impl CaseInsensitiveConfigValue for CpuPriority {
    const VARIANTS: &'static [&'static str] =
        &["VeryLow", "Low", "Normal", "High", "VeryHigh"];

    fn from_config_str(value: &str) -> Option<Self> {
        match value {
            value if value.eq_ignore_ascii_case("VeryLow") => Some(Self::VeryLow),
            value if value.eq_ignore_ascii_case("Low") => Some(Self::Low),
            value if value.eq_ignore_ascii_case("Normal") => Some(Self::Normal),
            value if value.eq_ignore_ascii_case("High") => Some(Self::High),
            value if value.eq_ignore_ascii_case("VeryHigh") => Some(Self::VeryHigh),
            _ => None,
        }
    }
}

impl CaseInsensitiveConfigValue for IoPriority {
    const VARIANTS: &'static [&'static str] = &["VeryLow", "Low", "Normal"];

    fn from_config_str(value: &str) -> Option<Self> {
        match value {
            value if value.eq_ignore_ascii_case("VeryLow") => Some(Self::VeryLow),
            value if value.eq_ignore_ascii_case("Low") => Some(Self::Low),
            value if value.eq_ignore_ascii_case("Normal") => Some(Self::Normal),
            _ => None,
        }
    }
}

impl CaseInsensitiveConfigValue for PowerQos {
    const VARIANTS: &'static [&'static str] = &["SystemManaged", "Eco", "High"];

    fn from_config_str(value: &str) -> Option<Self> {
        match value {
            value if value.eq_ignore_ascii_case("SystemManaged") => Some(Self::SystemManaged),
            value if value.eq_ignore_ascii_case("Eco") => Some(Self::Eco),
            value if value.eq_ignore_ascii_case("High") => Some(Self::High),
            _ => None,
        }
    }
}

fn deserialize_globset<'de, D>(deserializer: D) -> core::result::Result<GlobSet, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let patterns = Vec::<String>::deserialize(deserializer)?;
    let normalized_patterns = patterns
        .iter()
        .map(|p| {
            GlobBuilder::new(&normalize_path(p))
                .literal_separator(true)
                .build()
                .map_err(|e| e.into())
        })
        .collect::<Result<Vec<_>>>()
        .map_err(serde::de::Error::custom)?;

    GlobSet::new(normalized_patterns).map_err(serde::de::Error::custom)
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
