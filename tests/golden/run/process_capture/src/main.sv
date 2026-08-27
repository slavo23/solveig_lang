import std.bytes(bytesLength)
import std.env(*)
import std.path(*)
import std.process(*)
import std.time(milliseconds)

processToHost : ProcessError -> HostError
processToHost (ProcessHostError _ hostError) = hostError
processToHost (InvalidProcess reason) = HostError InvalidArgument "process" reason

requireProcess : Either ProcessError Process -> Task Process
requireProcess request =
    case request of
        Left error => failIO (processToHost error)
        | Right processValue => pureIO processValue

requirePath : String -> Task Path
requirePath spelling =
    case pathFromString spelling of
        Left error => failIO (HostError InvalidArgument "path" (show error))
        | Right path => pureIO path

decode : Either Utf8Error String -> String
decode (Left decodeError) = error (show decodeError)
decode (Right text) = text

configuredProcess : String -> Task Process
configuredProcess executable = do
    childDirectory <- requirePath "child"
    initial <- requireProcess (process executable ["--project-root", ".", "--", "alpha", "two words"])
    overridden <- requireProcess (withEnvironment "SOLVEIG_L3_CHILD_VALUE" "overridden" initial)
    cleaned <- requireProcess (withoutEnvironment "SOLVEIG_L3_REMOVED" overridden)
    pureIO
        (withStandardInputText
            "from-stdin\n"
            (withWorkingDirectory childDirectory cleaned))

reportRunProcessError : HostError -> Task ()
reportRunProcessError ioError =
    if hostErrorOperation ioError == "runProcess"
        then printLine "run-process-error"
        else printLine "wrong-error"

reportLimitedError : HostError -> Task String
reportLimitedError ioError =
    if hostErrorOperation ioError == "runProcess"
        then pureIO "process-output-limit"
        else pureIO "wrong-limited-error"

runFixture : String -> Task ()
runFixture executable = do
    setEnv "SOLVEIG_L3_CHILD_VALUE" "inherited"
    setEnv "SOLVEIG_L3_REMOVED" "hidden"
    request <- configuredProcess executable
    result <- mapError processToHost (runProcess request)
    printLine ("exit=" <> show (exitCodeToInt (processExitCode result)))
    printStr (decode (processStdoutText result))
    printStr (decode (processStderrText result))
    limited <- mapError processToHost (runProcessLimited request 128)
    if and
        (processStdoutBytes limited == processStdoutBytes result)
        (processStderrBytes limited == processStderrBytes result)
        then printLine "limited-capture"
        else printLine "wrong-limited-capture"
    tooLarge <- catchError
        (bindIO
            (mapError processToHost (runProcessLimited request 12))
            (fun captured => pureIO "wrong-limit-success"))
        reportLimitedError
    printLine tooLarge
    discarded <- mapError processToHost (runProcess (discardStandardError request))
    if bytesLength (processStderrBytes discarded) == 0
        then printLine "discarded-stderr"
        else printLine "retained-stderr"
    timed <- mapError processToHost
        (runProcessTimeout
            (milliseconds 1)
            (withArgumentsOrDefect ["delay"] request))
    case timed of
        Nothing => printLine "process-timeout"
        | Just completed => printLine "wrong-timeout-success"

withArgumentsOrDefect : [String] -> Process -> Process
withArgumentsOrDefect arguments processValue =
    case withArguments arguments processValue of
        Left processError => error (show processError)
        | Right configured => configured

runFailure : String -> Task ()
runFailure executable = do
    request <- requireProcess (process executable ["--definitely-invalid-l3-flag"])
    result <- mapError processToHost (runProcess request)
    printLine ("failure-exit=" <> show (exitCodeToInt (processExitCode result)))
    if bytesLength (processStderrBytes result) == 0
        then printLine "missing-failure-stderr"
        else printLine "failure-stderr"

main : Task ()
main = do
    arguments <- getArgs
    case arguments of
        executable :: [] => do
            runFixture executable
            runFailure executable
            missing <- requireProcess (process "solveig-l3-command-that-does-not-exist-9d42" [])
            catchError
                (bindIO
                    (mapError processToHost (runProcess missing))
                    (fun result => printLine "wrong-success"))
                reportRunProcessError
        | _ => printLine "missing-test-executable"
