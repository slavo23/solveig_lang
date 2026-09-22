import std.map
import std.bigint(parse)
import std.math(hypot)

data Shape = Square Int | Rectangle Int Int

area : Shape -> Int
area (Square side) = side * side
area (Rectangle width height) = width * height

data Priority = Low | High

class Render a where
    render : a -> String

instance Render Priority where
    render Low = "low"
    render High = "high"

data User = User String String

emails : [User] -> [String]
emails = map (fun (User _ address) => address)

type Named a = (String, a)

rename : String -> Named a -> Named a
rename next named = let (_, value) = named in (next, value)

useIdentity : (forall a. a -> a) -> (Int, String)
useIdentity identity = (identity 7, identity "seven")

polymorphicIdentity : forall a. a -> a
polymorphicIdentity value = value

prependOne : [Int] -> [Int]
prependOne = (::) 1

incrementRoute : String -> std.map.Map String Int -> std.map.Map String Int
incrementRoute route counts =
    let current =
        case std.map.lookup route counts of
            Nothing => 0
            | Just count => count
    in std.map.insert route (current + 1) counts

routeCounts : [String] -> std.map.Map String Int
routeCounts = foldl (flip incrementRoute) std.map.empty

main : Task ()
main = do
    printLine (render High <> ": " <> show (area (Rectangle 3 4)))
    printLine (show (emails [User "Ada" "ada@example.test"]))
    printLine (show (rename "answer" ("old", 42)))
    printLine (show (useIdentity polymorphicIdentity))
    printLine (show (prependOne [2, 3]))
    printLine $ show $ std.map.toAscList $ routeCounts ["/users", "/health", "/users"]
    printLine $ "3-4-5 diagonal: " <> show (hypot 3.0 4.0)
    printLine $ "large integer: " <> show (parse "123456789012345678901234567890")
