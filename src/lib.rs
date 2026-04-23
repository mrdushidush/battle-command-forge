// Clippy allows for the v0.1.0 public release. Categories below fall into two buckets:
//
// Refactor-required (intentionally deferred — project is in stable-maintenance mode):
//   too_many_arguments     — pipeline stages naturally take many args (rendering fns in tui.rs)
//   inherent_to_string     — ContextManager::to_string() is a display helper, not a Display impl
//   large_enum_variant     — TuiEvent variants vary in size; boxing would change the public event shape
//   only_used_in_recursion — recursive builder methods flagged as false positive
//
// Stylistic-preference (kept for readability on this codebase's idioms):
//   field_reassign_with_default — builder-style init is more readable in the pipeline orchestration
//   single_match                — future-proofed for additional arms that may be added
//   ptr_arg                     — &mut Vec<_> is consistent with adjacent signatures in the same module
//   unnecessary_sort_by         — explicit comparator reads more clearly than sort_by_key for mixed keys
//   len_without_is_empty        — len is informational, not collection-like
//   doc_lazy_continuation       — some doc blocks intentionally flow across lines without trailing spaces
#![allow(clippy::too_many_arguments)]
#![allow(clippy::inherent_to_string)]
#![allow(clippy::large_enum_variant)]
#![allow(clippy::only_used_in_recursion)]
#![allow(clippy::field_reassign_with_default)]
#![allow(clippy::single_match)]
#![allow(clippy::ptr_arg)]
#![allow(clippy::unnecessary_sort_by)]
#![allow(clippy::len_without_is_empty)]
#![allow(clippy::doc_lazy_continuation)]

pub mod benchmark;
pub mod codegen;
pub mod context;
pub mod cto;
pub mod custom_commands;
pub mod db;
pub mod editor;
pub mod enterprise;
pub mod github;
pub mod hardware;
pub mod llm;
pub mod memory;
pub mod mission;
pub mod model_config;
pub mod model_picker;
pub mod models;
pub mod report;
pub mod router;
pub mod sandbox;
pub mod snake;
pub mod space;
pub mod stress;
pub mod swarm;
pub mod swebench;
pub mod swebench_eval;
pub mod swebench_tools;
pub mod tui;
pub mod verifier;
pub mod voice;
pub mod workspace;

pub use mission::MissionRunner;
