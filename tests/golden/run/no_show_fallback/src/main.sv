data Nat = Z | S Nat
data Box = Box Nat [Int]

main : Box
main = Box (S (S Z)) [1, 2]
