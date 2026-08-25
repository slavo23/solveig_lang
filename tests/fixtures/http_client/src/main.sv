import std.bytes(*)
import std.env(getArgs)
import std.http(*)
import std.task(timeout)

showHttpError : std.http.HttpError -> String
showHttpError (InvalidUrl _) = "invalid-url"
showHttpError (InvalidHeaderName _) = "invalid-header-name"
showHttpError (InvalidHeaderValue _) = "invalid-header-value"
showHttpError (InvalidHttpLimit _ _) = "invalid-limit"
showHttpError (HttpIoError ioError) =
    case ioError of
        HttpTransportError _ => "transport"
        | HttpTlsError _ => "tls"
        | HttpTimeoutError _ => "timeout"
        | HttpRedirectError _ => "redirect"
        | HttpHeadersTooLarge limit => "headers-too-large:" <> show limit
        | HttpBodyTooLarge limit => "body-too-large:" <> show limit
        | _ => "other-io"

printResponse : String -> std.http.Response -> IO std.http.HttpError ()
printResponse label response =
    case std.http.responseText response of
        Left error => failIO error
        | Right body =>
            mapError HttpIoError (printLine
                (label
                    <> ":"
                    <> show (std.http.statusCode (std.http.responseStatus response))
                    <> ":"
                    <> show (std.http.findHeader "etag" response)
                    <> ":"
                    <> body))

fetch : String -> std.http.HttpLimits -> String -> Task ()
fetch label limits spelling =
    case std.http.urlFromString spelling of
        Left error => printLine (label <> ":" <> showHttpError error)
        | Right url =>
            catchError
                (std.http.request limits (std.http.get url) >>= printResponse label)
                (fun error => printLine (label <> ":" <> showHttpError error))

baseArgument : [String] -> String
baseArgument (base :: _) = base
baseArgument [] = "http://127.0.0.1:1"

tlsArgument : [String] -> String
tlsArgument (_ :: tlsBase :: _) = tlsBase
tlsArgument _ = "https://127.0.0.1:1"

requireUrl : String -> std.http.Url
requireUrl spelling =
    case std.http.urlFromString spelling of
        Right url => url
        | Left _ => error "fixture URL invalid"

cancelSlowRequest : String -> Task (Maybe std.http.Response)
cancelSlowRequest base =
    catchError
        (timeout 20
            (std.http.request
                (std.http.HttpLimits 65536 1024 1000 std.http.NoRedirects)
                (std.http.get (requireUrl (base <> "/slow")))))
        (fun requestError => pureIO Nothing)

main : Task ()
main = do
    args <- getArgs
    let base = baseArgument args
    let tlsBase = tlsArgument args
    catchError
        (std.http.fetchWith
            (std.http.HttpLimits 65536 1024 1000 std.http.NoRedirects)
            [("Accept", "text/plain")]
            (base <> "/chunked")
            >>= printResponse "fetch")
        (fun error => printLine ("fetch:" <> showHttpError error))
    fetch "chunked" (std.http.HttpLimits 65536 1024 1000 (std.http.FollowRedirects 2)) (base <> "/chunked")
    fetch "redirect" (std.http.HttpLimits 65536 1024 1000 (std.http.FollowRedirects 2)) (base <> "/redirect")
    fetch "status" (std.http.HttpLimits 65536 1024 1000 std.http.NoRedirects) (base <> "/missing")
    fetch "body-limit" (std.http.HttpLimits 65536 4 1000 std.http.NoRedirects) (base <> "/large")
    fetch "header-limit" (std.http.HttpLimits 80 1024 1000 std.http.NoRedirects) (base <> "/large-header")
    fetch "malformed" (std.http.HttpLimits 65536 1024 1000 std.http.NoRedirects) (base <> "/malformed")
    fetch "redirect-limit" (std.http.HttpLimits 65536 1024 1000 (std.http.FollowRedirects 1)) (base <> "/redirect-loop")
    fetch "host-timeout" (std.http.HttpLimits 65536 1024 20 std.http.NoRedirects) (base <> "/slow-timeout")
    fetch "tls" (std.http.HttpLimits 65536 1024 1000 std.http.NoRedirects) (tlsBase <> "/secure")
    timed <- cancelSlowRequest base
    printLine (case timed of Nothing => "cancelled:prompt" | Just _ => "cancelled:late")
