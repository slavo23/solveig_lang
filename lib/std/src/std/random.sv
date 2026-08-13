import prelude.prim

-- | Creates a deterministic immutable random generator from a chosen seed.
mkStdGen : Int -> StdGen
mkStdGen seed = prelude.prim.mkStdGen seed

-- | Creates a fresh runtime-local random generator. This is not cryptographic.
newStdGen : Task StdGen
newStdGen = prelude.prim.newStdGen

-- | Produces a signed random integer and the next immutable generator state.
randomInt : StdGen -> Pair Int StdGen
randomInt generator = prelude.prim.randomInt generator

-- | Produces an unbiased integer in the inclusive bounds and the next generator.
-- Reversed bounds raise an Error defect. This is not cryptographic.
randomRange : Int -> Int -> StdGen -> Pair Int StdGen
randomRange lower upper generator = prelude.prim.randomRange lower upper generator
