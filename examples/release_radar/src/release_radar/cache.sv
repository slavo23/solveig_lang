import std.fs
import std.io
import std.map
import std.text
import release_radar.model

data Cache = Cache (std.map.Map String String)

empty : Cache
empty = Cache std.map.empty

etagFor : String -> Cache -> Maybe String
etagFor repository (Cache entries) = std.map.lookup repository entries

parseCache : String -> Cache
parseCache text = Cache (foldl insertCacheLine std.map.empty (std.text.lines text))

insertCacheLine : std.map.Map String String -> String -> std.map.Map String String
insertCacheLine entries line =
    case std.text.splitOnce '\t' line of
        Nothing => entries
        | Just (MkPair repository etag) => std.map.insert repository etag entries

loadCache : String -> Task Cache
loadCache path = do
    exists <- std.fs.doesFileExist path
    if exists then fmap parseCache (std.io.readFileLimited path 65536)
    else pureIO empty

cacheText : [release_radar.model.ReleaseResult] -> String
cacheText = foldr appendCacheLine ""

appendCacheLine : release_radar.model.ReleaseResult -> String -> String
appendCacheLine (ReleaseFound repository _ _ _ etag) rest =
    if std.text.isEmpty etag then rest
    else repository <> "\t" <> etag <> "\n" <> rest
appendCacheLine (ReleaseUnchanged repository etag) rest =
    repository <> "\t" <> etag <> "\n" <> rest
appendCacheLine (ReleaseFailed _ _) rest = rest

writeCache : String -> [release_radar.model.ReleaseResult] -> Task ()
writeCache path = std.io.writeFile path . cacheText
