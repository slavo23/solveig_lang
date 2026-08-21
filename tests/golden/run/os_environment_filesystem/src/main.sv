import std.env
import std.fs
import std.io
import std.list(length)
lookupEnvironment : String -> [Pair String String] -> Maybe String;
lookupEnvironment name [] = Nothing;
lookupEnvironment name (MkPair key value :: rest) =
    if name == key then Just value else lookupEnvironment name rest;

printMaybe : Maybe String -> Task ();
printMaybe value =
    case value of
        Just text => printLine text
        | Nothing => printLine "missing";

printRequired : String -> Task ();
printRequired name =
    catchError
        (bindIO (requireEnv name) (mapError EnvIoError . printLine))
        (fun envError => printLine (describeEnvError envError));

loadSettings : String -> Task ();
loadSettings path =
    catchError
        (loadEnvFile path)
        (fun envError => printLine (describeEnvError envError));

missingEnvFile : EnvError -> Task Bool;
missingEnvFile envError = do
    printLine (describeEnvError envError)
    pureIO False;

printEnvParse : Result EnvError [Pair String String] -> Task ();
printEnvParse (Ok entries) = printLine ("env entries " <> show (length entries));
printEnvParse (Err envError) = printLine (describeEnvError envError);

ignoreExistingDirectory : IOError -> Task ();
ignoreExistingDirectory ioError =
    case ioError of
        CreateDirectoryError _ _ => pureIO ()
        | _ => failIO ioError;

reportError : IOError -> Task ();
reportError ioError =
    case ioError of
        GetEnvError _ => printLine "get-env-error"
        | SetEnvError _ => printLine "set-env-error"
        | SetCurrentDirectoryError _ _ => printLine "set-cwd-error"
        | ListDirectoryError _ _ => printLine "list-error"
        | CreateDirectoryError _ _ => printLine "create-error"
        | RemoveFileError _ _ => printLine "remove-error"
        | RenameFileError _ _ _ => printLine "rename-error"
        | GetFileSizeError _ _ => printLine "size-error"
        | GetModificationTimeError _ _ => printLine "modified-error"
        | _ => printLine "wrong-error";

failAfter : Task a -> Task ();
failAfter action = catchError (bindIO action (fun value => printLine "wrong-success")) reportError;

main : Task ();
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
    fixtureDirectory <- doesDirectoryExist "fixture"
    printLine (show fixtureDirectory)

    created <- catchError (createDirectory "generated") ignoreExistingDirectory
    writeFile "generated/data.txt" "abc"
    size <- getFileSize "generated/data.txt"
    printLine (show size)
    modified <- getModificationTime "generated/data.txt"
    if modified > 0 then printLine "modified" else printLine "wrong-time"
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
    missingDotEnv <- catchError
        (loadEnvFileIfExists "generated/missing.env")
        missingEnvFile
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
    failAfter (getModificationTime "missing");
