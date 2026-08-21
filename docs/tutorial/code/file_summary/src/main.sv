import std.io
import std.list(length)
import std.text(lines, trimAscii)

nonEmpty : String -> Bool
nonEmpty text = text /= ""

main : Task ()
main = do
    contents <- readFile "input/readings.txt"
    let readings = filter nonEmpty (lines (trimAscii contents))
    let report = "readings: " <> show (length readings) <> "\n"
    writeFile "summary.txt" report
    printStr report
