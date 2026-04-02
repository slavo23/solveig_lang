mod resolver;
mod surface_to_core;

use core::ast::Pretty;

use base::utils::diagnostics::emit_type_errors;
use chumsky::{input::Input as _, prelude::*};
use resolver::Resolver;
use surface::parser::{lexer, program_parser};
use syntax::inference::type_infer::check_and_elab_module;

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
        data List a = Nil | Cons a (List a);
        data Either a b = Left a | Right b;
        data Maybe a = Nothing | Just a;
        data Pair a b = Pair a b;
        data Nat = Z | S Nat;
        data Bool = True | False;

        pairId : forall a b. a -> b -> Pair a b;
        pairId x y = Pair (id x) (id y);

        max : Nat -> Nat -> Nat;
        max k m = 
            case Pair k m of 
                Pair Z Z => Z
                | Pair (S s0) Z => k
                | Pair Z (S s1) => m
                | Pair (S s0) (S s1) => max s0 s1;

        succ : Nat -> Nat;
        succ n = 
            case n of Z => S Z | S k => S (succ k);

        prev : Nat -> Nat;
        prev n = 
            case n of Z => Z | S k => prev k;

        isZero : Nat -> Bool;
        isZero n = 
            case n of Z => True | S _ => False;

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

        listMap : forall a b. (a -> b) -> List a -> List b;
        listMap f xs =
            case xs of
                Nil => Nil
                | Cons y ys => Cons (f y) (listMap f ys)
        ;

        listFilter : forall a. (a -> Bool) -> List a -> List a;
        listFilter f xs = 
            case xs of
                Nil => Nil
                | Cons y ys => 
                    if f y then Cons y (listFilter f ys)
                    else listFilter f ys;

        listMapId : forall a. List a -> List a;
        listMapId xs = listMap id xs;

        listSafeHead : forall a. List a -> Maybe a;
        listSafeHead xs = 
            case xs of
                Nil => Nothing
                | Cons y ys => Just y;

        listSafeTail : forall a. List a -> Maybe (List a);
        listSafeTail xs = 
            case xs of
                Nil => Nothing
                | Cons y ys => Just ys;

        const : forall a b. a -> b -> a;
        const x y = x;
        
        id : forall a. a -> a;
        id x = x;


        foldr : forall a b. (a -> b -> b) -> b -> List a -> b;
        foldr f z xs =
            case xs of
                Nil => z
                | Cons y ys => f y (foldr f z ys);


        append : forall a. List a -> List a -> List a;
        append xs ys = 
            case xs of 
                Nil => ys
                | Cons x1 x1s => Cons x1 (append x1s ys);

        constFun : forall a b. a -> b -> a;
        constFun = fun (x : a) (y : b) => x;

        makeList : forall a. a -> List a -> List a;
        makeList x = Cons x;

        mapSucc : List Nat -> List Nat;
        mapSucc xs = listMap (fun (x : Nat) => S x) xs;

        data FunctorDict f = MkFunctor (forall a b. (a -> b) -> f a -> f b);

        maybeMap : forall a b. (a -> b) -> Maybe a -> Maybe b;
        maybeMap f mx =
            case mx of
                Nothing => Nothing
                | Just x => Just (f x);

        getFmap : forall f a b. FunctorDict f -> (a -> b) -> f a -> f b;
        getFmap dict =
            case dict of
                MkFunctor fmapFn => fmapFn;

        listFunctor : FunctorDict List;
        listFunctor = MkFunctor listMap;

        maybeFunctor : FunctorDict Maybe;
        maybeFunctor = MkFunctor maybeMap;

        mapBoth : forall f a b. FunctorDict f -> (a -> b) -> f a -> f b;
        mapBoth dict f xs = getFmap dict f xs;

        brokenId : forall a. a -> a;
        brokenId x = x ();

        brokenConst : forall a b. a -> b -> a;
        brokenConst x y = y;

        brokenMaybe : forall a. a -> Maybe a;
        brokenMaybe x = Right x;

        testPolyMatch : Nat -> (forall a. a -> a);
        testPolyMatch n = case n of
              Z => id
            | S n => fun (y : a) => y;

        data Id = MkId (forall a. a -> a);

        testManyIds : List Id;
        testManyIds = Cons (MkId id) (Cons (MkId (fun x => x)) Nil);

    ";

    let tokens = lexer().parse(src).unwrap();
    let ast = program_parser()
        .parse(tokens[..].split_spanned((0..tokens.len()).into()))
        .unwrap();

    let resolver = Resolver::new();
    let resolved = resolver.resolve_surface_program(ast).unwrap();
    match check_and_elab_module(&resolved, "repl".to_string()) {
        Ok(()) => {
            println!("typecheck ok");
        }
        Err(errors) => {
            emit_type_errors(&errors, src).unwrap();
            std::process::exit(1);
        }
    }
}
