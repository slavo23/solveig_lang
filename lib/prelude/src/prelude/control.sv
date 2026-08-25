export (join, composeK, mapM, sequence, filterM, seqIO, mapIO_)

import prelude.builtins(*)
import prelude.list(*)
import prelude.typeclasses(*)

-- | Flattens one layer of monadic nesting.
join : forall m a. Monad m => m (m a) -> m a
join mmx = do
    mx <- mmx
    mx

-- | Composes two functions that return monadic results.
composeK : forall m a b c. Monad m => (a -> m b) -> (b -> m c) -> a -> m c
composeK f g x = do
    y <- f x
    g y

-- | Maps a function that returns monadic results over a list.
mapM : forall m a b. Monad m => (a -> m b) -> [a] -> m [b]
mapM f [] = pure []
mapM f (y :: ys) = do
    z <- f y
    zs <- mapM f ys
    pure (z :: zs)

-- | Turns a list of monadic computations into one computation producing a list.
sequence : forall m a. Monad m => [m a] -> m [a]
sequence xs = mapM (fun x => x) xs

-- | Filters a list with a predicate that returns monadic results.
filterM : forall m a. Monad m => (a -> m Bool) -> [a] -> m [a]
filterM p [] = pure []
filterM p (y :: ys) = do
    keep <- p y
    zs <- filterM p ys
    if keep then pure (y :: zs) else pure zs

-- | Runs two IO actions and keeps the result of the second.
seqIO : forall e a b. IO e a -> IO e b -> IO e b
seqIO left right = prelude.builtins.bindIO left (fun value => right)

-- | Runs an IO action for every element and discards the results.
mapIO_ : forall e a. (a -> IO e ()) -> [a] -> IO e ()
mapIO_ f [] = prelude.builtins.pureIO ()
mapIO_ f (y :: ys) =
    prelude.builtins.bindIO (f y) (fun value => mapIO_ f ys)
