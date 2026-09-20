import std.bytes(*)
import std.env(getArgs)
import std.http(*)
import std.task(timeout)
import std.time(milliseconds)

showHttpError : std.http.HttpError -> String
showHttpError (InvalidUrl _) = "invalid-url"
showHttpError (InvalidHeaderName _) = "invalid-header-name"
showHttpError (InvalidHeaderValue _) = "invalid-header-value"
showHttpError (InvalidStatus _) = "invalid-status"
showHttpError (InvalidHttpOption _ _) = "invalid-limit"
showHttpError (TransportError _) = "transport"
showHttpError (TlsError _) = "tls"
showHttpError (RedirectError _) = "redirect"
showHttpError (ResponseHeadersTooLarge maximum) = "headers-too-large:" <> show maximum
showHttpError (ResponseBodyTooLarge maximum) = "body-too-large:" <> show maximum
showHttpError (HttpIOError ioError) =
    case ioErrorKind ioError of
        TimeExpired => "timeout"
        | _ => "host"
showHttpError (InvalidResponseUtf8 _) = "invalid-response-utf8"
showHttpError (ManagedRequestHeader _) = "managed-header"
showHttpError (RequestFileError _ _) = "request-file"
showHttpError (RequestJsonError _) = "request-json"
showHttpError (BodyReplayError _) = "body-replay"
showHttpError ConflictingRequestBody = "conflicting-body"
showHttpError (UnexpectedStatus _) = "unexpected-status"

printResponse : String -> std.http.Response -> IO std.http.HttpError ()
printResponse label response =
    case std.http.responseText response of
        Left error => failIO error
        | Right body =>
            mapError
                HttpIOError
                (printLine
                    (label
                        <> ":"
                        <> show (std.http.statusCode (std.http.responseStatus response))
                        <> ":"
                        <> show (std.http.findHeader "etag" response)
                        <> ":"
                        <> body))

fetch : String -> std.http.HttpOptions -> String -> Task ()
fetch label configured spelling =
    catchError
        (std.http.getWith configured spelling >>= printResponse label)
        (fun error => printLine (label <> ":" <> showHttpError error))

baseArgument : [String] -> String
baseArgument (base :: _) = base
baseArgument [] = "http://127.0.0.1:1"

tlsArgument : [String] -> String
tlsArgument (_ :: tlsBase :: _) = tlsBase
tlsArgument _ = "https://127.0.0.1:1"

limits : Int -> Int -> Int -> RedirectPolicy -> HttpOptions
limits headerBytes bodyBytes timeoutMilliseconds redirects =
    withRedirectPolicy
        redirects
        (withTimeout
            (milliseconds timeoutMilliseconds)
            (withMaximumBufferedBodyBytes
                bodyBytes
                (withMaximumHeaderBytes headerBytes defaultOptions)))

cancelSlowRequest : String -> Task (Maybe std.http.Response)
cancelSlowRequest base =
    catchError
        (timeout
            (milliseconds 20)
            (std.http.getWith (limits 65536 1024 1000 std.http.NoRedirects) (base <> "/slow")))
        (fun requestError => pureIO Nothing)

main : Task ()
main = do
    args <- getArgs
    let base = baseArgument args
    let tlsBase = tlsArgument args
    catchError
        (std.http.getWith
            (withHeaders [("Accept", "text/plain")] (limits 65536 1024 1000 std.http.NoRedirects))
            (base <> "/chunked")
            >>= printResponse "fetch")
        (fun error => printLine ("fetch:" <> showHttpError error))
    fetch "chunked" (limits 65536 1024 1000 (std.http.FollowRedirects 2)) (base <> "/chunked")
    fetch "redirect" (limits 65536 1024 1000 (std.http.FollowRedirects 2)) (base <> "/redirect")
    fetch "status" (limits 65536 1024 1000 std.http.NoRedirects) (base <> "/missing")
    fetch "body-limit" (limits 65536 4 1000 std.http.NoRedirects) (base <> "/large")
    fetch "header-limit" (limits 80 1024 1000 std.http.NoRedirects) (base <> "/large-header")
    fetch "malformed" (limits 65536 1024 1000 std.http.NoRedirects) (base <> "/malformed")
    fetch
        "redirect-limit"
        (limits 65536 1024 1000 (std.http.FollowRedirects 1))
        (base <> "/redirect-loop")
    fetch "host-timeout" (limits 65536 1024 20 std.http.NoRedirects) (base <> "/slow-timeout")
    fetch "tls" (limits 65536 1024 1000 std.http.NoRedirects) (tlsBase <> "/secure")
    timed <- cancelSlowRequest base
    printLine
        (case timed of
            Nothing => "cancelled:prompt"
            | Just _ => "cancelled:late"
        )
