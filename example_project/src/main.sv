import utils.list(bigList);

sumEvenBigList =
    let evenList = filter even bigList in
    foldl (fun acc x => acc + x) 0 evenList;

main = sumEvenBigList;