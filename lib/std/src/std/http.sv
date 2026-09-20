export (
    Url,
    Method(..),
    Header,
    Status,
    Response,
    HttpError(..),
    RedirectPolicy(..),
    ReplayPolicy(..),
    HttpOptions,
    defaultOptions,
    urlFromString,
    urlToString,
    header,
    headerName,
    headerValue,
    headersFromList,
    withHeader,
    withHeaders,
    withQuery,
    withBody,
    withTextBody,
    withBuilderBody,
    withFileBody,
    withJsonBody,
    withFormBody,
    withoutBody,
    withTimeout,
    withRedirectPolicy,
    withReplayPolicy,
    withMaximumHeaderBytes,
    withMaximumBufferedBodyBytes,
    withMaximumResponseBytes,
    withChunkBytes,
    requestTimeout,
    redirectPolicy,
    replayPolicy,
    maximumHeaderBytes,
    maximumBufferedBodyBytes,
    maximumResponseBytes,
    chunkBytes,
    get,
    getWith,
    head,
    headWith,
    post,
    postWith,
    put,
    putWith,
    patch,
    patchWith,
    delete,
    deleteWith,
    options,
    optionsWith,
    request,
    requestWith,
    requestStream,
    requestStreamWith,
    withResponse,
    withResponseWith,
    withStreamResponse,
    withStreamResponseWith,
    statusFromInt,
    response,
    responseStatus,
    statusCode,
    isSuccessful,
    ensureSuccess,
    responseHeaders,
    responseBody,
    findHeader,
    responseText,
    bodyChunks,
    foldBodyChunks
)

import prelude.prim as CorePrim(stringLength, stringSlice)
import std.prim as HostPrim(
    httpOpenRaw,
    httpRequestRaw,
    httpUploadStartRaw,
    httpUploadChunkRaw,
    httpUploadCloseRaw,
    hClose,
    hGetBytes
)
import std.bytes(*)
import std.text(*)
import std.path(Path, pathToString)
import std.json(Json, JsonError, renderJson)
import std.time(Duration, seconds, durationToMilliseconds)
import std.stream(Stream, StreamStep(..), next, unfold)
import std.task(bracket)

-- | An absolute HTTP or HTTPS URL validated by urlFromString.
data Url = Url String

instance Eq Url where
    (==) (Url left) (Url right) = left == right

instance Show Url where
    show (Url value) = value

instance Hashable Url where
    hashWithSalt salt (Url value) = hashWithSalt salt value

-- | Standard HTTP request methods.
data Method = Get | Head | Post | Put | Patch | Delete | Options

instance Eq Method where
    (==) Get Get = True
    (==) Head Head = True
    (==) Post Post = True
    (==) Put Put = True
    (==) Patch Patch = True
    (==) Delete Delete = True
    (==) Options Options = True
    (==) _ _ = False

instance Show Method where
    show = methodText

-- | One HTTP header name and value.
data Header = Header String String

instance Eq Header where
    (==) (Header leftName leftValue) (Header rightName rightValue) =
        and (asciiStringEqual leftName rightName) (leftValue == rightValue)

instance Show Header where
    show (Header name value) = name <> ": " <> value

-- | A three-digit HTTP response status.
data Status = Status Int

instance Eq Status where
    (==) (Status left) (Status right) = left == right

instance PartialOrd Status where
    partialCompare (Status left) (Status right) = partialCompare left right

instance Ord Status where
    compare (Status left) (Status right) = compare left right

instance Show Status where
    show (Status code) = show code

instance Hashable Status where
    hashWithSalt salt (Status code) = hashWithSalt salt code

-- | A bounded HTTP response. Its body obeys the request's buffered-body limit.
data Response = Response Status [Header] Bytes

-- | Pure request-policy errors or a typed host HTTP failure.
data HttpError
    = InvalidUrl String
    | InvalidHeaderName String
    | InvalidHeaderValue String
    | InvalidStatus Int
    | InvalidHttpOption String Int
    | TransportError IOError
    | TlsError IOError
    | RedirectError IOError
    | ResponseHeadersTooLarge Int
    | ResponseBodyTooLarge Int
    | HttpIOError IOError
    | InvalidResponseUtf8 Utf8Error
    | ManagedRequestHeader String
    | RequestFileError Path IOError
    | RequestJsonError JsonError
    | BodyReplayError IOError
    | ConflictingRequestBody
    | UnexpectedStatus Status

instance Eq HttpError where
    (==) (InvalidUrl left) (InvalidUrl right) = left == right
    (==) (InvalidHeaderName left) (InvalidHeaderName right) = left == right
    (==) (InvalidHeaderValue left) (InvalidHeaderValue right) = left == right
    (==) (InvalidStatus left) (InvalidStatus right) = left == right
    (==) (InvalidHttpOption leftName leftValue) (InvalidHttpOption rightName rightValue) =
        and (leftName == rightName) (leftValue == rightValue)
    (==) (TransportError left) (TransportError right) = left == right
    (==) (TlsError left) (TlsError right) = left == right
    (==) (RedirectError left) (RedirectError right) = left == right
    (==) (ResponseHeadersTooLarge left) (ResponseHeadersTooLarge right) = left == right
    (==) (ResponseBodyTooLarge left) (ResponseBodyTooLarge right) = left == right
    (==) (HttpIOError left) (HttpIOError right) = left == right
    (==) (InvalidResponseUtf8 left) (InvalidResponseUtf8 right) = left == right
    (==) (ManagedRequestHeader left) (ManagedRequestHeader right) = left == right
    (==) (RequestFileError leftPath left) (RequestFileError rightPath right) =
        and (leftPath == rightPath) (left == right)
    (==) (RequestJsonError left) (RequestJsonError right) = left == right
    (==) (BodyReplayError left) (BodyReplayError right) = left == right
    (==) ConflictingRequestBody ConflictingRequestBody = True
    (==) (UnexpectedStatus left) (UnexpectedStatus right) = left == right
    (==) _ _ = False

instance Show HttpError where
    show (InvalidUrl spelling) = "invalid URL " <> show spelling
    show (InvalidHeaderName name) = "invalid HTTP header name " <> show name
    show (InvalidHeaderValue value) = "invalid HTTP header value " <> show value
    show (InvalidStatus code) = "invalid HTTP status " <> show code
    show (InvalidHttpOption name value) = "invalid HTTP " <> name <> " limit " <> show value
    show (TransportError ioError) = "HTTP transport: " <> show ioError
    show (TlsError ioError) = "HTTP TLS: " <> show ioError
    show (RedirectError ioError) = "HTTP redirect: " <> show ioError
    show (ResponseHeadersTooLarge maximum) =
        "HTTP response headers exceed " <> show maximum <> " bytes"
    show (ResponseBodyTooLarge maximum) = "HTTP response body exceeds " <> show maximum <> " bytes"
    show (HttpIOError ioError) = "HTTP: " <> show ioError
    show (InvalidResponseUtf8 utf8Error) = "HTTP response: " <> show utf8Error
    show (ManagedRequestHeader name) = "HTTP manages the request header " <> show name
    show (RequestFileError path ioError) = "HTTP upload " <> show path <> ": " <> show ioError
    show (RequestJsonError jsonError) = "HTTP request JSON: " <> show jsonError
    show (BodyReplayError ioError) = "HTTP body replay: " <> show ioError
    show ConflictingRequestBody = "HTTP stream upload conflicts with the configured request body"
    show (UnexpectedStatus status) = "unexpected HTTP status " <> show status

-- | Validates an absolute URL. Only http and https schemes are accepted.
urlFromString : String -> Either HttpError Url
urlFromString text = if validUrlText text then Right (Url text) else Left (InvalidUrl text)

-- | Returns the absolute spelling retained by a URL.
urlToString : Url -> String
urlToString (Url text) = text

-- | Builds and validates one HTTP header.
header : String -> String -> Either HttpError Header
header name value =
    if not (validHeaderName name)
        then Left (InvalidHeaderName name)
        else if not (validHeaderValue value)
                then Left (InvalidHeaderValue value)
                else Right (Header name value)

-- | Returns a header's field name.
headerName : Header -> String
headerName (Header name _) = name

-- | Returns a header's field value.
headerValue : Header -> String
headerValue (Header _ value) = value

-- | Validates an integer as a three-digit HTTP status.
statusFromInt : Int -> Either HttpError Status
statusFromInt code =
    if and (code >= 100) (code <= 599) then Right (Status code) else Left (InvalidStatus code)

-- | Constructs a strict response value from validated components.
response : Status -> [Header] -> Bytes -> Response
response = Response

-- | Returns a response's status.
responseStatus : Response -> Status
responseStatus (Response status _ _) = status

-- | Returns the integer status code.
statusCode : Status -> Int
statusCode (Status code) = code

-- | Reports whether a status is in the successful 2xx range.
isSuccessful : Status -> Bool
isSuccessful (Status code) = and (code >= 200) (code < 300)

-- | Returns all response headers in wire order.
responseHeaders : Response -> [Header]
responseHeaders (Response _ headers _) = headers

-- | Returns the bounded response body.
responseBody : Response -> Bytes
responseBody (Response _ _ body) = body

-- | Finds the first header by ASCII case-insensitive name.
findHeader : String -> Response -> Maybe String
findHeader name (Response _ headers _) = findHeaderIn name headers

findHeaderIn : String -> [Header] -> Maybe String
findHeaderIn name [] = Nothing
findHeaderIn name (Header candidate value :: rest) =
    if asciiStringEqual name candidate then Just value else findHeaderIn name rest

-- | Strictly decodes the bounded body as UTF-8.
responseText : Response -> Either HttpError String
responseText response =
    case std.bytes.bytesToString (responseBody response) of
        Right text => Right text
        | Left error => Left (InvalidResponseUtf8 error)

-- | Splits the bounded body into chunks no larger than the requested size.
-- |
-- | This is useful for incremental consumers while retaining the request's
-- | hard whole-body allocation bound. Non-positive chunk sizes are rejected.
bodyChunks : Int -> Response -> Either HttpError [Bytes]
bodyChunks size response =
    if size <= 0
        then Left (InvalidHttpOption "chunk bytes" size)
        else Right (chunksFrom size (responseBody response) 0)

-- | Folds bounded body chunks without allocating a list of chunks.
foldBodyChunks :
    forall state. Int -> (state -> Bytes -> state) -> state -> Response -> Either HttpError state
foldBodyChunks size step initial response =
    if size <= 0
        then Left (InvalidHttpOption "chunk bytes" size)
        else Right (foldChunksFrom size step initial (responseBody response) 0)

chunksFrom : Int -> Bytes -> Int -> [Bytes]
chunksFrom size bytes offset =
    let total =
        std.bytes.bytesLength bytes
    in if offset >= total
        then []
        else std.bytes.bytesSlice bytes offset size :: chunksFrom size bytes (offset + size)

foldChunksFrom : forall state. Int -> (state -> Bytes -> state) -> state -> Bytes -> Int -> state
foldChunksFrom size step state bytes offset =
    let total =
        std.bytes.bytesLength bytes
    in if offset >= total
        then state
        else
            foldChunksFrom
                size
                step
                (step state (std.bytes.bytesSlice bytes offset size))
                bytes
                (offset + size)

methodText : Method -> String
methodText Get = "GET"
methodText Head = "HEAD"
methodText Post = "POST"
methodText Put = "PUT"
methodText Patch = "PATCH"
methodText Delete = "DELETE"
methodText Options = "OPTIONS"

validUrlText : String -> Bool
validUrlText text =
    let start =
        if std.text.startsWith "https://" text
            then 8
            else if std.text.startsWith "http://" text then 7 else -1
    in and
        (start >= 0)
        (validUrlTail (CorePrim.stringSlice text start (CorePrim.stringLength text)))

validUrlTail : String -> Bool
validUrlTail tail = validUrlAuthority (std.text.stringToChars tail) 0

validUrlAuthority : [Char] -> Int -> Bool
validUrlAuthority [] count = count > 0
validUrlAuthority (character :: rest) count =
    if or (character == '/') (character == '?')
        then and (count > 0) (validUrlChars rest)
        else
            let point =
                std.text.charToCodePoint character
            in if or
                (or (and (point >= 48) (point <= 57)) (and (point >= 65) (point <= 90)))
                (or
                    (and (point >= 97) (point <= 122))
                    (containsCodePoint point [45, 46, 58, 91, 93]))
                then validUrlAuthority rest (count + 1)
                else False

validUrlChars : [Char] -> Bool
validUrlChars [] = True
validUrlChars (character :: rest) =
    let point =
        std.text.charToCodePoint character
    in and (and (point > 32) (point < 127)) (and (character /= '#') (validUrlChars rest))

validHeaderName : String -> Bool
validHeaderName name =
    and (CorePrim.stringLength name > 0) (allHeaderNameChars (std.text.stringToChars name))

allHeaderNameChars : [Char] -> Bool
allHeaderNameChars [] = True
allHeaderNameChars (character :: rest) = and (isHeaderNameChar character) (allHeaderNameChars rest)

isHeaderNameChar : Char -> Bool
isHeaderNameChar character =
    let point =
        std.text.charToCodePoint character
    in if or
        (and (point >= 48) (point <= 57))
        (or (and (point >= 65) (point <= 90)) (and (point >= 97) (point <= 122)))
        then True
        else containsCodePoint point [33, 35, 36, 37, 38, 39, 42, 43, 45, 46, 94, 95, 96, 124, 126]

containsCodePoint : Int -> [Int] -> Bool
containsCodePoint _ [] = False
containsCodePoint point (candidate :: rest) =
    if point == candidate then True else containsCodePoint point rest

validHeaderValue : String -> Bool
validHeaderValue value = validHeaderValueChars (std.text.stringToChars value)

validHeaderValueChars : [Char] -> Bool
validHeaderValueChars [] = True
validHeaderValueChars (character :: rest) =
    let point =
        std.text.charToCodePoint character
    in and (and (point /= 13) (point /= 10)) (and (point /= 0) (validHeaderValueChars rest))

-- | Validates ordinary string-pair headers for use with the low-level request API.
headersFromList : [(String, String)] -> Either HttpError [Header]
headersFromList [] = Right []
headersFromList ((name, value) :: rest) =
    case header name value of
        Left headerError => Left headerError
        | Right checked =>
            case headersFromList rest of
                Left restError => Left restError
                | Right headers => Right (checked :: headers)

asciiStringEqual : String -> String -> Bool
asciiStringEqual left right =
    asciiCharsEqual (std.text.stringToChars left) (std.text.stringToChars right)

asciiCharsEqual : [Char] -> [Char] -> Bool
asciiCharsEqual [] [] = True
asciiCharsEqual (left :: leftRest) (right :: rightRest) =
    and (asciiCode left == asciiCode right) (asciiCharsEqual leftRest rightRest)
asciiCharsEqual _ _ = False

asciiCode : Char -> Int
asciiCode character =
    let point =
        std.text.charToCodePoint character
    in if and (point >= 65) (point <= 90) then point + 32 else point

-- | Rejects 4xx and 5xx responses while preserving all other response values.
-- | HTTP requests themselves return these statuses normally.
ensureSuccess : Response -> Either HttpError Response
ensureSuccess value =
    if statusCode (responseStatus value) >= 400
        then Left (UnexpectedStatus (responseStatus value))
        else Right value

-- | Redirect behavior. Following zero redirects returns the original response.
data RedirectPolicy = NoRedirects | FollowRedirects Int

instance Eq RedirectPolicy where
    (==) NoRedirects NoRedirects = True
    (==) (FollowRedirects left) (FollowRedirects right) = left == right
    (==) _ _ = False

instance Show RedirectPolicy where
    show NoRedirects = "NoRedirects"
    show (FollowRedirects count) = "FollowRedirects " <> show count

-- | Whether a redirect may resend a repeatable request body.
-- | Streams are one-shot under either policy. Files are rewound, not snapshotted.
data ReplayPolicy = ReplayableBodies | NoReplay

instance Eq ReplayPolicy where
    (==) ReplayableBodies ReplayableBodies = True
    (==) NoReplay NoReplay = True
    (==) _ _ = False

instance Show ReplayPolicy where
    show ReplayableBodies = "ReplayableBodies"
    show NoReplay = "NoReplay"

data RequestBody
    = EmptyBody
    | StrictBody Bytes
    | TextBody String
    | BuilderBody BytesBuilder
    | FileBody Path
    | JsonBody Json
    | FormBody [(String, String)]

data RequestContent = RequestContent [(String, String)] [(String, String)] RequestBody

data ResponsePolicy = ResponsePolicy Int Int (Maybe Int) Int

-- | Reusable immutable request options, validated when a request is performed.
-- | Start from defaultOptions; only configure behavior the request needs.
-- | These values own no live connection, file, stream, or ambient client state.
data HttpOptions = HttpOptions RequestContent ResponsePolicy Duration RedirectPolicy ReplayPolicy

-- | Uses no custom headers, query parameters, or body, and follows five redirects.
-- | The request deadline is 15 seconds. Headers are bounded to 64 KiB and whole
-- | response buffering to 8 MiB. Scoped responses use 32 KiB pulls without a
-- | total-byte quota; withMaximumResponseBytes adds an explicit transfer quota.
defaultOptions : HttpOptions
defaultOptions =
    HttpOptions
        (RequestContent [] [] EmptyBody)
        (ResponsePolicy 65536 8388608 Nothing 32768)
        (seconds 15)
        (FollowRedirects 5)
        ReplayableBodies

-- | Sets a request header, replacing an earlier value with the same ASCII name.
-- | Invalid names or values fail the request before host work. Framing headers
-- | Host, Content-Length, and Transfer-Encoding are owned by the HTTP client.
withHeader : String -> String -> HttpOptions -> HttpOptions
withHeader name value = mapContent (setHeader name value)

-- | Sets ordinary string-pair headers in order; the last value for a name wins.
withHeaders : [(String, String)] -> HttpOptions -> HttpOptions
withHeaders headers options = foldl addHeaderPair options headers

-- | Replaces additional query parameters, preserving pair order and duplicate keys.
-- | UTF-8 names and values are encoded by the host URL library, not interpolated.
withQuery : [(String, String)] -> HttpOptions -> HttpOptions
withQuery pairs = mapContent (setQuery pairs)

-- | Replaces the body with arbitrary strict bytes, without a text conversion.
withBody : Bytes -> HttpOptions -> HttpOptions
withBody bytes = mapContent (setBody (StrictBody bytes))

-- | Replaces the body with explicitly UTF-8-encoded text.
-- | The default content type is text/plain; charset=utf-8 unless overridden.
withTextBody : String -> HttpOptions -> HttpOptions
withTextBody text = mapContent (setBody (TextBody text))

-- | Sends a builder's existing chunks in order without building one combined Bytes.
-- | The builder is finite and repeatable; its chunk references remain retained.
withBuilderBody : BytesBuilder -> HttpOptions -> HttpOptions
withBuilderBody builder = mapContent (setBody (BuilderBody builder))

-- | Uploads a file incrementally in binary mode and closes it after the request.
-- | Redirect replay rewinds the same file; callers must not modify it during use.
withFileBody : Path -> HttpOptions -> HttpOptions
withFileBody path = mapContent (setBody (FileBody path))

-- | Encodes a JSON value with std.json and defaults Content-Type to application/json.
-- | Rendering failures remain structured RequestJsonError values before host work.
withJsonBody : Json -> HttpOptions -> HttpOptions
withJsonBody value = mapContent (setBody (JsonBody value))

-- | Sends UTF-8 application/x-www-form-urlencoded fields, preserving duplicate keys.
withFormBody : [(String, String)] -> HttpOptions -> HttpOptions
withFormBody pairs = mapContent (setBody (FormBody pairs))

-- | Removes the request body without changing explicitly supplied headers.
withoutBody : HttpOptions -> HttpOptions
withoutBody = mapContent (setBody EmptyBody)

-- | Changes the positive whole-request deadline, including upload and response reads.
-- | Time spent in a scoped callback does not reset the transport deadline.
withTimeout : Duration -> HttpOptions -> HttpOptions
withTimeout duration (HttpOptions content response _ redirects replay) =
    HttpOptions content response duration redirects replay

-- | Changes whether redirects are followed and their maximum count.
withRedirectPolicy : RedirectPolicy -> HttpOptions -> HttpOptions
withRedirectPolicy redirects (HttpOptions content response duration _ replay) =
    HttpOptions content response duration redirects replay

-- | Changes permission to resend repeatable bodies; never makes a stream replayable.
withReplayPolicy : ReplayPolicy -> HttpOptions -> HttpOptions
withReplayPolicy replay (HttpOptions content response duration redirects _) =
    HttpOptions content response duration redirects replay

-- | Bounds response headers in bytes; the limit must be positive.
withMaximumHeaderBytes : Int -> HttpOptions -> HttpOptions
withMaximumHeaderBytes maximum = mapResponsePolicy (setHeaderBytes maximum)

-- | Bounds whole-response buffering in bytes, without restricting scoped transfers.
-- | The non-negative limit applies to get, post, request, and their With variants.
withMaximumBufferedBodyBytes : Int -> HttpOptions -> HttpOptions
withMaximumBufferedBodyBytes maximum = mapResponsePolicy (setBufferedBytes maximum)

-- | Sets an optional non-negative quota on all response bytes read.
-- | Nothing permits a large streamed transfer; it does not disable chunk bounds.
withMaximumResponseBytes : Maybe Int -> HttpOptions -> HttpOptions
withMaximumResponseBytes maximum = mapResponsePolicy (setResponseBytes maximum)

-- | Bounds each scoped response pull in bytes; the count must be positive.
withChunkBytes : Int -> HttpOptions -> HttpOptions
withChunkBytes maximum = mapResponsePolicy (setChunkBytes maximum)

-- | Returns the whole-request deadline.
requestTimeout : HttpOptions -> Duration
requestTimeout (HttpOptions _ _ duration _ _) = duration

-- | Returns redirect behavior.
redirectPolicy : HttpOptions -> RedirectPolicy
redirectPolicy (HttpOptions _ _ _ redirects _) = redirects

-- | Returns body replay permission.
replayPolicy : HttpOptions -> ReplayPolicy
replayPolicy (HttpOptions _ _ _ _ replay) = replay

-- | Returns the response-header byte bound.
maximumHeaderBytes : HttpOptions -> Int
maximumHeaderBytes (HttpOptions _ (ResponsePolicy maximum _ _ _) _ _ _) = maximum

-- | Returns the whole-response buffering byte bound.
maximumBufferedBodyBytes : HttpOptions -> Int
maximumBufferedBodyBytes (HttpOptions _ (ResponsePolicy _ maximum _ _) _ _ _) = maximum

-- | Returns the optional total response-transfer byte quota.
maximumResponseBytes : HttpOptions -> Maybe Int
maximumResponseBytes (HttpOptions _ (ResponsePolicy _ _ maximum _) _ _ _) = maximum

-- | Returns the maximum bytes per scoped response pull.
chunkBytes : HttpOptions -> Int
chunkBytes (HttpOptions _ (ResponsePolicy _ _ _ maximum) _ _ _) = maximum

mapContent : (RequestContent -> RequestContent) -> HttpOptions -> HttpOptions
mapContent transform (HttpOptions content response duration redirects replay) =
    HttpOptions (transform content) response duration redirects replay

mapResponsePolicy : (ResponsePolicy -> ResponsePolicy) -> HttpOptions -> HttpOptions
mapResponsePolicy transform (HttpOptions content response duration redirects replay) =
    HttpOptions content (transform response) duration redirects replay

setHeader : String -> String -> RequestContent -> RequestContent
setHeader name value (RequestContent headers query body) =
    RequestContent ((name, value) :: filter (differentHeader name) headers) query body

differentHeader : String -> (String, String) -> Bool
differentHeader name (candidate, _) = not (asciiStringEqual name candidate)

addHeaderPair : HttpOptions -> (String, String) -> HttpOptions
addHeaderPair options (name, value) = withHeader name value options

setQuery : [(String, String)] -> RequestContent -> RequestContent
setQuery query (RequestContent headers _ body) = RequestContent headers query body

setBody : RequestBody -> RequestContent -> RequestContent
setBody body (RequestContent headers query _) = RequestContent headers query body

setHeaderBytes : Int -> ResponsePolicy -> ResponsePolicy
setHeaderBytes maximum (ResponsePolicy _ buffered total chunk) =
    ResponsePolicy maximum buffered total chunk

setBufferedBytes : Int -> ResponsePolicy -> ResponsePolicy
setBufferedBytes maximum (ResponsePolicy headers _ total chunk) =
    ResponsePolicy headers maximum total chunk

setResponseBytes : Maybe Int -> ResponsePolicy -> ResponsePolicy
setResponseBytes maximum (ResponsePolicy headers buffered _ chunk) =
    ResponsePolicy headers buffered maximum chunk

setChunkBytes : Int -> ResponsePolicy -> ResponsePolicy
setChunkBytes maximum (ResponsePolicy headers buffered total _) =
    ResponsePolicy headers buffered total maximum

validateOptions : HttpOptions -> Either HttpError ()
validateOptions options =
    let headers =
        maximumHeaderBytes options
    in let buffered =
        maximumBufferedBodyBytes options
    in let chunk =
        chunkBytes options
    in let milliseconds =
        durationToMilliseconds (requestTimeout options)
    in if headers <= 0
        then Left (InvalidHttpOption "response header bytes" headers)
        else if buffered < 0
                then Left (InvalidHttpOption "buffered response bytes" buffered)
                else if chunk <= 0
                        then Left (InvalidHttpOption "chunk bytes" chunk)
                        else if milliseconds <= 0
                                then Left (InvalidHttpOption "timeout milliseconds" milliseconds)
                                else
                                    case maximumResponseBytes options of
                                        Just maximum =>
                                            if maximum < 0
                                                then
                                                    Left
                                                        (InvalidHttpOption "response bytes" maximum)
                                                else validateRedirects (redirectPolicy options)
                                        | Nothing => validateRedirects (redirectPolicy options)

validateRedirects : RedirectPolicy -> Either HttpError ()
validateRedirects NoRedirects = Right ()
validateRedirects (FollowRedirects count) =
    if count < 0 then Left (InvalidHttpOption "redirect count" count) else Right ()

requestContent : HttpOptions -> RequestContent
requestContent (HttpOptions content _ _ _ _) = content

-- | Performs a bounded GET with practical defaults.
get : String -> IO HttpError Response
get = request Get

-- | Performs a GET with explicit immutable options.
getWith : HttpOptions -> String -> IO HttpError Response
getWith configured = requestWith configured Get

-- | Performs a bounded HEAD with practical defaults.
head : String -> IO HttpError Response
head = request Head

-- | Performs a HEAD with explicit immutable options.
headWith : HttpOptions -> String -> IO HttpError Response
headWith configured = requestWith configured Head

-- | Performs a bounded POST with practical defaults.
post : String -> IO HttpError Response
post = request Post

-- | Performs a POST with explicit immutable options and request body.
postWith : HttpOptions -> String -> IO HttpError Response
postWith configured = requestWith configured Post

-- | Performs a bounded PUT with practical defaults.
put : String -> IO HttpError Response
put = request Put

-- | Performs a PUT with explicit immutable options and request body.
putWith : HttpOptions -> String -> IO HttpError Response
putWith configured = requestWith configured Put

-- | Performs a bounded PATCH with practical defaults.
patch : String -> IO HttpError Response
patch = request Patch

-- | Performs a PATCH with explicit immutable options and request body.
patchWith : HttpOptions -> String -> IO HttpError Response
patchWith configured = requestWith configured Patch

-- | Performs a bounded DELETE with practical defaults.
delete : String -> IO HttpError Response
delete = request Delete

-- | Performs a DELETE with explicit immutable options.
deleteWith : HttpOptions -> String -> IO HttpError Response
deleteWith configured = requestWith configured Delete

-- | Performs a bounded OPTIONS request with practical defaults.
options : String -> IO HttpError Response
options = request Options

-- | Performs an OPTIONS request with explicit immutable options.
optionsWith : HttpOptions -> String -> IO HttpError Response
optionsWith configured = requestWith configured Options

-- | Performs any supported method with practical defaults.
-- | 4xx and 5xx statuses are returned normally; use ensureSuccess when wanted.
request : Method -> String -> IO HttpError Response
request = requestWith defaultOptions

-- | Performs any supported method with reusable request options.
-- | Response buffering is bounded independently from scoped transfer quotas.
requestWith : HttpOptions -> Method -> String -> IO HttpError Response
requestWith configured method spelling =
    case prepareRequest configured method spelling of
        Left policyError => failIO policyError
        | Right prepared =>
            let maximum =
                bufferedMaximum configured
            in fmap responseFromRaw $
                mapError (httpIOError configured (Just maximum)) $
                HostPrim.httpRequestRaw prepared (limitsToRaw configured (Just maximum))

-- | Sends a one-shot scoped byte stream and buffers the bounded response.
-- | The stream is pulled only when the transport requests more input.
requestStream :
    forall upload. Method -> Stream upload HttpError Bytes -> String -> IO HttpError Response
requestStream = requestStreamWith defaultOptions

-- | Sends a one-shot scoped byte stream with explicit request options.
-- | Redirects that would resend the body fail with BodyReplayError.
requestStreamWith :
    forall upload. HttpOptions ->
    Method -> Stream upload HttpError Bytes -> String -> IO HttpError Response
requestStreamWith configured method upload spelling =
    let maximum =
        bufferedMaximum configured
    in withStreamResponseWith
        (withMaximumResponseBytes (Just maximum) configured)
        method
        upload
        spelling
        collectStrictResponse

collectStrictResponse :
    forall scope. Status -> [Header] -> Stream scope HttpError Bytes -> IO HttpError Response
collectStrictResponse status headers body =
    fmap (response status headers . buildBytes) $ collectResponseChunks emptyBytesBuilder body

collectResponseChunks :
    forall scope. BytesBuilder -> Stream scope HttpError Bytes -> IO HttpError BytesBuilder
collectResponseChunks builder body = do
    step <- next body
    case step of
        StreamDone => pureIO builder
        | StreamYield bytes rest => collectResponseChunks (builder <> bytesBuilder bytes) rest

-- | Uses a response body as a scoped, demand-driven byte stream.
-- | The default pull size is 32 KiB and the total streamed size is unbounded.
withResponse :
    forall result. Method ->
    String ->
    (forall scope. Status -> [Header] -> Stream scope HttpError Bytes -> IO HttpError result) ->
    IO HttpError result
withResponse method spelling use = withResponseWith defaultOptions method spelling use

-- | Acquires headers, then uses a single-consumer response stream in a fresh scope.
-- | withChunkBytes bounds every pull; withMaximumResponseBytes optionally bounds
-- | the whole transfer. Returning early drops the unread body without draining it.
withResponseWith :
    forall result. HttpOptions ->
    Method ->
    String ->
    (forall scope. Status -> [Header] -> Stream scope HttpError Bytes -> IO HttpError result) ->
    IO HttpError result
withResponseWith configured method spelling use =
    case prepareRequest configured method spelling of
        Left policyError => failIO policyError
        | Right prepared =>
            bracket
                (mapError (httpIOError configured (maximumResponseBytes configured)) $
                    HostPrim.httpOpenRaw
                        prepared
                        (limitsToRaw configured (maximumResponseBytes configured)))
                closeResponse
                (fun (code, rawHeaders, handle) =>
                    use
                        (Status code)
                        (map headerFromRaw rawHeaders)
                        (unfold (responseByteStep configured handle) ())
                )

-- | Sends a one-shot scoped byte stream and exposes the response as another scope.
withStreamResponse :
    forall upload result. Method ->
    Stream upload HttpError Bytes ->
    String ->
    (forall response. Status ->
    [Header] -> Stream response HttpError Bytes -> IO HttpError result) ->
    IO HttpError result
withStreamResponse method upload spelling use =
    withStreamResponseWith defaultOptions method upload spelling use

-- | Drives upload chunks only on host demand, then transfers the same resource
-- | into the rank-2 response callback. Cleanup joins the upload owner thread on
-- | success, typed failure, defect, and cancellation.
withStreamResponseWith :
    forall upload result. HttpOptions ->
    Method ->
    Stream upload HttpError Bytes ->
    String ->
    (forall response. Status ->
    [Header] -> Stream response HttpError Bytes -> IO HttpError result) ->
    IO HttpError result
withStreamResponseWith configured method upload spelling use =
    case prepareStreamRequest configured method spelling of
        Left policyError => failIO policyError
        | Right prepared =>
            bracket
                (mapError (httpIOError configured (maximumResponseBytes configured)) $
                    HostPrim.httpUploadStartRaw
                        prepared
                        (limitsToRaw configured (maximumResponseBytes configured)))
                closeUpload
                (feedAndUseUpload configured upload use)

type RawUploadEvent = (Handle, Maybe (Int, [(String, String)], Handle))

feedAndUseUpload :
    forall upload result. HttpOptions ->
    Stream upload HttpError Bytes ->
    (forall response. Status ->
    [Header] -> Stream response HttpError Bytes -> IO HttpError result) ->
    RawUploadEvent -> IO HttpError result
feedAndUseUpload configured upload use event = do
    (_, responseValue) <- feedUpload configured upload event
    case responseValue of
        (code, rawHeaders, handle) =>
            use
                (Status code)
                (map headerFromRaw rawHeaders)
                (unfold (responseByteStep configured handle) ())

feedUpload :
    forall upload. HttpOptions ->
    Stream upload HttpError Bytes ->
    RawUploadEvent -> IO HttpError (Handle, (Int, [(String, String)], Handle))
feedUpload configured upload (handle, responseValue) =
    case responseValue of
        Just responseValue => pureIO (handle, responseValue)
        | Nothing => do
            step <- next upload
            case step of
                StreamDone => continueUpload configured handle Nothing
                | StreamYield bytes rest =>
                    if bytesLength bytes == 0
                        then feedUpload configured rest (handle, Nothing)
                        else do
                                event <-
                                    mapError
                                        (httpIOError configured (maximumResponseBytes configured)) $
                                        HostPrim.httpUploadChunkRaw handle (Just bytes)
                                feedUpload configured rest event

continueUpload :
    HttpOptions -> Handle -> Maybe Bytes -> IO HttpError (Handle, (Int, [(String, String)], Handle))
continueUpload configured handle bytes = do
    event <-
        mapError (httpIOError configured (maximumResponseBytes configured)) $
            HostPrim.httpUploadChunkRaw handle bytes
    case event of
        (nextHandle, Just responseValue) => pureIO (nextHandle, responseValue)
        | (nextHandle, Nothing) => continueUpload configured nextHandle Nothing

closeUpload : RawUploadEvent -> IO HttpError ()
closeUpload (handle, _) = mapError HttpIOError $ HostPrim.httpUploadCloseRaw handle

type RawBody = (Int, Maybe Bytes, [Bytes], Maybe String, [(String, String)])
type RawRequest = (String, String, [(String, String)], [(String, String)], RawBody)

prepareRequest : HttpOptions -> Method -> String -> Either HttpError RawRequest
prepareRequest configured method spelling =
    case validateOptions configured of
        Left error => Left error
        | Right _ =>
            case urlFromString spelling of
                Left error => Left error
                | Right url => prepareContent configured method url

prepareStreamRequest : HttpOptions -> Method -> String -> Either HttpError RawRequest
prepareStreamRequest configured method spelling =
    let RequestContent _ _ body =
        requestContent configured
    in case body of
        EmptyBody => fmap markStreamBody (prepareRequest configured method spelling)
        | _ => Left ConflictingRequestBody

markStreamBody : RawRequest -> RawRequest
markStreamBody (method, url, headers, query, _) =
    (method, url, headers, query, (5, Nothing, [], Nothing, []))

prepareContent : HttpOptions -> Method -> Url -> Either HttpError RawRequest
prepareContent configured method url =
    let RequestContent rawHeaders query body =
        requestContent configured
    in case bodyToRaw body of
        Left error => Left error
        | Right (rawBody, defaultContentType) =>
            let headers =
                addDefaultContentType defaultContentType rawHeaders
            in case validateRequestHeaders headers of
                Left error => Left error
                | Right validHeaders =>
                    Right (methodText method, urlToString url, validHeaders, query, rawBody)

bodyToRaw : RequestBody -> Either HttpError (RawBody, Maybe String)
bodyToRaw EmptyBody = Right ((0, Nothing, [], Nothing, []), Nothing)
bodyToRaw (StrictBody bytes) = Right ((1, Just bytes, [], Nothing, []), Nothing)
bodyToRaw (TextBody text) =
    Right
        (
            (1, Just (std.bytes.stringToBytes text), [], Nothing, []),
            Just "text/plain; charset=utf-8"
        )
bodyToRaw (BuilderBody builder) =
    Right ((2, Nothing, std.bytes.builderChunks builder, Nothing, []), Nothing)
bodyToRaw (FileBody path) = Right ((3, Nothing, [], Just (pathToString path), []), Nothing)
bodyToRaw (JsonBody value) =
    case renderJson value of
        Left error => Left (RequestJsonError error)
        | Right text =>
            Right
                ((1, Just (std.bytes.stringToBytes text), [], Nothing, []), Just "application/json")
bodyToRaw (FormBody pairs) =
    Right ((4, Nothing, [], Nothing, pairs), Just "application/x-www-form-urlencoded")

addDefaultContentType : Maybe String -> [(String, String)] -> [(String, String)]
addDefaultContentType Nothing headers = headers
addDefaultContentType (Just value) headers =
    if hasHeader "content-type" headers then headers else ("Content-Type", value) :: headers

hasHeader : String -> [(String, String)] -> Bool
hasHeader _ [] = False
hasHeader name ((candidate, _) :: rest) = or (asciiStringEqual name candidate) (hasHeader name rest)

validateRequestHeaders : [(String, String)] -> Either HttpError [(String, String)]
validateRequestHeaders [] = Right []
validateRequestHeaders ((name, value) :: rest) =
    if isManagedHeader name
        then Left (ManagedRequestHeader name)
        else
            case header name value of
                Left error => Left error
                | Right _ =>
                    fmap (fun valid => (name, value) :: valid) (validateRequestHeaders rest)

isManagedHeader : String -> Bool
isManagedHeader name =
    or
        (asciiStringEqual name "host")
        (or (asciiStringEqual name "content-length") (asciiStringEqual name "transfer-encoding"))

bufferedMaximum : HttpOptions -> Int
bufferedMaximum configured =
    let buffered =
        maximumBufferedBodyBytes configured
    in case maximumResponseBytes configured of
        Nothing => buffered
        | Just total => if buffered <= total then buffered else total

limitsToRaw : HttpOptions -> Maybe Int -> (Int, Maybe Int, Int, Int, Bool)
limitsToRaw configured maximum =
    (
        maximumHeaderBytes configured,
        maximum,
        durationToMilliseconds (requestTimeout configured),
        redirectCount (redirectPolicy configured),
        replayPolicy configured == ReplayableBodies
    )

redirectCount : RedirectPolicy -> Int
redirectCount NoRedirects = 0
redirectCount (FollowRedirects count) = count

httpIOError : HttpOptions -> Maybe Int -> IOError -> HttpError
httpIOError configured maximum ioError =
    case ioErrorOperation ioError of
        "httpTransport" => TransportError ioError
        | "httpTls" => TlsError ioError
        | "httpRedirect" => RedirectError ioError
        | "httpBodyReplay" => BodyReplayError ioError
        | "httpRequestFile" => requestFileError configured ioError
        | "httpResponseHeaders" => ResponseHeadersTooLarge (maximumHeaderBytes configured)
        | "httpResponseBody" => responseBodyLimitError maximum ioError
        | _ => HttpIOError ioError

requestFileError : HttpOptions -> IOError -> HttpError
requestFileError configured ioError =
    let RequestContent _ _ body =
        requestContent configured
    in case body of
        FileBody path => RequestFileError path ioError
        | _ => HttpIOError ioError

responseBodyLimitError : Maybe Int -> IOError -> HttpError
responseBodyLimitError (Just maximum) _ = ResponseBodyTooLarge maximum
responseBodyLimitError Nothing ioError = HttpIOError ioError

closeResponse : (Int, [(String, String)], Handle) -> IO HttpError ()
closeResponse (_, _, handle) = mapError HttpIOError $ HostPrim.hClose handle

responseByteStep : HttpOptions -> Handle -> () -> IO HttpError (Maybe (Bytes, ()))
responseByteStep configured handle _ = do
    bytes <-
        mapError (responseReadError configured) $ HostPrim.hGetBytes handle (chunkBytes configured)
    if bytesLength bytes == 0 then pureIO Nothing else pureIO (Just (bytes, ()))

responseReadError : HttpOptions -> IOError -> HttpError
responseReadError configured ioError =
    if ioErrorOperation ioError == "httpResponseBody"
        then responseBodyLimitError (maximumResponseBytes configured) ioError
        else TransportError ioError

responseFromRaw : (Int, [(String, String)], Bytes) -> Response
responseFromRaw (code, rawHeaders, body) =
    Response (Status code) (map headerFromRaw rawHeaders) body

headerFromRaw : (String, String) -> Header
headerFromRaw (name, value) = Header name value
