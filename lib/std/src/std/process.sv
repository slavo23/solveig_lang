export (
    Process, ProcessError(..), process, withArgument, withArguments, processProgram,
    processArguments, ProcessResult, processExitCode, processStdout, processStderr,
    ExitCode, exitCodeFromInt, exitCodeToInt, isExitSuccess, runProcess,
    runProcessLimited, exitWith, exitSuccess, exitFailure
)

import prelude.prim(*)
import std.text(charToCodePoint, stringToChars)

-- | A validated child-process request.
-- |
-- | The program is passed directly to the host process API; no shell is used.
data Process = Process String [String]

instance Eq Process where
    (==) (Process leftProgram leftArguments) (Process rightProgram rightArguments) =
        and (leftProgram == rightProgram) (leftArguments == rightArguments)

instance Show Process where
    show (Process programName []) = "process " <> show programName
    show (Process programName arguments) =
        "process " <> show programName <> " " <> show arguments

-- | A child-process validation or host failure.
data ProcessError =
    InvalidProcess String
  | ProcessHostError Process HostError

instance Eq ProcessError where
    (==) (InvalidProcess left) (InvalidProcess right) = left == right
    (==) (ProcessHostError leftProcess leftHost) (ProcessHostError rightProcess rightHost) =
        and (leftProcess == rightProcess) (leftHost == rightHost)
    (==) _ _ = False

instance Show ProcessError where
    show (InvalidProcess reason) = "invalid process: " <> reason
    show (ProcessHostError processValue hostError) =
        show processValue <> ": " <> show hostError

-- | Builds a process request after validating the program and arguments.
process : String -> [String] -> Either ProcessError Process
process programName arguments =
    if programName == "" then Left (InvalidProcess "the program cannot be empty")
    else if containsNull programName then Left (InvalidProcess "the program contains a null character")
    else if argumentsContainNull arguments then
        Left (InvalidProcess "an argument contains a null character")
    else Right (Process programName arguments)

-- | Appends one argument to a process request.
withArgument : String -> Process -> Either ProcessError Process
withArgument argument (Process programName arguments) =
    process programName (append arguments [argument])

-- | Appends arguments to a process request in their given order.
withArguments : [String] -> Process -> Either ProcessError Process
withArguments extra (Process programName arguments) =
    process programName (append arguments extra)

-- | Returns the requested program spelling.
processProgram : Process -> String
processProgram (Process programName _) = programName

-- | Returns the process arguments in invocation order.
processArguments : Process -> [String]
processArguments (Process _ arguments) = arguments

-- | A completed process's opaque exit status.
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

-- | Converts an integer status to an exit code. Zero denotes success.
exitCodeFromInt : Int -> ExitCode
exitCodeFromInt = ExitCode

-- | Returns the integer status represented by an exit code.
exitCodeToInt : ExitCode -> Int
exitCodeToInt (ExitCode value) = value

-- | Reports whether an exit code denotes successful completion.
isExitSuccess : ExitCode -> Bool
isExitSuccess (ExitCode value) = value == 0

-- | Captured output and status from a completed child process.
data ProcessResult = MkProcessResult Int String String

instance Eq ProcessResult where
    (==)
        (MkProcessResult leftCode leftOutput leftError)
        (MkProcessResult rightCode rightOutput rightError) =
            and
                (leftCode == rightCode)
                (and (leftOutput == rightOutput) (leftError == rightError))

instance Show ProcessResult where
    show result =
        "ProcessResult "
            <> show (processExitCode result)
            <> " "
            <> show (processStdout result)
            <> " "
            <> show (processStderr result)

-- | Returns a child process's exit code.
processExitCode : ProcessResult -> ExitCode
processExitCode (MkProcessResult exitCode output errorOutput) = ExitCode exitCode

-- | Returns a child process's captured standard output.
processStdout : ProcessResult -> String
processStdout (MkProcessResult exitCode output errorOutput) = output

-- | Returns a child process's captured standard error.
processStderr : ProcessResult -> String
processStderr (MkProcessResult exitCode output errorOutput) = errorOutput

-- | Runs a child process and strictly captures its output without a library size limit.
runProcess : Process -> IO ProcessError ProcessResult
runProcess processValue =
    mapError
        (ProcessHostError processValue)
        (prelude.prim.runProcess
            (processProgram processValue)
            (processArguments processValue))

-- | Runs a child process, capturing at most maximum bytes from each output stream.
runProcessLimited : Process -> Int -> IO ProcessError ProcessResult
runProcessLimited processValue maximum =
    mapError
        (ProcessHostError processValue)
        (prelude.prim.runProcessLimited
            (processProgram processValue)
            (processArguments processValue)
            maximum)

-- | Terminates the main program with a host exit status after cleanup.
exitWith : forall a. ExitCode -> Task a
exitWith status = prelude.prim.exitWith (exitCodeToInt status)

-- | Terminates the main program successfully after cleanup.
exitSuccess : forall a. Task a
exitSuccess = exitWith (ExitCode 0)

-- | Terminates the main program unsuccessfully after cleanup.
exitFailure : forall a. Task a
exitFailure = exitWith (ExitCode 1)

containsNull : String -> Bool
containsNull text = charsContainNull (std.text.stringToChars text)

charsContainNull : [Char] -> Bool
charsContainNull [] = False
charsContainNull (character :: rest) =
    if std.text.charToCodePoint character == 0 then True else charsContainNull rest

argumentsContainNull : [String] -> Bool
argumentsContainNull [] = False
argumentsContainNull (argument :: rest) =
    if containsNull argument then True else argumentsContainNull rest
