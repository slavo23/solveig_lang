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
    printLine
        (appendString
            "whole seconds: "
            (intToString (durationToSeconds (seconds 3))))
    printLine
        (appendString
            "positive saturation: "
            (intToString (durationToNanoseconds (seconds 9223372036854775807))))
    printLine
        (appendString
            "negative saturation: "
            (intToString
                (durationToNanoseconds (seconds ((-9223372036854775807) - 1)))))
