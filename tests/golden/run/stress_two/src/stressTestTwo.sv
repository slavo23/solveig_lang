import std.list(*)

sumList : [Int] -> Int
;
sumList xs = foldl (fun acc x => acc + x) 0 xs
;

countDownMixGo : Int -> Int -> Int
;
countDownMixGo n acc =
    if n <= 0
        then acc
        else let next = ((n * 3) + (n * 2)) - (n + 7) in countDownMixGo (n - 1) (acc + next)
;

benchScalar : Int
;
benchScalar = countDownMixGo 8000000 0
;

innerLoop : Int -> Int -> Int -> Int
;
innerLoop i j acc = if j <= 0 then acc else innerLoop i (j - 1) (acc + (i * j) - (j + 3))
;

outerLoop : Int -> Int -> Int
;
outerLoop i acc = if i <= 0 then acc else outerLoop (i - 1) (innerLoop i 200 acc)
;

benchNestedLoops : Int
;
benchNestedLoops = outerLoop 20000 0
;

makeAffine : Int -> Int -> (Int -> Int)
;
makeAffine a b = fun x => (a * x) + b
;

applyMany : (Int -> Int) -> Int -> Int -> Int
;
applyMany f n acc = if n <= 0 then acc else applyMany f (n - 1) (f acc)
;

benchClosures : Int
;
benchClosures = let f = makeAffine 3 1 in applyMany f 20000000 0
;

benchPipeline : Int
;
benchPipeline =
    let xs =
        rangeInclusive 1 50000
    in let ys =
        map (fun x => (x * 3) + 1) xs
    in let zs = filter (fun x => x < 75000) ys in sumList zs
;

benchMixedFast : Int
;
benchMixedFast = benchScalar + benchNestedLoops + benchClosures + benchPipeline
;

runStressTestTwo : Int
;
runStressTestTwo = benchMixedFast
;
