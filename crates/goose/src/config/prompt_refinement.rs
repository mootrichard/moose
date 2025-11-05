use std::str::FromStr;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PromptRefinementMode {
    Disabled,
    PersistOnly,
    Enabled,
}

impl PromptRefinementMode {
    pub fn as_str(&self) -> &'static str {
        match self {
            PromptRefinementMode::Disabled => "disabled",
            PromptRefinementMode::PersistOnly => "persist-only",
            PromptRefinementMode::Enabled => "enabled",
        }
    }
}

impl FromStr for PromptRefinementMode {
    type Err = &'static str;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.trim().to_lowercase().as_str() {
            "enabled" => Ok(PromptRefinementMode::Enabled),
            "persist-only" | "persist_only" => Ok(PromptRefinementMode::PersistOnly),
            "disabled" => Ok(PromptRefinementMode::Disabled),
            _ => Err("invalid prompt refinement mode"),
        }
    }
}
