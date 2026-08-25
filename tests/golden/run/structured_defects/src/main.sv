import std.task(*)
import std.time(*)
import prelude.prim(raiseDefectIO)

raiseInChild : Defect -> Task ()
raiseInChild defect = prelude.prim.raiseDefectIO defect

peer : Task ()
peer = sleep 100

report : Defect -> Task ()
report defect =
    case defect of
        Error message => printLine (appendString "Error=" message)
        | DivisionByZero => printLine "DivisionByZero"
        | StackExhausted => printLine "StackExhausted"
        | HeapExhausted => printLine "HeapExhausted"

runDefect : Defect -> Task ()
runDefect defect =
    catchDefect
        (bindIO
            (concurrently (raiseInChild defect) peer)
            (fun unexpected => printLine "unexpected defect success"))
        report

main : Task ()
main = do
    runDefect (Error "boom")
    runDefect DivisionByZero
    runDefect StackExhausted
    runDefect HeapExhausted
