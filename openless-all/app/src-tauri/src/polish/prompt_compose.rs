//! Tauri compatibility re-exports for framework-independent prompt composition.

pub(crate) use openless_core::{
    assemble_polish_system_prompt, build_hotword_block, compose_hotword_block_preview,
    compose_polish_prompts, compose_qa_system_prompt, compose_system_prompt,
    compose_translate_prompts, context_premise, PolishSystemPromptAssembly,
};
