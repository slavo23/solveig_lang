mod resolver;
mod surface_to_core;

use core::ast::Pretty;

use chumsky::{input::Input as _, prelude::*};
use resolver::Resolver;
use surface::parser::{lexer, program_parser};

fn main() {
    let src = r"
        data Int = Int;
        data Maybe a = Just a | Nothing;

        data Either a b = Left a | Right b;

        data Pair a b = MkPair a b;

        data List a = Cons a (List a) | Nil;
        data Compose f g a = MkCompose (f (g a));

        data Tree a = MkRose a (Forest a);
        data Forest a = MkForest (List (Tree a));

        data Fix f = MkFix (f (Fix f));

        data Phantom a = MkPhantom;

        data Void = Void;

        data Q1 a = MkQ1;
        data Q2 = MkQ2 (Q1 Int);
        data T f = MkT (f Int);
  
        data AppInt f = Mk (f Int);

        id : forall a. a -> a;
        id x = x;

        idInt : Int;
        idInt = id Int;

        head : forall a. List a -> a;
        head xs =
            case xs of
             Cons x rest => x;

        fmap : forall f a b. (a -> b) -> f a -> f b;
        fmap g xs =
            case xs of
             Cons fst rst => Cons (g fst) (fmap g rst)
             | Nil => Nil;

        fromMaybe : forall a. a -> Maybe a -> a;
        fromMaybe def mx =
            case mx of
             Just x => x
             | Nothing => def;

        hrCompose1 : forall a. (forall f g. f (g a)) -> a -> a;
        hrCompose1 x y = y;

    ";

    let tokens = lexer().parse(src).unwrap();
    let ast = program_parser()
        .parse(tokens[..].split_spanned((0..tokens.len()).into()))
        .unwrap();

    let resolver = Resolver::new();
    let resolved = resolver.resolve_surface_program(ast).unwrap();

    println!("=== Data Definitions (kind-inferred) ===\n");
    for dd in &resolved.data_defs {
        println!("{} : {}", dd.name, dd.tycon_kind.pretty());
        for c in &dd.constructors {
            println!("  {} : {}", c.name, c.full_type.pretty());
        }
        println!();
    }

    println!("=== Constructor Types ===\n");
    let mut ctor_names: Vec<_> = resolved.constructor_types.keys().collect();
    ctor_names.sort();
    for name in ctor_names {
        println!("{} : {}", name, resolved.constructor_types[name].pretty());
    }

    println!("\n=== Function Signatures ===\n");
    for def in &resolved.func_defs {
        println!("{} : {}", def.name, def.signature.pretty());
    }
}
