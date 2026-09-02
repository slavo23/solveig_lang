import std.http(*)
import std.task(*)
import std.time(*)
import release_radar.cache(*)
import release_radar.model(*)

httpOptions : std.http.HttpOptions
httpOptions =
    std.http.withRedirectPolicy
        std.http.NoRedirects
        (std.http.withTimeout
            (std.time.milliseconds 10000)
            (std.http.withMaximumBufferedBodyBytes
                1048576
                (std.http.withMaximumHeaderBytes 65536 std.http.defaultOptions)))

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
            (std.http.getWith
                (std.http.withHeaders (accessHeaders token) httpOptions)
                (base <> "/rate_limit")))
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
            (std.http.getWith
                (std.http.withHeaders (requestHeaders token cache repository) httpOptions)
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
fetchConcurrentObserved workers base token cache repositories =
    mapError
        std.task.concurrentHostError
        (std.task.mapConcurrentlyBoundedOrdered
            workers
            (fetchObserved base token cache)
            repositories)

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
                        show (durationToMilliseconds elapsed) <>
                        " ms] " <>
                        release_radar.model.releaseRepository result <>
                        " — " <>
                        release_radar.model.completionLabel result)
                pureIO result

printResults : [release_radar.model.ReleaseResult] -> Task ()
printResults = mapIO_ (printLine . release_radar.model.renderResult)

describeHttpError : std.http.HttpError -> String
describeHttpError error = show error
