import std.env(getArgs)
import std.io
import std.list(length, isEmpty)
import std.text(lines)
import std.time
import release_radar.cache
import release_radar.client
import release_radar.fixture
import release_radar.model

repositoryPath : String
repositoryPath = "repos.txt"

cachePath : String
cachePath = ".release-radar-cache"

workerCount : Int
workerCount = 4

repositoriesFromText : String -> [String]
repositoriesFromText = filter ((/=) "") . std.text.lines

milliseconds : Int -> Int
milliseconds nanoseconds = div nanoseconds 1000000

printSummary : [String] -> [release_radar.model.ReleaseResult] -> Task ()
printSummary repositories results = do
    printLine
        ("checked "
            <> show (std.list.length repositories)
            <> " repositories with a bounded "
            <> show workerCount
            <> "-worker pool")
    release_radar.client.printResults results
    release_radar.cache.writeCache cachePath results

reportLocalComparison : [String] -> Int -> [release_radar.model.ReleaseResult] -> Int -> Task ()
reportLocalComparison repositories sequentialNanos results concurrentNanos =
    if concurrentNanos >= sequentialNanos then
        error "local concurrency comparison did not beat sequential execution"
    else do
        printLine
            ("local comparison: sequential "
                <> show (milliseconds sequentialNanos)
                <> " ms, concurrent "
                <> show (milliseconds concurrentNanos)
                <> " ms ("
                <> show (div sequentialNanos concurrentNanos)
                <> "x faster)")
        printSummary repositories results

runLocalAgainst : release_radar.cache.Cache -> [String] -> String -> Task ()
runLocalAgainst cache repositories base = do
    sequential <- std.time.measure
        (release_radar.client.fetchSequential base release_radar.cache.empty repositories)
    case sequential of
        MkPair sequentialNanos _ => do
            concurrent <- std.time.measure
                (release_radar.client.fetchConcurrent workerCount base cache repositories)
            case concurrent of
                MkPair concurrentNanos results =>
                    reportLocalComparison repositories sequentialNanos results concurrentNanos

runLocal : release_radar.cache.Cache -> [String] -> Task ()
runLocal cache repositories =
    release_radar.fixture.withFixture
        (2 * std.list.length repositories)
        (runLocalAgainst cache repositories)

runLive : release_radar.cache.Cache -> [String] -> Task ()
runLive cache repositories = do
    measured <- std.time.measure
        (release_radar.client.fetchConcurrent
            workerCount
            "https://api.github.com"
            cache
            repositories)
    case measured of
        MkPair elapsed results => do
            printLine ("live requests completed in " <> show (milliseconds elapsed) <> " ms")
            printSummary repositories results

liveMode : [String] -> Bool
liveMode ("--live" :: _) = True
liveMode _ = False

main : Task ()
main = do
    repositoryText <- std.io.readFileLimited repositoryPath 65536
    let repositories = repositoriesFromText repositoryText
    if std.list.isEmpty repositories then
        error "repos.txt must contain at least one repository"
    else do
        cache <- release_radar.cache.loadCache cachePath
        args <- getArgs
        if liveMode args then do
            printLine "release radar: live GitHub HTTPS"
            runLive cache repositories
        else do
            printLine "release radar: deterministic local concurrency comparison"
            runLocal cache repositories
