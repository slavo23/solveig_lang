import std.fs(FsError, withFileByteStream)
import std.path(currentPath)
import std.stream(Stream)

escape : IO FsError (Stream () FsError Bytes)
escape = withFileByteStream 8 currentPath escapeStream

escapeStream : forall scope. Stream scope FsError Bytes -> IO FsError (Stream () FsError Bytes)
escapeStream stream = pureIO stream

main : Task ()
main = do
    escaped <- escape
    pureIO ()
