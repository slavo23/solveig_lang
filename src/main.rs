mod surface_to_core;
use chumsky::{input::Input as _, prelude::*};
use surface::parser::{expr_parser, lexer, program_parser};
use surface_to_core::convert_to_core_program;

fn main() {
    let my_expr = r"
        let id: forall a. a -> a = fun x => x in id (fun y => y);
        let test = (id (fun x => x)) () in test
    ";
    let l = lexer().parse(my_expr).unwrap();
    let parser = program_parser()
        .parse(l[..].split_spanned((0..l.len()).into()))
        .unwrap();

    let stc = convert_to_core_program(parser);
    println!("{:?}", stc);
}
