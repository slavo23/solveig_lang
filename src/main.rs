mod surface_to_core;
use chumsky::{input::Input as _, prelude::*};
use surface::parser::{lexer, program_parser};
use surface_to_core::convert_to_core_program;
use syntax::inference::check_module;


fn main() {
    let my_expr = r"
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

        fix : forall a. (a -> a) -> a;
        fix f = f (fix f);


    ";
    let l = lexer().parse(my_expr).unwrap();
    let parser = program_parser()
        .parse(l[..].split_spanned((0..l.len()).into()))
        .unwrap();

    let stc = convert_to_core_program(parser);

    let inferred = check_module(stc);
}
