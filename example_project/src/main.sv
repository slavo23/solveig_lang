import utils.list(bigList);

sumEvenBigList =
    let evenList = filter even bigList in
    foldl (fun acc x => acc + x) 0 evenList;

twoArg x y = x;

main = twoArg 1 2 3;