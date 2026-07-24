import prelude.prim(stringToChars, charsToString)

data QuizCard = QuizCard String String

data State s a = State (s -> Pair a s)

runState : forall s a. State s a -> s -> Pair a s
runState action state =
    case action of
        State run => run state

statePure : forall s a. a -> State s a
statePure value = State (fun state => MkPair value state)

stateBind : forall s a b. State s a -> (a -> State s b) -> State s b
stateBind action continuation =
    State (fun state =>
        case runState action state of
            MkPair value nextState => runState (continuation value) nextState)

modifyState : forall s. (s -> s) -> State s ()
modifyState f = State (fun state => MkPair () (f state))

instance Functor (State s) where
    fmap f action =
        State (fun state =>
            case runState action state of
                MkPair value nextState => MkPair (f value) nextState)

instance Applicative (State s) where
    pure value = statePure value

    apply actionF actionX =
        stateBind actionF (fun f =>
            stateBind actionX (fun x =>
                statePure (f x)))

instance Monad (State s) where
    bind action continuation = stateBind action continuation

consNonEmpty : String -> [String] -> [String]
consNonEmpty line rest =
    if line == "" then rest else line :: rest

splitLines : String -> [String]
splitLines text = splitLinesChars (stringToChars text) []

splitLinesChars : [Char] -> [Char] -> [String]
splitLinesChars chars current =
    case chars of
        [] => consNonEmpty (charsToString (reverse current)) []
        | c :: rest =>
            if c == '\n' then
                consNonEmpty (charsToString (reverse current)) (splitLinesChars rest [])
            else
                splitLinesChars rest (c :: current)

splitQuestionAnswer : String -> Pair String String
splitQuestionAnswer line = splitQuestionAnswerChars (stringToChars line) []

splitQuestionAnswerChars : [Char] -> [Char] -> Pair String String
splitQuestionAnswerChars chars question =
    case chars of
        [] => MkPair (charsToString (reverse question)) ""
        | c :: rest =>
            if c == '|' then
                MkPair (charsToString (reverse question)) (charsToString rest)
            else
                splitQuestionAnswerChars rest (c :: question)

parseCard : String -> QuizCard
parseCard line =
    case splitQuestionAnswer line of
        MkPair question answer => QuizCard question answer

parseQuiz : String -> [QuizCard]
parseQuiz contents = map parseCard (splitLines contents)

askCard : QuizCard -> IO (Pair QuizCard String)
askCard card =
    case card of
        QuizCard question expected =>
            seqIO
                (printLine question)
                (seqIO
                    (printStr "> ")
                    (bindIO readLine (fun answer => pureIO (MkPair card answer))))

askCards : [QuizCard] -> IO [Pair QuizCard String]
askCards cards = mapM askCard cards

scoreResponse : Pair QuizCard String -> State Int ()
scoreResponse response =
    case response of
        MkPair card answer =>
            case card of
                QuizCard question expected =>
                    if answer == expected then
                        modifyState (fun score => score + 1)
                    else
                        statePure ()

scoreResponses : [Pair QuizCard String] -> State Int ()
scoreResponses responses =
    case responses of
        [] => statePure ()
        | response :: rest =>
            stateBind (scoreResponse response) (fun ignored => scoreResponses rest)

scoreQuiz : [Pair QuizCard String] -> Int
scoreQuiz responses =
    case runState (scoreResponses responses) 0 of
        MkPair ignored score => score

lengthCards : [QuizCard] -> Int
lengthCards cards =
    case cards of
        [] => 0
        | card :: rest => 1 + lengthCards rest

summaryLine : Int -> Int -> String
summaryLine score total =
    appendString "Score: " (appendString (show score) (appendString "/" (show total)))

withNewline : String -> String
withNewline text = appendString text "\n"

main : IO ()
main =
    bindIO (readFile "questions.txt") (fun contents =>
        let cards = parseQuiz contents in
            seqIO
                (printLine "Solveig quiz")
                (bindIO (askCards cards) (fun responses =>
                    let score = scoreQuiz responses in
                    let total = lengthCards cards in
                    let summary = summaryLine score total in
                        seqIO
                            (printLine summary)
                            (seqIO
                                (writeFile "last-score.txt" (withNewline summary))
                                (appendFile "score-history.txt" (withNewline summary))))))
