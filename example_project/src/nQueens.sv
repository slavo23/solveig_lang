absInt : Int -> Int;
absInt x =
    if x < 0 then
        0 - x
    else
        x;

safeColWithDist : Int -> Int -> [Int] -> Bool;
safeColWithDist col dist placed =
    case placed of
        [] => True
      | q :: qs =>
            if col == q then
                False
            else if absInt (col - q) == dist then
                False
            else
                safeColWithDist col (dist + 1) qs;

tryCols : Int -> Int -> [Int] -> [[Int]];
tryCols n col placed =
    if col > n then
        []
    else if safeColWithDist col 1 placed then
        (col :: placed) :: tryCols n (col + 1) placed
    else
        tryCols n (col + 1) placed;

extendAll : Int -> [[Int]] -> [[Int]];
extendAll n partials =
    case partials of
        [] => []
      | p :: ps =>
            append (tryCols n 1 p) (extendAll n ps);

queensFrom : Int -> Int -> [[Int]];
queensFrom n row =
    if row > n then
        [[]]
    else
        extendAll n (queensFrom n (row + 1));

solveNQueens : Int -> [[Int]];
solveNQueens n =
    queensFrom n 1;

countLists : forall a. [[a]] -> Int;
countLists xss =
    case xss of
        [] => 0
      | _ :: rest => 1 + countLists rest;

solveNQueensCount : Int -> Int;
solveNQueensCount n =
    countLists (solveNQueens n);
