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

testHostError : String -> HostError
testHostError operation = HostError OtherError operation "test failure"

toHostError : DomainError -> HostError
toHostError Missing = testHostError "mapped"

reportOperation : String -> String -> HostError -> Task ()
reportOperation expected label hostError =
    if hostErrorOperation hostError == expected
        then printLine label
        else printLine ("unexpected operation " <> hostErrorOperation hostError)

reportMapped : HostError -> Task ()
reportMapped error = reportOperation "mapped" "mapped-error" error

reportWrite : HostError -> Task ()
reportWrite error = reportOperation "writeFile" "write-file-error" error

reportFs : String -> String -> FsError -> Task ()
reportFs operation label (FsHostError _ hostError) = reportOperation operation label hostError
reportFs _ label (FsInvalidUtf8 _ _) = printLine ("unexpected UTF-8 failure in " <> label)

reportReadFs : FsError -> Task ()
reportReadFs = reportFs "readFile" "read-file-error"

reportWriteFs : FsError -> Task ()
reportWriteFs = reportFs "writeFile" "write-file-error"

reportAppendFs : FsError -> Task ()
reportAppendFs = reportFs "appendFile" "append-file-error"

infrastructureError : ConcurrentError DomainError
infrastructureError = ConcurrentInfrastructure (HostError PermissionDenied "openFile" "denied")

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
    bindIO (printLine (appendString "use " name)) (fun value => failIO (testHostError "use"))

cleanupFailure : Task ()
cleanupFailure =
    catchError
        (finally (failIO (testHostError "body")) (printLine "cleanup-failure"))
        (fun error => printLine "caught-failure")

bodyFailure : Task ()
bodyFailure = failIO (testHostError "body")

releaseFailure : Task ()
releaseFailure = failIO (testHostError "writeFile")

cleanupReplacesFailure : Task ()
cleanupReplacesFailure = catchError (finally bodyFailure releaseFailure) reportWrite

bracketFailure : Task ()
bracketFailure =
    catchError
        (bracket (pureIO "resource") releaseResource useResource)
        (fun error => printLine "caught-bracket")

fallbackAction : Task ()
fallbackAction = orElse (failIO (testHostError "first")) (printLine "fallback")

stopAction : Task ()
stopAction = failIO (testHostError "stop")

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
    catchError (mapError toHostError domainFailure) reportMapped
    fallbackAction
    skippedBind
    catchError
        (bindIO (std.fs.readFile (fixturePath "missing.txt")) (fun contents => pureIO ()))
        reportReadFs
    catchError (std.fs.writeFile (fixturePath "missing/child.txt") "text") reportWriteFs
    catchError (std.fs.appendFile (fixturePath "missing/child.txt") "text") reportAppendFs
    let denied = HostError PermissionDenied "openFile" "denied"
    printLine (show denied)
    printLine (show (FsHostError (fixturePath "settings.conf") denied))
    printLine (show (ProcessHostError (fixtureProcess "tool") denied))
    printLine (show (NetHostError "example.test:443" denied))
    printLine (show (HttpHostError denied))
    printLine (show (ResponseBodyTooLarge 1024))
    printLine (show (InvalidResponseUtf8 (InvalidUtf8 7)))
    printLine (show (MissingEnv "TOKEN"))
    printLine (show infrastructureError)
    printLine (show (ConcurrentWorker Missing))
