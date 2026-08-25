main : Task ()
main = failIO (HostError OtherError "main" "boom")
