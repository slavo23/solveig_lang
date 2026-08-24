export (mkStdGen, newStdGen, randomInt, randomRange)

import prelude.prim

-- | Creates a deterministic immutable random generator from a chosen seed.
mkStdGen : Int -> StdGen
mkStdGen seed = prelude.prim.mkStdGen seed

-- | Creates a fresh runtime-local random generator. This is not cryptographic.
newStdGen : Task StdGen
newStdGen = prelude.prim.newStdGen

-- | Produces a signed random integer and the next immutable generator state.
randomInt : StdGen -> (Int, StdGen)
randomInt generator = prelude.prim.randomInt generator

-- | Produces an unbiased integer in inclusive bounds when they are ordered.
-- This is not cryptographic.
randomRange : Int -> Int -> StdGen -> Maybe (Int, StdGen)
randomRange lower upper generator =
    if lower > upper then Nothing
    else Just (prelude.prim.randomRange lower upper generator)
