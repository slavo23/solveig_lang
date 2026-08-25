import std.process(*)
import std.env(*)

processToHost : ProcessError -> HostError
processToHost (ProcessHostError _ hostError) = hostError

runProcessTask : String -> [String] -> Task ProcessResult
runProcessTask program arguments =
    mapError processToHost (std.process.runProcess program arguments)

runProcessLimitedTask : String -> [String] -> Int -> Task ProcessResult
runProcessLimitedTask program arguments maximum =
    mapError processToHost (std.process.runProcessLimited program arguments maximum)

reportRunProcessError : HostError -> Task ()
reportRunProcessError ioError =
    if hostErrorOperation ioError == "runProcess"
        then printLine "run-process-error"
        else printLine "wrong-error"

reportLimitedError : String -> HostError -> Task String
reportLimitedError expected ioError =
    if hostErrorOperation ioError == "runProcess"
        then pureIO expected
        else pureIO "wrong-limited-error"

runFixture : String -> Task ()
runFixture executable = do
    setEnv "SOLVEIG_L3_CHILD_VALUE" "inherited"
    result <- runProcessTask executable ["--project-root", "child", "--", "alpha", "two words"]
    printLine (appendString "exit=" (show (processExitCode result)))
    printStr (processStdout result)
    printStr (processStderr result)
    limited <- runProcessLimitedTask executable ["--project-root", "child", "--", "alpha", "two words"] 26
    if and (processStdout limited == processStdout result) (processStderr limited == processStderr result) then
        printLine "limited-capture"
    else
        printLine "wrong-limited-capture"
    tooLarge <- catchError
        (bindIO
            (runProcessLimitedTask executable ["--project-root", "child", "--", "alpha", "two words"] 12)
            (fun captured => pureIO "wrong-limit-success"))
        (reportLimitedError "process-output-limit")
    negative <- catchError
        (bindIO
            (runProcessLimitedTask executable [] (0 - 1))
            (fun captured => pureIO "wrong-negative-success"))
        (reportLimitedError "negative-process-limit")
    printLine tooLarge
    printLine negative

runFailure : String -> Task ()
runFailure executable = do
    result <- runProcessTask executable ["--definitely-invalid-l3-flag"]
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
                    (runProcessTask "solveig-l3-command-that-does-not-exist-9d42" [])
                    (fun result => printLine "wrong-success"))
                reportRunProcessError
        | _ => printLine "missing-test-executable"
