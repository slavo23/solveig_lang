export (
    ProcessResult, processExitCode, processStdout, processStderr, runProcess,
    runProcessLimited, exitWith, exitSuccess, exitFailure
)

import prelude.prim

-- | Captured output and status from a completed child process.
data ProcessResult =
    -- | Builds a result from exit code, standard output, and standard error.
    MkProcessResult Int String String

-- | Returns a child process's exit code.
processExitCode : ProcessResult -> Int
processExitCode (MkProcessResult exitCode output errorOutput) = exitCode

-- | Returns a child process's captured standard output.
processStdout : ProcessResult -> String
processStdout (MkProcessResult exitCode output errorOutput) = output

-- | Returns a child process's captured standard error.
processStderr : ProcessResult -> String
processStderr (MkProcessResult exitCode output errorOutput) = errorOutput

-- | Runs a child process and strictly captures its output without a library size limit.
runProcess : String -> [String] -> Task ProcessResult
runProcess program arguments = prelude.prim.runProcess program arguments

-- | Runs a child process, capturing at most maximum bytes from each output stream.
runProcessLimited : String -> [String] -> Int -> Task ProcessResult
runProcessLimited program arguments maximum =
    prelude.prim.runProcessLimited program arguments maximum

-- | Terminates the main program with a host exit status after cleanup.
exitWith : forall a. Int -> Task a
exitWith status = prelude.prim.exitWith status

-- | Terminates the main program successfully after cleanup.
exitSuccess : forall a. Task a
exitSuccess = exitWith 0

-- | Terminates the main program unsuccessfully after cleanup.
exitFailure : forall a. Task a
exitFailure = exitWith 1
