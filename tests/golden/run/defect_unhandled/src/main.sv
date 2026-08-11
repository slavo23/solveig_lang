failLater : Task ()
failLater = bindIO (pureIO ()) (fun ignored => pureIO (error "boom"))

main : Task ()
main = failLater
