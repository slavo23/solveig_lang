import std.http(*)
import std.bytes(*)

main : Task ()
main =
    case std.http.urlFromString "https://example.com/releases" of
        Err _ => printLine "url: invalid"
        | Ok url =>
            case std.http.bodyChunks 3 (std.http.Response (std.http.Status 200) [] (std.bytes.stringToBytes "abcdefg")) of
                Err _ => printLine "chunks: invalid"
                | Ok chunks =>
                    bindIO (printLine ("url: " <> std.http.urlToString url)) (fun _ =>
                        bindIO (printLine ("chunks: " <> show (map std.bytes.bytesLength chunks))) (fun _ =>
                            printLine
                                ("header: " <> show
                                    (std.http.findHeader
                                        "etag"
                                        (std.http.Response
                                            (std.http.Status 200)
                                            [std.http.Header "ETag" "v1"]
                                            (std.bytes.stringToBytes ""))))))
