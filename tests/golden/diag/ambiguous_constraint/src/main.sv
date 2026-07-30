class FromText a where
    fromText : String -> a

renderText text = show (fromText text)

main = 0
