#![expect(
    clippy::panic,
    reason = "integration tests use panic messages to explain fixture failures"
)]

use std::fs;
use std::process::Command;

mod support;

#[test]
fn concrete_int_arithmetic_uses_primitive_bytecode_without_dictionary_projection() {
    let dump = compile_dump(
        "int_specialization",
        "main : Bool\nmain = ((1 + 2) * 3) == 9\n",
    );
    let chunk = named_chunk(&dump, "main.main");
    let instructions = chunk_instructions(&chunk);

    insta::assert_snapshot!(instructions, @r"
    0000 PushInt 1
    0001 PushInt 2
    0002 Binary Plus
    0003 PushInt 3
    0004 Binary Mult
    0005 PushInt 9
    0006 Binary Equal
    0007 Return
    ");
}

#[test]
fn concrete_constrained_function_call_uses_bounded_specialized_global() {
    let dump = compile_dump(
        "constrained_function_specialization",
        r#"addGeneric : forall a. Num a => a -> a -> a
addGeneric left right = left + right

main : Int
main = addGeneric 1 2
"#,
    );
    let chunk = named_chunk_with_prefix(&dump, "$specialize.main.addGeneric.");
    let instructions = chunk_instructions(&chunk);

    insta::assert_snapshot!(instructions, @r"
    0000 LoadLocal 0
    0001 LoadLocal 1
    0002 Binary Plus
    0003 Return
    ");
}

fn compile_dump(name: &str, source: &str) -> String {
    let project_root = support::temp_root("solveig_bytecode", name);
    fs::create_dir_all(project_root.join("src"))
        .expect("temporary source directory should be creatable");
    fs::write(project_root.join("src/main.sv"), source)
        .expect("temporary Solveig source should be writable");

    let output = Command::new(env!("CARGO_BIN_EXE_solveig"))
        .arg("--project-root")
        .arg(project_root.path())
        .arg("--dump-bytecode")
        .current_dir(project_root.path())
        .output()
        .expect("Solveig bytecode fixture should start");
    assert!(
        output.status.success(),
        "Solveig bytecode fixture failed:\n{}",
        String::from_utf8_lossy(&output.stderr)
    );

    fs::read_to_string(project_root.join("bytecode.txt")).expect("bytecode dump should be readable")
}

fn chunk_instructions(chunk: &str) -> String {
    chunk
        .lines()
        .skip(3)
        .map(str::trim_start)
        .collect::<Vec<_>>()
        .join("\n")
}

fn named_chunk(dump: &str, name: &str) -> String {
    let header_suffix = format!(": {name} ---");
    dump.split("--- chunk ")
        .find(|chunk| {
            chunk
                .lines()
                .next()
                .is_some_and(|header| header.ends_with(&header_suffix))
        })
        .map_or_else(
            || panic!("bytecode dump did not contain chunk `{name}`"),
            str::to_string,
        )
}

fn named_chunk_with_prefix(dump: &str, name_prefix: &str) -> String {
    dump.split("--- chunk ")
        .find(|chunk| {
            chunk.lines().next().is_some_and(|header| {
                header
                    .split_once(": ")
                    .is_some_and(|(_, name)| name.starts_with(name_prefix))
            })
        })
        .map_or_else(
            || panic!("bytecode dump did not contain chunk prefix `{name_prefix}`"),
            str::to_string,
        )
}
