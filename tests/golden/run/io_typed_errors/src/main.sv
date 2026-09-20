import std.env(EnvError(..))
import std.fs(FsError(..))
import std.http(HttpError(..))
import std.net(NetError(..))
import std.path(Path, pathFromString)
import std.process(Process, ProcessError(..), process)
import std.task(*)

data DomainError = Missing

instance Show DomainError where
    show Missing = "Missing"

domainFailure : IO DomainError ()
domainFailure = failIO Missing

testIOError : String -> IOError
testIOError operation = IOError OtherError operation "test failure"

toIOError : DomainError -> IOError
toIOError Missing = testIOError "mapped"

reportOperation : String -> String -> IOError -> Task ()
reportOperation expected label ioError =
    if ioErrorOperation ioError == expected
        then printLine label
        else printLine ("unexpected operation " <> ioErrorOperation ioError)

reportMapped : IOError -> Task ()
reportMapped error = reportOperation "mapped" "mapped-error" error

reportWrite : IOError -> Task ()
reportWrite error = reportOperation "writeFile" "write-file-error" error

reportFs : String -> String -> FsError -> Task ()
reportFs operation label (FsIOError _ ioError) = reportOperation operation label ioError
reportFs _ label (FsInvalidUtf8 _ _) = printLine ("unexpected UTF-8 failure in " <> label)

reportReadFs : FsError -> Task ()
reportReadFs = reportFs "readFile" "read-file-error"

reportWriteFs : FsError -> Task ()
reportWriteFs = reportFs "writeFile" "write-file-error"

reportAppendFs : FsError -> Task ()
reportAppendFs = reportFs "appendFile" "append-file-error"

infrastructureError : ConcurrentError DomainError
infrastructureError = ConcurrentInfrastructure (IOError PermissionDenied "openFile" "denied")

fixturePath : String -> Path
fixturePath spelling =
    case pathFromString spelling of
        Right path => path
        | Left _ => error "fixture path invalid"

fixtureProcess : String -> Process
fixtureProcess program =
    case process program [] of
        Right processValue => processValue
        | Left _ => error "fixture process invalid"

releaseResource : String -> Task ()
releaseResource name = printLine (appendString "release " name)

useResource : String -> Task ()
useResource name =
    bindIO (printLine (appendString "use " name)) (fun value => failIO (testIOError "use"))

cleanupFailure : Task ()
cleanupFailure =
    catchError
        (finally (failIO (testIOError "body")) (printLine "cleanup-failure"))
        (fun error => printLine "caught-failure")

bodyFailure : Task ()
bodyFailure = failIO (testIOError "body")

releaseFailure : Task ()
releaseFailure = failIO (testIOError "writeFile")

cleanupReplacesFailure : Task ()
cleanupReplacesFailure = catchError (finally bodyFailure releaseFailure) reportWrite

bracketFailure : Task ()
bracketFailure =
    catchError
        (bracket (pureIO "resource") releaseResource useResource)
        (fun error => printLine "caught-bracket")

fallbackAction : Task ()
fallbackAction = orElse (failIO (testIOError "first")) (printLine "fallback")

stopAction : Task ()
stopAction = failIO (testIOError "stop")

skippedBind : Task ()
skippedBind =
    catchError
        (bindIO stopAction (fun value => printLine "bind-ran"))
        (fun error => printLine "bind-skipped")

main : Task ()
main = do
    value <- finally (pureIO 7) (printLine "cleanup-success")
    printLine (appendString "value=" (show value))
    cleanupFailure
    cleanupReplacesFailure
    bracketFailure
    catchError (mapError toIOError domainFailure) reportMapped
    fallbackAction
    skippedBind
    catchError
        (bindIO (std.fs.readFile (fixturePath "missing.txt")) (fun contents => pureIO ()))
        reportReadFs
    catchError (std.fs.writeFile (fixturePath "missing/child.txt") "text") reportWriteFs
    catchError (std.fs.appendFile (fixturePath "missing/child.txt") "text") reportAppendFs
    let denied = IOError PermissionDenied "openFile" "denied"
    printLine (show denied)
    printLine (show (FsIOError (fixturePath "settings.conf") denied))
    printLine (show (ProcessIOError (fixtureProcess "tool") denied))
    printLine (show (NetIOError "example.test:443" denied))
    printLine (show (HttpIOError denied))
    printLine (show (ResponseBodyTooLarge 1024))
    printLine (show (InvalidResponseUtf8 (InvalidUtf8 7)))
    printLine (show (MissingEnv "TOKEN"))
    printLine (show infrastructureError)
    printLine (show (ConcurrentWorker Missing))
