import std.fs(FsError(..))
import std.list(length)
import std.path(Path, pathFromString)
import std.text(lines, trimAscii)

fsToHost : FsError -> HostError
fsToHost (FsHostError _ hostError) = hostError
fsToHost error = HostError InvalidArgument "filesystem" (show error)

requiredPath : String -> Path
requiredPath spelling =
    case pathFromString spelling of
        Right path => path
        | Left pathError => error (show pathError)

nonEmpty : String -> Bool
nonEmpty text = text /= ""

main : Task ()
main = do
    contents <- mapError fsToHost (std.fs.readFile (requiredPath "input/readings.txt"))
    let readings = filter nonEmpty (lines (trimAscii contents))
    let report = "readings: " <> show (length readings) <> "\n"
    mapError fsToHost (std.fs.writeFile (requiredPath "summary.txt") report)
    printStr report
