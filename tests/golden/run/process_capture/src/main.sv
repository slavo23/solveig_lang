import std.bytes(bytesLength, stringToBytes)
import std.env(*)
import std.path(*)
import std.process(*)
import std.stream(Stream, fold)
import std.task(concurrently, timeout)
import std.time(milliseconds)

processToIO : ProcessError -> IOError
processToIO (ProcessIOError _ ioError) = ioError
processToIO (InvalidProcess reason) = IOError InvalidArgument "process" reason

requireProcess : Either ProcessError Process -> Task Process
requireProcess request =
    case request of
        Left error => failIO (processToIO error)
        | Right processValue => pureIO processValue

requirePath : String -> Task Path
requirePath spelling =
    case pathFromString spelling of
        Left error => failIO (IOError InvalidArgument "path" (show error))
        | Right path => pureIO path

decode : Either Utf8Error String -> String
decode (Left decodeError) = error (show decodeError)
decode (Right text) = text

configuredProcess : String -> Task Process
configuredProcess executable = do
    childDirectory <- requirePath "child"
    initial <-
        requireProcess (process executable ["--project-root", ".", "--", "alpha", "two words"])
    overridden <- requireProcess (withEnvironment "SOLVEIG_L3_CHILD_VALUE" "overridden" initial)
    cleaned <- requireProcess (withoutEnvironment "SOLVEIG_L3_REMOVED" overridden)
    pureIO (withStandardInputText "from-stdin\n" (withWorkingDirectory childDirectory cleaned))

reportRunProcessError : IOError -> Task ()
reportRunProcessError ioError =
    if ioErrorOperation ioError == "runProcess"
        then printLine "run-process-error"
        else printLine "wrong-error"

reportLimitedError : IOError -> Task String
reportLimitedError ioError =
    if ioErrorOperation ioError == "runProcess"
        then pureIO "process-output-limit"
        else pureIO "wrong-limited-error"

runFixture : String -> Task ()
runFixture executable = do
    setEnv "SOLVEIG_L3_CHILD_VALUE" "inherited"
    setEnv "SOLVEIG_L3_REMOVED" "hidden"
    request <- configuredProcess executable
    result <- mapError processToIO (runProcess request)
    printLine ("exit=" <> show (exitCodeToInt (processExitCode result)))
    printStr (decode (processStdoutText result))
    printStr (decode (processStderrText result))
    limited <- mapError processToIO (runProcessLimited request 128)
    if and
        (processStdoutBytes limited == processStdoutBytes result)
        (processStderrBytes limited == processStderrBytes result)
        then printLine "limited-capture"
        else printLine "wrong-limited-capture"
    tooLarge <-
        catchError
            (bindIO
                (mapError processToIO (runProcessLimited request 12))
                (fun captured => pureIO "wrong-limit-success"))
            reportLimitedError
    printLine tooLarge
    discarded <- mapError processToIO (runProcess (discardStandardError request))
    if bytesLength (processStderrBytes discarded) == 0
        then printLine "discarded-stderr"
        else printLine "retained-stderr"
    timed <-
        mapError
            processToIO
            (runProcessTimeout (milliseconds 1) (withArgumentsOrDefect ["delay"] request))
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
    result <- mapError processToIO (runProcess request)
    printLine ("failure-exit=" <> show (exitCodeToInt (processExitCode result)))
    if bytesLength (processStderrBytes result) == 0
        then printLine "missing-failure-stderr"
        else printLine "failure-stderr"

childRequest : String -> String -> Task Process
childRequest executable mode =
    requireProcess (process executable ["--project-root", "child", "--", mode])

streamByteCount : forall scope. Stream scope ProcessError Bytes -> IO ProcessError Int
streamByteCount = fold (fun total chunk => total + bytesLength chunk) 0

runStreamingPipes : String -> Task ()
runStreamingPipes executable = do
    request <- childRequest executable "stream-burst"
    (stdoutCount, stderrCount) <-
        mapError
            processToIO
            (withProcess
                4096
                request
                (fun child =>
                    concurrently
                        (streamByteCount (processStdout child))
                        (streamByteCount (processStderr child))
                ))
    if and (stdoutCount == 163840) (stderrCount == 163840)
        then printLine "streaming-dual-pipe"
        else printLine "wrong-streaming-dual-pipe"

runStreamingDiscard : String -> Task ()
runStreamingDiscard executable = do
    request <- childRequest executable "stream-burst"
    (stdoutCount, stderrCount, status) <-
        mapError
            processToIO
            (withProcess 4096 (discardStandardError request) streamingDiscardSession)
    if and (and (stdoutCount == 163840) (stderrCount == 0)) (isExitSuccess status)
        then printLine "streaming-discard"
        else printLine "wrong-streaming-discard"

streamingDiscardSession : forall scope. ProcessHandle scope -> IO ProcessError (Int, Int, ExitCode)
streamingDiscardSession child = do
    stdoutCount <- streamByteCount (processStdout child)
    stderrCount <- streamByteCount (processStderr child)
    status <- waitProcess child
    pureIO (stdoutCount, stderrCount, status)

runStreamingInput : String -> Task ()
runStreamingInput executable = do
    request <- childRequest executable "stdin-echo"
    outcome <-
        mapError
            processToIO
            (withProcess
                16
                (pipeStandardInput (discardStandardError request))
                streamingInputSession)
    case outcome of
        (True, 13, status) =>
            if isExitSuccess status
                then
                    do
                        printLine "streaming-input-bound"
                        printLine "streaming-stdin"
                else printLine "wrong-streaming-stdin-exit"
        | _ => printLine "wrong-streaming-stdin"

streamingInputSession : forall scope. ProcessHandle scope -> IO ProcessError (Bool, Int, ExitCode)
streamingInputSession child = do
    bounded <-
        catchError
            (bindIO
                (writeProcessStdin child (stringToBytes "0123456789abcdefg"))
                (fun _ => pureIO False))
            inputBoundError
    writeProcessStdin child (stringToBytes "bounded-")
    writeProcessStdin child (stringToBytes "stdin")
    closeProcessStdin child
    closeProcessStdin child
    status <- waitProcess child
    outputBytes <- streamByteCount (processStdout child)
    pureIO (bounded, outputBytes, status)

inputBoundError : ProcessError -> IO ProcessError Bool
inputBoundError (InvalidProcess _) = pureIO True
inputBoundError (ProcessIOError _ _) = pureIO False

runExplicitTermination : String -> Task ()
runExplicitTermination executable = do
    request <- childRequest executable "delayed-output"
    (status, stdoutCount, stderrCount) <-
        mapError processToIO (withProcess 32 request terminateSession)
    if and (and (not (isExitSuccess status)) (stdoutCount == 0)) (stderrCount == 0)
        then printLine "streaming-terminate"
        else printLine "wrong-streaming-terminate"

terminateSession : forall scope. ProcessHandle scope -> IO ProcessError (ExitCode, Int, Int)
terminateSession child = do
    status <- terminateProcess child
    stdoutCount <- streamByteCount (processStdout child)
    stderrCount <- streamByteCount (processStderr child)
    pureIO (status, stdoutCount, stderrCount)

runScopedTimeout : String -> Task ()
runScopedTimeout executable = do
    request <- childRequest executable "delayed-output"
    result <-
        mapError
            processToIO
            (timeout (milliseconds 1) (withProcess 32 request (fun child => waitProcess child)))
    case result of
        Nothing => printLine "streaming-timeout"
        | Just status => printLine "wrong-streaming-timeout"

main : Task ()
main = do
    arguments <- getArgs
    case arguments of
        executable :: [] => do
            runFixture executable
            runFailure executable
            runStreamingPipes executable
            runStreamingDiscard executable
            runStreamingInput executable
            runExplicitTermination executable
            runScopedTimeout executable
            missing <- requireProcess (process "solveig-l3-command-that-does-not-exist-9d42" [])
            catchError
                (bindIO
                    (mapError processToIO (runProcess missing))
                    (fun result => printLine "wrong-success"))
                reportRunProcessError
        | _ => printLine "missing-test-executable"
