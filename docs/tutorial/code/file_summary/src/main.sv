import std.fs(FsError(..))
import std.list(length)
import std.path(Path, pathFromString)
import std.text(lines, trimAscii)

fsToIO : FsError -> IOError
fsToIO (FsIOError _ ioError) = ioError
fsToIO error = IOError InvalidArgument "filesystem" (show error)

requiredPath : String -> Path
requiredPath spelling =
    case pathFromString spelling of
        Right path => path
        | Left pathError => error (show pathError)

nonEmpty : String -> Bool
nonEmpty text = text /= ""

main : Task ()
main = do
    contents <- mapError fsToIO $ std.fs.readFile $ requiredPath "input/readings.txt"
    let readings = filter nonEmpty $ lines $ trimAscii contents
    let report = "readings: " <> show (length readings) <> "\n"
    mapError fsToIO $ std.fs.writeFile (requiredPath "summary.txt") report
    printStr report
