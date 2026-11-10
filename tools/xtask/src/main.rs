//! Repository test-tier orchestration.
//! The fast Cargo default stays independent from host, IDE, example, and stress gates.

use std::env;
use std::process::{Command, ExitCode};

fn main() -> ExitCode {
    let args = env::args().skip(1).collect::<Vec<_>>();
    let succeeded = match args.as_slice() {
        [command] if command == "test" => run(&["test"]),
        [command, tier] if command == "test" => test_tier(tier),
        _ => {
            eprintln!("usage: cargo xtask test [fast|host|ide|examples|stress|all]");
            false
        }
    };

    if succeeded {
        ExitCode::SUCCESS
    } else {
        ExitCode::FAILURE
    }
}

fn test_tier(tier: &str) -> bool {
    match tier {
        "fast" => run(&["test"]),
        "host" => run_integration_filters(&[
            "a1_runtime_goldens::",
            "a2_runtime_regressions::io_streams_prompt_readline_and_program_args",
            "a2_runtime_regressions::byte_file_and_handle_io_preserves_invalid_utf8",
            "a2_runtime_regressions::io_bind_chain_allocates_under_gc_stress",
            "a2_runtime_regressions::million_step_io_loop_runs_on_small_native_stack",
            "a2_runtime_regressions::repeated_checks_reuse_implicit_package_cache",
            "a2_runtime_regressions::file_io_reads_writes_and_appends_project_relative_text",
            "a2_runtime_regressions::cli_passes_program_args_after_separator",
            "a2_runtime_regressions::cli_exit_flushes_an_open_file_and_propagates_its_status",
            "a2_runtime_regressions::primitive_int_division_by_zero_reports_runtime_error",
            "a2_runtime_regressions::primitive_int_modulo_by_zero_reports_runtime_error",
            "a2_runtime_regressions::prelude_int_division_by_zero_reports_runtime_error",
            "a2_runtime_regressions::prelude_int_overflow_reports_structured_defect",
            "a2_runtime_regressions::catch_defect_recovers_from_managed_stack_exhaustion",
            "a2_runtime_regressions::catch_defect_recovers_from_managed_heap_exhaustion",
            "a2_runtime_regressions::cyclic_globals_report_runtime_error",
            "a4_http_runtime::",
            "a5_tls_runtime::",
            "format_cli::",
        ]),
        "ide" => {
            if !run_integration_filters(&[
                "a3_editor_token_sweeps::",
                "checking_granularity::",
                "code_actions_api::",
                "completion_api::",
                "editor_protocol_api::",
                "hover_api::",
                "inlay_hints_api::",
                "navigation_api::",
                "semantic_tokens_api::",
                "source_overlays::",
            ]) {
                return false;
            }
            run(&["test", "-p", "solveig-lsp", "--features", "ide-tests"])
        }
        "examples" => run_integration_filters(&[
            "a2_runtime_regressions::captures_smoke_suite_output",
            "a2_runtime_regressions::quiz_game_example_reads_file_prompts_scores_and_writes_reports",
            "a2_runtime_regressions::ray_tracer_streams_a_deterministic_512_square_scene_with_the_default_heap",
            "a2_runtime_regressions::scry_example_answers_rule_queries",
            "tutorial_examples::",
        ]),
        "stress" => {
            if !run_with_env(&["test", "-p", "solveig-vm"], &[("SOLVEIG_GC_STRESS", "1")]) {
                return false;
            }
            run(&[
                "test",
                "-p",
                "solveig-lsp",
                "--features",
                "ide-tests",
                "repeated_document_edits_keep_retained_lsp_state_bounded_stress",
                "--",
                "--ignored",
            ])
        }
        "all" => run(&["test", "--workspace", "--all-features"]),
        _ => {
            eprintln!("unknown test tier `{tier}`");
            false
        }
    }
}

fn run_integration_filters(filters: &[&str]) -> bool {
    for filter in filters {
        if !run(&[
            "test",
            "--features",
            "comprehensive-tests",
            "--test",
            "integration",
            filter,
        ]) {
            return false;
        }
    }
    true
}

fn run(args: &[&str]) -> bool {
    run_with_env(args, &[])
}

fn run_with_env(args: &[&str], environment: &[(&str, &str)]) -> bool {
    eprintln!("+ cargo {}", args.join(" "));
    match Command::new("cargo")
        .args(args)
        .envs(environment.iter().copied())
        .status()
    {
        Ok(status) => status.success(),
        Err(error) => {
            eprintln!("failed to start Cargo: {error}");
            false
        }
    }
}
