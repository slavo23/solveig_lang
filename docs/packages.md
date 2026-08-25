# Solveig Packages

Solveig packages are deliberately small and local-first. A package is a
directory containing a strict `solveig.toml` manifest and a `src/` tree. The
compiler discovers all `.sv` files below `src/`, assigns them names beneath the
package namespace, and loads path dependencies recursively.

This page is the complete package reference.

## Start a package

The smallest useful layout is:

```text
hello/
|-- solveig.toml
`-- src/
    `-- main.sv
```

Create `hello/solveig.toml`:

```toml
[package]
name = "hello"
version = "0.1.0"

[language]
entry = "main"
```

Create `hello/src/main.sv`:

```solveig
main : String
main = "hello, Solveig"
```

Run it from any directory:

```sh
solveig --project-root path/to/hello
```

The manifest entry selects the source module. The command-line `--entry`
option selects a definition inside that module and defaults to `main`. For
example, this runs a top-level definition named `demo` from the same module:

```sh
solveig --project-root path/to/hello --entry demo
```

## Layout and module names

Every source module is an `.sv` file below the package's `src/` directory.
Directories become module-name segments and the package name is always the
first segment of the loaded module name.

For a package named `hello`:

| Source file | Manifest entry | Loaded module name |
|---|---|---|
| `src/main.sv` | `main` | `hello.main` |
| `src/cli/main.sv` | `cli.main` | `hello.cli.main` |
| `src/hello/main.sv` | `hello.main` | `hello.main` |

The first two forms are recommended: do not repeat the package directory under
`src/` unless an existing layout already does so. The third form remains
supported and is useful when migrating an older project.

All `.sv` files below `src/` are discovered recursively. Discovery is sorted,
so filesystem iteration order cannot affect compilation. Non-`.sv` files can
live beside source files and are ignored by module discovery.

Imports within the same package may be relative to the package namespace:

```text
hello/
|-- solveig.toml
`-- src/
    |-- main.sv
    `-- text/
        `-- greeting.sv
```

```solveig
-- src/main.sv
import text.greeting(greeting)

main : String
main = greeting "Solveig"
```

`text.greeting` resolves as `hello.text.greeting`. Writing the fully qualified
`hello.text.greeting` is also accepted. Imports from another package use that
package's declared name as their first segment.

Two packages in one dependency graph cannot declare the same package name.
This keeps every fully qualified module name unambiguous.

## Module APIs and visibility

A source file owns one centralized API header. Directives come before imports,
and imports come before definitions:

```solveig
export (User(..), UserId, findUser, (<!>))
private (validateCache)

import std.map(Map)

data User = User UserId String
type UserId = Int

findUser : UserId -> Map UserId User -> Maybe User
findUser identifier users = std.map.lookup identifier users

validateCache : Map UserId User -> Bool
validateCache users = True
```

Top-level definitions are package-visible by default. Another module in the
same package can import `findUser` even before it is listed in `export`, but a
different package can see only the centralized export list. A module with no
`export` directive therefore exposes no names outside its package. Use
`export ()` when that empty external API is intentional, as in an
instances-only module.

There may be at most one `export` and one `private` directive in a module.
`private` is stricter than the package boundary: a listed name is usable only
inside its defining module. It cannot also be exported. Ordinary helper
definitions need no annotation; leave them out of `export` when package-level
reuse is useful, and list them in `private` only when other modules must not
depend on them.

An API entity has one of these forms:

```solveig
export (
    parse,       -- a value or function
    (<?>),       -- an operator, written in parentheses
    Decoder,     -- an opaque data type; constructors stay hidden
    Result(..),  -- a data type together with all of its constructors
    UserId,      -- a type alias
    Monad        -- a class together with its methods and fixities
)
```

Solveig deliberately has no partial constructor list. `T` means an opaque type
and `T(..)` means the type with every constructor. This keeps changes such as
adding a constructor visible at one clear API boundary. A class is similarly
indivisible: exporting it also exports its methods, defaults, superclass
contract, and operator fixities.

### Imports and qualification

Imports are qualified by default. This keeps a dependency from silently adding
hundreds of short names to a module:

```solveig
import std.text

clean = text.trimAscii "  ready  "
alsoClean = std.text.trimAscii "  ready  "
```

The final module segment is the convenient default qualifier, and the complete
module path remains valid. An explicit alias replaces both:

```solveig
import std.text as Text

clean = Text.trimAscii "  ready  "
```

With the alias present, `text.trimAscii` and `std.text.trimAscii` are not in
scope. This makes renames decisive instead of introducing a third spelling.

Parentheses choose the exact names that are also available unqualified. They
do not limit the qualified API:

```solveig
import std.text(lines, trimAscii as trim)

clean = trim "  ready  "
rows = lines (std.text.toUpperAscii clean)
```

Use `(*)` only when the complete visible API should be unqualified:

```solveig
import std.text(*)
```

Import entities use the same rules as module APIs:

```solveig
import data.model(
    User,                  -- opaque type
    Result(..) as Outcome, -- renamed type plus every original constructor
    Parser,                -- class together with every method and fixity
    (<?>) as (<||>)        -- renamed operator
)
```

Constructors are selected through their owning type with `T(..)`, never one by
one, and class methods are selected through their class. A rename changes only
the unqualified binding. The original name remains available through the
module qualifier, and opening a renamed type does not rename its constructors.

There is no hiding form. If two imports provide the same unqualified name,
using that name is an ambiguity error unless both routes lead to the same
definition. Qualifier collisions are reported at the import itself. Prelude
modules are the sole automatic exception: their visible API is imported
unqualified exactly once, while `std` and every user library remain explicit.

### Re-exports and facade modules

A module can republish another module without introducing forwarding
definitions:

```solveig
export (parseConfig)
reexport config.types(Config, ConfigError(..))
reexport config.text(parse as parseConfigText)
import config.text(parse)

parseConfig : String -> Result ConfigError Config
parseConfig source = parse source
```

The precise form can rename an ordinary or operator name with `as`. Type
opacity is preserved: `Config` does not expose constructors, while
`ConfigError(..)` does. To publish the source module's complete API, use:

```solveig
reexport config.defaults(*)
```

`reexport` is a real module dependency. Its source must be in the package graph,
module cycles are rejected, typed definitions keep their original identity,
and instances available through the source remain available through the
facade. Re-exported names are part of the facade's external API automatically;
the local `export` list describes definitions owned by the facade itself. They
are not introduced unqualified inside the facade: import a source name
explicitly when a local definition also needs to use it.

Export, private, and re-export selections reject missing or namespace-ambiguous
names. Re-export collisions are errors rather than order-dependent shadowing.

## Manifest reference

The manifest has three top-level tables. Unknown tables and keys are errors,
including unknown keys nested inside a dependency. This strictness catches
misspellings instead of silently ignoring configuration.

### `[package]`

```toml
[package]
name = "hello"
version = "0.1.0"
```

| Key | Required | Meaning |
|---|---|---|
| `name` | yes | Package namespace and the first segment of every module name |
| `version` | yes | Package version metadata |

`name` must be one valid ASCII Solveig module segment: it starts with a letter
or `_`, followed by letters, digits, `_`, or `'`. It cannot contain dots or
hyphens.

`version` must be non-empty. It is metadata only today: Solveig does not parse
semantic versions or use versions to choose a dependency.

### `[language]`

```toml
[language]
entry = "cli.main"
```

| Key | Required | Meaning |
|---|---|---|
| `entry` | yes | Dot-separated source path beneath `src/`, without `.sv` |
| `implicit` | no | Compiler-library setting used by the shipped Prelude |

For `entry = "cli.main"`, the compiler opens `src/cli/main.sv`. The entry must
contain the definition selected by the CLI's `--entry` option, normally
`main`.

Ordinary packages should omit `implicit`. It is recognized so the shipped
Prelude can declare its automatically imported modules; setting it in an
application or path dependency does not turn that package into another
Prelude.

### `[dependencies]`

Each dependency is a local path entry:

```toml
[dependencies]
text_tools = { path = "../text-tools" }
```

The path:

- is resolved relative to the manifest containing the dependency;
- must lead to a directory with its own valid `solveig.toml`;
- is canonicalized before graph construction;
- may itself contain further path dependencies.

The table key identifies the dependency edge but is not an import alias. The
dependency's own `[package].name` is its module prefix. Matching the key to the
declared package name, as above, is strongly recommended because it makes the
manifest and imports agree at a glance.

## A package with a dependency

Suppose an application and library are siblings:

```text
workspace/
|-- app/
|   |-- solveig.toml
|   `-- src/
|       `-- main.sv
`-- text-tools/
    |-- solveig.toml
    `-- src/
        `-- normalize.sv
```

The library manifest is:

```toml
[package]
name = "text_tools"
version = "0.1.0"

[language]
entry = "normalize"
```

Its `src/normalize.sv` explicitly publishes the definition:

```solveig
export (normalize)

normalize : String -> String
normalize text = text
```

The application manifest declares the path:

```toml
[package]
name = "app"
version = "0.1.0"

[dependencies]
text_tools = { path = "../text-tools" }

[language]
entry = "main"
```

The application imports the dependency through its package-qualified module:

```solveig
import text_tools.normalize(normalize)

main : String
main = normalize "package imports work"
```

Run the application package, not the dependency directory:

```sh
solveig --project-root workspace/app
```

The compiler loads the complete transitive path-dependency graph before source
resolution. Dependency cycles and duplicate package names are reported as
package errors. Unchanged packages are cached independently, so editing local
application source does not require rechecking an unchanged dependency.

Typeclass instance ownership is package-level. An instance may live in any
module of the package that defines its class or its outer type constructor, so
a focused `instances` module remains coherent. An instance defined in an
unrelated package is still an orphan and produces `TC0018`.

## Project discovery and scratch files

The command-line `--project-root` should point to the directory containing
`solveig.toml`. Editors work in the other direction: when a `.sv` file is
opened, Solveig walks upward and chooses the nearest ancestor containing a
manifest. The nearest manifest wins when packages are nested below a broader
editor workspace.

A bare `.sv` file with no manifest still works for editor scratch files. It is
treated as a manifest-free local project rooted at the file's directory, with
the opened file as its entry; imports of sibling source modules remain
available. For legacy command-line projects without a manifest, Solveig looks
for `src/main.sv`. This compatibility mode has no manifest dependency graph;
use a real package for applications and libraries.

## Prelude and standard library

Solveig ships two compiler-library packages:

- `prelude` is implicit. Its small universal vocabulary is automatically in
  scope in every ordinary module.
- `std` is available to every project but never imported automatically. Import
  the specific `std.*` modules your program uses.

For example:

```solveig
import std.fs(doesFileExist)
import std.list(take, zip)
import std.task(concurrently)
```

You do not add `prelude` or `std` to your application's `[dependencies]`.
Solveig loads the shipped copies itself. The boundary is intentional: adding a
new standard-library function cannot create a new name in every program.

See the [Prelude reference](prelude.md) for automatically available names and
the [standard-library reference](standard-library.md) for the explicit module
map.

## How compiler libraries are found

The compiler resolves one library root containing both packages:

```text
lib/
|-- prelude/
|   |-- solveig.toml
|   `-- src/
`-- std/
    |-- solveig.toml
    `-- src/
```

The search order is fixed:

1. `SOLVEIG_LIB`, when set. Its value must be the `lib/` directory shown
   above, not the parent directory and not one individual package.
2. `lib/` beside the running `solveig` executable.
3. The library sources embedded when the compiler binary was built.

An explicitly set but invalid `SOLVEIG_LIB` is an error; Solveig does not hide
the bad override by falling through. An executable-relative candidate is used
only when it contains both package manifests. The selected root and whether it
came from the environment, executable layout, or embedded fallback are written
to tracing output.

After `cargo install --path .`, copy `lib/prelude` and `lib/std` beside the
installed executable when you want navigation into editable library source.
Packagers should preserve the same layout.
The embedded fallback keeps a copied bare binary usable, but an installed
on-disk library is preferable because editor navigation can open the actual
Prelude and standard-library source files.

To develop against a custom library tree:

```sh
SOLVEIG_LIB=/absolute/path/to/lib solveig --project-root path/to/app
```

The override applies to both Prelude and `std`; keep their manifests and source
trees together.

## Editing dependencies in the IDE

Dependency and compiler-library source remains fully inspectable. Hover,
completion, go-to-definition, and find-references work across package
boundaries.

IDE-generated mutations are limited to the local package:

- rename refuses the whole operation if any declaration or reference that
  would change belongs to a dependency or implicit library;
- code actions never return edits for dependency or implicit-library files.

This is an editor safety boundary, not a filesystem permission. You can edit a
library file manually or update a path dependency on disk. Opening an ordinary
path dependency's directory as the local project enables package-wide rename,
code actions for that dependency.

The shipped `prelude` and `std` trees retain their compiler-library roles even
when opened directly, so IDE-generated edits remain protected there today.
Edit those sources manually in a checkout or custom `SOLVEIG_LIB` tree.

## What is deliberately absent

The current package system has no:

- package registry or publish/install command;
- version-based dependency resolution;
- lockfile;
- git, URL, or archive dependencies.

Versions are recorded but do not participate in resolution. Every external
dependency is an explicit local path, so reproducibility is currently the
responsibility of the containing repository or workspace.

## Troubleshooting

**`unknown field` while reading `solveig.toml`**

Manifest fields are strict. Check the spelling and move the key into the
correct table; unsupported metadata cannot be added speculatively.

**`entry file ... does not exist`**

`language.entry` maps directly beneath `src/`. For `entry = "cli.main"`, create
`src/cli/main.sv`.

**`package name ... is claimed by both ...`**

Two transitive dependencies declared the same `[package].name`. Rename one
package; dependency-table aliases do not create separate namespaces.

**`cyclic path dependency`**

The reported packages eventually depend on one another. Move their shared
definitions into a lower-level package so the path graph is acyclic.

**`invalid Solveig library root`**

Check that `SOLVEIG_LIB` points to the directory containing both
`prelude/solveig.toml` and `std/solveig.toml`.

**Rename or a code action refuses a library file**

The file is loaded as non-local. Open an ordinary path dependency's directory
as its own project when you intend to refactor it. Shipped `prelude` and `std`
source currently remains protected from IDE-generated edits; update it
manually.
