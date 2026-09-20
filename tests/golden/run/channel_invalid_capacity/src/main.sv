import std.channel(*)

report : String -> IOError -> Task ()
report expected (IOError InvalidArgument _ _) = printLine expected
report _ _ = printLine "unexpected-error"

invalidChannel : Int -> Task (Chan Int)
invalidChannel capacity = newBoundedChan capacity

checkInvalid : String -> Int -> Task ()
checkInvalid expected capacity =
    catchError
        (bindIO (invalidChannel capacity) (fun channel => printLine "unexpected-success"))
        (report expected)

main : Task ()
main = do
    checkInvalid "zero-capacity" 0
    checkInvalid "negative-capacity" (0 - 1)
    checkInvalid "oversized-capacity" 4294967296
