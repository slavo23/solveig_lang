sumOf : Int -> Int;
sumOf n = foldl (fun acc x => x + acc) 0 (range 1 n True);

stressTestFunc : Int -> Int;
stressTestFunc k = 
    let sumOne = sumOf k in
    let sumTwo = sumOf k + 1 in
    let sumThree = sumOf k + 2 in 
        sumOne + sumTwo + sumThree;


runStressTest : ();
runStressTest = 
    let s = map stressTestFunc (range 1 10000 True) in ();
