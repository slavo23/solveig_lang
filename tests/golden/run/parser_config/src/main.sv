import std.bytes(*)
import std.fs(FsError)
import std.parser(*)
import std.path(pathFromString)
import std.stream
import std.text(*)

data ConfigError = ReservedName String

instance Show ConfigError where
    show error = showConfigError error

data ConfigValue = TextValue String | ListValue [ConfigValue]

data Setting = Setting String ConfigValue

data Section = Section String [Setting]

isWhitespace : Char -> Bool
isWhitespace character =
    if character == ' '
        then True
        else if character == '\n' then True else if character == '\t' then True else False

isNameCharacter : Char -> Bool
isNameCharacter character =
    if character >= 'a'
        then character <= 'z'
        else if character >= 'A'
                then character <= 'Z'
                else if character >= '0' then character <= '9' else character == '_'

spaces : forall input. std.parser.InputOps input Char -> std.parser.Parser input ConfigError ()
spaces ops = std.parser.skipMany ops (std.parser.satisfy ops "whitespace" isWhitespace)

lexeme :
    forall input a. std.parser.InputOps input Char ->
    std.parser.Parser input ConfigError a -> std.parser.Parser input ConfigError a
lexeme ops = (<* spaces ops)

symbol :
    forall input. std.parser.InputOps input Char ->
    String -> std.parser.Parser input ConfigError [Char]
symbol ops name = lexeme ops (std.parser.tokens ops name (std.text.stringToChars name))

identifierLabel :
    forall input custom value. std.parser.Parser input custom value ->
    std.parser.Parser input custom value
identifierLabel = (<?> "identifier")

identifier :
    forall input. std.parser.InputOps input Char -> std.parser.Parser input ConfigError String
identifier ops =
    identifierLabel
        (lexeme
            ops
            (do
                characters <-
                    std.parser.some ops (std.parser.satisfy ops "name character" isNameCharacter)
                let name = std.text.charsToString characters
                if name == "reserved"
                    then std.parser.customFailure ops (ReservedName name)
                    else pure name
            ))

asTextValue : forall f. Functor f => f String -> f ConfigValue
asTextValue = (TextValue <$>)

parserBind :
    forall input custom a b. std.parser.Parser input custom a ->
    (a -> std.parser.Parser input custom b) -> std.parser.Parser input custom b
parserBind = (>>=)

configValue :
    forall input. std.parser.InputOps input Char -> std.parser.Parser input ConfigError ConfigValue
configValue ops =
    std.parser.context
        "value"
        (std.parser.choice
            ops
            (std.parser.attempt ops ((fun _ => TextValue "true") <$> symbol ops "true"))
            (std.parser.choice ops (asTextValue (identifier ops)) (configList ops)))

configList :
    forall input. std.parser.InputOps input Char -> std.parser.Parser input ConfigError ConfigValue
configList ops =
    ListValue
        <$> std.parser.between
            (symbol ops "[")
            (symbol ops "]")
            (std.parser.separatedBy
                ops
                (std.parser.defer (fun _ => configValue ops))
                (symbol ops ","))

setting :
    forall input. std.parser.InputOps input Char -> std.parser.Parser input ConfigError Setting
setting ops = Setting <$> identifier ops <*> (symbol ops "=" *> configValue ops <* symbol ops ";")

section :
    forall input. std.parser.InputOps input Char -> std.parser.Parser input ConfigError Section
section ops =
    symbol ops "section"
        *> std.parser.cut
            (Section
                <$> identifier ops
                <*> std.parser.between
                    (symbol ops "{")
                    (symbol ops "}")
                    (std.parser.many ops (setting ops)))

document :
    forall input. std.parser.InputOps input Char -> std.parser.Parser input ConfigError Section
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
showSection (Section name settings) = "section " <> name <> " {" <> showSettings settings <> "}"

printSectionReply : forall input. std.parser.ParseReply input ConfigError Section -> Task ()
printSectionReply reply =
    std.parser.foldReply
        (fun _ value _ => printLine (showSection value))
        (fun _ _ error => printLine (show error))
        (fun _ _ _ => printLine "awaiting input")
        reply

printIntListReply : forall input. std.parser.ParseReply input () [Int] -> Task ()
printIntListReply reply =
    std.parser.foldReply
        (fun _ value _ => printLine (show value))
        (fun _ _ error => printLine (std.parser.renderError (fun _ => "token error") error))
        (fun _ _ _ => printLine "awaiting tokens")
        reply

printStringReply : forall input. std.parser.ParseReply input () String -> Task ()
printStringReply reply =
    std.parser.foldReply
        (fun _ value _ => printLine value)
        (fun _ _ error => printLine (std.parser.renderError (fun _ => "token error") error))
        (fun _ _ _ => printLine "awaiting text")
        reply

printCharListReply : forall input. std.parser.ParseReply input () [Char] -> Task ()
printCharListReply reply =
    std.parser.foldReply
        (fun _ value _ => printLine (std.text.charsToString value))
        (fun _ _ error => printLine (std.parser.renderError (fun _ => "token error") error))
        (fun _ _ _ => printLine "awaiting text")
        reply

successfulText : String
successfulText = "section app { mode = truly; nested = [alpha, [beta, true]]; }"

streamedReply : std.parser.ParseReply (std.parser.StreamInput Char) ConfigError Section
streamedReply =
    let ops =
        std.parser.streamInputOps show
    in let initial =
        std.parser.runComplete ops (document ops) std.parser.streamInput
    in let first =
        std.parser.supply initial (std.text.stringToChars "sect")
    in let second =
        std.parser.supply first (std.text.stringToChars "ion app { mode = tr")
    in let third =
        std.parser.supply second (std.text.stringToChars "uly; nested = [alpha, [beta, true]]; }")
    in std.parser.finishReply third

streamFacade : Task (Either (std.parser.ParseError ConfigError) Section)
streamFacade =
    let ops =
        std.parser.streamInputOps show
    in std.parser.parseStream
        show
        (document ops)
        (std.stream.fromList
            [
                std.text.stringToChars "section ",
                [],
                std.text.stringToChars "app { mode = truly; ",
                std.text.stringToChars "nested = [alpha, [beta, true]]; }"
            ])

printSectionEither : Either (std.parser.ParseError ConfigError) Section -> Task ()
printSectionEither (Right sectionValue) = printLine (showSection sectionValue)
printSectionEither (Left error) = printLine (show error)

failingChunkStep : Int -> Task (Maybe ([Char], Int))
failingChunkStep 0 = pureIO (Just (std.text.stringToChars "section", 1))
failingChunkStep _ = failIO (IOError OtherError "parser chunks" "planned")

reportChunkFailure : IOError -> Task String
reportChunkFailure error = pureIO (ioErrorOperation error)

upstreamFailure : Task String
upstreamFailure =
    let ops =
        std.parser.streamInputOps show
    in catchError
        (std.parser.parseStream show (document ops) (std.stream.unfold failingChunkStep 0)
            *> pureIO "unexpected stream success")
        reportChunkFailure

parseFileChunks :
    forall scope. std.stream.Stream scope FsError String ->
    IO FsError (Either (std.parser.ParseError ConfigError) Section)
parseFileChunks chunks =
    std.parser.parseStream
        show
        (document (std.parser.streamInputOps show))
        (std.stream.map std.text.stringToChars chunks)

scopedFile : Task String
scopedFile =
    case pathFromString "input.txt" of
        Left pathError => pureIO (show pathError)
        | Right path =>
            catchError
                (fmap sectionResult (std.fs.withFileTextStream 3 path parseFileChunks))
                (pureIO . show)

sectionResult : Either (std.parser.ParseError ConfigError) Section -> String
sectionResult (Left parseError) = show parseError
sectionResult (Right value) = showSection value

rejectingChunks : Int -> Task (Maybe ([Char], Int))
rejectingChunks 0 = pureIO (Just (std.text.stringToChars "not a section", 1))
rejectingChunks _ = failIO (IOError OtherError "unexpected read ahead" "planned")

earlyRejection : Task String
earlyRejection =
    catchError
        (fmap
            sectionResult
            (std.parser.parseStream
                show
                (document (std.parser.streamInputOps show))
                (std.stream.unfold rejectingChunks 0)))
        reportChunkFailure

mappedStreamFailure : Task String
mappedStreamFailure = do
    let ops = std.parser.streamInputOps show
    result <-
        std.parser.parseStream
            show
            (std.parser.mapCustomError showConfigError (document ops))
            (std.stream.fromList
                [std.text.stringToChars "section res", std.text.stringToChars "erved {}"])
    pureIO
        (case result of
            Left parseError => std.parser.renderError id parseError
            | Right value => showSection value
        )

main : Task ()
main = do
    let textOps = std.parser.textInputOps
    printSectionReply (std.parser.parseText (document textOps) successfulText)
    printSectionReply streamedReply
    streamedFacadeResult <- streamFacade
    printSectionEither streamedFacadeResult
    upstreamOperation <- upstreamFailure
    printLine ("stream upstream: " <> upstreamOperation)
    fileResult <- scopedFile
    printLine ("scoped file: " <> fileResult)
    rejected <- earlyRejection
    printLine ("early stream rejection: " <> rejected)
    mappedFailure <- mappedStreamFailure
    printLine ("mapped streaming error: " <> mappedFailure)
    printSectionReply
        (std.parser.runComplete
            textOps
            (document textOps)
            (std.parser.textInput "section reserved {}"))
    let tokenOps = std.parser.listInputOps show
    printIntListReply
        (std.parser.parseTokenList show (std.parser.tokens tokenOps "header" [1, 2, 3]) [1, 2, 3])
    let sharedPrefix =
        std.parser.choice
            textOps
            (std.parser.mapResult (fun _ => "ab") (std.parser.tokens textOps "ab" ['a', 'b']))
            (std.parser.mapResult (fun _ => "ac") (std.parser.tokens textOps "ac" ['a', 'c']))
    printStringReply (std.parser.runComplete textOps sharedPrefix (std.parser.textInput "ac"))
    let backtrackingPrefix =
        std.parser.choice
            textOps
            (std.parser.attempt
                textOps
                (std.parser.mapResult (fun _ => "ab") (std.parser.tokens textOps "ab" ['a', 'b'])))
            (std.parser.mapResult (fun _ => "ac") (std.parser.tokens textOps "ac" ['a', 'c']))
    printStringReply (std.parser.runComplete textOps backtrackingPrefix (std.parser.textInput "ac"))
    printCharListReply
        (std.parser.runParser
            (std.parser.many textOps (std.parser.succeed 'x'))
            (std.parser.textInput ""))
    let bytesOps = std.parser.byteInputOps
    printIntListReply
        (std.parser.parseBytes
            (std.parser.tokens bytesOps "ASCII AB" [65, 66])
            (std.bytes.stringToBytes "AB"))
