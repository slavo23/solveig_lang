#![no_main]

use std::fs;

use arbitrary::Arbitrary;
use libfuzzer_sys::fuzz_target;
use solveig_driver::run_project_capture;

#[derive(Arbitrary, Debug)]
struct VmInput {
    program: ProgramShape,
}

#[derive(Arbitrary, Debug)]
enum ProgramShape {
    Arithmetic { lhs: i16, rhs: i16, op: IntOp },
    Branch { pred: bool, yes: i16, no: i16 },
    ListSum { values: Vec<i8> },
    TreeLookup { key: u8, values: Vec<i8> },
}

#[derive(Arbitrary, Debug)]
enum IntOp {
    Add,
    Sub,
    Mul,
    Div,
    Mod,
}

fuzz_target!(|input: VmInput| {
    let source = input.program.to_source();
    let Ok(project_root) = tempfile::tempdir() else {
        return;
    };
    let src_dir = project_root.path().join("src");
    if fs::create_dir_all(&src_dir).is_err() {
        return;
    }
    if fs::write(src_dir.join("main.sv"), source).is_err() {
        return;
    }

    let _ = run_project_capture(project_root.path(), "main");
});

impl ProgramShape {
    fn to_source(&self) -> String {
        match self {
            Self::Arithmetic { lhs, rhs, op } => op.source(i64::from(*lhs), i64::from(*rhs)),
            Self::Branch { pred, yes, no } => {
                format!(
                    "main : Int\nmain = if {} then {} else {}\n",
                    if *pred { "True" } else { "False" },
                    i64::from(*yes),
                    i64::from(*no)
                )
            }
            Self::ListSum { values } => {
                let list = int_list(values.iter().take(32).map(|value| i64::from(*value)));
                format!(
                    "sum xs =\n    case xs of\n        [] => 0\n        | x :: rest => x + sum rest\n\nmain : Int\nmain = sum {list}\n"
                )
            }
            Self::TreeLookup { key, values } => {
                let entries = values
                    .iter()
                    .take(32)
                    .enumerate()
                    .map(|(index, value)| format!("Entry {} {}", index % 256, i64::from(*value)))
                    .collect::<Vec<_>>()
                    .join(", ");
                format!(
                    "main : Int\nmain = findWithDefault 0 {} (fromList [{}])\n",
                    usize::from(*key),
                    entries
                )
            }
        }
    }
}

impl IntOp {
    fn source(&self, lhs: i64, rhs: i64) -> String {
        match self {
            Self::Add => format!("main : Int\nmain = {lhs} + {rhs}\n"),
            Self::Sub => format!("main : Int\nmain = {lhs} - {rhs}\n"),
            Self::Mul => format!("main : Int\nmain = {lhs} * {rhs}\n"),
            Self::Div => format!("main : Int\nmain = {lhs} / {rhs}\n"),
            Self::Mod => format!("main : Int\nmain = mod {lhs} {rhs}\n"),
        }
    }
}

fn int_list(values: impl Iterator<Item = i64>) -> String {
    format!(
        "[{}]",
        values
            .map(|value| value.to_string())
            .collect::<Vec<_>>()
            .join(", ")
    )
}
