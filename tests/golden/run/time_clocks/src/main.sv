import std.time(*)
import std.text(intToString)

main : Task ()
main = do
    monotonicBefore <- getMonotonicTime
    systemBefore <- getSystemTime
    sleep (milliseconds 5)
    monotonicAfter <- getMonotonicTime
    systemAfter <- getSystemTime
    printLine
        (appendString
            "monotonic delta: "
            (intToString (durationToNanoseconds (instantDifference monotonicBefore monotonicAfter))))
    printLine
        (appendString
            "system delta: "
            (intToString (durationToNanoseconds (systemTimeDifference systemBefore systemAfter))))
