import std.bytes(*)
import std.http(*)
import std.json(*)
import std.map(*)
import std.parser(*)
import std.text(*)

data RateInfo = RateInfo (Maybe String) (Maybe String) (Maybe String)

data ReleaseFailure =
    InvalidCredentials
    | PrimaryRateLimited (Maybe String) String
    | SecondaryRateLimited (Maybe String) String
    | NoPublishedRelease
    | HttpFailure Int String
    | InvalidReleaseJson String
    | RequestFailure String

data GitHubAccess = GitHubAccessReady RateInfo | GitHubAccessFailed ReleaseFailure

data ReleaseResult =
    ReleaseFound String String String String String
    | ReleaseUnchanged String String
    | ReleaseFailed String ReleaseFailure

rateInfo : std.http.Response -> RateInfo
rateInfo response =
    RateInfo
        (std.http.findHeader "x-ratelimit-limit" response)
        (std.http.findHeader "x-ratelimit-remaining" response)
        (std.http.findHeader "x-ratelimit-reset" response)

decodeAccess : std.http.Response -> GitHubAccess
decodeAccess response =
    let code =
        std.http.statusCode (std.http.responseStatus response)
    in if code == 200
        then case rateInfo response of
            RateInfo _ (Just "0") reset =>
                GitHubAccessFailed (PrimaryRateLimited reset (responseMessage response))
            | info => GitHubAccessReady info
        else GitHubAccessFailed (failureForStatus response code)

decodeResponse : String -> Maybe String -> std.http.Response -> ReleaseResult
decodeResponse repository cached response =
    let code =
        std.http.statusCode (std.http.responseStatus response)
    in if code == 304
        then case cached of
            Just etag => ReleaseUnchanged repository etag
            | Nothing =>
                ReleaseFailed
                    repository
                    (InvalidReleaseJson "server returned 304 without a cached ETag")
        else if code /= 200
            then ReleaseFailed repository (failureForStatus response code)
            else let etag =
                case std.http.findHeader "etag" response of
                    Just value => value
                    | Nothing => ""
            in decodeReleaseJson repository etag (std.http.responseBody response)

decodeReleaseJson : String -> String -> Bytes -> ReleaseResult
decodeReleaseJson repository etag body =
    std.parser.foldReply
        (fun _ value _ => releaseFromJson repository etag value)
        (fun _ _ _ => ReleaseFailed repository (InvalidReleaseJson "invalid release JSON"))
        (fun _ _ _ => ReleaseFailed repository (InvalidReleaseJson "incomplete release JSON"))
        (std.json.parseJson std.json.defaultJsonLimits body)

releaseFromJson : String -> String -> std.json.Json -> ReleaseResult
releaseFromJson repository etag (JsonObject object) =
    case stringField "tag_name" object of
        Nothing => ReleaseFailed repository (InvalidReleaseJson "JSON has no tag_name")
        | Just tag =>
            case stringField "published_at" object of
                Nothing => ReleaseFailed repository (InvalidReleaseJson "JSON has no published_at")
                | Just published =>
                    case stringField "html_url" object of
                        Nothing =>
                            ReleaseFailed repository (InvalidReleaseJson "JSON has no html_url")
                        | Just url => ReleaseFound repository tag published url etag
releaseFromJson repository _ _ =
    ReleaseFailed repository (InvalidReleaseJson "release JSON is not an object")

stringField : String -> std.map.Map String std.json.Json -> Maybe String
stringField name object =
    case std.map.lookup name object of
        Just (JsonString value) => Just value
        | _ => Nothing

failureForStatus : std.http.Response -> Int -> ReleaseFailure
failureForStatus _ 401 = InvalidCredentials
failureForStatus response code =
    if or (code == 403) (code == 429)
        then case std.http.findHeader "x-ratelimit-remaining" response of
            Just "0" =>
                PrimaryRateLimited
                    (std.http.findHeader "x-ratelimit-reset" response)
                    (responseMessage response)
            | _ =>
                SecondaryRateLimited
                    (std.http.findHeader "retry-after" response)
                    (responseMessage response)
        else if code == 404 then NoPublishedRelease else HttpFailure code (responseMessage response)

responseMessage : std.http.Response -> String
responseMessage response =
    std.parser.foldReply
        (fun _ value _ => githubMessageValue value)
        (fun _ _ _ => "")
        (fun _ _ _ => "")
        (std.json.parseJson std.json.defaultJsonLimits (std.http.responseBody response))

githubMessageValue : std.json.Json -> String
githubMessageValue (JsonObject object) =
    case stringField "message" object of
        Just message => message
        | Nothing => ""
githubMessageValue _ = ""

releaseRepository : ReleaseResult -> String
releaseRepository (ReleaseFound repository _ _ _ _) = repository
releaseRepository (ReleaseUnchanged repository _) = repository
releaseRepository (ReleaseFailed repository _) = repository

completionLabel : ReleaseResult -> String
completionLabel (ReleaseFound _ tag _ _ _) = "release " <> tag
completionLabel (ReleaseUnchanged _ _) = "unchanged"
completionLabel (ReleaseFailed _ failure) = "failed: " <> describeFailure failure

describeRateInfo : RateInfo -> String
describeRateInfo (RateInfo (Just limit) (Just remaining) reset) =
    remaining <> "/" <> limit <> " requests remaining" <> resetSuffix reset
describeRateInfo _ = "rate allowance unavailable"

resetSuffix : Maybe String -> String
resetSuffix Nothing = ""
resetSuffix (Just reset) = "; reset Unix second " <> reset

describeFailure : ReleaseFailure -> String
describeFailure InvalidCredentials =
    "GitHub rejected GITHUB_TOKEN; create or refresh a read-only token"
describeFailure (PrimaryRateLimited reset message) =
    "GitHub primary rate limit exhausted" <> resetSuffix reset <> messageSuffix message
describeFailure (SecondaryRateLimited retryAfter message) =
    "GitHub secondary throttling" <> retrySuffix retryAfter <> messageSuffix message
describeFailure NoPublishedRelease = "repository missing or has no published release"
describeFailure (HttpFailure code message) = "HTTP " <> show code <> messageSuffix message
describeFailure (InvalidReleaseJson message) = message
describeFailure (RequestFailure message) = message

retrySuffix : Maybe String -> String
retrySuffix Nothing = ""
retrySuffix (Just seconds) = "; retry after " <> seconds <> " seconds"

messageSuffix : String -> String
messageSuffix message = if std.text.isEmpty message then "" else ": " <> message

renderResult : ReleaseResult -> String
renderResult (ReleaseFound repository tag published url _) =
    repository <> "  " <> tag <> "  " <> published <> "\n  " <> url
renderResult (ReleaseUnchanged repository _) = repository <> "  unchanged (HTTP 304)"
renderResult (ReleaseFailed repository failure) =
    repository <> "  ERROR: " <> describeFailure failure
