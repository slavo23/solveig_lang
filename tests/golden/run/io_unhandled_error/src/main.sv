main : Task ()
main = failIO (IOError OtherError "main" "boom")
