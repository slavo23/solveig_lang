import nQueens(solveNQueensCount)
import sudoku(solveBoard, samplePuzzle)
import binaryTree(sampleTree)
import brainfuck(sampleAddOutput)
import stressTest(runStressTest)
import stressTestTwo(runStressTestTwo)
import quicksort(sampleSortedQuickSort)
import mergesort(sampleSortedMergeSort)
import knapsack(sampleTreeDp)
import treeMap(fromList, Map, intComparator, Entry, size)
import tcTests.classes(advancedTypeclassRegressionMain)
import tcTests.tcStressTest(typeclassSuperclassSuccessMain);
import tcTests.tcImportClient(importSmokeMain)
import tcTests.coolStuff(coolTypeclassExamplesMain)

runSudokuReadyExample = solveBoard samplePuzzle
runNQueensReadyExample = solveNQueensCount 8
runBinaryTreeExample = sampleTree
runBrainfuckExample = sampleAddOutput
runQuicksortExample = sampleSortedQuickSort 20000
runMergeSortExample = sampleSortedMergeSort 20000
runKnapsackExample = sampleTreeDp


treeFromList : Map Int Int
treeFromList =
    let intEntries = (map (fun iVal => Entry iVal iVal) (range 1 20000 True)) in 
        fromList intEntries

{- if you want the results, just copy-paste the particular example directly into main -}
runAllExamplesAndDiscardResult = 
    let one = runSudokuReadyExample in
    let two = runNQueensReadyExample in
    let three = runBinaryTreeExample in 
    let four = runBrainfuckExample in
    let five = runQuicksortExample in
    let six = runMergeSortExample in
    let seven = runKnapsackExample in
    let eight = advancedTypeclassRegressionMain in
    let nine = typeclassSuperclassSuccessMain in
    let ten = importSmokeMain in
    let eleven = coolTypeclassExamplesMain in ()

main = runAllExamplesAndDiscardResult