export (SecureRandomError(..), mkStdGen, newStdGen, randomInt, randomRange, secureRandomBytes)

import std.prim(*)

-- | A caller or host failure while obtaining cryptographic random bytes.
data SecureRandomError = InvalidSecureRandomByteCount Int | SecureRandomHostError HostError

instance Eq SecureRandomError where
    (==) (InvalidSecureRandomByteCount left) (InvalidSecureRandomByteCount right) = left == right
    (==) (SecureRandomHostError left) (SecureRandomHostError right) = left == right
    (==) _ _ = False

instance Show SecureRandomError where
    show (InvalidSecureRandomByteCount count) =
        "secure randomness byte count must be non-negative, got " <> show count
    show (SecureRandomHostError hostError) = "secure randomness: " <> show hostError

-- | Creates a deterministic immutable random generator from a chosen seed.
mkStdGen : Int -> StdGen
mkStdGen seed = std.prim.mkStdGen seed

-- | Creates a fresh runtime-local random generator. This is not cryptographic.
newStdGen : Task StdGen
newStdGen = std.prim.newStdGen

-- | Produces a signed random integer and the next immutable generator state.
randomInt : StdGen -> (Int, StdGen)
randomInt generator = std.prim.randomInt generator

-- | Produces an unbiased integer in inclusive bounds when they are ordered.
-- This is not cryptographic.
randomRange : Int -> Int -> StdGen -> Maybe (Int, StdGen)
randomRange lower upper generator =
    if lower > upper then Nothing else Just (std.prim.randomRange lower upper generator)

-- | Obtains exactly the requested number of bytes from the host's
-- | cryptographically secure random source.
-- |
-- | This operation is deliberately separate from StdGen: its output is not
-- | deterministic or reproducible, and it returns a typed host failure.
secureRandomBytes : Int -> IO SecureRandomError Bytes
secureRandomBytes count =
    if count < 0
        then failIO (InvalidSecureRandomByteCount count)
        else mapError SecureRandomHostError (std.prim.secureRandomBytes count)
