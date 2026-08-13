printMaybe : Maybe String -> Task ()
printMaybe value =
    case value of
        Just text => printLine text
        | Nothing => printLine "missing"

main : Task ()
main = do
    inherited <- getEnv "SOLVEIG_L3_CHILD_VALUE"
    printMaybe inherited
    arguments <- getArgs
    mapIO_ printLine arguments
    hPutStrLn stderr "child-stderr"
