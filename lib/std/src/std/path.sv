export (
    Path, PathError(..), pathFromString, pathToString, currentPath
)

import std.text(charToCodePoint, stringToChars)

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
    if spelling == "" then Left EmptyPath
    else if hasNull (stringToChars spelling) then Left NullPath
    else Right (Path spelling)

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

hasNull : [Char] -> Bool
hasNull [] = False
hasNull (character :: rest) =
    if charToCodePoint character == 0 then True else hasNull rest
