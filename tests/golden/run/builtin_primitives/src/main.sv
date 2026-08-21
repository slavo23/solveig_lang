import prelude.prim

intResults =
    [ intAdd 40 2
    , intSub 10 3
    , intMul 6 7
    , intDiv 7 2
    , intMod 7 2
    , intNeg 9
    , floatToInt 3.75
    ]

intChecks =
    [ intEq 4 4
    , intLt 1 2
    , intLe 2 2
    , intGt 3 2
    , intGe 3 3
    ]

floatResults =
    [ floatAdd 1.5 2.25
    , floatSub 4.5 1.25
    , floatMul 2.0 3.5
    , floatDiv 7.0 2.0
    , floatNeg 1.5
    , intToFloat 5
    ]

floatChecks =
    [ floatEq 1.0 1.0
    , floatLt 1.0 2.0
    , floatLe 2.0 2.0
    , floatGt 3.0 2.0
    , floatGe 3.0 3.0
    ]

stringResults =
    [ intToString 42
    , floatToString 2.5
    , charToString 'x'
    , stringAppend "sol" "veig"
    , stringSlice "abcdef" 2 3
    , charsToString ['o', 'k']
    ]

stringDetails =
    (((stringLength "hello"), (stringToChars "Az")), ((stringCharAt "abc" 1), ((stringCharAt "abc" 9), ((stringEq "a" "a"), ((intCompare 1 2), ((stringCompare "a" "b"), ((charEq 'x' 'x'), (charCompare 'b' 'a'))))))))

main =
    ((intResults, intChecks), ((floatResults, floatChecks), (stringResults, stringDetails)))
