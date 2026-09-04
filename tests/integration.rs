#[path = "common/fixture_shards.rs"]
mod fixture_shards;
mod snapshot_support;
mod support;

#[path = "golden_runner.rs"]
mod a1_runtime_goldens;
#[path = "run_project_capture.rs"]
mod a2_runtime_regressions;
#[path = "lsp_robustness_api.rs"]
mod a3_editor_token_sweeps;
#[path = "http_runtime.rs"]
mod a4_http_runtime;
#[path = "tls_runtime.rs"]
mod a5_tls_runtime;

mod bytecode_specialization;
mod checker_type_golden;
mod checking_granularity;
mod code_actions_api;
mod completion_api;
mod cst_golden_runner;
mod diagnostics_golden_runner;
mod doc_comments;
mod editor_protocol_api;
mod empty_data;
mod equation_groups_api;
mod format_cli;
mod hover_api;
mod inlay_hints_api;
mod instance_coherence_api;
mod module_visibility_api;
mod navigation_api;
mod pattern_binders_api;
mod semantic_tokens_api;
mod source_overlays;
mod tutorial_examples;
mod warning_runner;
