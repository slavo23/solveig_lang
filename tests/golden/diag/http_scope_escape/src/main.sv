-- A response body cannot escape the callback that owns its host handle.
import std.http(*)
import std.stream(Stream)

escape : String -> IO HttpError (Stream () HttpError Bytes)
escape spelling = withResponse Get spelling (fun _ _ body => pureIO body)

main : Task ()
main = pureIO ()
