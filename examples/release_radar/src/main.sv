import std.env(*)
import std.fs(*)
import std.list(length, isEmpty)
import std.process(exitFailure)
import std.path(*)
import std.text(lines, trimAscii, startsWith)
import std.time(*)
import release_radar.cache(*)
import release_radar.client(*)
import release_radar.fixture(*)
import release_radar.model(*)

repositoryPath : Path
repositoryPath = requiredPath "repos.txt"

cachePath : Path
cachePath = requiredPath ".release-radar-cache"

workerCount : Int
workerCount = 4

envPath : Path
envPath = requiredPath ".env"

repositoriesFromText : String -> [String]
repositoriesFromText = filter repositoryLine . map trimAscii . lines

repositoryLine : String -> Bool
repositoryLine line = and (line /= "") (not (startsWith "#" line))

printSummary : [String] -> [release_radar.model.ReleaseResult] -> Task ()
printSummary repositories results = do
    printLine
        ("checked "
            <> show (std.list.length repositories)
            <> " repositories with a bounded "
            <> show workerCount
            <> "-worker pool")
    printResults results

reportLocalComparison :
    [String] -> Duration -> [release_radar.model.ReleaseResult] -> Duration -> Task ()
reportLocalComparison repositories sequentialNanos results concurrentNanos =
    if concurrentNanos >= sequentialNanos
        then error "local concurrency comparison did not beat sequential execution"
        else do
                printLine
                    ("local comparison: sequential "
                        <> show (durationToMilliseconds sequentialNanos)
                        <> " ms, concurrent "
                        <> show (durationToMilliseconds concurrentNanos)
                        <> " ms ("
                        <> show
                            (div
                                (durationToNanoseconds sequentialNanos)
                                (durationToNanoseconds concurrentNanos))
                        <> "x faster)")
                printSummary repositories results

runLocalAgainst : [String] -> String -> Task ()
runLocalAgainst repositories base = do
    sequential <- time.measure (fetchSequential base Nothing release_radar.cache.empty repositories)
    case sequential of
        (sequentialNanos, _) => do
            concurrent <-
                time.measure
                    (fetchConcurrentObserved
                        workerCount
                        base
                        Nothing
                        release_radar.cache.empty
                        repositories)
            case concurrent of
                (concurrentNanos, results) =>
                    reportLocalComparison repositories sequentialNanos results concurrentNanos

runLocal : [String] -> Task ()
runLocal repositories =
    fixture.withFixture (2 * std.list.length repositories) (runLocalAgainst repositories)

liveToken : IO std.env.EnvError (Maybe String)
liveToken = do
    _ <- std.env.loadEnvFileIfExists envPath
    fmap usableToken (mapError std.env.EnvHostError (std.env.getEnv "GITHUB_TOKEN"))

requiredPath : String -> Path
requiredPath spelling =
    case pathFromString spelling of
        Right path => path
        | Left pathError => error ("invalid Release Radar path: " <> show pathError)

usableToken : Maybe String -> Maybe String
usableToken Nothing = Nothing
usableToken (Just token) =
    let cleaned = trimAscii token in if cleaned == "" then Nothing else Just cleaned

tokenOrExit : Task (Maybe String)
tokenOrExit =
    catchError
        liveToken
        (fun envError => do
            printLine ("release radar configuration error: " <> show envError)
            exitFailure
        )

runLive : release_radar.cache.Cache -> [String] -> Task ()
runLive cache repositories = do
    token <- tokenOrExit
    case token of
        Nothing => printLine "GitHub authentication: anonymous (60 requests/hour)"
        | Just _ => printLine "GitHub authentication: GITHUB_TOKEN"
    access <- release_radar.client.checkAccess "https://api.github.com" token
    case access of
        GitHubAccessFailed failure => do
            printLine ("GitHub access check failed: " <> describeFailure failure)
            exitFailure
        | GitHubAccessReady rate => do
            printLine ("GitHub API: " <> describeRateInfo rate)
            measured <-
                std.time.measure
                    (release_radar.client.fetchConcurrentObserved
                        workerCount
                        "https://api.github.com"
                        token
                        cache
                        repositories)
            case measured of
                (elapsed, results) => do
                    printLine
                        ("live requests completed in "
                            <> show (durationToMilliseconds elapsed)
                            <> " ms")
                    printSummary repositories results
                    writeCache cachePath (updateCache cache results)

liveMode : [String] -> Bool
liveMode ("--live" :: _) = True
liveMode _ = False

main : Task ()
main = do
    repositoryText <- mapError fsToHost (std.fs.readFileLimited repositoryPath 65536)
    let repositories = repositoriesFromText repositoryText
    if std.list.isEmpty repositories
        then error "repos.txt must contain at least one repository"
        else do
                args <- getArgs
                if liveMode args
                    then
                        do
                            printLine "release radar: live GitHub HTTPS"
                            cache <- loadCache cachePath
                            runLive cache repositories
                    else do
                            printLine "release radar: deterministic local concurrency comparison"
                            runLocal repositories
