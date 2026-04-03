mod resolver;

use base::utils::diagnostics::emit_type_errors;
use chumsky::{input::Input as _, prelude::*};
use resolver::Resolver;
use surface::parser::{lexer, program_parser};
use syntax::inference::type_infer::check_and_elab_module;
pub mod compile;
use compile::{interpret::Machine, lowering::lower_module};

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

        
        constFun = fun x y => x;

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

        testPolyMatch : Nat -> (forall a. a -> a);
        testPolyMatch n = case n of
              Z => id
            | S n => fun (y : a) => y;

        data Id = MkId (forall a. a -> a);

        testManyIds : List Id;
        testManyIds = Cons (MkId id) (Cons (MkId (fun x => x)) Nil);

        ifElsePoly : Nat -> (forall a. a -> a);
        ifElsePoly n =
            if isZero n then
                if isZero n then id else id
            else
                fun (x : a) => x;

        pairPartial x = Pair x;

        not : Bool -> Bool;
        not b = case b of True => False | False => True;

        natList : List Nat;
        natList = Cons Z (Cons (S Z) (Cons (S Z) (Cons Z (Cons Z Nil))));

        noZeros = listFilter (fun n => not (isZero n)) natList;

        not : Bool -> Bool;
        not b =
            case b of
                True => False
                | False => True;

  
        data Bool = True | False;


        not : Bool -> Bool;
        not b =
            case b of
                True => False
                | False => True;

        leq : Nat -> Nat -> Bool;
        leq n m =
            case n of
                Z => True
                | S n1 =>
                    case m of
                        Z => False
                        | S m1 => leq n1 m1;

        append : forall a. [a] -> [a] -> [a];
        append xs ys =
            case xs of
                Nil => ys
                | Cons x x1s => Cons x (append x1s ys);

        filter : forall a. (a -> Bool) -> [a] -> [a];
        filter p xs =
            case xs of
                [] => Nil
                | y :: ys =>
                    if p y then y :: (filter p ys)
                    else filter p ys;

        qsort : [Nat] -> [Nat];
        qsort xs =
            case xs of
                [] => Nil
                | pivot :: rest =>
                    append
                        (qsort (filter (fun x => leq x pivot) rest))
                        (pivot :: (qsort (filter (fun x => not (leq x pivot)) rest)));

        reverse : forall a. [a] -> [a];
        reverse xs = 
            case xs of
                [] => []
                 | y :: ys => append (reverse ys) [y];


        add : Nat -> Nat -> Nat;
        add n m =
            case n of
                Z => m
                | S k => S (add k m);

        mul : Nat -> Nat -> Nat;
        mul n m =
            case n of
                Z => Z
                | S k => add m (mul k m);

        pred n = 
            case n of
                Z => Z
                | S k => k;

        sub : Nat -> Nat -> Nat;
        sub n m = 
            case m of
                Z => n
                | S k => sub (pred n) k;

        one = S Z;
        two = S one;
        three = S two;
        four = S three;
        five = S four;
        six = add one five;
        seven = add one six;

        sample = [three, one, six, four, one, five, seven];

        constInf x y = x;
        swap f x y = f y x;

        testSwap = fun x => swap constInf x two;

        sumOfNats : [Nat] -> Nat;
        sumOfNats xs = foldr add Z xs;

        natRangeOfSize : Nat -> Bool -> [Nat];
        natRangeOfSize size incZero = 
            case size of
                Z => []
                | S Z => if (not incZero) then [] else [Z]
                | S k => k :: (natRangeOfSize k incZero);

        main : Nat;
        main = sumOfNats (reverse (natRangeOfSize (mul six (mul six five)) False));
    ";

    let tokens = lexer().parse(src).unwrap();
    let ast = program_parser()
        .parse(tokens[..].split_spanned((0..tokens.len()).into()))
        .unwrap();

    let resolver = Resolver::new();
    let resolved = resolver.resolve_surface_program(ast).unwrap();
    match check_and_elab_module(&resolved, "repl") {
        Ok(checked) => {
            let ir = lower_module(&checked);
            let result = Machine::exec(&ir, "main");
            println!("{}", result.display(&ir));
        }
        Err(errors) => {
            emit_type_errors(&errors, src).unwrap();
            std::process::exit(1);
        }
    }
}
