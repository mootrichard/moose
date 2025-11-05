#[cfg(test)]
use chrono::DateTime;
use chrono::Utc;
use serde::Serialize;
use serde_json::Value;
use std::collections::HashMap;

use crate::agents::extension::ExtensionInfo;
use crate::agents::prompt_instruction::{
    InstructionId, InstructionScope, InstructionSource, InstructionState, PromptInstruction,
    PromptStateSnapshot,
};
use crate::agents::recipe_tools::dynamic_task_tools::should_enabled_subagents;
use crate::agents::router_tools::llm_search_tool_prompt;
use crate::{
    config::{Config, GooseMode},
    prompt_template,
    utils::sanitize_unicode_tags,
};

const MAX_EXTENSIONS: usize = 5;
const MAX_TOOLS: usize = 50;

pub struct PromptManager {
    system_prompt_override: Option<String>,
    current_date_timestamp: String,
    instructions: HashMap<InstructionId, PromptInstruction>,
    applied_order: Vec<InstructionId>,
    source_index: HashMap<String, InstructionId>,
    order_counter: u64,
}

impl Default for PromptManager {
    fn default() -> Self {
        PromptManager::new()
    }
}

#[derive(Serialize)]
struct SystemPromptContext {
    extensions: Vec<ExtensionInfo>,
    #[serde(skip_serializing_if = "Option::is_none")]
    tool_selection_strategy: Option<String>,
    current_date_time: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    extension_tool_limits: Option<(usize, usize)>,
    goose_mode: GooseMode,
    is_autonomous: bool,
    enable_subagents: bool,
    max_extensions: usize,
    max_tools: usize,
}

pub struct SystemPromptBuilder<'a, M> {
    model_name: String,
    manager: &'a M,

    extensions_info: Vec<ExtensionInfo>,
    frontend_instructions: Option<String>,
    extension_tool_count: Option<(usize, usize)>,
    router_enabled: bool,
}

impl<'a> SystemPromptBuilder<'a, PromptManager> {
    pub fn with_extension(mut self, extension: ExtensionInfo) -> Self {
        self.extensions_info.push(extension);
        self
    }

    pub fn with_extensions(mut self, extensions: impl Iterator<Item = ExtensionInfo>) -> Self {
        for extension in extensions {
            self.extensions_info.push(extension);
        }
        self
    }

    pub fn with_frontend_instructions(mut self, frontend_instructions: Option<String>) -> Self {
        self.frontend_instructions = frontend_instructions;
        self
    }

    pub fn with_extension_and_tool_counts(
        mut self,
        extension_count: usize,
        tool_count: usize,
    ) -> Self {
        self.extension_tool_count = Some((extension_count, tool_count));
        self
    }

    pub fn with_router_enabled(mut self, enabled: bool) -> Self {
        self.router_enabled = enabled;
        self
    }

    pub fn build(self) -> String {
        let mut extensions_info = self.extensions_info;

        // Add frontend instructions to extensions_info to simplify json rendering
        if let Some(frontend_instructions) = self.frontend_instructions {
            extensions_info.push(ExtensionInfo::new(
                "frontend",
                &frontend_instructions,
                false,
            ));
        }
        // Stable tool ordering is important for multi session prompt caching.
        extensions_info.sort_by(|a, b| a.name.cmp(&b.name));

        let sanitized_extensions_info: Vec<ExtensionInfo> = extensions_info
            .into_iter()
            .map(|mut ext_info| {
                ext_info.instructions = sanitize_unicode_tags(&ext_info.instructions);
                ext_info
            })
            .collect();

        let config = Config::global();
        let goose_mode = config.get_goose_mode().unwrap_or(GooseMode::Auto);

        let extension_tool_limits = self
            .extension_tool_count
            .filter(|(extensions, tools)| *extensions > MAX_EXTENSIONS || *tools > MAX_TOOLS);

        let context = SystemPromptContext {
            extensions: sanitized_extensions_info,
            tool_selection_strategy: self.router_enabled.then(llm_search_tool_prompt),
            current_date_time: self.manager.current_date_timestamp.clone(),
            extension_tool_limits,
            goose_mode,
            is_autonomous: goose_mode == GooseMode::Auto,
            enable_subagents: should_enabled_subagents(self.model_name.as_str()),
            max_extensions: MAX_EXTENSIONS,
            max_tools: MAX_TOOLS,
        };

        let base_prompt = if let Some(override_prompt) = &self.manager.system_prompt_override {
            let sanitized_override_prompt = sanitize_unicode_tags(override_prompt);
            prompt_template::render_inline_once(&sanitized_override_prompt, &context)
        } else {
            prompt_template::render_global_file("system.md", &context)
        }
        .unwrap_or_else(|_| {
            "You are a general-purpose AI agent called goose, created by Block".to_string()
        });

        let mut system_prompt_extras = self.manager.active_instruction_texts();
        if goose_mode == GooseMode::Chat {
            system_prompt_extras.push(
                "Right now you are in the chat only mode, no access to any tool use and system."
                    .to_string(),
            );
        }

        let sanitized_system_prompt_extras: Vec<String> = system_prompt_extras
            .into_iter()
            .map(|extra| sanitize_unicode_tags(&extra))
            .collect();

        if sanitized_system_prompt_extras.is_empty() {
            base_prompt
        } else {
            format!(
                "{}\n\n# Additional Instructions:\n\n{}",
                base_prompt,
                sanitized_system_prompt_extras.join("\n\n")
            )
        }
    }
}

impl PromptManager {
    pub fn new() -> Self {
        PromptManager {
            system_prompt_override: None,
            // Use the fixed current date time so that prompt cache can be used.
            // Filtering to an hour to balance user time accuracy and multi session prompt cache hits.
            current_date_timestamp: Utc::now().format("%Y-%m-%d %H:00").to_string(),
            instructions: HashMap::new(),
            applied_order: Vec::new(),
            source_index: HashMap::new(),
            order_counter: 0,
        }
    }

    #[cfg(test)]
    pub fn with_timestamp(dt: DateTime<Utc>) -> Self {
        PromptManager {
            system_prompt_override: None,
            current_date_timestamp: dt.format("%Y-%m-%d %H:%M:%S").to_string(),
            instructions: HashMap::new(),
            applied_order: Vec::new(),
            source_index: HashMap::new(),
            order_counter: 0,
        }
    }

    /// Add an additional instruction to the system prompt
    pub fn add_system_prompt_extra(&mut self, instruction: String) {
        let _ = self.add_instruction_with_metadata(
            instruction,
            InstructionSource::Unknown,
            InstructionScope::Session,
        );
    }

    pub fn add_instruction_with_metadata(
        &mut self,
        instruction: String,
        source: InstructionSource,
        scope: InstructionScope,
    ) -> InstructionId {
        self.upsert_instruction(instruction, source, scope)
    }

    /// Override the system prompt with custom text
    pub fn set_system_prompt_override(&mut self, template: String) {
        self.system_prompt_override = Some(template);
    }

    pub fn instruction_stack(&self) -> Vec<&PromptInstruction> {
        self.applied_order
            .iter()
            .filter_map(|id| self.instructions.get(id))
            .filter(|instruction| instruction.state == InstructionState::Active)
            .collect()
    }

    pub fn active_instruction_texts(&self) -> Vec<String> {
        self.instruction_stack()
            .into_iter()
            .map(|instruction| instruction.content.clone())
            .collect()
    }

    pub fn retire_instruction_by_source(
        &mut self,
        source: &InstructionSource,
    ) -> Option<InstructionId> {
        let key = source.key()?;
        let instruction_id = self.source_index.remove(&key)?;
        self.retire_instruction(&instruction_id)
    }

    pub fn retire_instruction(&mut self, instruction_id: &InstructionId) -> Option<InstructionId> {
        if let Some(instruction) = self.instructions.get_mut(instruction_id) {
            instruction.state = InstructionState::Retired;
            instruction.updated_at = Utc::now();
            return Some(instruction_id.clone());
        }
        None
    }

    pub fn get_instruction(&self, instruction_id: &InstructionId) -> Option<&PromptInstruction> {
        self.instructions.get(instruction_id)
    }

    pub fn snapshot_state(&self) -> PromptStateSnapshot {
        PromptStateSnapshot {
            override_prompt: self.system_prompt_override.clone(),
            instructions: self.instructions.values().cloned().collect(),
            applied_order: self.applied_order.clone(),
            current_date_timestamp: self.current_date_timestamp.clone(),
            order_counter: self.order_counter,
        }
    }

    pub fn restore_from_snapshot(&mut self, snapshot: PromptStateSnapshot) {
        self.system_prompt_override = snapshot.override_prompt;
        self.current_date_timestamp = snapshot.current_date_timestamp;
        self.order_counter = snapshot.order_counter;

        self.instructions = snapshot
            .instructions
            .into_iter()
            .map(|instruction| (instruction.id.clone(), instruction))
            .collect();

        self.applied_order = snapshot.applied_order;
        self.source_index.clear();

        for instruction in self.instructions.values() {
            if let Some(key) = instruction.source.key() {
                self.source_index.insert(key, instruction.id.clone());
            }
        }
    }

    fn next_order(&mut self) -> u64 {
        self.order_counter += 1;
        self.order_counter
    }

    fn upsert_instruction(
        &mut self,
        instruction: String,
        source: InstructionSource,
        scope: InstructionScope,
    ) -> InstructionId {
        let key = source.key();
        if let Some(existing_id) = key.as_ref().and_then(|k| self.source_index.get(k)) {
            if let Some(existing_instruction) = self.instructions.get_mut(existing_id) {
                existing_instruction.scope = scope;
                existing_instruction.update_content(instruction);
                return existing_id.clone();
            }
        }

        let id = InstructionId::new_random();
        let now = Utc::now();
        let order = self.next_order();
        let prompt_instruction =
            PromptInstruction::new(id.clone(), source.clone(), scope, instruction, order, now);

        if let Some(source_key) = key {
            self.source_index.insert(source_key, id.clone());
        }

        self.applied_order.push(id.clone());
        self.instructions.insert(id.clone(), prompt_instruction);

        id
    }

    pub fn builder<'a>(&'a self, model_name: &str) -> SystemPromptBuilder<'a, Self> {
        SystemPromptBuilder {
            model_name: model_name.to_string(),
            manager: self,

            extensions_info: vec![],
            frontend_instructions: None,
            extension_tool_count: None,
            router_enabled: false,
        }
    }

    pub async fn get_recipe_prompt(&self) -> String {
        let context: HashMap<&str, Value> = HashMap::new();
        prompt_template::render_global_file("recipe.md", &context)
            .unwrap_or_else(|_| "The recipe prompt is busted. Tell the user.".to_string())
    }
}

#[cfg(test)]
mod tests {
    use insta::assert_snapshot;

    use super::*;

    #[test]
    fn test_build_system_prompt_sanitizes_override() {
        let mut manager = PromptManager::new();
        let malicious_override = "System prompt\u{E0041}\u{E0042}\u{E0043}with hidden text";
        manager.set_system_prompt_override(malicious_override.to_string());

        let result = manager.builder("gpt-4o").build();

        assert!(!result.contains('\u{E0041}'));
        assert!(!result.contains('\u{E0042}'));
        assert!(!result.contains('\u{E0043}'));
        assert!(result.contains("System prompt"));
        assert!(result.contains("with hidden text"));
    }

    #[test]
    fn test_build_system_prompt_sanitizes_extras() {
        let mut manager = PromptManager::new();
        let malicious_extra = "Extra instruction\u{E0041}\u{E0042}\u{E0043}hidden";
        manager.add_system_prompt_extra(malicious_extra.to_string());

        let result = manager.builder("gpt-4o").build();

        assert!(!result.contains('\u{E0041}'));
        assert!(!result.contains('\u{E0042}'));
        assert!(!result.contains('\u{E0043}'));
        assert!(result.contains("Extra instruction"));
        assert!(result.contains("hidden"));
    }

    #[test]
    fn test_build_system_prompt_sanitizes_multiple_extras() {
        let mut manager = PromptManager::new();
        manager.add_system_prompt_extra("First\u{E0041}instruction".to_string());
        manager.add_system_prompt_extra("Second\u{E0042}instruction".to_string());
        manager.add_system_prompt_extra("Third\u{E0043}instruction".to_string());

        let result = manager.builder("gpt-4o").build();

        assert!(!result.contains('\u{E0041}'));
        assert!(!result.contains('\u{E0042}'));
        assert!(!result.contains('\u{E0043}'));
        assert!(result.contains("Firstinstruction"));
        assert!(result.contains("Secondinstruction"));
        assert!(result.contains("Thirdinstruction"));
    }

    #[test]
    fn test_build_system_prompt_preserves_legitimate_unicode_in_extras() {
        let mut manager = PromptManager::new();
        let legitimate_unicode = "Instruction with 世界 and 🌍 emojis";
        manager.add_system_prompt_extra(legitimate_unicode.to_string());

        let result = manager.builder("gpt-4o").build();

        assert!(result.contains("世界"));
        assert!(result.contains("🌍"));
        assert!(result.contains("Instruction with"));
        assert!(result.contains("emojis"));
    }

    #[test]
    fn test_build_system_prompt_sanitizes_extension_instructions() {
        let manager = PromptManager::new();
        let malicious_extension_info = ExtensionInfo::new(
            "test_extension",
            "Extension help\u{E0041}\u{E0042}\u{E0043}hidden instructions",
            false,
        );

        let result = manager
            .builder("gpt-4o")
            .with_extension(malicious_extension_info)
            .build();

        assert!(!result.contains('\u{E0041}'));
        assert!(!result.contains('\u{E0042}'));
        assert!(!result.contains('\u{E0043}'));
        assert!(result.contains("Extension help"));
        assert!(result.contains("hidden instructions"));
    }

    #[test]
    fn test_basic() {
        let manager = PromptManager::with_timestamp(DateTime::<Utc>::from_timestamp(0, 0).unwrap());

        let system_prompt = manager.builder("gpt-4o").build();

        assert_snapshot!(system_prompt)
    }

    #[test]
    fn test_one_extension() {
        let manager = PromptManager::with_timestamp(DateTime::<Utc>::from_timestamp(0, 0).unwrap());

        let system_prompt = manager
            .builder("gpt-4o")
            .with_extension(ExtensionInfo::new(
                "test",
                "how to use this extension",
                true,
            ))
            .with_router_enabled(true)
            .build();

        assert_snapshot!(system_prompt)
    }

    #[test]
    fn test_typical_setup() {
        let manager = PromptManager::with_timestamp(DateTime::<Utc>::from_timestamp(0, 0).unwrap());

        let system_prompt = manager
            .builder("gpt-4o")
            .with_extension(ExtensionInfo::new(
                "extension_A",
                "<instructions on how to use extension A>",
                true,
            ))
            .with_extension(ExtensionInfo::new(
                "extension_B",
                "<instructions on how to use extension B (no resources)>",
                false,
            ))
            .with_router_enabled(true)
            .with_extension_and_tool_counts(MAX_EXTENSIONS + 1, MAX_TOOLS + 1)
            .build();

        assert_snapshot!(system_prompt)
    }
}
