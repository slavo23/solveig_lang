import std.task(*)
import std.time(*)
import std.channel(*)

data Order = Order Int Int

data Fulfilment =
    Accepted Int
  | RejectedOrder Int String

validateOrder : Order -> Fulfilment
validateOrder (Order orderId quantity) =
    if quantity <= 0 then
        RejectedOrder orderId "quantity must be positive"
    else if quantity > 50 then
        RejectedOrder orderId "manual review required"
    else
        Accepted orderId

processOrder : Chan Fulfilment -> Order -> Task ()
processOrder results order = do
    sleep 10
    send results (validateOrder order)

worker : Chan Order -> Chan Fulfilment -> Task ()
worker jobs results =
    catchError
        (workerStep jobs results)
        (fun error =>
            case error of
                HostError ResourceVanished _ _ => pureIO ()
                | other => failIO other)

workerStep : Chan Order -> Chan Fulfilment -> Task ()
workerStep jobs results = do
    order <- recv jobs
    processOrder results order
    worker jobs results

produce : Chan Order -> Task ()
produce jobs = do
    send jobs (Order 1001 4)
    send jobs (Order 1002 0)
    send jobs (Order 1003 80)
    send jobs (Order 1004 2)
    closeChan jobs

countAccepted : Fulfilment -> Int
countAccepted result =
    case result of
        Accepted orderId => 1
        | RejectedOrder orderId reason => 0

collect : Chan Fulfilment -> Int -> Int -> Task Int
collect results remaining accepted =
    if remaining == 0 then
        pureIO accepted
    else do
        result <- recv results
        collect results (remaining - 1) (accepted + countAccepted result)

runPipeline : Chan Order -> Chan Fulfilment -> Task Int
runPipeline jobs results =
    withThread
        (worker jobs results)
        (fun firstWorker =>
            withThread
                (worker jobs results)
                (fun secondWorker => do
                    completed <- concurrently
                        (produce jobs)
                        (collect results 4 0)
                    case completed of
                        (produced, accepted) => pureIO accepted))

main : Task ()
main = do
    jobs <- newBoundedChan 2
    results <- newBoundedChan 2
    accepted <- runPipeline jobs results
    closeChan results
    printLine (appendString "accepted orders: " (show accepted))
