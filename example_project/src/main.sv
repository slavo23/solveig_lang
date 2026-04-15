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
runNQueensReadyExample = solveNQueensCount 10;
runBinaryTreeExample = sampleTree;
runBrainfuckExample = sampleAddOutput;
runQuicksortExample = sampleSortedQuickSort 1000000;
runMergeSortExample = sampleSortedMergeSort 1000000;
runKnapsackExample = sampleTreeDp;

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