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

main : Task ()
main =
    case std.http.urlFromString "https://example.com/releases" of
        Left _ => printLine "url: invalid"
        | Right url =>
            case std.http.bodyChunks 3 (response (mustStatus 200) [] (std.bytes.stringToBytes "abcdefg")) of
                Left _ => printLine "chunks: invalid"
                | Right chunks =>
                    bindIO (printLine ("url: " <> std.http.urlToString url)) (fun _ =>
                        bindIO (printLine ("chunks: " <> show (map std.bytes.bytesLength chunks))) (fun _ =>
                            printLine
                                ("header: " <> show
                                    (std.http.findHeader
                                        "etag"
                                        (response
                                            (mustStatus 200)
                                            [mustHeader "ETag" "v1"]
                                            (std.bytes.stringToBytes ""))))))
