import std.env(*)
import std.fs(FsError(..))
import std.list(length)
import std.path(*)
import std.time(SystemTime, systemTimeNanoseconds)

fsToIO : FsError -> IOError
fsToIO (FsIOError _ ioError) = ioError
fsToIO (FsInvalidUtf8 path utf8Error) =
    IOError InvalidArgument "filesystemUtf8" (show path <> ": " <> show utf8Error)

fixturePath : String -> Path
fixturePath spelling =
    case pathFromString spelling of
        Right path => path
        | Left pathError => error ("invalid fixture path: " <> show pathError)

getCurrentDirectory : Task Path
getCurrentDirectory = mapError fsToIO std.fs.getCurrentDirectory

setCurrentDirectory : String -> Task ()
setCurrentDirectory path = mapError fsToIO (std.fs.setCurrentDirectory (fixturePath path))

doesFileExist : String -> Task Bool
doesFileExist path = mapError fsToIO (std.fs.doesFileExist (fixturePath path))

doesDirectoryExist : Path -> Task Bool
doesDirectoryExist path = mapError fsToIO (std.fs.doesDirectoryExist path)

listDirectory : String -> Task [String]
listDirectory path =
    fmap (map pathToString) (mapError fsToIO (std.fs.listDirectory (fixturePath path)))

createDirectory : String -> Task ()
createDirectory path = mapError fsToIO (std.fs.createDirectory (fixturePath path))

removeFile : String -> Task ()
removeFile path = mapError fsToIO (std.fs.removeFile (fixturePath path))

renameFile : String -> String -> Task ()
renameFile source destination =
    mapError fsToIO (std.fs.renameFile (fixturePath source) (fixturePath destination))

getFileSize : String -> Task Int
getFileSize path = mapError fsToIO (std.fs.getFileSize (fixturePath path))

getModificationTime : String -> Task SystemTime
getModificationTime path = mapError fsToIO (std.fs.getModificationTime (fixturePath path))

readFile : String -> Task String
readFile = mapError fsToIO . std.fs.readFile . fixturePath

writeFile : String -> String -> Task ()
writeFile path = mapError fsToIO . std.fs.writeFile (fixturePath path)

lookupEnvironment : String -> [(String, String)] -> Maybe String
;
lookupEnvironment name [] = Nothing
;
lookupEnvironment name ((key, value) :: rest) =
    if name == key then Just value else lookupEnvironment name rest
;

printMaybe : Maybe String -> Task ()
;
printMaybe value =
    case value of
        Just text => printLine text
        | Nothing => printLine "missing" ;

printRequired : String -> Task ()
;
printRequired name =
    catchError
        (bindIO (requireEnv name) (mapError EnvIOError . printLine))
        (fun envError => printLine (show envError))
;

loadSettings : String -> Task ()
;
loadSettings path =
    catchError (loadEnvFile (fixturePath path)) (fun envError => printLine (show envError))
;

missingEnvFile : EnvError -> Task Bool
;
missingEnvFile envError = do
    printLine (show envError)
    pureIO False

printEnvParse : Either EnvError [(String, String)] -> Task ()
;
printEnvParse (Right entries) = printLine ("env entries " <> show (length entries))
;
printEnvParse (Left envError) = printLine (show envError)
;

ignoreExistingDirectory : IOError -> Task ()
;
ignoreExistingDirectory ioError =
    if ioErrorOperation ioError == "createDirectory" then pureIO () else failIO ioError
;

reportError : IOError -> Task ()
;
reportError ioError =
    let operation =
        ioErrorOperation ioError
    in if operation == "getEnv"
        then printLine "get-env-error"
        else if operation == "setEnv"
                then printLine "set-env-error"
                else if operation == "setCurrentDirectory"
                        then printLine "set-cwd-error"
                        else if operation == "listDirectory"
                                then printLine "list-error"
                                else if operation == "createDirectory"
                                        then printLine "create-error"
                                        else if operation == "removeFile"
                                                then printLine "remove-error"
                                                else if operation == "renameFile"
                                                        then printLine "rename-error"
                                                        else if operation == "getFileSize"
                                                                then printLine "size-error"
                                                                else if operation
                                                                        == "getModificationTime"
                                                                        then
                                                                            printLine
                                                                                "modified-error"
                                                                        else printLine "wrong-error"
;

failAfter : Task a -> Task ()
;
failAfter action = catchError (bindIO action (fun value => printLine "wrong-success")) reportError
;

main : Task ()
;
main = do
    program <- getProgName
    printLine program
    absent <- getEnv "SOLVEIG_L3_GOLDEN_MISSING_9D42"
    printMaybe absent
    setEnv "SOLVEIG_L3_COLOR" "violet"
    color <- getEnv "SOLVEIG_L3_COLOR"
    printMaybe color
    environment <- getEnvironment
    printMaybe (lookupEnvironment "SOLVEIG_L3_COLOR" environment)
    fallback <- getEnvOr "SOLVEIG_L3_GOLDEN_MISSING_9D42" "fallback"
    printLine fallback
    printRequired "SOLVEIG_L3_COLOR"
    printRequired "SOLVEIG_L3_GOLDEN_MISSING_9D42"
    cwd <- getCurrentDirectory
    cwdExists <- doesDirectoryExist cwd
    if cwdExists then printLine "cwd" else printLine "wrong-cwd"
    fixtureFile <- doesFileExist "fixture/input.txt"
    printLine (show fixtureFile)
    missingFile <- doesFileExist "fixture/missing.txt"
    printLine (show missingFile)
    fixtureDirectory <- doesDirectoryExist (fixturePath "fixture")
    printLine (show fixtureDirectory)
    created <- catchError (createDirectory "generated") ignoreExistingDirectory
    writeFile "generated/data.txt" "abc"
    size <- getFileSize "generated/data.txt"
    printLine (show size)
    modified <- getModificationTime "generated/data.txt"
    if systemTimeNanoseconds modified > 0 then printLine "modified" else printLine "wrong-time"
    entries <- listDirectory "generated"
    mapIO_ printLine entries
    writeFile
        "generated/settings.env"
        "# application settings\nSOLVEIG_L3_COLOR=red\nexport SOLVEIG_L3_FROM_FILE='azure'\nSOLVEIG_L3_EMPTY=\n"
    loadSettings "generated/settings.env"
    loadedColor <- getEnv "SOLVEIG_L3_COLOR"
    printMaybe loadedColor
    loadedFile <- getEnv "SOLVEIG_L3_FROM_FILE"
    printMaybe loadedFile
    loadedEmpty <- getEnv "SOLVEIG_L3_EMPTY"
    printMaybe loadedEmpty
    missingDotEnv <-
        catchError (loadEnvFileIfExists (fixturePath "generated/missing.env")) missingEnvFile
    printLine (show missingDotEnv)
    printEnvParse (parseEnvFile "GOOD=one\nSECOND=two\n")
    printEnvParse (parseEnvFile "BAD-NAME=value\n")
    printEnvParse (parseEnvFile "DUP=one\nDUP=two\n")
    removeFile "generated/settings.env"
    renameFile "generated/data.txt" "generated/renamed.txt"
    oldExists <- doesFileExist "generated/data.txt"
    printLine (show oldExists)
    renamedExists <- doesFileExist "generated/renamed.txt"
    printLine (show renamedExists)
    removeFile "generated/renamed.txt"
    removedExists <- doesFileExist "generated/renamed.txt"
    printLine (show removedExists)
    setCurrentDirectory "fixture"
    contents <- readFile "input.txt"
    printStr contents
    setCurrentDirectory ".."
    failAfter (getEnv "")
    failAfter (setEnv "bad=name" "value")
    failAfter (setCurrentDirectory "missing")
    failAfter (listDirectory "missing")
    failAfter (createDirectory "missing/child")
    failAfter (removeFile "missing")
    failAfter (renameFile "missing" "other")
    failAfter (getFileSize "missing")
    failAfter (getModificationTime "missing")
