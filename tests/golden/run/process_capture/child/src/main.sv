import std.env(*)
import std.fs(FsError(..), doesFileExist)
import std.io(*)
import std.path(*)
import std.time(milliseconds, sleep)

printMaybe : Maybe String -> Task ()
printMaybe value =
    case value of
        Just text => printLine text
        | Nothing => printLine "missing"

printCwd : Task ()
printCwd =
    case pathFromString "src/main.sv" of
        Left error => printLine "cwd-wrong"
        | Right manifest => do
            exists <- mapError fsToHost (doesFileExist manifest)
            if exists then printLine "cwd-ok" else printLine "cwd-wrong"

fsToHost : FsError -> HostError
fsToHost (FsHostError _ hostError) = hostError
fsToHost (FsInvalidUtf8 _ utf8Error) = HostError OtherError "fs" (show utf8Error)

main : Task ()
main = do
    arguments <- getArgs
    if hasDelay arguments then sleep (milliseconds 250) else pureIO ()
    inherited <- getEnv "SOLVEIG_L3_CHILD_VALUE"
    removed <- getEnv "SOLVEIG_L3_REMOVED"
    input <- hGetContents stdin
    printMaybe inherited
    printMaybe removed
    printCwd
    printLine input
    mapIO_ printLine arguments
    hPutStrLn stderr "child-stderr"

hasDelay : [String] -> Bool
hasDelay [] = False
hasDelay (argument :: rest) =
    if argument == "delay" then True else hasDelay rest
