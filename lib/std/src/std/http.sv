import prelude.prim(httpRequestRaw, stringLength, stringSlice)
import std.bytes
import std.text

-- | An absolute HTTP or HTTPS URL validated by urlFromString.
data Url = Url String

-- | Standard HTTP request methods.
data Method =
    Get
  | Head
  | Post
  | Put
  | Patch
  | Delete
  | Options

-- | One HTTP header name and value.
data Header = Header String String

-- | An HTTP request with explicit headers and an optional bounded byte body.
data Request = Request Method Url [Header] (Maybe Bytes)

-- | A three-digit HTTP response status.
data Status = Status Int

-- | A bounded HTTP response. The body is never larger than maximumBodyBytes.
data Response = Response Status [Header] Bytes

-- | Redirect behavior. FollowRedirects zero behaves like NoRedirects.
data RedirectPolicy = NoRedirects | FollowRedirects Int

-- | Resource limits for one request.
-- |
-- | Fields are maximum response-header bytes, maximum response-body bytes,
-- | the whole-request timeout in milliseconds, and redirect policy.
data HttpLimits = HttpLimits Int Int Int RedirectPolicy

-- | Pure request-policy errors or a typed host HTTP failure.
data HttpError =
    InvalidUrl String
  | InvalidHeaderName String
  | InvalidHeaderValue String
  | InvalidHttpLimit String Int
  | HttpIoError IOError

-- | Practical defaults: 64 KiB headers, 8 MiB body, 15 seconds, five redirects.
defaultHttpLimits : HttpLimits
defaultHttpLimits = HttpLimits 65536 8388608 15000 (FollowRedirects 5)

-- | Performs a bounded GET from a URL string with practical default limits.
fetch : String -> IO HttpError Response
fetch spelling = fetchWith defaultHttpLimits [] spelling

-- | Performs a bounded GET from a URL string with validated string-pair headers.
fetchWith : HttpLimits -> [(String, String)] -> String -> IO HttpError Response
fetchWith limits rawHeaders spelling =
    case urlFromString spelling of
        Err urlError => failIO urlError
        | Ok url =>
            case headersFromList rawHeaders of
                Err headerError => failIO headerError
                | Ok headers => request limits (withHeaders headers (get url))

-- | Performs a prepared request with practical default limits.
send : Request -> IO HttpError Response
send requestValue = request defaultHttpLimits requestValue

-- | Validates an absolute URL. Only http and https schemes are accepted.
urlFromString : String -> Result HttpError Url
urlFromString text =
    if validUrlText text then Ok (Url text)
    else Err (InvalidUrl text)

-- | Returns the absolute spelling retained by a URL.
urlToString : Url -> String
urlToString (Url text) = text

-- | Builds and validates one HTTP header.
header : String -> String -> Result HttpError Header
header name value =
    if not (validHeaderName name) then Err (InvalidHeaderName name)
    else if not (validHeaderValue value) then Err (InvalidHeaderValue name)
    else Ok (Header name value)

-- | Builds a GET request without headers or a body.
get : Url -> Request
get url = Request Get url [] Nothing

-- | Builds a HEAD request without headers or a body.
head : Url -> Request
head url = Request Head url [] Nothing

-- | Builds a POST request without headers or a body.
post : Url -> Request
post url = Request Post url [] Nothing

-- | Builds a PUT request without headers or a body.
put : Url -> Request
put url = Request Put url [] Nothing

-- | Builds a PATCH request without headers or a body.
patch : Url -> Request
patch url = Request Patch url [] Nothing

-- | Builds a DELETE request without headers or a body.
delete : Url -> Request
delete url = Request Delete url [] Nothing

-- | Adds a previously validated header before existing request headers.
withHeader : Header -> Request -> Request
withHeader value (Request method url headers body) =
    Request method url (value :: headers) body

-- | Adds validated headers in list order before a request's existing headers.
withHeaders : [Header] -> Request -> Request
withHeaders headers requestValue = foldr withHeader requestValue headers

-- | Replaces a request body with UTF-8 text bytes.
withTextBody : String -> Request -> Request
withTextBody text requestValue =
    withBody (std.bytes.stringToBytes text) requestValue

-- | Replaces the request body with bytes.
withBody : Bytes -> Request -> Request
withBody body (Request method url headers _) =
    Request method url headers (Just body)

-- | Removes a request body.
withoutBody : Request -> Request
withoutBody (Request method url headers _) =
    Request method url headers Nothing

-- | Performs one bounded request without blocking other Solveig green threads.
-- |
-- | 4xx and 5xx statuses are returned normally. Transport, TLS, timeout,
-- | redirect, and response-limit failures are returned as HttpIoError.
request : HttpLimits -> Request -> IO HttpError Response
request limits requestValue =
    case validateRequest limits requestValue of
        Err policyError => failIO policyError
        | Ok (Request method (Url url) headers body) =>
            case limits of
                HttpLimits maximumHeaders maximumBody timeout redirects =>
                    fmap
                        responseFromRaw
                        (mapError HttpIoError
                            (prelude.prim.httpRequestRaw
                                (methodText method, url, map headerToRaw headers, body)
                                (maximumHeaders, maximumBody, timeout, redirectCount redirects)))

responseFromRaw : (Int, [(String, String)], Bytes) -> Response
responseFromRaw (code, rawHeaders, body) =
    Response (Status code) (map headerFromRaw rawHeaders) body

headerToRaw : Header -> (String, String)
headerToRaw (Header name value) = (name, value)

headerFromRaw : (String, String) -> Header
headerFromRaw (name, value) = Header name value

-- | Performs a GET with default limits.
getUrl : Url -> IO HttpError Response
getUrl url = request defaultHttpLimits (get url)

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
    if asciiStringEqual name candidate then Just value
    else findHeaderIn name rest

-- | Strictly decodes the bounded body as UTF-8.
responseText : Response -> Result HttpError String
responseText response =
    case std.bytes.bytesToString (responseBody response) of
        Ok text => Ok text
        | Err error => Err (HttpIoError error)

-- | Splits the bounded body into chunks no larger than the requested size.
-- |
-- | This is useful for incremental consumers while retaining the request's
-- | hard whole-body allocation bound. Non-positive chunk sizes are rejected.
bodyChunks : Int -> Response -> Result HttpError [Bytes]
bodyChunks size response =
    if size <= 0 then Err (InvalidHttpLimit "chunk bytes" size)
    else Ok (chunksFrom size (responseBody response) 0)

-- | Folds bounded body chunks without allocating a list of chunks.
foldBodyChunks : forall state. Int -> (state -> Bytes -> state) -> state -> Response -> Result HttpError state
foldBodyChunks size step initial response =
    if size <= 0 then Err (InvalidHttpLimit "chunk bytes" size)
    else Ok (foldChunksFrom size step initial (responseBody response) 0)

chunksFrom : Int -> Bytes -> Int -> [Bytes]
chunksFrom size bytes offset =
    let total = std.bytes.bytesLength bytes in
    if offset >= total then []
    else std.bytes.bytesSlice bytes offset size :: chunksFrom size bytes (offset + size)

foldChunksFrom : forall state. Int -> (state -> Bytes -> state) -> state -> Bytes -> Int -> state
foldChunksFrom size step state bytes offset =
    let total = std.bytes.bytesLength bytes in
    if offset >= total then state
    else foldChunksFrom
        size
        step
        (step state (std.bytes.bytesSlice bytes offset size))
        bytes
        (offset + size)

validateRequest : HttpLimits -> Request -> Result HttpError Request
validateRequest (HttpLimits maximumHeaders maximumBody timeout redirects) requestValue =
    if maximumHeaders <= 0 then Err (InvalidHttpLimit "response header bytes" maximumHeaders)
    else if maximumBody < 0 then Err (InvalidHttpLimit "response body bytes" maximumBody)
    else if timeout <= 0 then Err (InvalidHttpLimit "timeout milliseconds" timeout)
    else case redirects of
        FollowRedirects count =>
            if count < 0 then Err (InvalidHttpLimit "redirect count" count)
            else validateRequestFields requestValue
        | NoRedirects => validateRequestFields requestValue

validateRequestFields : Request -> Result HttpError Request
validateRequestFields (Request method (Url url) headers body) =
    if not (validUrlText url) then Err (InvalidUrl url)
    else validateHeaders headers (Request method (Url url) headers body)

validateHeaders : [Header] -> Request -> Result HttpError Request
validateHeaders [] requestValue = Ok requestValue
validateHeaders (Header name value :: rest) requestValue =
    if not (validHeaderName name) then Err (InvalidHeaderName name)
    else if not (validHeaderValue value) then Err (InvalidHeaderValue name)
    else validateHeaders rest requestValue

redirectCount : RedirectPolicy -> Int
redirectCount NoRedirects = 0
redirectCount (FollowRedirects count) = count

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
        if std.text.startsWith "https://" text then 8
        else if std.text.startsWith "http://" text then 7
        else -1
    in and
        (start >= 0)
        (validUrlTail (prelude.prim.stringSlice text start (prelude.prim.stringLength text)))

validUrlTail : String -> Bool
validUrlTail tail =
    validUrlAuthority (std.text.stringToChars tail) 0

validUrlAuthority : [Char] -> Int -> Bool
validUrlAuthority [] count = count > 0
validUrlAuthority (character :: rest) count =
    if or (character == '/') (character == '?') then
        and (count > 0) (validUrlChars rest)
    else
        let point = std.text.charToCodePoint character in
        if or
            (or
                (and (point >= 48) (point <= 57))
                (and (point >= 65) (point <= 90)))
            (or
                (and (point >= 97) (point <= 122))
                (containsCodePoint point [45, 46, 58, 91, 93]))
        then validUrlAuthority rest (count + 1)
        else False

validUrlChars : [Char] -> Bool
validUrlChars [] = True
validUrlChars (character :: rest) =
    let point = std.text.charToCodePoint character in
    and
        (and (point > 32) (point < 127))
        (and (character /= '#') (validUrlChars rest))

validHeaderName : String -> Bool
validHeaderName name =
    and
        (prelude.prim.stringLength name > 0)
        (allHeaderNameChars (std.text.stringToChars name))

allHeaderNameChars : [Char] -> Bool
allHeaderNameChars [] = True
allHeaderNameChars (character :: rest) =
    and (isHeaderNameChar character) (allHeaderNameChars rest)

isHeaderNameChar : Char -> Bool
isHeaderNameChar character =
    let point = std.text.charToCodePoint character in
    if or
        (and (point >= 48) (point <= 57))
        (or
            (and (point >= 65) (point <= 90))
            (and (point >= 97) (point <= 122)))
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
    let point = std.text.charToCodePoint character in
    and
        (and (point /= 13) (point /= 10))
        (and (point /= 0) (validHeaderValueChars rest))

-- | Validates ordinary string-pair headers for use with the low-level request API.
headersFromList : [(String, String)] -> Result HttpError [Header]
headersFromList [] = Ok []
headersFromList ((name, value) :: rest) =
    case header name value of
        Err headerError => Err headerError
        | Ok checked =>
            case headersFromList rest of
                Err restError => Err restError
                | Ok headers => Ok (checked :: headers)

asciiStringEqual : String -> String -> Bool
asciiStringEqual left right =
    asciiCharsEqual (std.text.stringToChars left) (std.text.stringToChars right)

asciiCharsEqual : [Char] -> [Char] -> Bool
asciiCharsEqual [] [] = True
asciiCharsEqual (left :: leftRest) (right :: rightRest) =
    and
        (asciiCode left == asciiCode right)
        (asciiCharsEqual leftRest rightRest)
asciiCharsEqual _ _ = False

asciiCode : Char -> Int
asciiCode character =
    let point = std.text.charToCodePoint character in
    if and (point >= 65) (point <= 90) then point + 32 else point
