export (
    EnvError(..), getArgs, getEnv, getEnvOr, requireEnv, setEnv, getEnvironment,
    getProgName, parseEnvFile, loadEnvFile, loadEnvFileLimited, loadEnvFileIfExists,
    describeEnvError
)

import prelude.prim(getArgs, getEnv, setEnv, getEnvironment, getProgName)
import std.fs
import std.io
import std.list(reverse)
import std.map
import std.text

-- | A failure while resolving or loading application environment settings.
data EnvError =
    EnvIoError IOError
  | MissingEnv String
  | InvalidEnvLine Int String

-- | Reads the command-line arguments passed to the program.
getArgs : forall e. IO e [String]
getArgs = prelude.prim.getArgs

-- | Reads a runtime-local environment variable when it is present.
getEnv : String -> Task (Maybe String)
getEnv name = prelude.prim.getEnv name

-- | Reads an environment variable, returning the fallback when it is absent.
getEnvOr : String -> String -> Task String
getEnvOr name fallback =
    fmap (fun value => case value of Nothing => fallback | Just found => found) (getEnv name)

-- | Reads a required environment variable or returns MissingEnv.
requireEnv : String -> IO EnvError String
requireEnv name = do
    value <- mapError EnvIoError (getEnv name)
    case value of
        Nothing => failIO (MissingEnv name)
        | Just found => pureIO found

-- | Sets a runtime-local environment variable inherited by child processes.
setEnv : String -> String -> Task ()
setEnv name value = prelude.prim.setEnv name value

-- | Returns the runtime environment sorted by variable name.
getEnvironment : Task [(String, String)]
getEnvironment = prelude.prim.getEnvironment

-- | Returns the current Solveig project's directory name.
getProgName : Task String
getProgName = prelude.prim.getProgName

-- | Parses strict dotenv assignments without interpolation or shell expansion.
-- |
-- | Blank lines and lines beginning with # are ignored. An optional `export `
-- | prefix and matching single or double quotes around values are accepted.
-- | Duplicate names and invalid ASCII environment names are rejected without
-- | retaining the possibly secret source line in the error.
parseEnvFile : String -> Result EnvError [(String, String)]
parseEnvFile text =
    parseEnvLines (std.text.lines text) 1 std.map.empty []

-- | Loads a dotenv file no larger than 64 KiB without replacing existing variables.
loadEnvFile : String -> IO EnvError ()
loadEnvFile path = loadEnvFileLimited path 65536

-- | Loads a bounded dotenv file without replacing variables already in the process environment.
loadEnvFileLimited : String -> Int -> IO EnvError ()
loadEnvFileLimited path maximum = do
    contents <- mapError EnvIoError (std.io.readFileLimited path maximum)
    case parseEnvFile contents of
        Err parseError => failIO parseError
        | Ok entries => setMissingEntries entries

-- | Loads a dotenv file when it exists and reports whether it was present.
loadEnvFileIfExists : String -> IO EnvError Bool
loadEnvFileIfExists path = do
    exists <- mapError EnvIoError (std.fs.doesFileExist path)
    if exists then do
        loadEnvFile path
        pureIO True
    else pureIO False

-- | Renders an environment error without including dotenv source contents.
describeEnvError : EnvError -> String
describeEnvError (EnvIoError _) = "environment IO failed"
describeEnvError (MissingEnv name) = "missing environment variable " <> name
describeEnvError (InvalidEnvLine line reason) =
    "invalid environment file line " <> show line <> ": " <> reason

parseEnvLines : [String] -> Int -> std.map.Map String () -> [(String, String)] -> Result EnvError [(String, String)]
parseEnvLines [] _ _ reversedEntries = Ok (reverse reversedEntries)
parseEnvLines (sourceLine :: rest) lineNumber names reversedEntries =
    let line = std.text.trimAscii sourceLine in
    if or (std.text.isEmpty line) (std.text.startsWith "#" line) then
        parseEnvLines rest (lineNumber + 1) names reversedEntries
    else
        let assignment = case std.text.stripPrefix "export " line of
            Nothing => line
            | Just exported => std.text.trimAscii exported
        in case std.text.splitOnce '=' assignment of
            Nothing => Err (InvalidEnvLine lineNumber "expected NAME=VALUE")
            | Just (rawName, rawValue) =>
                let name = std.text.trimAscii rawName in
                if not (validEnvName name) then
                    Err (InvalidEnvLine lineNumber "invalid variable name")
                else if std.map.member name names then
                    Err (InvalidEnvLine lineNumber "duplicate variable name")
                else case envValue lineNumber (std.text.trimAscii rawValue) of
                    Err valueError => Err valueError
                    | Ok value =>
                        parseEnvLines
                            rest
                            (lineNumber + 1)
                            (std.map.insert name () names)
                            ((name, value) :: reversedEntries)

validEnvName : String -> Bool
validEnvName name =
    case std.text.stringToChars name of
        [] => False
        | first :: rest => and (validEnvNameStart first) (allValidEnvNameChars rest)

validEnvNameStart : Char -> Bool
validEnvNameStart character =
    let point = std.text.charToCodePoint character in
    or
        (character == '_')
        (or
            (and (point >= 65) (point <= 90))
            (and (point >= 97) (point <= 122)))

allValidEnvNameChars : [Char] -> Bool
allValidEnvNameChars [] = True
allValidEnvNameChars (character :: rest) =
    let point = std.text.charToCodePoint character in
    and
        (or (validEnvNameStart character) (and (point >= 48) (point <= 57)))
        (allValidEnvNameChars rest)

envValue : Int -> String -> Result EnvError String
envValue _ "" = Ok ""
envValue lineNumber value =
    case std.text.stringToChars value of
        '\'' :: rest => quotedEnvValue lineNumber '\'' rest
        | '"' :: rest => quotedEnvValue lineNumber '"' rest
        | _ => Ok value

quotedEnvValue : Int -> Char -> [Char] -> Result EnvError String
quotedEnvValue lineNumber quote characters =
    case reverse characters of
        closing :: reversedValue =>
            if closing == quote then Ok (std.text.charsToString (reverse reversedValue))
            else Err (InvalidEnvLine lineNumber "unterminated quoted value")
        | [] => Err (InvalidEnvLine lineNumber "unterminated quoted value")

setMissingEntries : [(String, String)] -> IO EnvError ()
setMissingEntries [] = pureIO ()
setMissingEntries ((name, value) :: rest) = do
    existing <- mapError EnvIoError (getEnv name)
    case existing of
        Nothing => mapError EnvIoError (setEnv name value)
        | Just _ => pureIO ()
    setMissingEntries rest
