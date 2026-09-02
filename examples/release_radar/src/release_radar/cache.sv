import std.fs(*)
import std.io(*)
import std.map(*)
import std.path(*)
import std.text(*)
import release_radar.model(*)

data Cache = Cache (std.map.Map String String)

fsToHost : std.fs.FsError -> HostError
fsToHost (std.fs.FsHostError _ hostError) = hostError
fsToHost error = HostError InvalidArgument "filesystem" (show error)

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
        | Just (repository, etag) => std.map.insert repository etag entries

loadCache : String -> Task Cache
loadCache path = do
    exists <- mapError fsToHost (std.fs.doesFileExist (requiredPath path))
    if exists then fmap parseCache (std.io.readFileLimited path 65536) else pureIO empty

updateCache : Cache -> [release_radar.model.ReleaseResult] -> Cache
updateCache (Cache entries) results = Cache (foldl updateEntry entries results)

updateEntry :
    std.map.Map String String -> release_radar.model.ReleaseResult -> std.map.Map String String
updateEntry entries (ReleaseFound repository _ _ _ etag) =
    if std.text.isEmpty etag then entries else std.map.insert repository etag entries
updateEntry entries (ReleaseUnchanged repository etag) = std.map.insert repository etag entries
updateEntry entries (ReleaseFailed _ _) = entries

cacheText : Cache -> String
cacheText (Cache entries) = foldr appendCacheEntry "" (std.map.toAscList entries)

appendCacheEntry : (String, String) -> String -> String
appendCacheEntry (repository, etag) rest = repository <> "\t" <> etag <> "\n" <> rest

writeCache : String -> Cache -> Task ()
writeCache path = std.io.writeFile path . cacheText

requiredPath : String -> Path
requiredPath spelling =
    case pathFromString spelling of
        Right path => path
        | Left pathError => error ("invalid Release Radar path: " <> show pathError)
