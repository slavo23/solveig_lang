mod surface_to_core;
use chumsky::{input::Input as _, prelude::*};
use surface::parser::{lexer, program_parser};
use surface_to_core::convert_to_core_program;
use syntax::inference::TypeInference;

fn main() {
    let my_expr = r"        
        const : forall a b. a -> b -> a;
        const x y = x;
        
        id : forall a. a -> a;
        id x = x;

        testConst : ();
        testConst = const () id;

        flipArgs : forall a b c. (a -> b -> c) -> (b -> a -> c);
        flipArgs f x1 x2 = f x2 x1;

        testConst2 : ();
        testConst2 = (flipArgs const) id ();
        
        pid : forall a. a -> a;
        pid x = x;

        polyComp : (forall a. a -> a) -> (forall a. a -> a) -> (forall a. a -> a);
        polyComp f g x = f (g x);

        polyId2 : forall a. a -> a;
        polyId2 = polyComp pid pid;

        result : ();
        result = polyId2 ();

        partial : forall a b. a -> (b -> a);
        partial x = const x;

        partialTest: ();
        partialTest = (partial ()) ();

        letId : forall a. a -> a;
        letId x = let func = fun y => y in func x

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
