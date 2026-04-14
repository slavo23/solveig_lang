import nQueens(solveNQueensCount);
import sudoku(solveBoard, samplePuzzle);
import binaryTree(sampleTree);
import brainfuck(sampleAddOutput);
import stressTest(runStressTest);
import stressTestTwo(runStressTestTwo);
import quicksort(sampleSortedQuickSort);
import mergesort(sampleSortedMergeSort);

runSudokuReadyExample = solveBoard samplePuzzle;
runNQueensReadyExample = solveNQueensCount 8;
runBinaryTreeExample = sampleTree;
runBrainfuckExample = sampleAddOutput;
runQuicksortExample = sampleSortedQuickSort 1000000;
runMergeSortExample = sampleSortedMergeSort 1000000;

{- if you want the results, just copy-paste the example directly into main -}
runAllExamplesAndDiscardResult = 
    let one = runSudokuReadyExample in
    let two = runNQueensReadyExample in
    let three = runBinaryTreeExample in 
    let four = runBrainfuckExample in
    let five = runQuicksortExample in
    let six = runMergeSortExample in ();

main = runAllExamplesAndDiscardResult;