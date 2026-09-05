import std.task(*)
import std.channel(*)
import std.fs(FsError(..))
import std.path(Path, pathFromString)
import std.text(stringToChars)

data LogJob = LogJob Path

data LogCounts = LogCounts Int Int Int Int

data AuditResult = Audited LogCounts | MissingLog

data AuditSummary = AuditSummary Int Int Int Int Int Int

emptySummary : AuditSummary
emptySummary = AuditSummary 0 0 0 0 0 0

fsToHost : FsError -> HostError
fsToHost (FsHostError _ hostError) = hostError
fsToHost error = HostError InvalidArgument "filesystem" (show error)

requiredPath : String -> Path
requiredPath spelling =
    case pathFromString spelling of
        Right path => path
        | Left pathError => error (show pathError)

scanLog : [Char] -> Bool -> Int -> Int -> Int -> Int -> LogCounts
scanLog chars lineStart events infos warnings errors =
    case chars of
        [] => LogCounts events infos warnings errors
        | c :: rest =>
            if lineStart
                then if c == 'I'
                    then scanLog rest False (events + 1) (infos + 1) warnings errors
                    else if c == 'W'
                        then scanLog rest False (events + 1) infos (warnings + 1) errors
                        else if c == 'E'
                            then scanLog rest False (events + 1) infos warnings (errors + 1)
                            else if c == '\n'
                                then scanLog rest True events infos warnings errors
                                else scanLog rest False (events + 1) infos warnings errors
                else if c == '\n'
                    then scanLog rest True events infos warnings errors
                    else scanLog rest False events infos warnings errors

analyzeLog : String -> LogCounts
analyzeLog contents = scanLog (stringToChars contents) True 0 0 0 0

auditOne : Chan AuditResult -> LogJob -> Task ()
auditOne results (LogJob path) =
    catchError
        (auditFile results path)
        (fun error =>
            if hostErrorOperation error == "readFile" then send results MissingLog else failIO error
        )

auditFile : Chan AuditResult -> Path -> Task ()
auditFile results path = do
    contents <- mapError fsToHost (std.fs.readFile path)
    send results (Audited (analyzeLog contents))

worker : Chan LogJob -> Chan AuditResult -> Task ()
worker jobs results =
    catchError
        (workerStep jobs results)
        (fun error =>
            case hostErrorKind error of
                ResourceVanished => pureIO ()
                | _ => failIO error
        )

workerStep : Chan LogJob -> Chan AuditResult -> Task ()
workerStep jobs results = do
    job <- recv jobs
    auditOne results job
    worker jobs results

enqueueJobs : Chan LogJob -> Task ()
enqueueJobs jobs = do
    send jobs (LogJob (requiredPath "logs/api.log"))
    send jobs (LogJob (requiredPath "logs/billing.log"))
    send jobs (LogJob (requiredPath "logs/missing.log"))
    send jobs (LogJob (requiredPath "logs/search.log"))
    send jobs (LogJob (requiredPath "logs/worker.log"))
    closeChan jobs

includeResult : AuditSummary -> AuditResult -> AuditSummary
includeResult (AuditSummary files missing events infos warnings errors) result =
    case result of
        MissingLog => AuditSummary files (missing + 1) events infos warnings errors
        | Audited (LogCounts fileEvents fileInfos fileWarnings fileErrors) =>
            AuditSummary
                (files + 1)
                missing
                (events + fileEvents)
                (infos + fileInfos)
                (warnings + fileWarnings)
                (errors + fileErrors)

collectResults : Chan AuditResult -> Int -> AuditSummary -> Task AuditSummary
collectResults results remaining summary =
    if remaining == 0
        then pureIO summary
        else do
            result <- recv results
            collectResults results (remaining - 1) (includeResult summary result)

reportLine : String -> Int -> String
reportLine label value = appendString label (appendString (show value) "\n")

renderReport : AuditSummary -> String
renderReport (AuditSummary files missing events infos warnings errors) =
    appendString
        "Solveig log audit\n"
        (appendString
            (reportLine "files analyzed: " files)
            (appendString
                (reportLine "files missing: " missing)
                (appendString
                    (reportLine "events: " events)
                    (appendString
                        (reportLine "info: " infos)
                        (appendString
                            (reportLine "warnings: " warnings)
                            (reportLine "errors: " errors))))))

runAudit : Chan LogJob -> Chan AuditResult -> Task AuditSummary
runAudit jobs results =
    withThread
        (worker jobs results)
        (fun firstWorker =>
            withThread
                (worker jobs results)
                (fun secondWorker =>
                    withThread
                        (worker jobs results)
                        (fun thirdWorker =>
                            do
                                completed <-
                                    concurrently
                                    (enqueueJobs jobs)
                                    (collectResults results 5 emptySummary)
                                case completed of
                                    (produced, summary) => pureIO summary
                        )
                )
        )

main : Task ()
main = do
    jobs <- newBoundedChan 2
    results <- newBoundedChan 2
    summary <- runAudit jobs results
    closeChan results
    let reportPath = requiredPath "output/report.txt"
    mapError fsToHost (std.fs.writeFile reportPath (renderReport summary))
    report <- mapError fsToHost (std.fs.readFile reportPath)
    printStr report
