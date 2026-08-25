import std.http(*)
import std.task(*)
import std.time(*)
import release_radar.cache(*)
import release_radar.model(*)

httpLimits : std.http.HttpLimits
httpLimits = std.http.HttpLimits 65536 1048576 10000 std.http.NoRedirects

commonHeaders : [(String, String)]
commonHeaders =
    [
        ("User-Agent", "solveig-release-radar/0.1"),
        ("Accept", "application/vnd.github+json"),
        ("X-GitHub-Api-Version", "2022-11-28")
    ]

requestHeaders : Maybe String -> release_radar.cache.Cache -> String -> [(String, String)]
requestHeaders token cache repository =
    tokenHeader token (etagHeader (release_radar.cache.etagFor repository cache) commonHeaders)

tokenHeader : Maybe String -> [(String, String)] -> [(String, String)]
tokenHeader Nothing headers = headers
tokenHeader (Just token) headers = ("Authorization", ("Bearer " <> token)) :: headers

etagHeader : Maybe String -> [(String, String)] -> [(String, String)]
etagHeader Nothing headers = headers
etagHeader (Just etag) headers = ("If-None-Match", etag) :: headers

accessHeaders : Maybe String -> [(String, String)]
accessHeaders token = tokenHeader token commonHeaders

checkAccess : String -> Maybe String -> Task release_radar.model.GitHubAccess
checkAccess base token =
    catchError
        (fmap
            release_radar.model.decodeAccess
            (std.http.fetchWith httpLimits (accessHeaders token) (base <> "/rate_limit")))
        (pureIO .
            release_radar.model.GitHubAccessFailed .
            release_radar.model.RequestFailure .
            describeHttpError)

fetchRelease :
    String ->
    Maybe String -> release_radar.cache.Cache -> String -> Task release_radar.model.ReleaseResult
fetchRelease base token cache repository =
    catchError
        (fmap
            (release_radar.model.decodeResponse
                repository
                (release_radar.cache.etagFor repository cache))
            (std.http.fetchWith
                httpLimits
                (requestHeaders token cache repository)
                (base <> "/repos/" <> repository <> "/releases/latest")))
        (pureIO .
            release_radar.model.ReleaseFailed repository .
            release_radar.model.RequestFailure .
            describeHttpError)

fetchSequential :
    String ->
    Maybe String ->
    release_radar.cache.Cache -> [String] -> Task [release_radar.model.ReleaseResult]
fetchSequential base token cache = mapM (fetchRelease base token cache)

fetchConcurrentObserved :
    Int ->
    String ->
    Maybe String ->
    release_radar.cache.Cache -> [String] -> Task [release_radar.model.ReleaseResult]
fetchConcurrentObserved workers base token cache =
    std.task.mapConcurrentlyBounded workers (fetchObserved base token cache)

fetchObserved :
    String ->
    Maybe String -> release_radar.cache.Cache -> String -> Task release_radar.model.ReleaseResult
fetchObserved base token cache repository = do
    printLine ("[start] " <> repository)
    measured <- std.time.measure (fetchRelease base token cache repository)
    case measured of
        (elapsed, result) =>
            do
                printLine
                    ("[done " <>
                        show (milliseconds elapsed) <>
                        " ms] " <>
                        release_radar.model.releaseRepository result <>
                        " — " <>
                        release_radar.model.completionLabel result)
                pureIO result

milliseconds : Int -> Int
milliseconds nanoseconds = div nanoseconds 1000000

printResults : [release_radar.model.ReleaseResult] -> Task ()
printResults = mapIO_ (printLine . release_radar.model.renderResult)

describeHttpError : std.http.HttpError -> String
describeHttpError (InvalidUrl _) = "invalid URL"
describeHttpError (InvalidHeaderName _) = "invalid header name"
describeHttpError (InvalidHeaderValue _) = "invalid header value"
describeHttpError (InvalidHttpLimit name value) = "invalid " <> name <> ": " <> show value
describeHttpError (HttpIoError ioError) =
    case ioError of
        HttpTransportError message => "transport: " <> message
        | HttpTlsError message => "TLS: " <> message
        | HttpTimeoutError _ => "request timed out"
        | HttpRedirectError message => "redirect: " <> message
        | HttpHeadersTooLarge maximum => "response headers exceed " <> show maximum <> " bytes"
        | HttpBodyTooLarge maximum => "response body exceeds " <> show maximum <> " bytes"
        | _ => "unexpected IO error"
