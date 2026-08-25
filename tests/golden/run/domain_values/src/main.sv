import std.http(*)
import std.net(*)
import std.path(*)
import std.process(*)
import std.time(*)

printPath : Task ()
printPath =
    case (pathFromString "", pathFromString "data/input.txt") of
        (Left EmptyPath, Right path) =>
            printLine
                ("path=" <> pathToString path
                    <> ":" <> show (path == path)
                    <> ":" <> show (hash path == hash path))
        | _ => printLine "path=wrong"

printPort : Task ()
printPort =
    case (portFromInt (0 - 1), portFromInt 443, portFromInt 65536) of
        (Left (InvalidPort _), Right port, Left (InvalidPort _)) =>
            printLine ("port=" <> show port <> ":" <> show (portToInt anyPort))
        | _ => printLine "port=wrong"

printTime : Task ()
printTime =
    let duration = milliseconds 1250 in
    printLine
        ("duration="
            <> show (durationToMilliseconds duration)
            <> ":"
            <> show (durationToMicroseconds duration)
            <> ":"
            <> show (duration > nanoseconds 0))

requireUrl : String -> Url
requireUrl spelling =
    case urlFromString spelling of
        Right url => url
        | Left _ => error "fixture URL invalid"

requireStatus : Int -> Status
requireStatus code =
    case statusFromInt code of
        Right status => status
        | Left _ => error "fixture status invalid"

requireHeader : String -> String -> Header
requireHeader name value =
    case header name value of
        Right result => result
        | Left _ => error "fixture header invalid"

printHttp : Task ()
printHttp =
    let url = requireUrl "https://example.com/items" in
    let field = requireHeader "Accept" "application/json" in
    let prepared = withHeader field (get url) in
    let limits =
        withRedirectPolicy
            NoRedirects
            (withHttpTimeout
                (seconds 2)
                (withMaximumBodyBytes 4096 defaultHttpLimits))
    in case
        ( statusFromInt 99
        , header "Bad Name" "value"
        , header "X-Test" "bad\nvalue"
        , urlFromString "relative/path"
        )
    of
        (Left (InvalidStatus _), Left (InvalidHeaderName _), Left (InvalidHeaderValue _), Left (InvalidUrl _)) =>
            printLine
                ("http="
                    <> show (requestMethod prepared)
                    <> ":"
                    <> urlToString (requestUrl prepared)
                    <> ":"
                    <> headerName field
                    <> "="
                    <> headerValue field
                    <> ":"
                    <> show (statusCode (requireStatus 204))
                    <> ":"
                    <> show (maximumBodyBytes limits)
                    <> ":"
                    <> show (durationToMilliseconds (httpTimeout limits)))
        | _ => printLine "http=wrong"

printProcess : Task ()
printProcess =
    case (process "" [], process "echo" ["hello"]) of
        (Left (InvalidProcess _), Right initial) =>
            case withArgument "world" initial of
                Left _ => printLine "process=wrong"
                | Right processValue =>
                    let success = exitCodeFromInt 0 in
                    let failure = exitCodeFromInt 7 in
                    printLine
                        ("process="
                            <> processProgram processValue
                            <> ":"
                            <> show (processArguments processValue)
                            <> ":"
                            <> show success
                            <> ":"
                            <> show failure
                            <> ":"
                            <> show (isExitSuccess success)
                            <> ":"
                            <> show (exitCodeToInt failure))
        | _ => printLine "process=wrong"

main : Task ()
main = do
    printPath
    printPort
    printTime
    printHttp
    printProcess
