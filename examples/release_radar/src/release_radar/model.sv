import std.bytes
import std.http
import std.json
import std.map
import std.parser

data ReleaseResult =
    ReleaseFound String String String String String
  | ReleaseUnchanged String String
  | ReleaseFailed String String

decodeResponse : String -> Maybe String -> std.http.Response -> ReleaseResult
decodeResponse repository cached response =
    let code = std.http.statusCode (std.http.responseStatus response) in
    if code == 304 then
        case cached of
            Just etag => ReleaseUnchanged repository etag
            | Nothing => ReleaseFailed repository "server returned 304 without a cached ETag"
    else if code /= 200 then
        ReleaseFailed repository ("HTTP status " <> show code)
    else
        let etag = case std.http.findHeader "etag" response of
            Just value => value
            | Nothing => ""
        in decodeReleaseJson repository etag (std.http.responseBody response)

decodeReleaseJson : String -> String -> Bytes -> ReleaseResult
decodeReleaseJson repository etag body =
    std.parser.foldReply
        (fun _ value _ => releaseFromJson repository etag value)
        (fun _ _ _ => ReleaseFailed repository "invalid release JSON")
        (fun _ _ _ => ReleaseFailed repository "incomplete release JSON")
        (std.json.parseJson std.json.defaultJsonLimits body)

releaseFromJson : String -> String -> std.json.Json -> ReleaseResult
releaseFromJson repository etag (JsonObject object) =
    case stringField "tag_name" object of
        Nothing => ReleaseFailed repository "JSON has no tag_name"
        | Just tag =>
            case stringField "published_at" object of
                Nothing => ReleaseFailed repository "JSON has no published_at"
                | Just published =>
                    case stringField "html_url" object of
                        Nothing => ReleaseFailed repository "JSON has no html_url"
                        | Just url => ReleaseFound repository tag published url etag
releaseFromJson repository _ _ = ReleaseFailed repository "release JSON is not an object"

stringField : String -> std.map.Map String std.json.Json -> Maybe String
stringField name object =
    case std.map.lookup name object of
        Just (JsonString value) => Just value
        | _ => Nothing

renderResult : ReleaseResult -> String
renderResult (ReleaseFound repository tag published url _) =
    repository <> "  " <> tag <> "  " <> published <> "\n  " <> url
renderResult (ReleaseUnchanged repository _) = repository <> "  unchanged (HTTP 304)"
renderResult (ReleaseFailed repository message) = repository <> "  ERROR: " <> message
