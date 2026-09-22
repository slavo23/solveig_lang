export ()

import prelude.builtins(*)
import prelude.control(*)
import prelude.list(*)
import prelude.prim(*)
import prelude.show(*)
import prelude.typeclasses(*)

instance Eq Int where
    (==) x y = intEq x y

instance Eq BigInt where
    (==) x y = bigIntEq x y

instance Eq Float where
    (==) x y = floatEq x y

instance Eq Char where
    (==) x y = charEq x y

instance Eq String where
    (==) x y = stringEq x y

instance Eq Bytes where
    (==) x y = bytesEq x y

instance Eq () where
    (==) _ _ = True

instance Eq Bool where
    (==) True True = True
    (==) False False = True
    (==) _ _ = False

instance Eq Ordering where
    (==) LT LT = True
    (==) EQ EQ = True
    (==) GT GT = True
    (==) _ _ = False

instance Eq IOErrorKind where
    (==) AlreadyExists AlreadyExists = True
    (==) DoesNotExist DoesNotExist = True
    (==) PermissionDenied PermissionDenied = True
    (==) ResourceBusy ResourceBusy = True
    (==) ResourceExhausted ResourceExhausted = True
    (==) EndOfFile EndOfFile = True
    (==) InvalidArgument InvalidArgument = True
    (==) UnsupportedOperation UnsupportedOperation = True
    (==) TimeExpired TimeExpired = True
    (==) ResourceVanished ResourceVanished = True
    (==) Interrupted Interrupted = True
    (==) NetworkUnreachable NetworkUnreachable = True
    (==) ConnectionRefused ConnectionRefused = True
    (==) AddressInUse AddressInUse = True
    (==) ProtocolError ProtocolError = True
    (==) OtherError OtherError = True
    (==) _ _ = False

instance Eq IOError where
    (==)
        (IOError leftKind leftOperation leftMessage)
        (IOError rightKind rightOperation rightMessage) =
        and
            (leftKind == rightKind)
            (and (leftOperation == rightOperation) (leftMessage == rightMessage))

instance Eq Utf8Error where
    (==) (InvalidUtf8 leftOffset) (InvalidUtf8 rightOffset) = leftOffset == rightOffset

instance Eq a => Eq (Maybe a) where
    (==) Nothing Nothing = True
    (==) (Just x) (Just y) = x == y
    (==) _ _ = False

instance Eq a => Eq (List a) where
    (==) [] [] = True
    (==) (x :: xs) (y :: ys) = if x == y then xs == ys else False
    (==) _ _ = False

instance (Eq a, Eq b) => Eq (a, b) where
    (==) (leftA, leftB) (rightA, rightB) = if leftA == rightA then leftB == rightB else False

instance (Eq a, Eq b, Eq c) => Eq (a, b, c) where
    (==) (a, b, c) (x, y, z) = if a == x then if b == y then c == z else False else False

instance (Eq a, Eq b, Eq c, Eq d) => Eq (a, b, c, d) where
    (==) (a, b, c, d) (w, x, y, z) =
        if a == w then if b == x then if c == y then d == z else False else False else False

instance (Eq a, Eq b, Eq c, Eq d, Eq e) => Eq (a, b, c, d, e) where
    (==) (a, b, c, d, e) (v, w, x, y, z) =
        if a == v
            then
                if b == w then if c == x then if d == y then e == z else False else False else False
            else False

instance (Eq a, Eq b, Eq c, Eq d, Eq e, Eq f) => Eq (a, b, c, d, e, f) where
    (==) (a, b, c, d, e, f) (u, v, w, x, y, z) =
        if a == u
            then
                if b == v
                    then
                        if c == w
                            then if d == x then if e == y then f == z else False else False
                            else False
                    else False
            else False

instance (Eq a, Eq b, Eq c, Eq d, Eq e, Eq f, Eq g) => Eq (a, b, c, d, e, f, g) where
    (==) (a, b, c, d, e, f, g) (t, u, v, w, x, y, z) =
        if a == t
            then
                if b == u
                    then
                        if c == v
                            then
                                if d == w
                                    then if e == x then if f == y then g == z else False else False
                                    else False
                            else False
                    else False
            else False

instance (Eq a, Eq b, Eq c, Eq d, Eq e, Eq f, Eq g, Eq h) => Eq (a, b, c, d, e, f, g, h) where
    (==) (a, b, c, d, e, f, g, h) (s, t, u, v, w, x, y, z) =
        if a == s
            then
                if b == t
                    then
                        if c == u
                            then
                                if d == v
                                    then
                                        if e == w
                                            then
                                                if f == x
                                                    then if g == y then h == z else False
                                                    else False
                                            else False
                                    else False
                            else False
                    else False
            else False

instance (Eq a, Eq b) => Eq (Either a b) where
    (==) (Left x) (Left y) = x == y
    (==) (Right x) (Right y) = x == y
    (==) _ _ = False

instance PartialOrd Int where
    partialCompare x y = Just (intCompare x y)

instance PartialOrd BigInt where
    partialCompare x y = Just (bigIntCompare x y)

instance PartialOrd Float where
    partialCompare x y =
        if floatLt x y
            then Just LT
            else if floatEq x y then Just EQ else if floatGt x y then Just GT else Nothing

instance PartialOrd Char where
    partialCompare x y = Just (charCompare x y)

instance PartialOrd String where
    partialCompare x y = Just (stringCompare x y)

instance PartialOrd () where
    partialCompare _ _ = Just EQ

instance PartialOrd Bool where
    partialCompare False False = Just EQ
    partialCompare False True = Just LT
    partialCompare True False = Just GT
    partialCompare True True = Just EQ

instance PartialOrd Ordering where
    partialCompare LT LT = Just EQ
    partialCompare LT _ = Just LT
    partialCompare EQ LT = Just GT
    partialCompare EQ EQ = Just EQ
    partialCompare EQ GT = Just LT
    partialCompare GT GT = Just EQ
    partialCompare GT _ = Just GT

instance PartialOrd a => PartialOrd (Maybe a) where
    partialCompare Nothing Nothing = Just EQ
    partialCompare Nothing (Just y) = Just LT
    partialCompare (Just x) Nothing = Just GT
    partialCompare (Just x) (Just y) = partialCompare x y

instance PartialOrd a => PartialOrd (List a) where
    partialCompare [] [] = Just EQ
    partialCompare [] (y :: ys) = Just LT
    partialCompare (x :: xs) [] = Just GT
    partialCompare (x :: xs) (y :: ys) =
        case partialCompare x y of
            Nothing => Nothing
            | Just LT => Just LT
            | Just GT => Just GT
            | Just EQ => partialCompare xs ys

instance (PartialOrd a, PartialOrd b) => PartialOrd (a, b) where
    partialCompare (leftA, leftB) (rightA, rightB) =
        case partialCompare leftA rightA of
            Nothing => Nothing
            | Just LT => Just LT
            | Just GT => Just GT
            | Just EQ => partialCompare leftB rightB

instance (PartialOrd a, PartialOrd b, PartialOrd c) => PartialOrd (a, b, c) where
    partialCompare (a, b, c) (x, y, z) =
        case partialCompare a x of
            Nothing => Nothing
            | Just LT => Just LT
            | Just GT => Just GT
            | Just EQ =>
                case partialCompare b y of
                    Nothing => Nothing
                    | Just LT => Just LT
                    | Just GT => Just GT
                    | Just EQ => partialCompare c z

instance (PartialOrd a, PartialOrd b, PartialOrd c, PartialOrd d) => PartialOrd (a, b, c, d) where
    partialCompare (a, b, c, d) (w, x, y, z) =
        case partialCompare a w of
            Nothing => Nothing
            | Just LT => Just LT
            | Just GT => Just GT
            | Just EQ =>
                case partialCompare b x of
                    Nothing => Nothing
                    | Just LT => Just LT
                    | Just GT => Just GT
                    | Just EQ =>
                        case partialCompare c y of
                            Nothing => Nothing
                            | Just LT => Just LT
                            | Just GT => Just GT
                            | Just EQ => partialCompare d z

instance (PartialOrd a, PartialOrd b, PartialOrd c, PartialOrd d, PartialOrd e) =>
    PartialOrd (a, b, c, d, e) where
    partialCompare (a, b, c, d, e) (v, w, x, y, z) =
        case partialCompare a v of
            Nothing => Nothing
            | Just LT => Just LT
            | Just GT => Just GT
            | Just EQ =>
                case partialCompare b w of
                    Nothing => Nothing
                    | Just LT => Just LT
                    | Just GT => Just GT
                    | Just EQ =>
                        case partialCompare c x of
                            Nothing => Nothing
                            | Just LT => Just LT
                            | Just GT => Just GT
                            | Just EQ =>
                                case partialCompare d y of
                                    Nothing => Nothing
                                    | Just LT => Just LT
                                    | Just GT => Just GT
                                    | Just EQ => partialCompare e z

instance (PartialOrd a, PartialOrd b, PartialOrd c, PartialOrd d, PartialOrd e, PartialOrd f) =>
    PartialOrd (a, b, c, d, e, f) where
    partialCompare (a, b, c, d, e, f) (u, v, w, x, y, z) =
        case partialCompare a u of
            Nothing => Nothing
            | Just LT => Just LT
            | Just GT => Just GT
            | Just EQ =>
                case partialCompare b v of
                    Nothing => Nothing
                    | Just LT => Just LT
                    | Just GT => Just GT
                    | Just EQ =>
                        case partialCompare c w of
                            Nothing => Nothing
                            | Just LT => Just LT
                            | Just GT => Just GT
                            | Just EQ =>
                                case partialCompare d x of
                                    Nothing => Nothing
                                    | Just LT => Just LT
                                    | Just GT => Just GT
                                    | Just EQ =>
                                        case partialCompare e y of
                                            Nothing => Nothing
                                            | Just LT => Just LT
                                            | Just GT => Just GT
                                            | Just EQ => partialCompare f z

instance (
        PartialOrd a,
        PartialOrd b,
        PartialOrd c,
        PartialOrd d,
        PartialOrd e,
        PartialOrd f,
        PartialOrd g
    ) =>
    PartialOrd (a, b, c, d, e, f, g) where
    partialCompare (a, b, c, d, e, f, g) (t, u, v, w, x, y, z) =
        case partialCompare a t of
            Nothing => Nothing
            | Just LT => Just LT
            | Just GT => Just GT
            | Just EQ =>
                case partialCompare b u of
                    Nothing => Nothing
                    | Just LT => Just LT
                    | Just GT => Just GT
                    | Just EQ =>
                        case partialCompare c v of
                            Nothing => Nothing
                            | Just LT => Just LT
                            | Just GT => Just GT
                            | Just EQ =>
                                case partialCompare d w of
                                    Nothing => Nothing
                                    | Just LT => Just LT
                                    | Just GT => Just GT
                                    | Just EQ =>
                                        case partialCompare e x of
                                            Nothing => Nothing
                                            | Just LT => Just LT
                                            | Just GT => Just GT
                                            | Just EQ =>
                                                case partialCompare f y of
                                                    Nothing => Nothing
                                                    | Just LT => Just LT
                                                    | Just GT => Just GT
                                                    | Just EQ => partialCompare g z

instance (
        PartialOrd a,
        PartialOrd b,
        PartialOrd c,
        PartialOrd d,
        PartialOrd e,
        PartialOrd f,
        PartialOrd g,
        PartialOrd h
    ) =>
    PartialOrd (a, b, c, d, e, f, g, h) where
    partialCompare (a, b, c, d, e, f, g, h) (s, t, u, v, w, x, y, z) =
        case partialCompare a s of
            Nothing => Nothing
            | Just LT => Just LT
            | Just GT => Just GT
            | Just EQ =>
                case partialCompare b t of
                    Nothing => Nothing
                    | Just LT => Just LT
                    | Just GT => Just GT
                    | Just EQ =>
                        case partialCompare c u of
                            Nothing => Nothing
                            | Just LT => Just LT
                            | Just GT => Just GT
                            | Just EQ =>
                                case partialCompare d v of
                                    Nothing => Nothing
                                    | Just LT => Just LT
                                    | Just GT => Just GT
                                    | Just EQ =>
                                        case partialCompare e w of
                                            Nothing => Nothing
                                            | Just LT => Just LT
                                            | Just GT => Just GT
                                            | Just EQ =>
                                                case partialCompare f x of
                                                    Nothing => Nothing
                                                    | Just LT => Just LT
                                                    | Just GT => Just GT
                                                    | Just EQ =>
                                                        case partialCompare g y of
                                                            Nothing => Nothing
                                                            | Just LT => Just LT
                                                            | Just GT => Just GT
                                                            | Just EQ => partialCompare h z

instance (PartialOrd a, PartialOrd b) => PartialOrd (Either a b) where
    partialCompare (Left x) (Left y) = partialCompare x y
    partialCompare (Left x) (Right y) = Just LT
    partialCompare (Right x) (Left y) = Just GT
    partialCompare (Right x) (Right y) = partialCompare x y

instance Ord Int where
    compare x y = intCompare x y

instance Ord BigInt where
    compare x y = bigIntCompare x y

instance Ord Char where
    compare x y = charCompare x y

instance Ord String where
    compare x y = stringCompare x y

instance Ord () where
    compare _ _ = EQ

instance Ord Bool where
    compare False False = EQ
    compare False True = LT
    compare True False = GT
    compare True True = EQ

instance Ord Ordering where
    compare LT LT = EQ
    compare LT _ = LT
    compare EQ LT = GT
    compare EQ EQ = EQ
    compare EQ GT = LT
    compare GT GT = EQ
    compare GT _ = GT

instance Ord a => Ord (Maybe a) where
    compare Nothing Nothing = EQ
    compare Nothing (Just y) = LT
    compare (Just x) Nothing = GT
    compare (Just x) (Just y) = compare x y

instance Ord a => Ord (List a) where
    compare [] [] = EQ
    compare [] (y :: ys) = LT
    compare (x :: xs) [] = GT
    compare (x :: xs) (y :: ys) =
        case compare x y of
            LT => LT
            | GT => GT
            | EQ => compare xs ys

instance (Ord a, Ord b) => Ord (a, b) where
    compare (leftA, leftB) (rightA, rightB) =
        case compare leftA rightA of
            LT => LT
            | GT => GT
            | EQ => compare leftB rightB

instance (Ord a, Ord b, Ord c) => Ord (a, b, c) where
    compare (a, b, c) (x, y, z) =
        case compare a x of
            LT => LT
            | GT => GT
            | EQ =>
                case compare b y of
                    LT => LT
                    | GT => GT
                    | EQ => compare c z

instance (Ord a, Ord b, Ord c, Ord d) => Ord (a, b, c, d) where
    compare (a, b, c, d) (w, x, y, z) =
        case compare a w of
            LT => LT
            | GT => GT
            | EQ =>
                case compare b x of
                    LT => LT
                    | GT => GT
                    | EQ =>
                        case compare c y of
                            LT => LT
                            | GT => GT
                            | EQ => compare d z

instance (Ord a, Ord b, Ord c, Ord d, Ord e) => Ord (a, b, c, d, e) where
    compare (a, b, c, d, e) (v, w, x, y, z) =
        case compare a v of
            LT => LT
            | GT => GT
            | EQ =>
                case compare b w of
                    LT => LT
                    | GT => GT
                    | EQ =>
                        case compare c x of
                            LT => LT
                            | GT => GT
                            | EQ =>
                                case compare d y of
                                    LT => LT
                                    | GT => GT
                                    | EQ => compare e z

instance (Ord a, Ord b, Ord c, Ord d, Ord e, Ord f) => Ord (a, b, c, d, e, f) where
    compare (a, b, c, d, e, f) (u, v, w, x, y, z) =
        case compare a u of
            LT => LT
            | GT => GT
            | EQ =>
                case compare b v of
                    LT => LT
                    | GT => GT
                    | EQ =>
                        case compare c w of
                            LT => LT
                            | GT => GT
                            | EQ =>
                                case compare d x of
                                    LT => LT
                                    | GT => GT
                                    | EQ =>
                                        case compare e y of
                                            LT => LT
                                            | GT => GT
                                            | EQ => compare f z

instance (Ord a, Ord b, Ord c, Ord d, Ord e, Ord f, Ord g) => Ord (a, b, c, d, e, f, g) where
    compare (a, b, c, d, e, f, g) (t, u, v, w, x, y, z) =
        case compare a t of
            LT => LT
            | GT => GT
            | EQ =>
                case compare b u of
                    LT => LT
                    | GT => GT
                    | EQ =>
                        case compare c v of
                            LT => LT
                            | GT => GT
                            | EQ =>
                                case compare d w of
                                    LT => LT
                                    | GT => GT
                                    | EQ =>
                                        case compare e x of
                                            LT => LT
                                            | GT => GT
                                            | EQ =>
                                                case compare f y of
                                                    LT => LT
                                                    | GT => GT
                                                    | EQ => compare g z

instance (Ord a, Ord b, Ord c, Ord d, Ord e, Ord f, Ord g, Ord h) =>
    Ord (a, b, c, d, e, f, g, h) where
    compare (a, b, c, d, e, f, g, h) (s, t, u, v, w, x, y, z) =
        case compare a s of
            LT => LT
            | GT => GT
            | EQ =>
                case compare b t of
                    LT => LT
                    | GT => GT
                    | EQ =>
                        case compare c u of
                            LT => LT
                            | GT => GT
                            | EQ =>
                                case compare d v of
                                    LT => LT
                                    | GT => GT
                                    | EQ =>
                                        case compare e w of
                                            LT => LT
                                            | GT => GT
                                            | EQ =>
                                                case compare f x of
                                                    LT => LT
                                                    | GT => GT
                                                    | EQ =>
                                                        case compare g y of
                                                            LT => LT
                                                            | GT => GT
                                                            | EQ => compare h z

instance (Ord a, Ord b) => Ord (Either a b) where
    compare (Left x) (Left y) = compare x y
    compare (Left x) (Right y) = LT
    compare (Right x) (Left y) = GT
    compare (Right x) (Right y) = compare x y

instance Num Int where
    (+) x y = intAdd x y
    (*) x y = intMul x y
    negate x = intNeg x
    abs x = intAbs x
    signum x = intSignum x
    powNat x exponent = intPow x exponent

instance Integral Int where
    div x y = intDiv x y
    mod x y = intMod x y
    quot x y = intQuot x y
    rem x y = intRem x y

instance Num BigInt where
    (+) x y = bigIntAdd x y
    (*) x y = bigIntMul x y
    negate x = bigIntNeg x
    abs x = bigIntAbs x
    signum x = bigIntSignum x
    powNat x exponent = bigIntPow x exponent

instance Integral BigInt where
    div x y = bigIntDiv x y
    mod x y = bigIntMod x y
    quot x y = bigIntQuot x y
    rem x y = bigIntRem x y

instance Num Float where
    (+) x y = floatAdd x y
    (*) x y = floatMul x y
    negate x = floatNeg x
    abs x = floatAbs x
    signum x = floatSignum x
    powNat x exponent =
        if intLt exponent 0
            then prelude.builtins.error "negative exponent used with (^)"
            else floatPow x (intToFloat exponent)

instance Fractional Float where
    (/) x y = floatDiv x y

instance Functor Maybe where
    fmap f Nothing = Nothing
    fmap f (Just x) = Just (f x)

instance Functor List where
    fmap f xs = map f xs

instance Functor ((,) e) where
    fmap f (e, x) = (e, f x)

instance Functor ((,,) e1 e2) where
    fmap f (e1, e2, x) = (e1, e2, f x)

instance Functor ((,,,) e1 e2 e3) where
    fmap f (e1, e2, e3, x) = (e1, e2, e3, f x)

instance Functor ((,,,,) e1 e2 e3 e4) where
    fmap f (e1, e2, e3, e4, x) = (e1, e2, e3, e4, f x)

instance Functor ((,,,,,) e1 e2 e3 e4 e5) where
    fmap f (e1, e2, e3, e4, e5, x) = (e1, e2, e3, e4, e5, f x)

instance Functor ((,,,,,,) e1 e2 e3 e4 e5 e6) where
    fmap f (e1, e2, e3, e4, e5, e6, x) = (e1, e2, e3, e4, e5, e6, f x)

instance Functor ((,,,,,,,) e1 e2 e3 e4 e5 e6 e7) where
    fmap f (e1, e2, e3, e4, e5, e6, e7, x) = (e1, e2, e3, e4, e5, e6, e7, f x)

instance Functor (Either e) where
    fmap f (Left error) = Left error
    fmap f (Right x) = Right (f x)

instance Functor (IO e) where
    fmap f action = prelude.builtins.bindIO action (fun value => prelude.builtins.pureIO (f value))

instance Applicative Maybe where
    pure x = Just x
    apply Nothing mx = Nothing
    apply (Just f) mx = fmap f mx

instance Applicative List where
    pure x = [x]
    apply fs xs = concat (map (fun f => map f xs) fs)

instance Applicative (Either e) where
    pure x = Right x
    apply (Left error) mx = Left error
    apply (Right f) mx = fmap f mx

instance Applicative (IO e) where
    pure value = prelude.builtins.pureIO value
    apply actionF actionX =
        prelude.builtins.bindIO
            actionF
            (fun f => prelude.builtins.bindIO actionX (fun x => prelude.builtins.pureIO (f x)))

instance Monad Maybe where
    bind Nothing f = Nothing
    bind (Just x) f = f x

instance Monad List where
    bind xs f = concat (map f xs)

instance Monad (Either e) where
    bind (Left error) f = Left error
    bind (Right x) f = f x

instance Monad (IO e) where
    bind action f = prelude.builtins.bindIO action f

instance Show Int where
    show value = intToString value

instance Show BigInt where
    show value = bigIntToString value

instance Hashable BigInt where
    hashWithSalt salt value = hashWithSalt salt (bigIntToString value)

instance Show Float where
    show value = floatToString value

instance Show Char where
    show value = stringAppend "'" (stringAppend (showCharPayload value) "'")

instance Show String where
    show value = stringAppend "\"" (stringAppend (showStringChars (stringToChars value)) "\"")

instance Show () where
    show value = "()"

instance Show Bool where
    show True = "True"
    show False = "False"

instance Show Ordering where
    show LT = "LT"
    show EQ = "EQ"
    show GT = "GT"

instance Show IOErrorKind where
    show AlreadyExists = "already exists"
    show DoesNotExist = "does not exist"
    show PermissionDenied = "permission denied"
    show ResourceBusy = "resource busy"
    show ResourceExhausted = "resource exhausted"
    show EndOfFile = "end of file"
    show InvalidArgument = "invalid argument"
    show UnsupportedOperation = "unsupported operation"
    show TimeExpired = "time expired"
    show ResourceVanished = "resource vanished"
    show Interrupted = "interrupted"
    show NetworkUnreachable = "network unreachable"
    show ConnectionRefused = "connection refused"
    show AddressInUse = "address in use"
    show ProtocolError = "protocol error"
    show OtherError = "other I/O error"

instance Show IOError where
    show (IOError kind operation message) = operation <> ": " <> show kind <> ": " <> message

instance Show Utf8Error where
    show (InvalidUtf8 offset) = "invalid UTF-8 at byte " <> show offset

instance Show a => Show (Maybe a) where
    show Nothing = "Nothing"
    show (Just x) = stringAppend "Just " (show x)

instance Show a => Show (List a) where
    show value = stringAppend "[" (stringAppend (showListItems value) "]")

instance (Show a, Show b) => Show (a, b) where
    show (a, b) =
        stringAppend "(" (stringAppend (show a) (stringAppend ", " (stringAppend (show b) ")")))

instance (Show a, Show b, Show c) => Show (a, b, c) where
    show (a, b, c) = "(" <> show a <> ", " <> show b <> ", " <> show c <> ")"

instance (Show a, Show b, Show c, Show d) => Show (a, b, c, d) where
    show (a, b, c, d) = "(" <> show a <> ", " <> show b <> ", " <> show c <> ", " <> show d <> ")"

instance (Show a, Show b, Show c, Show d, Show e) => Show (a, b, c, d, e) where
    show (a, b, c, d, e) =
        "(" <> show a <> ", " <> show b <> ", " <> show c <> ", " <> show d <> ", " <> show e <> ")"

instance (Show a, Show b, Show c, Show d, Show e, Show f) => Show (a, b, c, d, e, f) where
    show (a, b, c, d, e, f) =
        "("
            <> show a
            <> ", "
            <> show b
            <> ", "
            <> show c
            <> ", "
            <> show d
            <> ", "
            <> show e
            <> ", "
            <> show f
            <> ")"

instance (Show a, Show b, Show c, Show d, Show e, Show f, Show g) =>
    Show (a, b, c, d, e, f, g) where
    show (a, b, c, d, e, f, g) =
        "("
            <> show a
            <> ", "
            <> show b
            <> ", "
            <> show c
            <> ", "
            <> show d
            <> ", "
            <> show e
            <> ", "
            <> show f
            <> ", "
            <> show g
            <> ")"

instance (Show a, Show b, Show c, Show d, Show e, Show f, Show g, Show h) =>
    Show (a, b, c, d, e, f, g, h) where
    show (a, b, c, d, e, f, g, h) =
        "("
            <> show a
            <> ", "
            <> show b
            <> ", "
            <> show c
            <> ", "
            <> show d
            <> ", "
            <> show e
            <> ", "
            <> show f
            <> ", "
            <> show g
            <> ", "
            <> show h
            <> ")"

instance (Show a, Show b) => Show (Either a b) where
    show (Left x) = stringAppend "Left " (show x)
    show (Right x) = stringAppend "Right " (show x)

instance Semigroup String where
    combine x y = stringAppend x y

instance Monoid String where
    empty = ""

instance Semigroup (List a) where
    combine xs ys = append xs ys

instance Monoid (List a) where
    empty = []

instance (Semigroup a, Semigroup b) => Semigroup (a, b) where
    combine (a, b) (x, y) = (combine a x, combine b y)

instance (Monoid a, Monoid b) => Monoid (a, b) where
    empty = (empty, empty)

instance (Semigroup a, Semigroup b, Semigroup c) => Semigroup (a, b, c) where
    combine (a, b, c) (x, y, z) = (combine a x, combine b y, combine c z)

instance (Monoid a, Monoid b, Monoid c) => Monoid (a, b, c) where
    empty = (empty, empty, empty)

instance (Semigroup a, Semigroup b, Semigroup c, Semigroup d) => Semigroup (a, b, c, d) where
    combine (a, b, c, d) (w, x, y, z) = (combine a w, combine b x, combine c y, combine d z)

instance (Monoid a, Monoid b, Monoid c, Monoid d) => Monoid (a, b, c, d) where
    empty = (empty, empty, empty, empty)

instance (Semigroup a, Semigroup b, Semigroup c, Semigroup d, Semigroup e) =>
    Semigroup (a, b, c, d, e) where
    combine (a, b, c, d, e) (v, w, x, y, z) =
        (combine a v, combine b w, combine c x, combine d y, combine e z)

instance (Monoid a, Monoid b, Monoid c, Monoid d, Monoid e) => Monoid (a, b, c, d, e) where
    empty = (empty, empty, empty, empty, empty)

instance (Semigroup a, Semigroup b, Semigroup c, Semigroup d, Semigroup e, Semigroup f) =>
    Semigroup (a, b, c, d, e, f) where
    combine (a, b, c, d, e, f) (u, v, w, x, y, z) =
        (combine a u, combine b v, combine c w, combine d x, combine e y, combine f z)

instance (Monoid a, Monoid b, Monoid c, Monoid d, Monoid e, Monoid f) =>
    Monoid (a, b, c, d, e, f) where
    empty = (empty, empty, empty, empty, empty, empty)

instance (
        Semigroup a,
        Semigroup b,
        Semigroup c,
        Semigroup d,
        Semigroup e,
        Semigroup f,
        Semigroup g
    ) =>
    Semigroup (a, b, c, d, e, f, g) where
    combine (a, b, c, d, e, f, g) (t, u, v, w, x, y, z) =
        (combine a t, combine b u, combine c v, combine d w, combine e x, combine f y, combine g z)

instance (Monoid a, Monoid b, Monoid c, Monoid d, Monoid e, Monoid f, Monoid g) =>
    Monoid (a, b, c, d, e, f, g) where
    empty = (empty, empty, empty, empty, empty, empty, empty)

instance (
        Semigroup a,
        Semigroup b,
        Semigroup c,
        Semigroup d,
        Semigroup e,
        Semigroup f,
        Semigroup g,
        Semigroup h
    ) =>
    Semigroup (a, b, c, d, e, f, g, h) where
    combine (a, b, c, d, e, f, g, h) (s, t, u, v, w, x, y, z) =
        (
            combine a s,
            combine b t,
            combine c u,
            combine d v,
            combine e w,
            combine f x,
            combine g y,
            combine h z
        )

instance (Monoid a, Monoid b, Monoid c, Monoid d, Monoid e, Monoid f, Monoid g, Monoid h) =>
    Monoid (a, b, c, d, e, f, g, h) where
    empty = (empty, empty, empty, empty, empty, empty, empty, empty)
