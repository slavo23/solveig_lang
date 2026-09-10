export (
    Process,
    ProcessError(..),
    ProcessOutputPolicy(..),
    process,
    withArgument,
    withArguments,
    withWorkingDirectory,
    inheritProcessWorkingDirectory,
    inheritProcessEnvironment,
    clearProcessEnvironment,
    withEnvironment,
    withoutEnvironment,
    withStandardInput,
    withStandardInputText,
    withoutStandardInput,
    pipeStandardInput,
    captureStandardOutput,
    discardStandardOutput,
    captureStandardError,
    discardStandardError,
    processProgram,
    processArguments,
    processWorkingDirectory,
    ProcessResult,
    processExitCode,
    processStdoutBytes,
    processStderrBytes,
    processStdoutText,
    processStderrText,
    ExitCode,
    exitCodeFromInt,
    exitCodeToInt,
    isExitSuccess,
    runProcess,
    runProcessLimited,
    runProcessTimeout,
    ProcessHandle,
    withProcess,
    processStdout,
    processStderr,
    writeProcessStdin,
    closeProcessStdin,
    waitProcess,
    terminateProcess,
    exitWith,
    exitSuccess,
    exitFailure
)

import std.prim(ChildProcess, exitWith, runProcessConfigured, startProcessConfigured)
import std.bytes(bytesLength, bytesToString, stringToBytes)
import std.path(Path, pathToString)
import std.stream(Stream, emptyScoped, unfoldScoped)
import std.task(bracket, timeout)
import std.text(charToCodePoint, stringToChars)
import std.time(Duration)

-- | Selects whether one child output stream is retained in the result.
data ProcessOutputPolicy = CaptureOutput | DiscardOutput

instance Eq ProcessOutputPolicy where
    (==) CaptureOutput CaptureOutput = True
    (==) DiscardOutput DiscardOutput = True
    (==) _ _ = False

instance Show ProcessOutputPolicy where
    show CaptureOutput = "CaptureOutput"
    show DiscardOutput = "DiscardOutput"

data ProcessCommand = ProcessCommand String [String]

instance Eq ProcessCommand where
    (==) (ProcessCommand leftProgram leftArguments) (ProcessCommand rightProgram rightArguments) =
        and (leftProgram == rightProgram) (leftArguments == rightArguments)

data ProcessWorkingDirectory = InheritWorkingDirectory | WorkingDirectory Path

instance Eq ProcessWorkingDirectory where
    (==) InheritWorkingDirectory InheritWorkingDirectory = True
    (==) (WorkingDirectory left) (WorkingDirectory right) = left == right
    (==) _ _ = False

data ProcessEnvironmentBase = InheritEnvironment | EmptyEnvironment

instance Eq ProcessEnvironmentBase where
    (==) InheritEnvironment InheritEnvironment = True
    (==) EmptyEnvironment EmptyEnvironment = True
    (==) _ _ = False

data ProcessEnvironment = ProcessEnvironment ProcessEnvironmentBase [(String, String)] [String]

instance Eq ProcessEnvironment where
    (==)
        (ProcessEnvironment leftBase leftSets leftRemovals)
        (ProcessEnvironment rightBase rightSets rightRemovals) =
        and (leftBase == rightBase) (and (leftSets == rightSets) (leftRemovals == rightRemovals))

data ProcessInput = StrictInput Bytes | StreamInput

instance Eq ProcessInput where
    (==) (StrictInput left) (StrictInput right) = left == right
    (==) StreamInput StreamInput = True
    (==) _ _ = False

data ProcessOutputs = ProcessOutputs ProcessOutputPolicy ProcessOutputPolicy

instance Eq ProcessOutputs where
    (==)
        (ProcessOutputs leftStandardOutput leftStandardError)
        (ProcessOutputs rightStandardOutput rightStandardError) =
        and (leftStandardOutput == rightStandardOutput) (leftStandardError == rightStandardError)

data ProcessConfig
    = ProcessConfig ProcessWorkingDirectory ProcessEnvironment ProcessInput ProcessOutputs

instance Eq ProcessConfig where
    (==)
        (ProcessConfig leftCwd leftEnvironment leftInput leftOutputs)
        (ProcessConfig rightCwd rightEnvironment rightInput rightOutputs) =
        and
            (leftCwd == rightCwd)
            (and
                (leftEnvironment == rightEnvironment)
                (and (leftInput == rightInput) (leftOutputs == rightOutputs)))

-- | A validated strict child-process request.
-- |
-- | The program and arguments are passed directly to the host process API;
-- | no shell parses or expands them. The default request inherits Solveig's
-- | logical environment and working directory, has closed standard input, and
-- | captures both output streams as arbitrary bytes.
data Process = Process ProcessCommand ProcessConfig

instance Eq Process where
    (==) (Process leftCommand leftConfig) (Process rightCommand rightConfig) =
        and (leftCommand == rightCommand) (leftConfig == rightConfig)

instance Show Process where
    show processValue =
        "process "
            <> show (processProgram processValue)
            <> " "
            <> show (processArguments processValue)

-- | A child-process validation or host failure.
data ProcessError = InvalidProcess String | ProcessHostError Process HostError

instance Eq ProcessError where
    (==) (InvalidProcess left) (InvalidProcess right) = left == right
    (==) (ProcessHostError leftProcess leftHost) (ProcessHostError rightProcess rightHost) =
        and (leftProcess == rightProcess) (leftHost == rightHost)
    (==) _ _ = False

instance Show ProcessError where
    show (InvalidProcess reason) = "invalid process: " <> reason
    show (ProcessHostError processValue hostError) = show processValue <> ": " <> show hostError

-- | Builds a process request after validating the program and arguments.
process : String -> [String] -> Either ProcessError Process
process programName arguments =
    if programName == ""
        then Left (InvalidProcess "the program cannot be empty")
        else if containsNull programName
                then Left (InvalidProcess "the program contains a null character")
                else if argumentsContainNull arguments
                        then Left (InvalidProcess "an argument contains a null character")
                        else
                            Right
                                (Process
                                    (ProcessCommand programName arguments)
                                    (ProcessConfig
                                        InheritWorkingDirectory
                                        (ProcessEnvironment InheritEnvironment [] [])
                                        (StrictInput (stringToBytes ""))
                                        (ProcessOutputs CaptureOutput CaptureOutput)))

-- | Appends one argument in invocation order.
withArgument : String -> Process -> Either ProcessError Process
withArgument argument processValue = withArguments [argument] processValue

-- | Appends arguments in invocation order.
withArguments : [String] -> Process -> Either ProcessError Process
withArguments extra (Process (ProcessCommand programName arguments) config) =
    if argumentsContainNull extra
        then Left (InvalidProcess "an argument contains a null character")
        else Right (Process (ProcessCommand programName (append arguments extra)) config)

-- | Runs the process relative to a specific working directory.
withWorkingDirectory : Path -> Process -> Process
withWorkingDirectory cwd (Process command (ProcessConfig _ environment input outputs)) =
    Process command (ProcessConfig (WorkingDirectory cwd) environment input outputs)

-- | Restores use of the runtime's logical working directory.
inheritProcessWorkingDirectory : Process -> Process
inheritProcessWorkingDirectory (Process command (ProcessConfig _ environment input outputs)) =
    Process command (ProcessConfig InheritWorkingDirectory environment input outputs)

-- | Uses the runtime's logical environment as the base for explicit changes.
inheritProcessEnvironment : Process -> Process
inheritProcessEnvironment
    (Process command (ProcessConfig cwd (ProcessEnvironment _ sets removals) input outputs)) =
    Process
        command
        (ProcessConfig cwd (ProcessEnvironment InheritEnvironment sets removals) input outputs)

-- | Starts from an empty environment and removes all previous changes.
clearProcessEnvironment : Process -> Process
clearProcessEnvironment (Process command (ProcessConfig cwd _ input outputs)) =
    Process command (ProcessConfig cwd (ProcessEnvironment EmptyEnvironment [] []) input outputs)

-- | Sets one environment variable for the child.
withEnvironment : String -> String -> Process -> Either ProcessError Process
withEnvironment name value (Process
        command
        (ProcessConfig cwd (ProcessEnvironment base sets removals) input outputs)) =
    case validateEnvironment name value of
        Left error => Left error
        | Right _ =>
            Right
                (Process
                    command
                    (ProcessConfig
                        cwd
                        (ProcessEnvironment
                            base
                            (append (withoutEnvironmentEntry name sets) [(name, value)])
                            (filter (fun removedName => removedName /= name) removals))
                        input
                        outputs))

-- | Removes one inherited or explicitly set variable from the child.
withoutEnvironment : String -> Process -> Either ProcessError Process
withoutEnvironment
    name
    (Process command (ProcessConfig cwd (ProcessEnvironment base sets removals) input outputs)) =
    if invalidEnvironmentName name
        then
            Left (InvalidProcess "an environment name is empty or contains '=' or a null character")
        else
            Right
                (Process
                    command
                    (ProcessConfig
                        cwd
                        (ProcessEnvironment
                            base
                            (withoutEnvironmentEntry name sets)
                            (append
                                (filter (fun removedName => removedName /= name) removals)
                                [name]))
                        input
                        outputs))

-- | Supplies arbitrary bytes to standard input and closes it after writing.
withStandardInput : Bytes -> Process -> Process
withStandardInput input (Process command (ProcessConfig cwd environment _ outputs)) =
    Process command (ProcessConfig cwd environment (StrictInput input) outputs)

-- | Supplies UTF-8 text to standard input and closes it after writing.
withStandardInputText : String -> Process -> Process
withStandardInputText input = withStandardInput (stringToBytes input)

-- | Supplies an immediate end-of-file on standard input.
withoutStandardInput : Process -> Process
withoutStandardInput = withStandardInput (stringToBytes "")

-- | Exposes bounded writes to the child's standard input inside withProcess.
-- |
-- | Strict runProcess calls reject this policy because no scoped handle exists.
pipeStandardInput : Process -> Process
pipeStandardInput (Process command (ProcessConfig cwd environment _ outputs)) =
    Process command (ProcessConfig cwd environment StreamInput outputs)

-- | Retains standard output in the completed result.
captureStandardOutput : Process -> Process
captureStandardOutput
    (Process command (ProcessConfig cwd environment input (ProcessOutputs _ stderrPolicy))) =
    Process
        command
        (ProcessConfig cwd environment input (ProcessOutputs CaptureOutput stderrPolicy))

-- | Drains standard output without retaining it in memory.
discardStandardOutput : Process -> Process
discardStandardOutput
    (Process command (ProcessConfig cwd environment input (ProcessOutputs _ stderrPolicy))) =
    Process
        command
        (ProcessConfig cwd environment input (ProcessOutputs DiscardOutput stderrPolicy))

-- | Retains standard error in the completed result.
captureStandardError : Process -> Process
captureStandardError
    (Process command (ProcessConfig cwd environment input (ProcessOutputs stdoutPolicy _))) =
    Process
        command
        (ProcessConfig cwd environment input (ProcessOutputs stdoutPolicy CaptureOutput))

-- | Drains standard error without retaining it in memory.
discardStandardError : Process -> Process
discardStandardError
    (Process command (ProcessConfig cwd environment input (ProcessOutputs stdoutPolicy _))) =
    Process
        command
        (ProcessConfig cwd environment input (ProcessOutputs stdoutPolicy DiscardOutput))

processProgram : Process -> String
processProgram (Process (ProcessCommand programName _) _) = programName

processArguments : Process -> [String]
processArguments (Process (ProcessCommand _ arguments) _) = arguments

processWorkingDirectory : Process -> Maybe Path
processWorkingDirectory (Process _ (ProcessConfig cwd _ _ _)) = workingDirectoryPath cwd

-- | Binary-safe captured output and status from a completed child process.
data ProcessResult = MkProcessResult Int Bytes Bytes

instance Eq ProcessResult where
    (==) (MkProcessResult lc lo le) (MkProcessResult rc ro re) =
        and (lc == rc) (and (lo == ro) (le == re))

instance Show ProcessResult where
    show result =
        "ProcessResult "
            <> show (processExitCode result)
            <> " { stdoutBytes = "
            <> show (bytesLength (processStdoutBytes result))
            <> ", stderrBytes = "
            <> show (bytesLength (processStderrBytes result))
            <> " }"

data ExitCode = ExitCode Int

instance Eq ExitCode where
    (==) (ExitCode left) (ExitCode right) = left == right

instance PartialOrd ExitCode where
    partialCompare (ExitCode left) (ExitCode right) = partialCompare left right

instance Ord ExitCode where
    compare (ExitCode left) (ExitCode right) = compare left right

instance Show ExitCode where
    show (ExitCode 0) = "ExitSuccess"
    show (ExitCode value) = "ExitFailure " <> show value

instance Hashable ExitCode where
    hashWithSalt salt (ExitCode value) = hashWithSalt salt value

exitCodeFromInt : Int -> ExitCode
exitCodeFromInt = ExitCode

exitCodeToInt : ExitCode -> Int
exitCodeToInt (ExitCode value) = value

isExitSuccess : ExitCode -> Bool
isExitSuccess (ExitCode value) = value == 0

processExitCode : ProcessResult -> ExitCode
processExitCode (MkProcessResult exitCode _ _) = ExitCode exitCode

processStdoutBytes : ProcessResult -> Bytes
processStdoutBytes (MkProcessResult _ output _) = output

processStderrBytes : ProcessResult -> Bytes
processStderrBytes (MkProcessResult _ _ errorOutput) = errorOutput

-- | Decodes captured standard output as UTF-8 without replacement.
processStdoutText : ProcessResult -> Either Utf8Error String
processStdoutText = bytesToString . processStdoutBytes

-- | Decodes captured standard error as UTF-8 without replacement.
processStderrText : ProcessResult -> Either Utf8Error String
processStderrText = bytesToString . processStderrBytes

-- | Runs a child and captures each requested output stream without a size limit.
runProcess : Process -> IO ProcessError ProcessResult
runProcess processValue = runProcessWithLimit processValue Nothing

-- | Runs a child and captures at most maximum bytes from each captured stream.
runProcessLimited : Process -> Int -> IO ProcessError ProcessResult
runProcessLimited processValue maximum =
    if maximum < 0
        then failIO (InvalidProcess "the output limit must be non-negative")
        else runProcessWithLimit processValue (Just maximum)

-- | Runs a child until the duration expires.
-- |
-- | Expiry interrupts the blocked host request; the runtime closes the pipes,
-- | terminates the direct child, and waits for it before returning Nothing.
runProcessTimeout : Duration -> Process -> IO ProcessError (Maybe ProcessResult)
runProcessTimeout duration processValue = timeout duration (runProcess processValue)

runProcessWithLimit : Process -> Maybe Int -> IO ProcessError ProcessResult
runProcessWithLimit processValue maximum =
    case processValue of
        Process
                (ProcessCommand programName arguments)
                (ProcessConfig
                    cwd
                    (ProcessEnvironment environmentBase environment removed)
                    input
                    (ProcessOutputs stdoutPolicy stderrPolicy)) =>
            case input of
                StreamInput =>
                    failIO (InvalidProcess "streamed standard input requires withProcess")
                | StrictInput stdinBytes =>
                    mapError
                        (ProcessHostError processValue)
                        (std.prim.runProcessConfigured
                            (
                                programName,
                                arguments,
                                (
                                    workingDirectoryText cwd,
                                    inheritsEnvironment environmentBase,
                                    environment,
                                    removed,
                                    (False, stdinBytes),
                                    captures stdoutPolicy,
                                    captures stderrPolicy,
                                    maximum
                                )
                            ))

-- | A live direct child whose lifetime cannot escape withProcess.
data ProcessHandle scope = ProcessHandle Process ChildProcess Int ProcessInput ProcessOutputs

-- | Runs a child with demand-driven output streams and scoped cleanup.
-- |
-- | The positive chunk size bounds every stdout/stderr pull. CaptureOutput
-- | exposes that stream through processStdout or processStderr; DiscardOutput
-- | drains it concurrently and exposes an empty stream. Returning, failing,
-- | raising a defect, or interruption terminates and reaps a still-running
-- | child before the scope exits.
withProcess :
    forall result. Int ->
    Process ->
    (forall scope. ProcessHandle scope -> IO ProcessError result) -> IO ProcessError result
withProcess chunkSize processValue use =
    if chunkSize <= 0
        then failIO (InvalidProcess "the process stream chunk size must be positive")
        else
            bracket
                (mapProcessHostError
                    processValue
                    (std.prim.startProcessConfigured (streamingProcessRequest processValue)))
                (fun child => mapProcessHostError processValue (std.prim.closeProcess child))
                (fun child => use (processHandle processValue child chunkSize))

-- The public callback is universal in scope, so this private implementation
-- may instantiate the erased phantom with () without weakening escape safety.
processHandle : Process -> ChildProcess -> Int -> ProcessHandle ()
processHandle processValue child chunkSize =
    case processValue of
        Process _ (ProcessConfig _ _ input outputs) =>
            ProcessHandle processValue child chunkSize input outputs

-- | Pulls non-empty stdout chunks on demand, or ends immediately when discarded.
-- |
-- | Call once per handle and consume the returned stream through one continuation.
processStdout : forall scope. ProcessHandle scope -> Stream scope ProcessError Bytes
processStdout (ProcessHandle processValue child chunkSize _ (ProcessOutputs stdoutPolicy _)) =
    case stdoutPolicy of
        DiscardOutput => emptyScoped
        | CaptureOutput => unfoldScoped (processStdoutStep processValue child chunkSize) ()

-- | Pulls non-empty stderr chunks on demand, or ends immediately when discarded.
-- |
-- | Call once per handle and consume the returned stream through one continuation.
processStderr : forall scope. ProcessHandle scope -> Stream scope ProcessError Bytes
processStderr (ProcessHandle processValue child chunkSize _ (ProcessOutputs _ stderrPolicy)) =
    case stderrPolicy of
        DiscardOutput => emptyScoped
        | CaptureOutput => unfoldScoped (processStderrStep processValue child chunkSize) ()

processStdoutStep : Process -> ChildProcess -> Int -> () -> IO ProcessError (Maybe (Bytes, ()))
processStdoutStep processValue child chunkSize _ = do
    bytes <- mapProcessHostError processValue (std.prim.readProcessStdout child chunkSize)
    streamBytes bytes

processStderrStep : Process -> ChildProcess -> Int -> () -> IO ProcessError (Maybe (Bytes, ()))
processStderrStep processValue child chunkSize _ = do
    bytes <- mapProcessHostError processValue (std.prim.readProcessStderr child chunkSize)
    streamBytes bytes

streamBytes : Bytes -> IO ProcessError (Maybe (Bytes, ()))
streamBytes bytes = if bytesLength bytes == 0 then pureIO Nothing else pureIO (Just (bytes, ()))

-- | Writes one complete bounded chunk to streamed stdin with backpressure.
-- |
-- | Writes on one handle are intended to be sequenced rather than raced.
writeProcessStdin : forall scope. ProcessHandle scope -> Bytes -> IO ProcessError ()
writeProcessStdin (ProcessHandle processValue child chunkSize input _) bytes =
    case input of
        StrictInput _ => failIO (InvalidProcess "writeProcessStdin requires pipeStandardInput")
        | StreamInput =>
            if bytesLength bytes > chunkSize
                then
                    failIO
                        (InvalidProcess
                            ("standard-input chunk exceeds withProcess chunk size of "
                                <> show chunkSize
                                <> " bytes"))
                else mapProcessHostError processValue (std.prim.writeProcessStdin child bytes)

-- | Flushes, closes streamed stdin, and is idempotent after a successful close.
closeProcessStdin : forall scope. ProcessHandle scope -> IO ProcessError ()
closeProcessStdin (ProcessHandle processValue child _ input _) =
    case input of
        StrictInput _ => failIO (InvalidProcess "closeProcessStdin requires pipeStandardInput")
        | StreamInput => mapProcessHostError processValue (std.prim.closeProcessStdin child)

-- | Waits for natural child completion without closing unread output streams.
waitProcess : forall scope. ProcessHandle scope -> IO ProcessError ExitCode
waitProcess (ProcessHandle processValue child _ _ _) =
    fmap ExitCode (mapProcessHostError processValue (std.prim.waitProcess child))

-- | Terminates and reaps the direct child while leaving its pipes readable to EOF.
terminateProcess : forall scope. ProcessHandle scope -> IO ProcessError ExitCode
terminateProcess (ProcessHandle processValue child _ _ _) =
    fmap ExitCode (mapProcessHostError processValue (std.prim.terminateProcess child))

mapProcessHostError : forall a. Process -> IO HostError a -> IO ProcessError a
mapProcessHostError processValue = mapError (ProcessHostError processValue)

streamingProcessRequest processValue =
    case processValue of
        Process
                (ProcessCommand programName arguments)
                (ProcessConfig
                    cwd
                    (ProcessEnvironment environmentBase environment removed)
                    input
                    (ProcessOutputs stdoutPolicy stderrPolicy)) =>
            (
                programName,
                arguments,
                (
                    workingDirectoryText cwd,
                    inheritsEnvironment environmentBase,
                    environment,
                    removed,
                    streamingInput input,
                    captures stdoutPolicy,
                    captures stderrPolicy,
                    Nothing
                )
            )

streamingInput : ProcessInput -> (Bool, Bytes)
streamingInput (StrictInput bytes) = (False, bytes)
streamingInput StreamInput = (True, stringToBytes "")

workingDirectoryPath : ProcessWorkingDirectory -> Maybe Path
workingDirectoryPath InheritWorkingDirectory = Nothing
workingDirectoryPath (WorkingDirectory cwd) = Just cwd

workingDirectoryText : ProcessWorkingDirectory -> String
workingDirectoryText InheritWorkingDirectory = ""
workingDirectoryText (WorkingDirectory cwd) = pathToString cwd

inheritsEnvironment : ProcessEnvironmentBase -> Bool
inheritsEnvironment InheritEnvironment = True
inheritsEnvironment EmptyEnvironment = False

captures : ProcessOutputPolicy -> Bool
captures CaptureOutput = True
captures DiscardOutput = False

validateEnvironment : String -> String -> Either ProcessError ()
validateEnvironment name value =
    if invalidEnvironmentName name
        then
            Left (InvalidProcess "an environment name is empty or contains '=' or a null character")
        else if containsNull value
                then Left (InvalidProcess "an environment value contains a null character")
                else Right ()

invalidEnvironmentName : String -> Bool
invalidEnvironmentName name =
    or (name == "") (or (containsNull name) (containsEquals (stringToChars name)))

withoutEnvironmentEntry : String -> [(String, String)] -> [(String, String)]
withoutEnvironmentEntry name = filter (fun (entryName, _) => entryName /= name)

containsEquals : [Char] -> Bool
containsEquals [] = False
containsEquals (character :: rest) =
    if charToCodePoint character == 61 then True else containsEquals rest

containsNull : String -> Bool
containsNull text = charsContainNull (stringToChars text)

charsContainNull : [Char] -> Bool
charsContainNull [] = False
charsContainNull (character :: rest) =
    if charToCodePoint character == 0 then True else charsContainNull rest

argumentsContainNull : [String] -> Bool
argumentsContainNull [] = False
argumentsContainNull (argument :: rest) =
    if containsNull argument then True else argumentsContainNull rest

exitWith : forall a. ExitCode -> Task a
exitWith status = std.prim.exitWith (exitCodeToInt status)

exitSuccess : forall a. Task a
exitSuccess = exitWith (ExitCode 0)

exitFailure : forall a. Task a
exitFailure = exitWith (ExitCode 1)
