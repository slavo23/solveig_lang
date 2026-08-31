import std.http(*)
import std.bytes(*)

mustStatus : Int -> Status
mustStatus code =
    case statusFromInt code of
        Right status => status
        | Left _ => error "fixture status invalid"

mustHeader : String -> String -> Header
mustHeader name value =
    case header name value of
        Right result => result
        | Left _ => error "fixture header invalid"

surface : Task ()
surface =
    case std.http.urlFromString "https://example.com/releases" of
        Left _ => printLine "url: invalid"
        | Right url =>
            case std.http.bodyChunks
                3
                (response (mustStatus 200) [] (std.bytes.stringToBytes "abcdefg")) of
                Left _ => printLine "chunks: invalid"
                | Right chunks =>
                    bindIO
                        (printLine ("url: " <> std.http.urlToString url))
                        (fun _ =>
                            bindIO
                                (printLine ("chunks: " <> show (map std.bytes.bytesLength chunks)))
                                (fun _ =>
                                    printLine
                                    ("header: " <>
                                    show
                                    (std.http.findHeader
                                    "etag"
                                    (response
                                    (mustStatus 200)
                                    [mustHeader "ETag" "v1"]
                                    (std.bytes.stringToBytes ""))))
                                )
                        )

rejectInvalidScope : Task ()
rejectInvalidScope =
    case urlFromString "http://127.0.0.1:1/never-requested" of
        Left _ => error "valid fixture URL"
        | Right url =>
            catchError
                (withResponseWith
                    defaultHttpLimits
                    0
                    (get url)
                    (fun _ _ _ => error "invalid chunk size reached callback"))
                (fun problem => printLine (show problem))

main : Task ()
main = do
    surface
    rejectInvalidScope
