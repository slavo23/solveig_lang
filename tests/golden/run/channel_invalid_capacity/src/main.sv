report : Defect -> Task ()
report (Error _) = printLine "invalid-capacity"
report _ = printLine "unexpected-defect"

invalid : Task ()
invalid = do
    _ <- invalidChannel
    printLine "unexpected-success"

invalidChannel : Task (Chan Int)
invalidChannel = newBoundedChan 0

main : Task ()
main = catchDefect invalid report
