import std.process
import std.env
reportRunProcessError : IOError -> Task ()
reportRunProcessError ioError =
    case ioError of
        RunProcessError _ _ => printLine "run-process-error"
        | _ => printLine "wrong-error"

runFixture : String -> Task ()
runFixture executable = do
    setEnv "SOLVEIG_L3_CHILD_VALUE" "inherited"
    result <- runProcess executable ["--project-root", "child", "--", "alpha", "two words"]
    printLine (appendString "exit=" (show (processExitCode result)))
    printStr (processStdout result)
    printStr (processStderr result)

runFailure : String -> Task ()
runFailure executable = do
    result <- runProcess executable ["--definitely-invalid-l3-flag"]
    printLine (appendString "failure-exit=" (show (processExitCode result)))
    if processStderr result == "" then
        printLine "missing-failure-stderr"
    else
        printLine "failure-stderr"

main : Task ()
main = do
    arguments <- getArgs
    case arguments of
        executable :: [] => do
            runFixture executable
            runFailure executable
            catchError
                (bindIO
                    (runProcess "solveig-l3-command-that-does-not-exist-9d42" [])
                    (fun result => printLine "wrong-success"))
                reportRunProcessError
        | _ => printLine "missing-test-executable"
