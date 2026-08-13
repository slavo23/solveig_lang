import prelude.prim(stringToChars)

data LogJob = LogJob String

data LogCounts = LogCounts Int Int Int Int

data AuditResult =
    Audited LogCounts
  | MissingLog

data AuditSummary = AuditSummary Int Int Int Int Int Int

emptySummary : AuditSummary
emptySummary = AuditSummary 0 0 0 0 0 0

scanLog : [Char] -> Bool -> Int -> Int -> Int -> Int -> LogCounts
scanLog chars lineStart events infos warnings errors =
    case chars of
        [] => LogCounts events infos warnings errors
        | c :: rest =>
            if lineStart then
                if c == 'I' then
                    scanLog rest False (events + 1) (infos + 1) warnings errors
                else if c == 'W' then
                    scanLog rest False (events + 1) infos (warnings + 1) errors
                else if c == 'E' then
                    scanLog rest False (events + 1) infos warnings (errors + 1)
                else if c == '\n' then
                    scanLog rest True events infos warnings errors
                else
                    scanLog rest False (events + 1) infos warnings errors
            else if c == '\n' then
                scanLog rest True events infos warnings errors
            else
                scanLog rest False events infos warnings errors

analyzeLog : String -> LogCounts
analyzeLog contents = scanLog (stringToChars contents) True 0 0 0 0

auditOne : Chan AuditResult -> LogJob -> Task ()
auditOne results (LogJob path) =
    catchError
        (auditFile results path)
        (fun error =>
            case error of
                ReadFileError failedPath message => send results MissingLog
                | other => failIO other)

auditFile : Chan AuditResult -> String -> Task ()
auditFile results path = do
    contents <- readFile path
    send results (Audited (analyzeLog contents))

worker : Chan LogJob -> Chan AuditResult -> Task ()
worker jobs results =
    catchError
        (workerStep jobs results)
        (fun error =>
            case error of
                ChannelClosed => pureIO ()
                | other => failIO other)

workerStep : Chan LogJob -> Chan AuditResult -> Task ()
workerStep jobs results = do
    job <- recv jobs
    auditOne results job
    worker jobs results

enqueueJobs : Chan LogJob -> Task ()
enqueueJobs jobs = do
    send jobs (LogJob "logs/api.log")
    send jobs (LogJob "logs/billing.log")
    send jobs (LogJob "logs/missing.log")
    send jobs (LogJob "logs/search.log")
    send jobs (LogJob "logs/worker.log")
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
    if remaining == 0 then
        pureIO summary
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
                        (fun thirdWorker => do
                            completed <- concurrently
                                (enqueueJobs jobs)
                                (collectResults results 5 emptySummary)
                            case completed of
                                MkPair produced summary => pureIO summary)))

main : Task ()
main = do
    jobs <- newBoundedChan 2
    results <- newBoundedChan 2
    summary <- runAudit jobs results
    closeChan results
    writeFile "output/report.txt" (renderReport summary)
    report <- readFile "output/report.txt"
    printStr report
