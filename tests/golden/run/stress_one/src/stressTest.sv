import std.list(*)

sumList : List Int -> Int
;
sumList xs = foldl (fun acc x => acc + x) 0 xs
;

len : forall a. List a -> Int
;
len xs = foldl (fun acc x => acc + 1) 0 xs
;

countDownSumGo : Int -> Int -> Int
;
countDownSumGo n acc = if n <= 0 then acc else countDownSumGo (n - 1) (acc + n)
;

divValue = div 2 2
;

benchScalar : Int
;
benchScalar = countDownSumGo 3000000 0
;

benchRangeFold : Int
;
benchRangeFold = sumList (rangeInclusive 1 500000)
;

benchPipeline : Int
;
benchPipeline =
    let xs =
        rangeInclusive 1 50000
    in let ys = map (fun x => x + 3) xs in let zs = filter even ys in sumList zs
;

benchReverseFold : Int
;
benchReverseFold = let xs = rangeInclusive 1 40000 in let ys = reverse xs in sumList ys
;

makeAdder : Int -> (Int -> Int)
;
makeAdder n = fun x => x + n
;

applyMany : (Int -> Int) -> Int -> Int -> Int
;
applyMany f n acc = if n <= 0 then acc else applyMany f (n - 1) (f acc)
;

benchClosures : Int
;
benchClosures = let f = makeAdder 3 in applyMany f 1500000 0
;

benchMixedModerate : Int
;
benchMixedModerate = benchScalar + benchRangeFold + benchPipeline + benchReverseFold + benchClosures
;

runStressTest : Int
;
runStressTest = benchPipeline
;
