main : IO ()
main = bindIO getArgs (fun args => mapIO_ printLine args)
