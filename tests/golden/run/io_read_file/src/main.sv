import std.fs(FsError(..))
import std.path(Path, pathFromString)

fsToIO : FsError -> IOError
fsToIO (FsIOError _ ioError) = ioError
fsToIO error = IOError InvalidArgument "filesystem" (show error)

requiredPath : String -> Path
requiredPath spelling =
    case pathFromString spelling of
        Right path => path
        | Left pathError => error (show pathError)

main : Task ()
main = do
    contents <- mapError fsToIO (std.fs.readFile (requiredPath "questions.txt"))
    printStr contents
