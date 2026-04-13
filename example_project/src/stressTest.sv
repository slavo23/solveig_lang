
sumList : List Int -> Int;
sumList xs =
    foldl (fun acc x => acc + x) 0 xs;

len : forall a. List a -> Int;
len xs =
    foldl (fun acc x => acc + 1) 0 xs;

concatMap : forall a b. (a -> List b) -> List a -> List b;
concatMap f xs =
    foldl (fun acc x => append acc (f x)) [] xs;

replicate : forall a. Int -> a -> List a;
replicate n x =
    if n <= 0 then
        []
    else
        x :: replicate (n - 1) x;


countDownSumGo : Int -> Int -> Int;
countDownSumGo n acc =
    if n <= 0 then
        acc
    else
        countDownSumGo (n - 1) (acc + n);

benchArithmetic : Int;
benchArithmetic =
    countDownSumGo 5000000 0;


benchRangeFold : Int;
benchRangeFold =
    sumList (range 1 100000 True);

benchMapFold : Int;
benchMapFold =
    let xs = range 1 50000 True in
    let ys = map (fun x => x + 3) xs in
        sumList ys;


benchReverseLen : Int;
benchReverseLen =
    let xs = range 1 50000 True in
    let ys = reverse xs in
        len ys;


makeAdder : Int -> (Int -> Int);
makeAdder n =
    fun x => x + n;

applyMany : (Int -> Int) -> Int -> Int -> Int;
applyMany f n acc =
    if n <= 0 then
        acc
    else
        applyMany f (n - 1) (f acc);

benchClosureCalls : Int;
benchClosureCalls =
    let f = makeAdder 3 in
        applyMany f 2000000 0;


makeChunk : Int -> List Int;
makeChunk n = [n, n + 1, n + 2, n + 3, n + 4];

benchConcatMap : Int;
benchConcatMap =
    let xs = range 1 10000 True in
    let ys = concatMap makeChunk xs in
        sumList ys;

benchMixed : Int;
benchMixed = sumList [
    benchArithmetic,
    benchRangeFold,
    benchMapFold,
    benchReverseLen,
    benchClosureCalls,
    benchConcatMap
];


runStressTest : Int;
runStressTest =
    benchMixed;
