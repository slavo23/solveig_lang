mod surface_to_core;
use chumsky::{input::Input as _, prelude::*};
use surface::parser::{lexer, program_parser};
use surface_to_core::convert_to_core_program;
use syntax::inference::TypeInference;

fn main() {
    let my_expr = r"
        let const: forall a b. a -> b -> a =
            fun x => fun y => x;
        
        let id: forall a. a -> a = fun x => x;
        let test_const: () = const () id;

        let flip_args: forall a b c. (a -> b -> c) -> (b -> a -> c) = 
            fun f =>
             fun x1 =>
              fun x2 => f x2 x1;

        let test_const_2: () = (flip_args const) id ();
        
        let pid: forall a. a -> a = fun x => x;

        let poly_comp:
            (forall a. a -> a) -> (forall a. a -> a) -> (forall a. a -> a) =
            fun f => fun g => fun x => f (g x);

        let poly_id2: forall a. a -> a = poly_comp pid pid;

        let result: () = poly_id2 ()
    ";
    let l = lexer().parse(my_expr).unwrap();
    let parser = program_parser()
        .parse(l[..].split_spanned((0..l.len()).into()))
        .unwrap();

    let stc = convert_to_core_program(parser);

    let mut inference = TypeInference::new();
    let inferred = inference.check_program(stc);
    println!("{:?}", inferred);
}
