data MaybePos = NoPos | FoundPos Int Int
;
data MaybeBoard = NoBoard | Solved [[Int]]
;

digits : [Int]
;
digits = [1, 2, 3, 4, 5, 6, 7, 8, 9]
;

samplePuzzle : [[Int]]
;
samplePuzzle =
    [
        [5, 3, 0, 0, 7, 0, 0, 0, 0],
        [6, 0, 0, 1, 9, 5, 0, 0, 0],
        [0, 9, 8, 0, 0, 0, 0, 6, 0],
        [8, 0, 0, 0, 6, 0, 0, 0, 3],
        [4, 0, 0, 8, 0, 3, 0, 0, 1],
        [7, 0, 0, 0, 2, 0, 0, 0, 6],
        [0, 6, 0, 0, 0, 0, 2, 8, 0],
        [0, 0, 0, 4, 1, 9, 0, 0, 5],
        [0, 0, 0, 0, 8, 0, 0, 7, 9]
    ]
;

getAtInt : Int -> [Int] -> Int -> Int
;
getAtInt target xs current =
    case xs of
        [] => 0
        | x :: rest => if current == target then x else getAtInt target rest (current + 1) ;

rowAt : forall a. [[a]] -> Int -> Int -> [a]
;
rowAt board target current =
    case board of
        [] => []
        | row :: rest => if current == target then row else rowAt rest target (current + 1) ;

replaceAt : forall a. Int -> a -> [a] -> Int -> [a]
;
replaceAt target newValue xs current =
    case xs of
        [] => []
        | x :: rest =>
            if current == target
                then newValue :: rest
                else x :: replaceAt target newValue rest (current + 1) ;

setCell : [[Int]] -> Int -> Int -> Int -> [[Int]]
;
setCell board row col value =
    let oldRow =
        rowAt board row 0
    in let newRow = replaceAt col value oldRow 0 in replaceAt row newRow board 0
;

columnAt : [[Int]] -> Int -> [Int]
;
columnAt board target =
    case board of
        [] => []
        | row :: rest => getAtInt target row 0 :: columnAt rest target ;

rowSlice : [Int] -> Int -> Int -> Int -> [Int]
;
rowSlice row startCol endCol current =
    case row of
        [] => []
        | x :: rest =>
            if current < startCol
                then rowSlice rest startCol endCol (current + 1)
                else if current > endCol
                        then []
                        else x :: rowSlice rest startCol endCol (current + 1) ;

blockRows : [[Int]] -> Int -> Int -> Int -> Int -> Int -> [Int]
;
blockRows board startRow endRow startCol endCol currentRow =
    case board of
        [] => []
        | row :: rest =>
            if currentRow < startRow
                then blockRows rest startRow endRow startCol endCol (currentRow + 1)
                else if currentRow > endRow
                        then []
                        else
                            append
                                (rowSlice row startCol endCol 0)
                                (blockRows rest startRow endRow startCol endCol (currentRow + 1)) ;

blockAt : [[Int]] -> Int -> Int -> [Int]
;
blockAt board row col =
    let startRow =
        (div row 3) * 3
    in let startCol =
        (div col 3) * 3
    in blockRows board startRow (startRow + 2) startCol (startCol + 2) 0
;

notInInts : Int -> [Int] -> Bool
;
notInInts value xs =
    case xs of
        [] => True
        | x :: rest => if x == value then False else notInInts value rest ;

isValidChoice : [[Int]] -> Int -> Int -> Int -> Bool
;
isValidChoice board row col value =
    if notInInts value (rowAt board row 0)
        then
            if notInInts value (columnAt board col)
                then notInInts value (blockAt board row col)
                else False
        else False
;

findEmptyRow : [Int] -> Int -> Int -> MaybePos
;
findEmptyRow row rowIndex colIndex =
    case row of
        [] => NoPos
        | x :: rest =>
            if x == 0
                then FoundPos rowIndex colIndex
                else findEmptyRow rest rowIndex (colIndex + 1) ;

findEmptyBoard : [[Int]] -> Int -> MaybePos
;
findEmptyBoard board rowIndex =
    case board of
        [] => NoPos
        | row :: rest =>
            case findEmptyRow row rowIndex 0 of
                NoPos => findEmptyBoard rest (rowIndex + 1)
                | FoundPos r c => FoundPos r c ;

tryDigits : [[Int]] -> Int -> Int -> [Int] -> MaybeBoard
;
tryDigits board row col ds =
    case ds of
        [] => NoBoard
        | d :: rest =>
            if isValidChoice board row col d
                then
                    case solveBoard (setCell board row col d) of
                        NoBoard => tryDigits board row col rest
                        | Solved solved => Solved solved
                else tryDigits board row col rest ;

solveBoard : [[Int]] -> MaybeBoard
;
solveBoard board =
    case findEmptyBoard board 0 of
        NoPos => Solved board
        | FoundPos row col => tryDigits board row col digits ;
