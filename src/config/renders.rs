use serde::Deserialize;

/// Wrapper for the full renders.toml file.
#[derive(Debug, Deserialize)]
pub struct RendersFile {
    pub renders: Vec<RenderConfig>,
}

/// A single render definition.
#[derive(Debug, Deserialize, Clone)]
pub struct RenderConfig {
    pub target: String,
    pub template: String,
    pub trigger: String,
    pub event_types: Option<Vec<String>>,
}
