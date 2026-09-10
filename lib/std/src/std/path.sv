export (
    Path,
    PathError(..),
    pathFromString,
    pathToString,
    currentPath,
    joinPath,
    (</>),
    parent,
    fileName,
    extension,
    normalise,
    isAbsolute,
    isRelative
)

import std.prim(pathExtension, pathFileName, pathIsAbsolute, pathJoin, pathNormalise, pathParent)
import std.text(charToCodePoint, stringToChars)

infixr 5 </>

-- | A failure while constructing a host path value.
data PathError = EmptyPath | NullPath

instance Eq PathError where
    (==) EmptyPath EmptyPath = True
    (==) NullPath NullPath = True
    (==) _ _ = False

instance Show PathError where
    show EmptyPath = "a path cannot be empty"
    show NullPath = "a path cannot contain a null character"

-- | A non-empty host path without embedded null characters.
-- |
-- | The representation is opaque so filesystem APIs cannot be called with an
-- | arbitrary string by accident. Platform-aware path operations belong here
-- | rather than in callers.
data Path = Path String

instance Eq Path where
    (==) (Path left) (Path right) = left == right

instance PartialOrd Path where
    partialCompare (Path left) (Path right) = partialCompare left right

instance Ord Path where
    compare (Path left) (Path right) = compare left right

instance Show Path where
    show (Path spelling) = show spelling

instance Hashable Path where
    hashWithSalt salt (Path spelling) = hashWithSalt salt spelling

-- | Validates a host path spelling.
pathFromString : String -> Either PathError Path
pathFromString spelling =
    if spelling == ""
        then Left EmptyPath
        else if hasNull (stringToChars spelling) then Left NullPath else Right (Path spelling)

-- | Returns the host spelling retained by a path.
pathToString : Path -> String
pathToString (Path spelling) = spelling

-- Package-internal conversion for paths returned by host primitives, whose
-- Rust values cannot contain a null character.
pathFromHost : String -> Path
pathFromHost = Path

-- | The current-directory path.
currentPath : Path
currentPath = Path "."

-- | Joins two paths using the host platform's path rules.
-- |
-- | An absolute second path replaces the first path. The operation is lexical:
-- | it neither accesses the filesystem nor resolves symbolic links.
joinPath : Path -> Path -> Path
joinPath (Path base) (Path child) = Path (std.prim.pathJoin base child)

-- | Infix form of joinPath, with the same precedence as Haskell's (</>).
(</>) : Path -> Path -> Path
(</>) = joinPath

-- | Returns the lexical parent, or Nothing when the path has no non-empty parent.
parent : Path -> Maybe Path
parent (Path spelling) = fmap Path (std.prim.pathParent spelling)

-- | Returns the final normal path component.
-- |
-- | Root paths and paths ending in `.` or `..` have no file name.
fileName : Path -> Maybe String
fileName (Path spelling) = std.prim.pathFileName spelling

-- | Returns the final component's extension without the leading dot.
-- |
-- | A leading dot alone does not introduce an extension, so `.gitignore` has
-- | no extension. A trailing dot produces Just "".
extension : Path -> Maybe String
extension (Path spelling) = std.prim.pathExtension spelling

-- | Lexically removes redundant `.` components and collapses `name/..` pairs.
-- |
-- | Leading `..` components of relative paths are retained, while attempts to
-- | walk above a root are ignored. This does not access the filesystem,
-- | canonicalise case, or resolve symbolic links.
normalise : Path -> Path
normalise (Path spelling) = Path (std.prim.pathNormalise spelling)

-- | Reports whether the host platform treats a path as absolute.
isAbsolute : Path -> Bool
isAbsolute (Path spelling) = std.prim.pathIsAbsolute spelling

-- | Reports whether the host platform treats a path as relative.
isRelative : Path -> Bool
isRelative = not . isAbsolute

hasNull : [Char] -> Bool
hasNull [] = False
hasNull (character :: rest) = if charToCodePoint character == 0 then True else hasNull rest
