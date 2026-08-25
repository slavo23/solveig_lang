import prelude.prim
import std.env(EnvError(..))
import std.fs(FsError(..))
import std.http(HttpError(..))
import std.net(NetError(..))
import std.process(ProcessError(..))
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
    if hostErrorOperation hostError == expected then printLine label
    else printLine ("unexpected operation " <> hostErrorOperation hostError)

reportMapped : HostError -> Task ()
reportMapped error = reportOperation "mapped" "mapped-error" error

reportRead : HostError -> Task ()
reportRead error = reportOperation "readFile" "read-file-error" error

reportWrite : HostError -> Task ()
reportWrite error = reportOperation "writeFile" "write-file-error" error

reportAppend : HostError -> Task ()
reportAppend error = reportOperation "appendFile" "append-file-error" error

infrastructureError : ConcurrentError DomainError
infrastructureError =
    ConcurrentInfrastructure (HostError PermissionDenied "openFile" "denied")

releaseResource : String -> Task ()
releaseResource name = printLine (appendString "release " name)

useResource : String -> Task ()
useResource name =
    bindIO
        (printLine (appendString "use " name))
        (fun value => failIO (testHostError "use"))

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
cleanupReplacesFailure =
    catchError
        (finally bodyFailure releaseFailure)
        reportWrite

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
        (bindIO
            stopAction
            (fun value => printLine "bind-ran"))
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
        (bindIO (prelude.prim.readFile "missing.txt") (fun contents => pureIO ()))
        reportRead
    catchError (prelude.prim.writeFile "missing/child.txt" "text") reportWrite
    catchError (prelude.prim.appendFile "missing/child.txt" "text") reportAppend
    let denied = HostError PermissionDenied "openFile" "denied"
    printLine (show denied)
    printLine (show (FsHostError "settings.conf" denied))
    printLine (show (ProcessHostError "tool" denied))
    printLine (show (NetHostError "example.test:443" denied))
    printLine (show (HttpHostError denied))
    printLine (show (ResponseBodyTooLarge 1024))
    printLine (show (InvalidResponseUtf8 (InvalidUtf8 7)))
    printLine (show (MissingEnv "TOKEN"))
    printLine (show infrastructureError)
    printLine (show (ConcurrentWorker Missing))
