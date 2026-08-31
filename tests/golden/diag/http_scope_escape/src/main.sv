-- A response body cannot escape the callback that owns its host handle.
import std.http(*)
import std.stream(Stream)

escape : Request -> IO HttpError (Stream () HttpError Bytes)
escape requestValue = withResponse requestValue (fun _ _ body => pureIO body)

main : Task ()
main = pureIO ()
