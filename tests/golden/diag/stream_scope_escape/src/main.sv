import std.io(withFileByteStream)
import std.stream(Stream)

escape : Task (Stream () HostError Bytes)
escape =
    withFileByteStream
        8
        "input.txt"
        escapeStream

escapeStream : forall scope. Stream scope HostError Bytes -> Task (Stream () HostError Bytes)
escapeStream stream = pureIO stream

main : Task ()
main = do
    escaped <- escape
    pureIO ()
