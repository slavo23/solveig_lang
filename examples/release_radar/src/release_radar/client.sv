import std.http
import std.task
import release_radar.cache
import release_radar.model

httpLimits : std.http.HttpLimits
httpLimits = std.http.HttpLimits 65536 1048576 10000 (std.http.FollowRedirects 3)

requiredHeader : String -> String -> std.http.Header
requiredHeader name value =
    case std.http.header name value of
        Ok result => result
        | Err _ => error "release-radar constant header is invalid"

requiredUrl : String -> std.http.Url
requiredUrl spelling =
    case std.http.urlFromString spelling of
        Ok result => result
        | Err _ => error ("release-radar constructed invalid URL: " <> spelling)

repositoryRequest : String -> release_radar.cache.Cache -> String -> std.http.Request
repositoryRequest base cache repository =
    let initial = std.http.get
        (requiredUrl (base <> "/repos/" <> repository <> "/releases/latest")) in
    let identified = std.http.withHeader
        (requiredHeader "User-Agent" "solveig-release-radar/0.1") initial in
    let accepting = std.http.withHeader
        (requiredHeader "Accept" "application/vnd.github+json") identified in
    case release_radar.cache.etagFor repository cache of
        Nothing => accepting
        | Just etag => std.http.withHeader (requiredHeader "If-None-Match" etag) accepting

fetchRelease : String -> release_radar.cache.Cache -> String -> Task release_radar.model.ReleaseResult
fetchRelease base cache repository =
    catchError
        (fmap
            (release_radar.model.decodeResponse
                repository
                (release_radar.cache.etagFor repository cache))
            (std.http.request httpLimits (repositoryRequest base cache repository)))
        (pureIO . release_radar.model.ReleaseFailed repository . describeHttpError)

fetchSequential : String -> release_radar.cache.Cache -> [String] -> Task [release_radar.model.ReleaseResult]
fetchSequential base cache = mapM (fetchRelease base cache)

fetchConcurrent : Int -> String -> release_radar.cache.Cache -> [String] -> Task [release_radar.model.ReleaseResult]
fetchConcurrent workers base cache =
    std.task.mapConcurrentlyBounded workers (fetchRelease base cache)

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
