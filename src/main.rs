mod surface_to_core;
use chumsky::{input::Input as _, prelude::*};
use surface::parser::{lexer, program_parser};
use surface_to_core::convert_to_core_program;
use syntax::inference::{TypeInference, check_module};

fn main() {
    let my_expr = r"
        data List a = Nil | Cons a (List a);

        data Either a b = Left a | Right b;

        singletonList : forall a. a -> List a;
        singletonList x = Cons x Nil;

        left : forall a b. a -> Either a b;
        left x = Left x;

        right : forall a b. b -> Either a b;
        right x = Right x;

        eitherListUnit : forall a. a -> Either (List a) ();
        eitherListUnit x = Left (Cons x Nil);

        eitherListUnit2 : forall a. a -> Either (List a) ();
        eitherListUnit2 x = Right ();

        listConcat : forall a. List a -> List a -> List a;
        listConcat xs ys = (Cons xs) ys;

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
        polyComp f g = f g;

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

    let inferred = check_module(stc);
}
