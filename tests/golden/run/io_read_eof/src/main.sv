main : IO ()
main = bindIO readLine (fun line => printLine line)
