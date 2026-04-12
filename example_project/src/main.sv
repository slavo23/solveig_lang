import stressTest(runStressTest);

bigList = range 1 10000 True;

sumEvenBigList = foldl (fun acc x => acc + x) 0 (filter even bigList);


main = runStressTest;