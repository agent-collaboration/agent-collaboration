//! Provider-owned file metadata readers. Native loading and execution stay native.
mod claude_filesystem;
mod claude_legacy_commands;
mod claude_plugin_export;
mod claude_plugin_skills;
mod claude_skill_frontmatter;
mod cursor_filesystem;
mod cursor_skill_metadata;
mod skill_document;
mod skill_file;

pub use claude_skill_frontmatter::{
    ClaudeSkillFileMetadata, SkillFileReadError, read_claude_skill_metadata,
};
pub use cursor_filesystem::{
    CursorFileSkill, CursorFileSkillScope, CursorFilesystemCatalogue, CursorFilesystemContext,
    CursorFilesystemReadError, CursorSkillFileFailure, CursorSkillFileFailureReason,
    read_cursor_filesystem_skills,
};

pub use claude_filesystem::{
    ClaudeFileSkill, ClaudeFileSkillScope, ClaudeFilesystemCatalogue, ClaudeFilesystemReadError,
    ClaudeSkillFileFailure, ClaudeSkillOverride, ClaudeSkillRoot, read_claude_filesystem_skills,
};

pub use claude_plugin_export::{
    ClaudePluginExport, ClaudePluginExportError, ClaudePluginExportScope,
    read_claude_plugin_export, regular_claude_plugin_key,
};

pub use claude_legacy_commands::read_claude_legacy_skill_files;

pub use claude_plugin_skills::read_claude_plugin_skill_roots;
