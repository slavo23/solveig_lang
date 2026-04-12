makeGarbage : Int -> [Int];
makeGarbage n =
    filter (fun x => x > 1) (range 1 n True);

loop : Int -> Int -> Int;
loop k n =
    if k == 0 then
        0
    else
        let xs = makeGarbage n in
        let s = foldl (fun acc x => acc + 1) 0 xs in
        s + loop (k - 1) n;

runStressTest : Int;
runStressTest =
    loop 6 5000;