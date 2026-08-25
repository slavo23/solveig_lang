import std.bytes(*)
import std.parser(*)
import std.text(*)

data ConfigError = ReservedName String

data ConfigValue =
    TextValue String
  | ListValue [ConfigValue]

data Setting = Setting String ConfigValue

data Section = Section String [Setting]

isWhitespace : Char -> Bool
isWhitespace character =
    if character == ' ' then True
    else if character == '\n' then True
    else if character == '\t' then True
    else False

isNameCharacter : Char -> Bool
isNameCharacter character =
    if character >= 'a' then character <= 'z'
    else if character >= 'A' then character <= 'Z'
    else if character >= '0' then character <= '9'
    else character == '_'

spaces : forall input. std.parser.InputOps input Char -> std.parser.Parser input ConfigError ()
spaces ops =
    std.parser.skipMany ops
        (std.parser.satisfy ops "whitespace" isWhitespace)

lexeme : forall input a. std.parser.InputOps input Char -> std.parser.Parser input ConfigError a -> std.parser.Parser input ConfigError a
lexeme ops = (<* spaces ops)

symbol : forall input. std.parser.InputOps input Char -> String -> std.parser.Parser input ConfigError [Char]
symbol ops name =
    lexeme ops
        (std.parser.tokens ops name (std.text.stringToChars name))

identifierLabel : forall input custom value.
    std.parser.Parser input custom value ->
    std.parser.Parser input custom value
identifierLabel = (<?> "identifier")

identifier : forall input. std.parser.InputOps input Char -> std.parser.Parser input ConfigError String
identifier ops =
    identifierLabel
        (lexeme ops
            (do
                characters <-
                    std.parser.some ops
                        (std.parser.satisfy ops "name character" isNameCharacter)
                let name = std.text.charsToString characters
                if name == "reserved" then
                    std.parser.customFailure ops (ReservedName name)
                else
                    pure name))

asTextValue : forall f. Functor f => f String -> f ConfigValue
asTextValue = (TextValue <$>)

parserBind : forall input custom a b.
    std.parser.Parser input custom a ->
    (a -> std.parser.Parser input custom b) ->
    std.parser.Parser input custom b
parserBind = (>>=)

configValue : forall input. std.parser.InputOps input Char -> std.parser.Parser input ConfigError ConfigValue
configValue ops =
    std.parser.context "value"
        (std.parser.choice ops
            (std.parser.attempt ops
                ((fun _ => TextValue "true") <$> symbol ops "true"))
            (std.parser.choice ops
                (asTextValue (identifier ops))
                (configList ops)))

configList : forall input. std.parser.InputOps input Char -> std.parser.Parser input ConfigError ConfigValue
configList ops =
    ListValue <$>
        std.parser.between
            (symbol ops "[")
            (symbol ops "]")
            (std.parser.separatedBy ops
                (std.parser.defer (fun _ => configValue ops))
                (symbol ops ","))

setting : forall input. std.parser.InputOps input Char -> std.parser.Parser input ConfigError Setting
setting ops =
    Setting <$> identifier ops <*>
        (symbol ops "=" *> configValue ops <* symbol ops ";")

section : forall input. std.parser.InputOps input Char -> std.parser.Parser input ConfigError Section
section ops =
    symbol ops "section" *>
        std.parser.cut
            (Section <$> identifier ops <*>
                std.parser.between
                    (symbol ops "{")
                    (symbol ops "}")
                    (std.parser.many ops (setting ops)))

document : forall input. std.parser.InputOps input Char -> std.parser.Parser input ConfigError Section
document ops = spaces ops *> section ops

showConfigError : ConfigError -> String
showConfigError (ReservedName name) = "reserved name " <> show name

showValue : ConfigValue -> String
showValue (TextValue text) = text
showValue (ListValue values) = "[" <> showValues values <> "]"

showValues : [ConfigValue] -> String
showValues [] = ""
showValues (value :: []) = showValue value
showValues (value :: rest) = showValue value <> "," <> showValues rest

showSetting : Setting -> String
showSetting (Setting name value) = name <> "=" <> showValue value

showSettings : [Setting] -> String
showSettings [] = ""
showSettings (value :: []) = showSetting value
showSettings (value :: rest) = showSetting value <> ";" <> showSettings rest

showSection : Section -> String
showSection (Section name settings) =
    "section " <> name <> " {" <> showSettings settings <> "}"

printSectionReply : forall input. std.parser.ParseReply input ConfigError Section -> Task ()
printSectionReply reply =
    std.parser.foldReply
        (fun _ value _ => printLine (showSection value))
        (fun _ _ error => printLine (std.parser.renderError showConfigError error))
        (fun _ _ _ => printLine "awaiting input")
        reply

printIntListReply : forall input. std.parser.ParseReply input () [Int] -> Task ()
printIntListReply reply =
    std.parser.foldReply
        (fun _ value _ => printLine (show value))
        (fun _ _ error =>
            printLine (std.parser.renderError (fun _ => "token error") error))
        (fun _ _ _ => printLine "awaiting tokens")
        reply

printStringReply : forall input. std.parser.ParseReply input () String -> Task ()
printStringReply reply =
    std.parser.foldReply
        (fun _ value _ => printLine value)
        (fun _ _ error =>
            printLine (std.parser.renderError (fun _ => "token error") error))
        (fun _ _ _ => printLine "awaiting text")
        reply

printCharListReply : forall input. std.parser.ParseReply input () [Char] -> Task ()
printCharListReply reply =
    std.parser.foldReply
        (fun _ value _ => printLine (std.text.charsToString value))
        (fun _ _ error =>
            printLine (std.parser.renderError (fun _ => "token error") error))
        (fun _ _ _ => printLine "awaiting text")
        reply

successfulText : String
successfulText =
    "section app { mode = truly; nested = [alpha, [beta, true]]; }"

streamedReply : std.parser.ParseReply (std.parser.StreamInput Char) ConfigError Section
streamedReply =
    let ops = std.parser.streamInputOps show in
    let initial = std.parser.runComplete ops (document ops) std.parser.streamInput in
    let first = std.parser.supply initial (std.text.stringToChars "sect") in
    let second = std.parser.supply first (std.text.stringToChars "ion app { mode = tr") in
    let third = std.parser.supply second (std.text.stringToChars "uly; nested = [alpha, [beta, true]]; }") in
    std.parser.finishReply third

main : Task ()
main = do
    let textOps = std.parser.textInputOps
    printSectionReply
        (std.parser.runComplete textOps (document textOps)
            (std.parser.textInput successfulText))
    printSectionReply streamedReply
    printSectionReply
        (std.parser.runComplete textOps (document textOps)
            (std.parser.textInput "section reserved {}"))
    let tokenOps = std.parser.listInputOps show
    printIntListReply
        (std.parser.runComplete tokenOps
            (std.parser.tokens tokenOps "header" [1, 2, 3])
            (std.parser.listInput [1, 2, 3]))
    let sharedPrefix =
        std.parser.choice textOps
            (std.parser.mapResult (fun _ => "ab")
                (std.parser.tokens textOps "ab" ['a', 'b']))
            (std.parser.mapResult (fun _ => "ac")
                (std.parser.tokens textOps "ac" ['a', 'c']))
    printStringReply
        (std.parser.runComplete textOps sharedPrefix (std.parser.textInput "ac"))
    let backtrackingPrefix =
        std.parser.choice textOps
            (std.parser.attempt textOps
                (std.parser.mapResult (fun _ => "ab")
                    (std.parser.tokens textOps "ab" ['a', 'b'])))
            (std.parser.mapResult (fun _ => "ac")
                (std.parser.tokens textOps "ac" ['a', 'c']))
    printStringReply
        (std.parser.runComplete textOps backtrackingPrefix (std.parser.textInput "ac"))
    printCharListReply
        (std.parser.runParser
            (std.parser.many textOps (std.parser.succeed 'x'))
            (std.parser.textInput ""))
    let bytesOps = std.parser.byteInputOps
    printIntListReply
        (std.parser.runComplete bytesOps
            (std.parser.tokens bytesOps "ASCII AB" [65, 66])
            (std.parser.byteInput (std.bytes.stringToBytes "AB")))
