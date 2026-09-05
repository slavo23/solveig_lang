//! Measures representative Solveig programs through the public driver API.
//! Each iteration uses the package cache but includes lowering and VM startup.

use std::hint::black_box;
use std::io::{Cursor, sink};
use std::path::Path;
use std::time::Duration;

use criterion::{Criterion, criterion_group, criterion_main};
use solveig_driver::{Compilation, CompileOptions};

const PROJECT_ROOT: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/benches/programs/language");

fn benchmark_language(c: &mut Criterion) {
    benchmark_cold_pipeline(c);
    benchmark_pure(c, "allocation", "allocationMain", "1250025000");
    benchmark_pure(c, "arithmetic", "arithmeticMain", "1701658789");
    benchmark_pure(
        c,
        "constraint_generic",
        "constraintGenericMain",
        "20000100000",
    );
    benchmark_pure(
        c,
        "constraint_concrete",
        "constraintConcreteMain",
        "20000100000",
    );
    benchmark_pure(c, "closures", "closureMain", "1400001");
    benchmark_pure(c, "static_closures", "staticClosureMain", "200001");
    benchmark_pure(c, "patterns", "patternMain", "524288");
    benchmark_pure(c, "string_bytes", "stringBytesMain", "2260000");
    benchmark_pure(c, "vector_index", "vectorIndexMain", "1022680");
    benchmark_pure(c, "list_index", "listIndexMain", "1022680");
    benchmark_pure(c, "map_lookup", "mapLookupMain", "2045360");
    benchmark_pure(c, "association_lookup", "associationLookupMain", "2045360");
    benchmark_pure(c, "queue", "queueMain", "500500");
    benchmark_pure(c, "naive_queue", "naiveQueueMain", "500500");
    let input = Path::new(PROJECT_ROOT).join("src/main.sv");
    benchmark_io(c, "file_io", "fileIoMain", "64\n", &[path_arg(&input)]);
    benchmark_io(c, "channels", "channelMain", "50005000\n", &[]);
}

fn benchmark_cold_pipeline(c: &mut Criterion) {
    let entry = "compilerPipelineMain";
    validate(entry, "0", &[]);

    c.bench_function("language/compiler_pipeline", |b| {
        b.iter(|| {
            let mut compilation = Compilation::new(PROJECT_ROOT);
            run(&mut compilation, entry, &[]);
        });
    });
}

fn benchmark_pure(c: &mut Criterion, name: &str, entry: &str, expected: &str) {
    validate(entry, expected, &[]);
    let mut compilation = Compilation::new(PROJECT_ROOT);

    c.bench_function(&format!("language/{name}"), |b| {
        b.iter(|| run(&mut compilation, entry, &[]));
    });
}

fn benchmark_io(c: &mut Criterion, name: &str, entry: &str, expected: &str, args: &[String]) {
    validate(entry, expected, args);
    let mut compilation = Compilation::new(PROJECT_ROOT);

    c.bench_function(&format!("language/{name}"), |b| {
        b.iter(|| run(&mut compilation, entry, args));
    });
}

fn validate(entry: &str, expected: &str, args: &[String]) {
    let output = Compilation::new(PROJECT_ROOT)
        .run_project_capture_with_stdin_and_args(entry, "", args)
        .map_err(|error| error.to_string());

    assert_eq!(output.as_deref(), Ok(expected));
}

fn run(compilation: &mut Compilation, entry: &str, args: &[String]) {
    let mut stdin = Cursor::new(&[][..]);
    let mut stdout = sink();
    let result = compilation.run_project_with_streams(
        entry,
        CompileOptions::default(),
        &mut stdin,
        &mut stdout,
        args,
    );
    let _result = black_box(result);
}

fn path_arg(path: &Path) -> String {
    path.to_string_lossy().into_owned()
}

criterion_group! {
    name = benches;
    config = Criterion::default()
        .warm_up_time(Duration::from_secs(1))
        .measurement_time(Duration::from_secs(3))
        .sample_size(20);
    targets = benchmark_language
}
criterion_main!(benches);
