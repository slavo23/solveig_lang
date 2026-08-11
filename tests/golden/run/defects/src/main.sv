reportDefect : Defect -> Task ()
reportDefect defect =
    case defect of
        Error message => printLine (appendString "error=" message)
        | DivisionByZero => printLine "division-by-zero"
        | StackExhausted => printLine "stack-exhausted"
        | HeapExhausted => printLine "heap-exhausted"

reportTypedError : IOError -> Task ()
reportTypedError ioError = printLine "typed-error"

explicitDefect : Task ()
explicitDefect =
    bindIO (pureIO ()) (fun ignored => pureIO (error "boom"))

divisionDefect : Task ()
divisionDefect =
    bindIO (pureIO ()) (fun ignored =>
        bindIO (pureIO (div 1 0)) (fun value => pureIO ()))

release : String -> Task ()
release resource = printLine (appendString "release " resource)

use : String -> Task ()
use resource = do
    printLine (appendString "use " resource)
    divisionDefect

typedFailure : Task ()
typedFailure = failIO (StdinError "typed")

main : Task ()
main = do
    catchDefect explicitDefect reportDefect
    catchDefect (catchError divisionDefect reportTypedError) reportDefect
    catchDefect (finally explicitDefect divisionDefect) reportDefect
    catchDefect
        (bracket (pureIO "resource") release use)
        reportDefect
    catchError
        (catchDefect typedFailure reportDefect)
        reportTypedError
