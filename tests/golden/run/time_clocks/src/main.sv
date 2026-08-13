import prelude.prim(intToString)

main : Task ()
main = do
    monotonicBefore <- getMonotonicTime
    systemBefore <- getSystemTime
    sleep 5
    monotonicAfter <- getMonotonicTime
    systemAfter <- getSystemTime
    printLine (appendString "monotonic delta: " (intToString (monotonicAfter - monotonicBefore)))
    printLine (appendString "system delta: " (intToString (systemAfter - systemBefore)))
