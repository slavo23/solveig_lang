import std.net(*)
import std.task(*)
import std.channel(*)
import std.io(*)

data DomainError = Missing

domainFailure : IO DomainError ()
domainFailure = failIO Missing

toIOError : DomainError -> IOError
toIOError Missing = StdinError "mapped"

reportMapped : IOError -> Task ()
reportMapped error =
    case error of
        StdinError _ => printLine "mapped-error"
        | StdoutError _ => printLine "unexpected-stdout"
        | ReadFileError _ _ => printLine "unexpected-read"
        | WriteFileError _ _ => printLine "unexpected-write"
        | AppendFileError _ _ => printLine "unexpected-append"
        | ChannelClosed => printLine "unexpected-channel"
        | InvalidChannelCapacity _ => printLine "unexpected-channel-capacity"
        | InvalidUtf8 _ => printLine "unexpected-utf8"
        | ResolveHostError _ => printLine "unexpected-resolve-host"
        | ConnectError _ => printLine "unexpected-connect"
        | ListenError _ => printLine "unexpected-listen"
        | AcceptError _ => printLine "unexpected-accept"
        | SendError _ => printLine "unexpected-send"
        | RecvError _ => printLine "unexpected-recv"
        | CloseSocketError _ => printLine "unexpected-close-socket"
        | OpenHandleError _ _ => printLine "unexpected-open-handle"
        | CloseHandleError _ => printLine "unexpected-close-handle"
        | FlushHandleError _ => printLine "unexpected-flush-handle"
        | ReadHandleError _ => printLine "unexpected-read-handle"
        | WriteHandleError _ => printLine "unexpected-write-handle"
        | SetBufferingError _ => printLine "unexpected-buffering"
        | GetEnvError _ => printLine "unexpected-get-env"
        | SetEnvError _ => printLine "unexpected-set-env"
        | GetCurrentDirectoryError _ => printLine "unexpected-get-cwd"
        | SetCurrentDirectoryError _ _ => printLine "unexpected-set-cwd"
        | DoesFileExistError _ _ => printLine "unexpected-file-exists"
        | DoesDirectoryExistError _ _ => printLine "unexpected-directory-exists"
        | ListDirectoryError _ _ => printLine "unexpected-list-directory"
        | CreateDirectoryError _ _ => printLine "unexpected-create-directory"
        | RemoveFileError _ _ => printLine "unexpected-remove-file"
        | RenameFileError _ _ _ => printLine "unexpected-rename-file"
        | GetFileSizeError _ _ => printLine "unexpected-file-size"
        | GetModificationTimeError _ _ => printLine "unexpected-modification-time"
        | RunProcessError _ _ => printLine "unexpected-run-process"
        | _ => printLine "unexpected-http"

reportRead : IOError -> Task ()
reportRead error =
    case error of
        ReadFileError _ _ => printLine "read-file-error"
        | StdoutError _ => printLine "unexpected-stdout"
        | StdinError _ => printLine "unexpected-stdin"
        | WriteFileError _ _ => printLine "unexpected-write"
        | AppendFileError _ _ => printLine "unexpected-append"
        | ChannelClosed => printLine "unexpected-channel"
        | InvalidChannelCapacity _ => printLine "unexpected-channel-capacity"
        | InvalidUtf8 _ => printLine "unexpected-utf8"
        | ResolveHostError _ => printLine "unexpected-resolve-host"
        | ConnectError _ => printLine "unexpected-connect"
        | ListenError _ => printLine "unexpected-listen"
        | AcceptError _ => printLine "unexpected-accept"
        | SendError _ => printLine "unexpected-send"
        | RecvError _ => printLine "unexpected-recv"
        | CloseSocketError _ => printLine "unexpected-close-socket"
        | OpenHandleError _ _ => printLine "unexpected-open-handle"
        | CloseHandleError _ => printLine "unexpected-close-handle"
        | FlushHandleError _ => printLine "unexpected-flush-handle"
        | ReadHandleError _ => printLine "unexpected-read-handle"
        | WriteHandleError _ => printLine "unexpected-write-handle"
        | SetBufferingError _ => printLine "unexpected-buffering"
        | GetEnvError _ => printLine "unexpected-get-env"
        | SetEnvError _ => printLine "unexpected-set-env"
        | GetCurrentDirectoryError _ => printLine "unexpected-get-cwd"
        | SetCurrentDirectoryError _ _ => printLine "unexpected-set-cwd"
        | DoesFileExistError _ _ => printLine "unexpected-file-exists"
        | DoesDirectoryExistError _ _ => printLine "unexpected-directory-exists"
        | ListDirectoryError _ _ => printLine "unexpected-list-directory"
        | CreateDirectoryError _ _ => printLine "unexpected-create-directory"
        | RemoveFileError _ _ => printLine "unexpected-remove-file"
        | RenameFileError _ _ _ => printLine "unexpected-rename-file"
        | GetFileSizeError _ _ => printLine "unexpected-file-size"
        | GetModificationTimeError _ _ => printLine "unexpected-modification-time"
        | RunProcessError _ _ => printLine "unexpected-run-process"
        | _ => printLine "unexpected-http"

reportWrite : IOError -> Task ()
reportWrite error =
    case error of
        WriteFileError _ _ => printLine "write-file-error"
        | StdoutError _ => printLine "unexpected-stdout"
        | StdinError _ => printLine "unexpected-stdin"
        | ReadFileError _ _ => printLine "unexpected-read"
        | AppendFileError _ _ => printLine "unexpected-append"
        | ChannelClosed => printLine "unexpected-channel"
        | InvalidChannelCapacity _ => printLine "unexpected-channel-capacity"
        | InvalidUtf8 _ => printLine "unexpected-utf8"
        | ResolveHostError _ => printLine "unexpected-resolve-host"
        | ConnectError _ => printLine "unexpected-connect"
        | ListenError _ => printLine "unexpected-listen"
        | AcceptError _ => printLine "unexpected-accept"
        | SendError _ => printLine "unexpected-send"
        | RecvError _ => printLine "unexpected-recv"
        | CloseSocketError _ => printLine "unexpected-close-socket"
        | OpenHandleError _ _ => printLine "unexpected-open-handle"
        | CloseHandleError _ => printLine "unexpected-close-handle"
        | FlushHandleError _ => printLine "unexpected-flush-handle"
        | ReadHandleError _ => printLine "unexpected-read-handle"
        | WriteHandleError _ => printLine "unexpected-write-handle"
        | SetBufferingError _ => printLine "unexpected-buffering"
        | GetEnvError _ => printLine "unexpected-get-env"
        | SetEnvError _ => printLine "unexpected-set-env"
        | GetCurrentDirectoryError _ => printLine "unexpected-get-cwd"
        | SetCurrentDirectoryError _ _ => printLine "unexpected-set-cwd"
        | DoesFileExistError _ _ => printLine "unexpected-file-exists"
        | DoesDirectoryExistError _ _ => printLine "unexpected-directory-exists"
        | ListDirectoryError _ _ => printLine "unexpected-list-directory"
        | CreateDirectoryError _ _ => printLine "unexpected-create-directory"
        | RemoveFileError _ _ => printLine "unexpected-remove-file"
        | RenameFileError _ _ _ => printLine "unexpected-rename-file"
        | GetFileSizeError _ _ => printLine "unexpected-file-size"
        | GetModificationTimeError _ _ => printLine "unexpected-modification-time"
        | RunProcessError _ _ => printLine "unexpected-run-process"
        | _ => printLine "unexpected-http"

reportAppend : IOError -> Task ()
reportAppend error =
    case error of
        AppendFileError _ _ => printLine "append-file-error"
        | StdoutError _ => printLine "unexpected-stdout"
        | StdinError _ => printLine "unexpected-stdin"
        | ReadFileError _ _ => printLine "unexpected-read"
        | WriteFileError _ _ => printLine "unexpected-write"
        | ChannelClosed => printLine "unexpected-channel"
        | InvalidChannelCapacity _ => printLine "unexpected-channel-capacity"
        | InvalidUtf8 _ => printLine "unexpected-utf8"
        | ResolveHostError _ => printLine "unexpected-resolve-host"
        | ConnectError _ => printLine "unexpected-connect"
        | ListenError _ => printLine "unexpected-listen"
        | AcceptError _ => printLine "unexpected-accept"
        | SendError _ => printLine "unexpected-send"
        | RecvError _ => printLine "unexpected-recv"
        | CloseSocketError _ => printLine "unexpected-close-socket"
        | OpenHandleError _ _ => printLine "unexpected-open-handle"
        | CloseHandleError _ => printLine "unexpected-close-handle"
        | FlushHandleError _ => printLine "unexpected-flush-handle"
        | ReadHandleError _ => printLine "unexpected-read-handle"
        | WriteHandleError _ => printLine "unexpected-write-handle"
        | SetBufferingError _ => printLine "unexpected-buffering"
        | GetEnvError _ => printLine "unexpected-get-env"
        | SetEnvError _ => printLine "unexpected-set-env"
        | GetCurrentDirectoryError _ => printLine "unexpected-get-cwd"
        | SetCurrentDirectoryError _ _ => printLine "unexpected-set-cwd"
        | DoesFileExistError _ _ => printLine "unexpected-file-exists"
        | DoesDirectoryExistError _ _ => printLine "unexpected-directory-exists"
        | ListDirectoryError _ _ => printLine "unexpected-list-directory"
        | CreateDirectoryError _ _ => printLine "unexpected-create-directory"
        | RemoveFileError _ _ => printLine "unexpected-remove-file"
        | RenameFileError _ _ _ => printLine "unexpected-rename-file"
        | GetFileSizeError _ _ => printLine "unexpected-file-size"
        | GetModificationTimeError _ _ => printLine "unexpected-modification-time"
        | RunProcessError _ _ => printLine "unexpected-run-process"
        | _ => printLine "unexpected-http"

releaseResource : String -> Task ()
releaseResource name = printLine (appendString "release " name)

useResource : String -> Task ()
useResource name =
    bindIO
        (printLine (appendString "use " name))
        (fun value => failIO (StdinError "use failed"))

cleanupFailure : Task ()
cleanupFailure =
    catchError
        (finally (failIO (StdinError "body")) (printLine "cleanup-failure"))
        (fun error => printLine "caught-failure")

bodyFailure : Task ()
bodyFailure = failIO (StdinError "body")

releaseFailure : Task ()
releaseFailure = failIO (WriteFileError "cleanup" "failed")

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
fallbackAction = orElse (failIO (StdinError "first")) (printLine "fallback")

stopAction : Task ()
stopAction = failIO (StdinError "stop")

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
    catchError (mapError toIOError domainFailure) reportMapped
    fallbackAction
    skippedBind
    catchError
        (bindIO (readFile "missing.txt") (fun contents => pureIO ()))
        reportRead
    catchError (writeFile "missing/child.txt" "text") reportWrite
    catchError (appendFile "missing/child.txt" "text") reportAppend
