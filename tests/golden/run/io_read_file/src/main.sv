import std.fs(FsError(..))
import std.path(Path, pathFromString)

fsToHost : FsError -> HostError
fsToHost (FsHostError _ hostError) = hostError
fsToHost error = HostError InvalidArgument "filesystem" (show error)

requiredPath : String -> Path
requiredPath spelling =
    case pathFromString spelling of
        Right path => path
        | Left pathError => error (show pathError)

main : Task ()
main = do
    contents <- mapError fsToHost (std.fs.readFile (requiredPath "questions.txt"))
    printStr contents
