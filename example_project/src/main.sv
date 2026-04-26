import nQueens(solveNQueensCount);
import sudoku(solveBoard, samplePuzzle);
import binaryTree(sampleTree);
import brainfuck(sampleAddOutput);
import stressTest(runStressTest);
import stressTestTwo(runStressTestTwo);
import quicksort(sampleSortedQuickSort);
import mergesort(sampleSortedMergeSort);
import knapsack(sampleTreeDp);
import treeMap(fromList, Map, intComparator, Entry, size);

runSudokuReadyExample = solveBoard samplePuzzle;
runNQueensReadyExample = solveNQueensCount 8;
runBinaryTreeExample = sampleTree;
runBrainfuckExample = sampleAddOutput;
runQuicksortExample = sampleSortedQuickSort 500000;
runMergeSortExample = sampleSortedMergeSort 500000;
runKnapsackExample = sampleTreeDp;

data EqDict a b = MkEqDict (a -> b -> Bool);

getEqFn : forall a b. EqDict a b -> (a -> b -> Bool);
getEqFn dict = 
    case dict of
        MkEqDict f => f;

same : forall a b. EqDict a b -> a -> b -> Bool;
same dict x y = (getEqFn dict) x y;

intEqDict : EqDict Int Int;
intEqDict = MkEqDict (fun x y => x == y);

treeFromList : Map Int Int;
treeFromList =
    let intEntries = (map (fun iVal => Entry iVal iVal) (range 1 100000 True)) in 
        fromList intComparator intEntries;

{- if you want the results, just copy-paste the particular example directly into main -}
runAllExamplesAndDiscardResult = 
    let one = runSudokuReadyExample in
    let two = runNQueensReadyExample in
    let three = runBinaryTreeExample in 
    let four = runBrainfuckExample in
    let five = runQuicksortExample in
    let six = runMergeSortExample in
    let seven = runKnapsackExample in ();

main = runAllExamplesAndDiscardResult;