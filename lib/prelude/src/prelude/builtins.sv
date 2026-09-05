export (Int,
    Float,
    Char,
    String,
    IO,
    Bool(..),
    ThreadId,
    Chan,
    Bytes,
    Handle,
    StdGen,
    Maybe(..),
    Either(..),
    Tuple2(..),
    Tuple3(..),
    Tuple4(..),
    Tuple5(..),
    Tuple6(..),
    Tuple7(..),
    Tuple8(..),
    Never,
    HostErrorKind(..),
    HostError(..),
    hostErrorKind,
    hostErrorOperation,
    hostErrorMessage,
    Utf8Error(..),
    Defect(..),
    Task,
    pureIO,
    bindIO,
    failIO,
    error,
    catchError,
    mapError,
    orElse,
    printLine,
    printStr,
    readLine,
    appendString,
    even,
    odd,
    not,
    and,
    or)

import prelude.prim(*)

-- | A value that may be present or absent.
data Maybe a =
    -- | Wraps a present value.
    Just a
    | -- | Represents the absence of a value.
    Nothing

-- | A value that can hold one of two alternatives.
data Either a b =
    -- | Holds a value from the left side.
    Left a
    | -- | Holds a value from the right side.
    Right b

-- | A heterogeneous product of two values, written `(a, b)`.
data Tuple2 a b = MkTuple2 a b

-- | A heterogeneous product of three values, written `(a, b, c)`.
data Tuple3 a b c = MkTuple3 a b c

-- | A heterogeneous product of four values, written `(a, b, c, d)`.
data Tuple4 a b c d = MkTuple4 a b c d

-- | A heterogeneous product of five values, written `(a, b, c, d, e)`.
data Tuple5 a b c d e = MkTuple5 a b c d e

-- | A heterogeneous product of six values, written `(a, b, c, d, e, f)`.
data Tuple6 a b c d e f = MkTuple6 a b c d e f

-- | A heterogeneous product of seven values, written `(a, b, c, d, e, f, g)`.
data Tuple7 a b c d e f g = MkTuple7 a b c d e f g

-- | A heterogeneous product of eight values, written `(a, b, c, d, e, f, g, h)`.
data Tuple8 a b c d e f g h = MkTuple8 a b c d e f g h

-- | An error type with no possible values.
data Never

-- | Stable, cross-platform categories for low-level host failures.
-- |
-- | The categories deliberately describe causes rather than individual
-- | operations. Domain libraries add paths, endpoints, or policy context.
data HostErrorKind =
    AlreadyExists
    | DoesNotExist
    | PermissionDenied
    | ResourceBusy
    | ResourceExhausted
    | EndOfFile
    | InvalidArgument
    | UnsupportedOperation
    | TimeExpired
    | ResourceVanished
    | Interrupted
    | NetworkUnreachable
    | ConnectionRefused
    | AddressInUse
    | ProtocolError
    | OtherError

-- | A low-level failure with a stable category, operation, and host message.
data HostError = HostError HostErrorKind String String

-- | Returns a host error's stable category.
hostErrorKind : HostError -> HostErrorKind
hostErrorKind (HostError kind _ _) = kind

-- | Returns the low-level operation that failed.
hostErrorOperation : HostError -> String
hostErrorOperation (HostError _ operation _) = operation

-- | Returns the platform message retained for diagnostics.
hostErrorMessage : HostError -> String
hostErrorMessage (HostError _ _ message) = message

-- | A failure to decode strict UTF-8 text.
data Utf8Error =
    -- | Invalid UTF-8 beginning at the zero-based byte offset.
    InvalidUtf8 Int

-- | A defect raised by pure evaluation or a managed runtime limit.
data Defect =
    -- | An explicit unrecoverable condition raised by user code.
    Error String
    | -- | Integer division or modulo used a zero divisor.
    DivisionByZero
    | -- | A process exceeded the managed VM stack limit.
    StackExhausted
    | -- | The runtime exceeded the managed heap-object limit.
    HeapExhausted

-- | An IO action that can fail at the low-level host boundary.
type Task a = IO HostError a

-- | Creates an IO action that immediately returns a value.
pureIO : forall e a. a -> IO e a
pureIO value = prelude.prim.pureIO value

-- | Sequences an IO action and passes its result to a continuation.
bindIO : forall e a b. IO e a -> (a -> IO e b) -> IO e b
bindIO action continuation = prelude.prim.bindIO action continuation

-- | Creates an IO action that fails with a typed error.
failIO : forall e a. e -> IO e a
failIO error = prelude.prim.failIO error

-- | Raises a defect from pure code.
error : forall a. String -> a
error message = prelude.prim.error message

-- | Handles a typed error and may change the action's error type.
catchError : forall e f a. IO e a -> (e -> IO f a) -> IO f a
catchError action handler = prelude.prim.catchIO action handler

-- | Transforms the typed error of an IO action.
mapError : forall e f a. (e -> f) -> IO e a -> IO f a
mapError transform action = catchError action (fun actionError => failIO (transform actionError))

-- | Uses a fallback action when the first action fails.
orElse : forall e f a. IO e a -> IO f a -> IO f a
orElse action fallback = catchError action (fun actionError => fallback)

-- | Writes text followed by a newline.
printLine : String -> Task ()
printLine text = prelude.prim.hPutStrLn prelude.prim.stdout text

-- | Writes text without adding a newline.
printStr : String -> Task ()
printStr text =
    bindIO
        (prelude.prim.hPutStr prelude.prim.stdout text)
        (fun written => prelude.prim.hFlush prelude.prim.stdout)

-- | Reads one line of input.
readLine : Task String
readLine = prelude.prim.hGetLine prelude.prim.stdin

-- | Concatenates two strings.
appendString : String -> String -> String
appendString left right = prelude.prim.stringAppend left right

-- | Checks whether an integer is even.
even : Int -> Bool
even n = intEq (intMod n 2) 0

-- | Checks whether an integer is odd.
odd : Int -> Bool
odd n = not (even n)

-- | Flips a boolean value.
not : Bool -> Bool
not True = False
not False = True

-- | Returns True only when both inputs are True.
and : Bool -> Bool -> Bool
and True True = True
and _ _ = False

-- | Returns True when at least one input is True.
or : Bool -> Bool -> Bool
or False False = False
or _ _ = True
