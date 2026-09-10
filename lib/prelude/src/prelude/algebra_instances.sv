export ()

import prelude.builtins(*)
import prelude.list(*)
import prelude.prim(*)
import prelude.typeclasses(*)

instance Foldable Maybe where
    foldr combine initial Nothing = initial
    foldr combine initial (Just value) = combine value initial
    foldl combine initial Nothing = initial
    foldl combine initial (Just value) = combine initial value

instance Foldable List where
    foldr combine initial values = foldrList combine initial values
    foldl combine initial values = foldlList combine initial values

instance Foldable ((,) e) where
    foldr combine initial (e, value) = combine value initial
    foldl combine initial (e, value) = combine initial value

instance Foldable ((,,) e1 e2) where
    foldr combine initial (e1, e2, value) = combine value initial
    foldl combine initial (e1, e2, value) = combine initial value

instance Foldable ((,,,) e1 e2 e3) where
    foldr combine initial (e1, e2, e3, value) = combine value initial
    foldl combine initial (e1, e2, e3, value) = combine initial value

instance Foldable ((,,,,) e1 e2 e3 e4) where
    foldr combine initial (e1, e2, e3, e4, value) = combine value initial
    foldl combine initial (e1, e2, e3, e4, value) = combine initial value

instance Foldable ((,,,,,) e1 e2 e3 e4 e5) where
    foldr combine initial (e1, e2, e3, e4, e5, value) = combine value initial
    foldl combine initial (e1, e2, e3, e4, e5, value) = combine initial value

instance Foldable ((,,,,,,) e1 e2 e3 e4 e5 e6) where
    foldr combine initial (e1, e2, e3, e4, e5, e6, value) = combine value initial
    foldl combine initial (e1, e2, e3, e4, e5, e6, value) = combine initial value

instance Foldable ((,,,,,,,) e1 e2 e3 e4 e5 e6 e7) where
    foldr combine initial (e1, e2, e3, e4, e5, e6, e7, value) = combine value initial
    foldl combine initial (e1, e2, e3, e4, e5, e6, e7, value) = combine initial value

instance Foldable (Either e) where
    foldr combine initial (Left error) = initial
    foldr combine initial (Right value) = combine value initial
    foldl combine initial (Left error) = initial
    foldl combine initial (Right value) = combine initial value

instance Traversable Maybe where
    traverse transform Nothing = pure Nothing
    traverse transform (Just value) = fmap Just (transform value)

instance Traversable List where
    traverse transform values =
        fmap
            reverseList
            (foldlList
                (fun effectReversed value =>
                    liftA2
                        (fun reversed transformed => transformed :: reversed)
                        effectReversed
                        (transform value)
                )
                (pure [])
                values)

instance Traversable ((,) e) where
    traverse transform (e, value) = fmap (fun transformed => (e, transformed)) (transform value)

instance Traversable ((,,) e1 e2) where
    traverse transform (e1, e2, value) =
        fmap (fun transformed => (e1, e2, transformed)) (transform value)

instance Traversable ((,,,) e1 e2 e3) where
    traverse transform (e1, e2, e3, value) =
        fmap (fun transformed => (e1, e2, e3, transformed)) (transform value)

instance Traversable ((,,,,) e1 e2 e3 e4) where
    traverse transform (e1, e2, e3, e4, value) =
        fmap (fun transformed => (e1, e2, e3, e4, transformed)) (transform value)

instance Traversable ((,,,,,) e1 e2 e3 e4 e5) where
    traverse transform (e1, e2, e3, e4, e5, value) =
        fmap (fun transformed => (e1, e2, e3, e4, e5, transformed)) (transform value)

instance Traversable ((,,,,,,) e1 e2 e3 e4 e5 e6) where
    traverse transform (e1, e2, e3, e4, e5, e6, value) =
        fmap (fun transformed => (e1, e2, e3, e4, e5, e6, transformed)) (transform value)

instance Traversable ((,,,,,,,) e1 e2 e3 e4 e5 e6 e7) where
    traverse transform (e1, e2, e3, e4, e5, e6, e7, value) =
        fmap (fun transformed => (e1, e2, e3, e4, e5, e6, e7, transformed)) (transform value)

instance Traversable (Either e) where
    traverse transform (Left error) = pure (Left error)
    traverse transform (Right value) = fmap Right (transform value)

instance Hashable Int where
    hashWithSalt salt value = mixHash salt value

instance Hashable Char where
    hashWithSalt salt value = mixHash salt (charToCodePoint value)

instance Hashable String where
    hashWithSalt salt value =
        mixHash (foldlList hashWithSalt (mixHash salt 1) (stringToChars value)) 2

instance Hashable () where
    hashWithSalt salt _ = mixHash salt 3

instance Hashable Bool where
    hashWithSalt salt False = mixHash salt 4
    hashWithSalt salt True = mixHash salt 5

instance Hashable Ordering where
    hashWithSalt salt LT = mixHash salt 6
    hashWithSalt salt EQ = mixHash salt 7
    hashWithSalt salt GT = mixHash salt 8

instance Hashable a => Hashable (Maybe a) where
    hashWithSalt salt Nothing = mixHash salt 9
    hashWithSalt salt (Just value) = hashWithSalt (mixHash salt 10) value

instance Hashable a => Hashable (List a) where
    hashWithSalt salt values = mixHash (foldlList hashWithSalt (mixHash salt 11) values) 12

instance (Hashable a, Hashable b) => Hashable (a, b) where
    hashWithSalt salt (a, b) = hashWithSalt (hashWithSalt (mixHash salt 13) a) b

instance (Hashable a, Hashable b, Hashable c) => Hashable (a, b, c) where
    hashWithSalt salt (a, b, c) = hashWithSalt (hashWithSalt (hashWithSalt (mixHash salt 14) a) b) c

instance (Hashable a, Hashable b, Hashable c, Hashable d) => Hashable (a, b, c, d) where
    hashWithSalt salt (a, b, c, d) =
        hashWithSalt (hashWithSalt (hashWithSalt (hashWithSalt (mixHash salt 15) a) b) c) d

instance (Hashable a, Hashable b, Hashable c, Hashable d, Hashable e) =>
    Hashable (a, b, c, d, e) where
    hashWithSalt salt (a, b, c, d, e) =
        hashWithSalt
            (hashWithSalt (hashWithSalt (hashWithSalt (hashWithSalt (mixHash salt 16) a) b) c) d)
            e

instance (Hashable a, Hashable b, Hashable c, Hashable d, Hashable e, Hashable f) =>
    Hashable (a, b, c, d, e, f) where
    hashWithSalt salt (a, b, c, d, e, f) =
        hashWithSalt
            (hashWithSalt
                (hashWithSalt
                    (hashWithSalt (hashWithSalt (hashWithSalt (mixHash salt 17) a) b) c)
                    d)
                e)
            f

instance (Hashable a, Hashable b, Hashable c, Hashable d, Hashable e, Hashable f, Hashable g) =>
    Hashable (a, b, c, d, e, f, g) where
    hashWithSalt salt (a, b, c, d, e, f, g) =
        hashWithSalt
            (hashWithSalt
                (hashWithSalt
                    (hashWithSalt
                        (hashWithSalt (hashWithSalt (hashWithSalt (mixHash salt 18) a) b) c)
                        d)
                    e)
                f)
            g

instance (
        Hashable a,
        Hashable b,
        Hashable c,
        Hashable d,
        Hashable e,
        Hashable f,
        Hashable g,
        Hashable h
    ) =>
    Hashable (a, b, c, d, e, f, g, h) where
    hashWithSalt salt (a, b, c, d, e, f, g, h) =
        hashWithSalt
            (hashWithSalt
                (hashWithSalt
                    (hashWithSalt
                        (hashWithSalt
                            (hashWithSalt (hashWithSalt (hashWithSalt (mixHash salt 19) a) b) c)
                            d)
                        e)
                    f)
                g)
            h

instance (Hashable a, Hashable b) => Hashable (Either a b) where
    hashWithSalt salt (Left value) = hashWithSalt (mixHash salt 20) value
    hashWithSalt salt (Right value) = hashWithSalt (mixHash salt 21) value
