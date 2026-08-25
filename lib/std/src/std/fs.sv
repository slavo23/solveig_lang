export (
    getCurrentDirectory, setCurrentDirectory, doesFileExist, doesDirectoryExist,
    listDirectory, createDirectory, removeFile, renameFile, getFileSize,
    getModificationTime
)

import prelude.prim(*)

-- | Returns the runtime's current working directory.
getCurrentDirectory : Task String
getCurrentDirectory = prelude.prim.getCurrentDirectory

-- | Changes the runtime's working directory after verifying it exists.
setCurrentDirectory : String -> Task ()
setCurrentDirectory path = prelude.prim.setCurrentDirectory path

-- | Reports whether a path names a regular file.
doesFileExist : String -> Task Bool
doesFileExist path = prelude.prim.doesFileExist path

-- | Reports whether a path names a directory.
doesDirectoryExist : String -> Task Bool
doesDirectoryExist path = prelude.prim.doesDirectoryExist path

-- | Lists a directory's immediate entries in lexical order.
listDirectory : String -> Task [String]
listDirectory path = prelude.prim.listDirectory path

-- | Creates one directory; its parent must already exist.
createDirectory : String -> Task ()
createDirectory path = prelude.prim.createDirectory path

-- | Removes one file.
removeFile : String -> Task ()
removeFile path = prelude.prim.removeFile path

-- | Renames a file or directory.
renameFile : String -> String -> Task ()
renameFile source destination = prelude.prim.renameFile source destination

-- | Returns a file's size in bytes.
getFileSize : String -> Task Int
getFileSize path = prelude.prim.getFileSize path

-- | Returns nanoseconds since the Unix epoch for a path's modification time.
getModificationTime : String -> Task Int
getModificationTime path = prelude.prim.getModificationTime path
