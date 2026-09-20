import std.env(*)
import std.fs(FsError(..), doesFileExist)
import std.io(*)
import std.path(*)
import std.time(milliseconds, sleep)

burstChunk : String
burstChunk = growText 8 "0123456789abcdef"

growText : Int -> String -> String
growText count text = if count <= 0 then text else growText (count - 1) (text <> text)

writeBurst : Int -> Task ()
writeBurst count =
    if count <= 0
        then pureIO ()
        else do
                hPutStr stdout burstChunk
                hPutStr stderr burstChunk
                writeBurst (count - 1)

runMode : [String] -> Task Bool
runMode [] = pureIO False
runMode (mode :: rest) =
    if mode == "stream-burst"
        then
            do
                writeBurst 40
                pureIO True
        else if mode == "stdin-echo"
                then
                    do
                        input <- hGetContents stdin
                        hPutStr stdout input
                        pureIO True
                else if mode == "delayed-output"
                        then
                            do
                                sleep (milliseconds 250)
                                printLine "late-child-output"
                                pureIO True
                        else pureIO False

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
            exists <- mapError fsToIO (doesFileExist manifest)
            if exists then printLine "cwd-ok" else printLine "cwd-wrong"

fsToIO : FsError -> IOError
fsToIO (FsIOError _ ioError) = ioError
fsToIO (FsInvalidUtf8 _ utf8Error) = IOError OtherError "fs" (show utf8Error)

main : Task ()
main = do
    arguments <- getArgs
    handled <- runMode arguments
    if handled
        then pureIO ()
        else do
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
hasDelay (argument :: rest) = if argument == "delay" then True else hasDelay rest
