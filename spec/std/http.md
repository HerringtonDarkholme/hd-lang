# Http

Status: standard library specification draft.

This chapter defines `std.http`, which `lib/std` writes in ordinary hd
over the language tier:

- the capability trait `Http`, and the free helpers `get!` and `post!`;
- the struct values `Request` and `Response`, and the enums `Method` and
  `HttpError`;
- status checking, retained error responses, and the JSON calls;
- which request URLs are valid;
- `ScriptedHttp`, the deterministic provider of `Http`;
- what the playground binds for `Http`, and how a `block_on` call waits
  there.

Which provider a command binds, and which hosts its grant covers, is CLI
tier ([Host Capabilities](../cli/command-line.md#host-capabilities)).

## Read-Once Bodies

A `Body` is a value whose copies share private consumption state.

1. r[std-http.body.declaration] `std.http` declares `Body` with private shared state.
2. r[std-http.body.from-bytes] `Body::from_bytes(bytes: List[u8]) -> Body` creates a fresh unread body holding those bytes.
3. r[std-http.body.shared-state] Copying a `Body` shares its bytes and read state with the original.
4. r[std-http.body.bytes] `bytes(&self) -> Result[List[u8], BodyError]` claims the body and returns its bytes.
5. r[std-http.body.used] Any later read through any copy returns `BodyError.Used`.
6. r[std-http.body.claim-before-decode] Every reader claims the body before decoding; a decoding failure leaves it used.
7. r[std-http.body.empty-claim] Reading an empty body also claims it.
8. r[std-http.body.text] `text(&self) -> Result[string, BodyError]` reads UTF-8, replacing malformed sequences with U+FFFD.
9. r[std-http.body.json] `json[T < Deserialize](&self) -> Result[T, BodyError]` decodes the body's text through `std.json`.
10. r[std-http.body.json-error] A JSON decoding failure returns `BodyError.Json` containing the decoder's error.
11. r[std-http.body.form] `form[T < Deserialize](&self) -> Result[T, BodyError]` decodes a URL-encoded form using [Typed Query And Form Values](#typed-query-and-form-values).
12. r[std-http.body.form-error] A form decoding failure returns `BodyError.Form` containing its `FormError`.
13. r[std-http.body.value-equality] `Body` implements `Eq` by comparing payload and read state, independently of allocation identity.
14. r[std-http.body.debug-private] `Body` implements `Debug`, showing its length and read state without its bytes.

## Typed Query And Form Values

Query pairs and URL-encoded bodies share one serde codec.

1. r[std-http.form-codec.query-reader] `Url.query_as[T < Deserialize](&self) -> Result[T, FormError]` decodes its query pairs into `T`.
2. r[std-http.form-codec.parameter-record] The root value is a struct whose fields are parameters named by their serde wire names.
3. r[std-http.form-codec.scalar] Scalar values use the existing serde textual conversions.
4. r[std-http.form-codec.deep-object] A struct-valued parameter uses one bracket level, `parameter[property]`, whose properties are scalars or optional scalars.
5. r[std-http.form-codec.repeated-scalars] A list or tuple of supported scalars uses repeated occurrences of its parameter name, in occurrence order.
6. r[std-http.form-codec.tuple-count] A tuple with the wrong number of values returns `Invalid(field, text)`.
7. r[std-http.form-codec.duplicate-scalar] Repeated scalar parameters return `Invalid(field, text)` with the first occurrence's text, even when the texts are equal.
8. r[std-http.form-codec.missing-list] An absent list parameter decodes to an empty list.
9. r[std-http.form-codec.optional] An absent optional value decodes to `None`; an explicit empty string remains present.
10. r[std-http.form-codec.defaults] An absent field with a declared serde default takes that default.
11. r[std-http.form-codec.unknown] Unknown keys are ignored after percent-encoding and UTF-8 validation.
12. r[std-http.form-codec.missing-scalar] A missing required scalar returns `Missing(field)` with its declared wire path.
13. r[std-http.form-codec.invalid-scalar] A failed scalar conversion returns `Invalid(field, text)` with its declared wire path and original text.
14. r[std-http.form-codec.unit-enum] A selected unit enum variant is a scalar whose text is its variant name.
15. r[std-http.form-codec.payload-enum] A visited enum variant with payload fields returns `Unsupported(field)`.
16. r[std-http.form-codec.unsupported-containers] A visited dynamic map, deeper record or nested list returns `Unsupported(field)`.
17. r[std-http.form-codec.unsupported-visits] Unsupported values are rejected when visited, without schema preflight.
18. r[std-http.form-codec.empty-unvisited] Empty record lists and absent optional records encode and decode as empty or absent without error.
19. r[std-http.form-codec.bracketed-rename] A member wire name containing `[` or `]` returns `Unsupported(field)`.
20. r[std-http.form-codec.body-pairs] Form decoding splits nonempty parts at `&` and each part's first `=`, retaining their order.
21. r[std-http.form-codec.body-empty-value] A form part without `=` has an empty value.
22. r[std-http.form-codec.body-decode] Form names and values percent-decode UTF-8 and interpret `+` as space.
23. r[std-http.form-codec.strict-input] Invalid percent-encoding or invalid UTF-8 returns `FormError`, including in unknown pairs.
24. r[std-http.form-codec.query-writer] `Url.set_query_as[T < Serialize](&mut self, value: T) -> void` replaces the query using the same serde codec.
25. r[std-http.form-codec.query-write-order] Writing retains member declaration order and list or tuple item order as decoded query pairs.
26. r[std-http.form-codec.query-write-absence] Writing an absent optional value emits no pair.
27. r[std-http.form-codec.query-write-empty] Every query write clears the explicit empty-query presence bit.
28. r[std-http.form-codec.query-write-error] A failed query write retains a private `FormError`, which later `query_as` returns directly.
29. r[std-http.form-codec.query-replacement] A successful `set_query_as` replaces the query and clears its pending preparation error.
30. r[std-http.form-codec.query-append-error] `add_param` does not clear a pending query preparation error.
31. r[std-http.form-codec.query-resolve-inherit] Resolution retains a pending query error exactly when RFC 3986 inherits the base query, including empty and fragment-only references.
32. r[std-http.form-codec.query-resolve-replace] A reference replacing the query clears its pending error, including an explicit query, new path, authority or scheme.

## Named Status Codes

1. r[std-http.status.declaration] `std.http` declares `HttpStatus` with the variants and numeric codes below, and `Other(code: u16)`.

| Variant | Code |
| --- | --- |
| `Ok` | 200 |
| `Created` | 201 |
| `Accepted` | 202 |
| `NoContent` | 204 |
| `PartialContent` | 206 |
| `MovedPermanently` | 301 |
| `Found` | 302 |
| `SeeOther` | 303 |
| `NotModified` | 304 |
| `TemporaryRedirect` | 307 |
| `PermanentRedirect` | 308 |
| `BadRequest` | 400 |
| `Unauthorized` | 401 |
| `Forbidden` | 403 |
| `NotFound` | 404 |
| `MethodNotAllowed` | 405 |
| `RequestTimeout` | 408 |
| `Conflict` | 409 |
| `Gone` | 410 |
| `PreconditionFailed` | 412 |
| `ContentTooLarge` | 413 |
| `UnsupportedMediaType` | 415 |
| `UnprocessableContent` | 422 |
| `TooManyRequests` | 429 |
| `InternalServerError` | 500 |
| `NotImplemented` | 501 |
| `BadGateway` | 502 |
| `ServiceUnavailable` | 503 |
| `GatewayTimeout` | 504 |

2. r[std-http.status.code] `HttpStatus.code(&self) -> u16` returns the variant's numeric code.
3. r[std-http.status.from-code] `HttpStatus::from_code(code: u16) -> HttpStatus` returns the named variant for a listed code, and `Other(code)` otherwise.
4. r[std-http.status.is-success] `HttpStatus.is_success(&self) -> bool` is true exactly for codes from 200 through 299.
5. r[std-http.status.numeric-equality] Two `HttpStatus` values are equal exactly when their numeric codes are equal, including named variants and `Other`.
6. r[std-http.status.numeric-hash] `HttpStatus` implements `Hash` by hashing its `u16` code.
7. r[std-http.status.debug] `HttpStatus` implements structural `Debug`.

## Header Names

1. r[std-http.header-name.declaration] `std.http` declares `HeaderName` with the variants and wire names below, and `Other(name: string)`.

| Variant | Wire name |
| --- | --- |
| `Accept` | `accept` |
| `AcceptEncoding` | `accept-encoding` |
| `AcceptLanguage` | `accept-language` |
| `ContentDisposition` | `content-disposition` |
| `ContentEncoding` | `content-encoding` |
| `ContentLength` | `content-length` |
| `ContentRange` | `content-range` |
| `ContentType` | `content-type` |
| `Range` | `range` |
| `CacheControl` | `cache-control` |
| `ETag` | `etag` |
| `Expires` | `expires` |
| `IfMatch` | `if-match` |
| `IfModifiedSince` | `if-modified-since` |
| `IfNoneMatch` | `if-none-match` |
| `LastModified` | `last-modified` |
| `Vary` | `vary` |
| `Authorization` | `authorization` |
| `Cookie` | `cookie` |
| `RetryAfter` | `retry-after` |
| `SetCookie` | `set-cookie` |
| `WwwAuthenticate` | `www-authenticate` |
| `AccessControlAllowCredentials` | `access-control-allow-credentials` |
| `AccessControlAllowHeaders` | `access-control-allow-headers` |
| `AccessControlAllowMethods` | `access-control-allow-methods` |
| `AccessControlAllowOrigin` | `access-control-allow-origin` |
| `AccessControlMaxAge` | `access-control-max-age` |
| `AccessControlRequestHeaders` | `access-control-request-headers` |
| `AccessControlRequestMethod` | `access-control-request-method` |
| `Origin` | `origin` |
| `ContentSecurityPolicy` | `content-security-policy` |
| `StrictTransportSecurity` | `strict-transport-security` |
| `XContentTypeOptions` | `x-content-type-options` |
| `XFrameOptions` | `x-frame-options` |
| `Allow` | `allow` |
| `Forwarded` | `forwarded` |
| `Host` | `host` |
| `Location` | `location` |
| `Referer` | `referer` |
| `XForwardedFor` | `x-forwarded-for` |
| `XForwardedHost` | `x-forwarded-host` |
| `XForwardedProto` | `x-forwarded-proto` |
| `Date` | `date` |
| `Server` | `server` |
| `UserAgent` | `user-agent` |
| `XRequestId` | `x-request-id` |

2. r[std-http.header-name.wire] `HeaderName.wire_name(&self) -> string` returns the listed wire name, or the `Other` name with ASCII uppercase letters lowercased.
3. r[std-http.header-name.parse] `HeaderName` implements `Parse[Error = HeaderError]`.
4. r[std-http.header-name.parse-known] Parsing a listed wire name, ignoring ASCII case, returns its named variant.
5. r[std-http.header-name.parse-other] Parsing another valid field name returns `Other` with its ASCII letters lowercased.
6. r[std-http.header-name.parse-token] A valid field name is a nonempty sequence of ASCII letters, digits, or a punctuation character from `` !#$%&'*+-.^_`|~ ``, as [RFC 9110 §5.6.2](https://www.rfc-editor.org/rfc/rfc9110.html#section-5.6.2) defines.
7. r[std-http.header-name.parse-invalid] Parsing any other text returns `HeaderError.InvalidName` holding the original text.
8. r[std-http.header-name.wire-equality] Two `HeaderName` values are equal exactly when their wire names are equal.
9. r[std-http.header-name.wire-hash] `HeaderName` implements `Hash` by hashing its wire name as a string.
10. r[std-http.header-name.debug] `HeaderName` implements structural `Debug`.
11. r[std-http.header-error.declaration] `HeaderError` is an error enum with `InvalidName(name: string)` and `InvalidValue(name: string)`.
12. r[std-http.header-error.display] Those variants display `invalid header name NAME` and `invalid value for header NAME`, respectively.
13. r[std-http.header-error.traits] `HeaderError` implements `Eq` and `Debug`.

## Header Collections

1. r[std-http.header-collection.declaration] `Headers` is an ordered collection of `(HeaderName, string)` pairs with private storage.
2. r[std-http.header-collection.new] `Headers::new() -> Headers` returns an empty collection.
3. r[std-http.header-collection.from-pairs] `From[List[(string, string)]]` retains every pair in input order, parsing valid names as `HeaderName` and retaining invalid names as `Other` for wire validation.
4. r[std-http.header-collection.from-map] `From[Map[string, string]]` converts each map entry to one header pair.
5. r[std-http.header-collection.get] `get(&self, name: HeaderName) -> string?` returns matching values joined by `", "` in their stored order, or `None` when absent, except for `SetCookie`.
6. r[std-http.header-collection.set-cookie-first] `get(SetCookie)` returns the first raw `Set-Cookie` value without joining lines; `response_cookies()` reads every line.
7. r[std-http.header-collection.set] `set(&mut self, name: HeaderName, value: string) -> void` replaces every matching pair with one pair holding `value`.
8. r[std-http.header-collection.remove] `remove(&mut self, name: HeaderName) -> void` removes every matching pair.
9. r[std-http.header-collection.entries] `entries(&self) -> List[(HeaderName, string)]` returns all stored pairs in order.
10. r[std-http.header-collection.traits] `Headers` implements structural `Eq` and `Debug`.
11. r[std-http.request-cookies.read] `request_cookies(&self) -> List[(string, string)]` returns name/value pairs from `Cookie` fields in received order, retaining duplicate names.
12. r[std-http.request-cookies.first] `request_cookie(&self, name: string) -> string?` returns the first value for that case-sensitive cookie name, or `None` when absent.
13. r[std-http.request-cookies.set] `set_request_cookie(&mut self, name: string, value: string) -> void` replaces that name's cookies while retaining other cookies in one `Cookie` header.

## Response Cookies

1. r[std-http.cookie.same-site] `SameSite` has the variants `Strict`, `Lax` and `None`, and implements `Eq` and `Debug`.
2. r[std-http.cookie.declaration] `Cookie` has public fields `name: string`, `value: string`, `path: string?`, `domain: string?`, `max_age: Duration?`, `expires: Timestamp?`, `secure: bool`, `http_only: bool` and `same_site: SameSite`.
3. r[std-http.cookie.defaults] A constructed cookie defaults to `path = Some("/")`, absent domain, max-age and expires, `secure = true`, `http_only = true` and `same_site = Lax`.
4. r[std-http.cookie.traits] `Cookie` implements structural `Eq` and `Debug`.
5. r[std-http.cookie.expired] `Cookie::expired(name: string) -> Cookie` returns that name with an empty value, `max_age = Some(Duration::seconds(0))` and the other outgoing defaults.
6. r[std-http.response-cookies.read] `Headers.response_cookies(&self) -> List[Cookie]` parses every separate `Set-Cookie` line in stored order.
7. r[std-http.response-cookies.incoming-defaults] Absent Path, Domain, Max-Age and Expires parse as `None`, absent Secure and HttpOnly as `false`, and absent or unknown SameSite as `Lax`.
8. r[std-http.response-cookies.crlf] A `Set-Cookie` line containing CR or LF contributes no parsed cookie.
9. r[std-http.response-cookies.unknown-attributes] Parsing ignores unrecognized cookie attributes.
10. r[std-http.response-cookies.attributes] Recognized cookie attributes are parsed according to [RFC 6265 §5.2](https://www.rfc-editor.org/rfc/rfc6265.html#section-5.2), ignoring ASCII case in attribute names.
11. r[std-http.response-cookies.expires] Expires uses the cookie-date algorithm in [RFC 6265 §5.1.1](https://www.rfc-editor.org/rfc/rfc6265.html#section-5.1.1), with invalid dates ignored.
12. r[std-http.response-cookies.same-site] SameSite parses `Strict`, `Lax` and `None` ignoring ASCII case.
13. r[std-http.response-cookies.contextual-path] An empty or non-slash Path parses as `None`, clearing any earlier Path; CookieJar later applies the request's default path.

## Writing Response Cookies

1. r[std-http.response-cookies.set] `Headers.set_response_cookie(&mut self, cookie: Cookie) -> void` writes one `Set-Cookie` line, replacing earlier cookies with that case-sensitive name.
2. r[std-http.response-cookies.set-unrelated] Setting a response cookie retains unrelated headers and cookies in their order.
3. r[std-http.response-cookies.wire-pair] A written cookie begins with its name and value joined by `=`, without encoding them.
4. r[std-http.response-cookies.wire-optional] Present Path, Domain, Max-Age and Expires fields become corresponding attributes; absent ones are omitted.
5. r[std-http.response-cookies.max-age-ceiling] Max-Age is the duration's whole seconds rounded up; a positive fraction yields at least `1`, and only zero yields `0`.
6. r[std-http.response-cookies.expires-text] Expires uses an RFC 6265 HTTP date in UTC, with English weekday and month names and whole-second precision.
7. r[std-http.response-cookies.wire-flags] Secure and HttpOnly attributes are included exactly when their corresponding flags are true.
8. r[std-http.response-cookies.wire-same-site] Every written cookie includes SameSite with its `Strict`, `Lax` or `None` spelling.
9. r[std-http.request-cookies.set-components] Request cookie setters retain the supplied name and value without interpreting delimiters inside either component.

## Message Conversion Errors

1. r[std-http.body-error.declaration] `BodyError` is an error enum with `Used`, `Json(error: JsonError)` and `Form(error: FormError)`.
2. r[std-http.body-error.display-used] `BodyError.Used` displays `the body was already read`.
3. r[std-http.body-error.display-json] `BodyError.Json(error)` displays `invalid JSON body: ERROR`, with the JSON error's Display text.
4. r[std-http.body-error.display-form] `BodyError.Form(error)` displays `invalid form body: ERROR`, with the form error's Display text.
5. r[std-http.body-error.traits] `BodyError` implements structural `Eq` and `Debug`.
6. r[std-http.form-error.declaration] `FormError` is an error enum with `Unsupported(field: string)`, `Missing(field: string)` and `Invalid(field: string, text: string)`.
7. r[std-http.form-error.display-unsupported] `FormError.Unsupported(field)` displays `field FIELD has no form encoding`.
8. r[std-http.form-error.display-missing] `FormError.Missing(field)` displays `missing field FIELD`.
9. r[std-http.form-error.display-invalid] `FormError.Invalid(field, text)` displays `field FIELD: cannot read TEXT`.
10. r[std-http.form-error.traits] `FormError` implements structural `Eq` and `Debug`.
11. r[std-http.url-error.declaration] `UrlError` is an error enum with `Invalid(text: string)` and `RelativeWithoutBase(text: string)`.
12. r[std-http.url-error.display-invalid-text] `UrlError.Invalid(text)` normally displays `invalid URL: TEXT`.
13. r[std-http.url-error.display-relative] `UrlError.RelativeWithoutBase(text)` displays `relative URL without a base: TEXT`.
14. r[std-http.url-error.traits] `UrlError` implements structural `Eq` and `Debug`.
15. r[std-http.url-error.into-http] `From[UrlError] for HttpError` maps either variant to `HttpError.InvalidUrl`, retaining its input text.
16. r[std-http.url-error.display-user-info] For user-info input, the Invalid error omits credentials from Display and recommends using the Authorization header.

## Parsed URLs

1. r[std-http.parsed-url.declaration] `std.http.Url` is a value with these public fields and private encoding metadata.

| Field | Type | Meaning |
| --- | --- | --- |
| `scheme` | `string` | HTTP or HTTPS scheme |
| `host` | `string` | ASCII authority host, including IP-literal brackets |
| `port` | `u16?` | Explicit port when present |
| `path` | `string` | Percent-decoded path |
| `query` | `List[(string, string)]` | Decoded query pairs in order |
| `fragment` | `string?` | Decoded fragment when present |

2. r[std-http.parsed-url.parse] `Url::parse(text: string) -> Result[Url, UrlError]` parses an absolute HTTP or HTTPS URL satisfying the ASCII-host rule in [URLs](#urls).
3. r[std-http.parsed-url.relative] Input without a scheme returns `UrlError.RelativeWithoutBase` with the original text.
4. r[std-http.parsed-url.invalid] Malformed absolute input, unsupported schemes, non-ASCII hosts or invalid ports return `UrlError.Invalid` with the original text.
5. r[std-http.parsed-url.user-info] An authority containing user-info is rejected as `UrlError.Invalid`.
6. r[std-http.parsed-url.controls] A literal ASCII control character, space or DEL in the input makes the URL invalid.
7. r[std-http.parsed-url.scheme-host-case] Parsing lowercases ASCII letters in the scheme and host.
8. r[std-http.parsed-url.port] A nonempty explicit port must be decimal text within the `u16` range; an absent or empty port becomes `None`.
9. r[std-http.parsed-url.path] Parsing percent-decodes the path as UTF-8 without interpreting encoded `/` or `?` as component separators.
10. r[std-http.parsed-url.query] Parsing splits nonempty query parts at `&` and their first `=`, retaining pair order and repeated names.
11. r[std-http.parsed-url.query-empty-value] A query part without `=` has an empty value.
12. r[std-http.parsed-url.query-decode] Query names and values percent-decode UTF-8 and interpret `+` as space.
13. r[std-http.parsed-url.fragment] Parsing percent-decodes the fragment as UTF-8, retaining an explicit empty fragment as `Some("")`.
14. r[std-http.parsed-url.invalid-escape] Invalid percent escapes or invalid decoded UTF-8 in the path, query or fragment make the input invalid.
15. r[std-http.parsed-url.raw-path] Display retains the private escaped path while decoding it gives the current public path.
16. r[std-http.parsed-url.changed-path] Otherwise Display percent-encodes the public path as UTF-8, preserving RFC 3986 path characters and escaping literal `%` and `?`.
17. r[std-http.parsed-url.raw-path-unicode] Literal non-ASCII path bytes are percent-encoded without changing existing valid escapes.
18. r[std-http.parsed-url.display-query] Display writes nonempty query pairs as encoded `name=value` parts joined by `&`, encoding each component's UTF-8 bytes separately.
19. r[std-http.parsed-url.display-fragment] Display writes a present fragment after `#`, percent-encoding its UTF-8 bytes while retaining permitted fragment characters.
20. r[std-http.parsed-url.empty-query] Parsing an explicit empty query retains a private presence bit, so an empty public list can still display a trailing `?`.
21. r[std-http.parsed-url.nonempty-query] A nonempty public query list overrides that presence bit in Display.
22. r[std-http.parsed-url.param] `param(&self, name: string) -> string?` returns the first value whose name equals `name`, or `None` when absent.
23. r[std-http.parsed-url.add-param] `add_param[T < Display](&mut self, name: string, value: T) -> void` appends the name and the value's Display text as decoded data.
24. r[std-http.parsed-url.add-param-empty-query] `add_param` clears the earlier empty-query presence bit.
25. r[std-http.parsed-url.resolve] `resolve(&self, reference: string) -> Result[Url, UrlError]` resolves a reference by RFC 3986 section 5, preserving encoded path separators and query presence.
26. r[std-http.parsed-url.traits] `Url` implements `Display`, structural `Eq` and `Debug`.

## Requests And Responses

Requests and responses use parsed fields and a shared read-once body:

```text
pub enum Method:
    Get
    Head
    Post
    Put
    Patch
    Delete
    Options
    Other(name: string)

pub struct Request:
    pub method: Method = .Get
    pub url: Url
    pub headers: Headers = Headers::new()
    pub body: Body = Body::empty()

pub struct Response:
    pub status: HttpStatus = .Ok
    pub headers: Headers = Headers::new()
    pub body: Body = Body::empty()
    pub url: Url? = .None
```

1. r[std-http.types.parsed-messages] `std.http` declares `Method`, `Request`, and `Response` as above.
2. r[std-http.types.traits] `Method`, `Request`, and `Response` implement `Eq` and `Debug`.
3. r[std-http.request.message-defaults] A request supplying only `url` has method `Get`, empty headers, and a fresh unread empty body.
4. r[std-http.request.default-timeout] A provider that sends requests to a host has a default timeout of 30 seconds.
5. r[std-http.headers.typed-fields] Request and response headers are `Headers` values.
6. r[std-http.headers.case] Header names compare ignoring ASCII case, so a provider may change the case of a response header's name.
7. r[std-http.headers.combined] A provider may combine repeated non-cookie response fields according to [RFC 9110 §5.3](https://www.rfc-editor.org/rfc/rfc9110.html#section-5.3).
8. r[std-http.headers.cookies-separate] A provider must deliver each `Set-Cookie` value as a separate pair, in received order.
9. r[std-http.headers.values-order] Repeated values of one response field retain their received order.
10. r[std-http.headers.platform-order] Response headers of different names may follow the platform's exposed order.
11. r[std-http.response.header-typed] `Response.header(&self, name: HeaderName) -> string?` delegates to `headers.get(name)`.
12. r[std-http.response.text-read-once] `Response.text(&self) -> Result[string, BodyError]` delegates to `body.text()`.
13. r[std-http.response.optional-url] A received response has `url = Some(final_url)`, while a response constructed without a URL has `None`.
14. r[std-http.method.eq.variant-payload] Two `Method` values compare equal only when their variants and corresponding payloads are equal.
15. r[std-http.types.message-copy] Copying a request or response copies its fields, sharing only the body's private consumption state.
16. r[std-http.response.message-defaults] An omitted response status is `Ok`, with empty headers, a fresh unread empty body, and no URL.
17. r[std-http.request.read-body] Request `bytes`, `text`, `json[T]`, and `form[T]` borrow `&self` and delegate to the corresponding body reader.
18. r[std-http.request.read-query] `Request.query_as[T < Deserialize](&self) -> Result[T, FormError]` delegates to its URL's query reader.
19. r[std-http.request.read-header] `Request.header(&self, name: HeaderName) -> string?` delegates to `headers.get(name)`.
20. r[std-http.request.read-cookie] `Request.cookie(&self, name: string) -> string?` delegates to `headers.request_cookie(name)`.
21. r[std-http.response.read-bytes] `Response.bytes(&self) -> Result[List[u8], BodyError]` delegates to `body.bytes()`.
22. r[std-http.response.read-cookies] `Response.cookies(&self) -> List[Cookie]` delegates to `headers.response_cookies()`.

## Request Builder

Message writers return changed values for chaining; readers borrow locally.

1. r[std-http.request.new-value] `Request::new(method: Method, url: Url) -> Request` returns a value with those fields and the other defaults.
2. r[std-http.request.get-value] `Request::get(url: Url) -> Request` returns `Request::new(.Get, url)`.
3. r[std-http.request.post-value] `Request::post(url: Url) -> Request` returns `Request::new(.Post, url)`.
4. r[std-http.message.writer-value] Every `set_*` message writer takes `mut self` and returns the changed message value.
5. r[std-http.message.writer-detach] Replacing a message's body creates a fresh unread body without changing copies of the earlier message.
6. r[std-http.message.set-json] Request and response `set_json[T < Serialize]` replace the body with UTF-8 bytes from `std.json.encode(value)`.
7. r[std-http.message.json-once] `set_json` serializes its value once.
8. r[std-http.message.json-content-type] `set_json` replaces `Content-Type` with `application/json`.
9. r[std-http.message.set-text] Request and response `set_text(text: string)` replace the body with the text's UTF-8 bytes.
10. r[std-http.message.text-content-type] `set_text` replaces `Content-Type` with `text/plain; charset=utf-8`.
11. r[std-http.message.set-bytes] Request and response `set_bytes(bytes: List[u8])` replace the body without changing headers.
12. r[std-http.message.set-header] Request and response `set_header(name: HeaderName, value: string)` delegate to `headers.set`.
13. r[std-http.request.set-form] `Request.set_form[T < Serialize]` prepares the body using [Typed Query And Form Values](#typed-query-and-form-values).
14. r[std-http.request.form-escaping] Form names and values encode UTF-8 bytes, keeping only ASCII letters, digits, `*`, `-`, `.`, and `_` unchanged.
15. r[std-http.request.form-space] Form encoding writes a space as `+` and each remaining escaped byte as `%` followed by two uppercase hexadecimal digits.
16. r[std-http.request.form-pairs] Form encoding joins each name and value with `=` and each pair with `&`.
17. r[std-http.request.form-content-type-replace] `set_form` replaces `Content-Type` with `application/x-www-form-urlencoded`.
18. r[std-http.request.form-preparation-error] A failed `set_form` retains its `FormError` in the new body instead of panicking.
19. r[std-http.request.form-error-read] Reading or sending that body returns `BodyError.Form(error)` through the corresponding result type.
20. r[std-http.request.set-query] `Request.set_query_as[T < Serialize]` delegates to `url.set_query_as`.
21. r[std-http.request.set-cookie] `Request.set_cookie(name: string, value: string)` delegates to `headers.set_request_cookie`.
22. r[std-http.response.status-constructor] `Response::status(code: HttpStatus) -> Response` supplies that status and the other defaults.
23. r[std-http.response.set-cookie] `Response.set_cookie(cookie: Cookie)` delegates to `headers.set_response_cookie`.
24. r[std-http.request.send-borrowed] `Request.send!(&self, redirects: usize = 10) -> Result[Response, HttpError] $ Http` validates the request, then calls its covering provider.
25. r[std-http.request.send-query-error] A pending query encoding failure returns `HttpError.Body(BodyError.Form(error))` before calling the provider.
26. r[std-http.request.send-body-claim] Sending claims the body once, so another send or reader through any message copy returns `BodyError.Used`.
27. r[std-http.request.send-validation-first] Header and URL validation failures leave the body unread.
28. r[std-http.send.zero-redirects-supported] With `redirects` zero, Node and Wasmtime providers return the first response without following or validating its `Location`.

> **Why.** Local borrows permit reading messages without copying fields.
> Copies share consumption, while a value writer detaches its replacement body.
> Typed query and form values use one codec and one error vocabulary.

The provider's default timeout still applies; `std.task.apply!` with `std.task.timeout` can impose a shorter bound around a send.

## Multipart Forms

`Multipart` builds and reads buffered `multipart/form-data` messages.
It reuses the read-once [`Body`](#read-once-bodies) and ordered [`Headers`](#header-collections) values.

### Multipart Values

1. r[std-http.multipart.part.declaration] `std.http` declares `pub struct MultipartPart` with public `name: string`, `filename: string? = .None`, `headers: Headers = Headers::new()`, and `body: Body` fields.
2. r[std-http.multipart.declaration] `std.http` declares `pub struct Multipart` with the public field `parts: List[MultipartPart]`.
3. r[std-http.multipart.traits] `MultipartPart`, `Multipart`, and `MultipartError` implement structural `Eq` and `Debug`.
4. r[std-http.multipart.new] `Multipart::new() -> Multipart` returns a value with no parts.
5. r[std-http.multipart.add] `add(self, part: MultipartPart) -> Multipart` appends `part` without changing its fields.
6. r[std-http.multipart.order] Parts retain insertion order, including parts whose names repeat.
7. r[std-http.multipart.text] `text(self, name: string, value: string) -> Multipart` appends one UTF-8 body with `Content-Type: text/plain; charset=utf-8`.
8. r[std-http.multipart.file] `file(self, name: string, filename: string, content_type: string, bytes: List[u8]) -> Multipart` appends one file part.
9. r[std-http.multipart.file-content] A file part keeps `filename`, writes `content_type` as `Content-Type`, and stores `bytes` in a fresh body.

```hd
form := Multipart::new()
    .text("caption", "launch day")
    .file("photo", "launch.png", "image/png", photo)
```

### Encoding

1. r[std-http.multipart.set] `Request.set_multipart(self, form: Multipart) -> Result[Request, MultipartError]` encodes `form` into a changed request.
2. r[std-http.multipart.set.claim] `set_multipart` claims each part body once, in part order.
3. r[std-http.multipart.set.used] An already claimed part body returns `MultipartError.UsedPart(name)`.
4. r[std-http.multipart.set.name] An empty name, or one containing carriage return, line feed, or NUL, returns `MultipartError.InvalidName(name)`.
5. r[std-http.multipart.set.filename] A filename containing carriage return, line feed, or NUL returns `MultipartError.InvalidFilename(filename)`.
6. r[std-http.multipart.set.disposition-owned] A supplied `Content-Disposition` part header returns `MultipartError.Header(HeaderError.InvalidValue("content-disposition"))`.
7. r[std-http.multipart.set.headers] Every other part header must be valid and is written unchanged in its original order.
8. r[std-http.multipart.set.disposition] The encoder writes one `Content-Disposition: form-data` header containing the part's name and optional filename.
9. r[std-http.multipart.set.quoted] A quotation mark or backslash in a disposition parameter is escaped with one backslash.
10. r[std-http.multipart.set.boundary] The encoder chooses a deterministic valid ASCII boundary from one through seventy bytes, with no trailing space.
11. r[std-http.multipart.set.collision] The chosen boundary differs from every delimiter line in every part body.
12. r[std-http.multipart.set.framing] Encoding follows RFC 7578 framing, using carriage-return-line-feed line endings and one closing delimiter.
13. r[std-http.multipart.set.replace] Success replaces the request body and `Content-Type`, while retaining every other request header.
14. r[std-http.multipart.set.content-type] The replacement Content-Type is `multipart/form-data; boundary=BOUNDARY` with the chosen boundary.

```hd
request := Request::post(url).set_multipart(form)?
```

### Decoding

1. r[std-http.multipart.read] `Request.multipart(&self) -> Result[Multipart, MultipartError]` claims and decodes the request body.
2. r[std-http.multipart.read.claim] Every result claims the request body, so a later body reader through any copy returns `BodyError.Used`.
3. r[std-http.multipart.read.content-type] An absent Content-Type returns `MissingContentType`, while another media type returns `WrongContentType(value)`.
4. r[std-http.multipart.read.boundary-required] A multipart Content-Type without a boundary parameter returns `MissingBoundary`.
5. r[std-http.multipart.read.boundary-valid] A boundary must be valid RFC 2046 text from one through seventy ASCII bytes, without trailing space.
6. r[std-http.multipart.read.boundary-quoted] A boundary parameter may be a quoted string with quoted-pair escapes.
7. r[std-http.multipart.read.order] Decoding returns parts in wire order and preserves repeated names.
8. r[std-http.multipart.read.disposition] Every part requires exactly one `Content-Disposition: form-data` header and one nonempty `name` parameter.
9. r[std-http.multipart.read.filename] An optional filename is returned exactly as sent and is untrusted display metadata, not a filesystem path.
10. r[std-http.multipart.read.headers] Other valid part headers retain their wire order in the returned `Headers` value.
11. r[std-http.multipart.read.body] Each payload becomes a fresh unread `Body`; copied part bodies share that body's consumption state.
12. r[std-http.multipart.read.binary] A payload may contain any bytes, including malformed UTF-8.
13. r[std-http.multipart.read.single-level] A nested multipart Content-Type remains an opaque part body and is not decoded recursively.
14. r[std-http.multipart.read.framing] Missing delimiters, unfinished headers, invalid header UTF-8, and trailing malformed framing are errors.

```hd
for part in request.multipart()?.parts:
    println("${part.name}: ${part.body.bytes()?.len()}")
```

### Multipart Errors

1. r[std-http.multipart.error.declaration] `std.http` declares the error enum `MultipartError` with the variants below.
2. r[std-http.multipart.error.body] `Body(error: BodyError)` displays `multipart body: ERROR` and retains the body error.
3. r[std-http.multipart.error.missing-content-type] `MissingContentType` displays `missing multipart Content-Type`.
4. r[std-http.multipart.error.wrong-content-type] `WrongContentType(value)` displays `expected multipart/form-data, got VALUE`.
5. r[std-http.multipart.error.missing-boundary] `MissingBoundary` displays `missing multipart boundary`.
6. r[std-http.multipart.error.invalid-boundary] `InvalidBoundary(value)` displays `invalid multipart boundary VALUE`.
7. r[std-http.multipart.error.invalid-name] `InvalidName(name)` displays `invalid multipart part name NAME`.
8. r[std-http.multipart.error.invalid-filename] `InvalidFilename(filename)` displays `invalid multipart filename FILENAME`.
9. r[std-http.multipart.error.header] `Header(error: HeaderError)` displays `invalid multipart header: ERROR` and retains the header error.
10. r[std-http.multipart.error.malformed] `Malformed(position: usize)` displays `malformed multipart body at byte POSITION`.
11. r[std-http.multipart.error.used-part] `UsedPart(name)` displays `multipart part NAME was already read`.

```hd
match request.multipart():
    .Err(.MissingBoundary) => println("multipart boundary required")
    _ => pass
```

> **Why.** Buffering matches `Request.body` today and avoids adding a streaming resource lifetime.
> Typed names own `Content-Disposition`, so a custom header cannot disagree with them.
> Filenames remain untrusted because sender text is not local path authority.

## Client

A client owns a serializable cookie jar and follows redirects through its
covering provider.

1. r[std-http.client.jar-value] `std.http` exports the value struct `Client`.
2. r[std-http.client.jar-field] `Client` has the public field `jar: CookieJar`, defaulting to `CookieJar::new()`.
3. r[std-http.client.new-value] `Client::new() -> Client` returns a value with an empty jar.
4. r[std-http.client.with-jar] `Client::with_jar(jar: CookieJar) -> Client` returns a value containing that jar.
5. r[std-http.client.send-borrowed-clock] `Client.send!(&mut self, request: &Request, redirects: usize = 10) -> Result[Response, HttpError] $ Http + Clock` sends through `Http`.
6. r[std-http.client.clock-once] A send passing validation reads `Clock.now()` once, using that timestamp for every cookie lookup and response store across redirects.
7. r[std-http.client.body-claim] Sending claims the supplied request's body once and prepares an independent body for each provider request.
8. r[std-http.client.fields-retained] Sending leaves request fields unchanged, while its shared body becomes used.
9. r[std-http.client.unchecked-final-response] Sending returns the final response without checking its status, or propagates a provider error unchanged.
10. r[std-http.client.policy-timeout] `Client` has no timeout setter; a caller applies `std.task.timeout` around its Result-returning send through `std.task.apply!`.
11. r[std-http.client.jar-fill] Before each request, sending fills an absent `Cookie` header with eligible jar pairs joined by `; ` in jar order.
12. r[std-http.client.jar-store] Every response updates the jar before following another redirect, including responses returned without following.
13. r[std-http.client.cookies.explicit] An explicit `Cookie` header takes precedence over the jar for that request without replacing stored cookies.
14. r[std-http.client.redirects.zero-provider] The client passes `redirects = 0` to each provider call and follows redirects itself.
15. r[std-http.client.redirects.status] Responses with status 301, 302, 303, 307 or 308 and a `Location` header are redirects.
16. r[std-http.client.redirects.zero-argument] A zero `redirects` argument returns the first response without following its `Location`.
17. r[std-http.client.redirects.method] A 303 changes the method to `Get`; a 301 or 302 changes `Post` to `Get` and preserves other methods.
18. r[std-http.client.redirects.body] Changing a method to `Get` removes its body and its `Content-Type`, `Content-Length` and `Transfer-Encoding` headers.
19. r[std-http.client.redirects.resolve] A redirect's `Location` resolves against the current request URL as an RFC 3986 reference.
20. r[std-http.client.redirects.cross-host] A redirect to another hostname removes the previous request's `Authorization` and `Cookie` headers before applying the target host's jar.
21. r[std-http.client.redirects.argument-limit] Exceeding a positive redirect argument returns `HttpError.TooManyRedirects(url)`, naming the next target without sending to it.
22. r[std-http.client.redirects.no-location] A response without `Location` is returned without following another request.
23. r[std-http.client.redirects.parsed-response-url] The returned response's `url` is `Some(final_request_url)`.
24. r[std-http.client.no-default-builders] `Client` has no base-URL, default-header, or cookie-lookup builder methods; callers use `Url`, message headers, and `jar` directly.

> **Why.** Reading every response retains cookies set during redirects.
> Plain `Request.send!` needs only `Http`; a cookie-aware client also declares clock authority.

### Cookie Jar

`CookieJar` is plain data; each time-dependent operation takes its timestamp
explicitly, following [RFC 6265 sections 5.1–5.4](https://www.rfc-editor.org/rfc/rfc6265#section-5.1).

1. r[std-http.jar.declaration] `CookieJar` has private storage and implements `Debug`, `Eq`, `Serialize`, and `Deserialize`.
2. r[std-http.jar.new] `CookieJar::new() -> CookieJar` returns an empty jar.
3. r[std-http.jar.explicit-time] A jar retains no clock and reads no implicit time.
4. r[std-http.jar.cookies-for] `cookies_for(&self, url: &Url, now: Timestamp) -> List[(string, string)]` returns the name/value pairs eligible at that time.
5. r[std-http.jar.store] `store(&mut self, url: &Url, response: &Response, now: Timestamp) -> void` processes response cookies in header order.
6. r[std-http.jar.clear-expired] `clear_expired(&mut self, now: Timestamp) -> void` removes cookies whose expiry is at or before `now`.
7. r[std-http.jar.host-only] Without a Domain attribute, a cookie belongs only to the response request's ASCII-lowercase hostname, independently of its port.
8. r[std-http.jar.domain-match] A Domain attribute must match the response hostname exactly or as a suffix preceded by a dot.
9. r[std-http.jar.domain-ip] An IP address matches a Domain attribute only exactly.
10. r[std-http.jar.domain-reject] A cookie whose Domain does not match the response request's hostname is ignored.
11. r[std-http.jar.domain-send] A domain cookie is eligible for its domain and matching subdomains; a host-only cookie requires an exact hostname.
12. r[std-http.jar.default-path] An absent or invalid Path uses the escaped request URI's directory, or `/` when no directory exists.
13. r[std-http.jar.path-match] A request path matches exactly, or extends a cookie path ending with `/` or followed by `/`.
14. r[std-http.jar.escaped-path] Path matching uses escaped URI paths, so an encoded slash does not create a directory boundary.
15. r[std-http.jar.secure] A Secure cookie is eligible only for `https` requests.
16. r[std-http.jar.http-only] HttpOnly does not exclude cookies from these HTTP request operations.
17. r[std-http.jar.expiry-max-age] A valid Max-Age sets expiry relative to `now`, taking precedence over Expires.
18. r[std-http.jar.expiry-expires] Without Max-Age, a valid Expires sets absolute expiry; without either, the cookie has no expiry.
19. r[std-http.jar.expired-ineligible] A cookie expiring at or before `now` is ineligible and removes an existing cookie with the same name, domain, and path.
20. r[std-http.jar.identity] A cookie replaces the stored cookie with the same name, domain, and path, preserving its position among equally long paths.
21. r[std-http.jar.order] Eligible cookies are ordered by decreasing path length, then original creation order.
22. r[std-http.jar.duplicate-names] Different scopes may supply repeated cookie names in the outgoing pairs.
23. r[std-http.jar.serde-state] Serde retains cookie values, scopes, Secure and HttpOnly flags, and absolute expiry times without retaining a clock.
24. r[std-http.jar.serde-expiry-milliseconds] Private expiry storage serializes absolute Unix milliseconds as an optional `i64`, without a calendar-year cap.
25. r[std-http.jar.expiry-beyond-range] A positive Max-Age whose deadline exceeds every representable timestamp cannot expire at a representable timestamp.
26. r[std-http.jar.empty-request-path] An empty request URI path is treated as `/` for cookie matching.


## URLs

A request URL is an RFC 3986 URL, so its host is plain ASCII:

```hd
use std.http.{Http, HttpError, Response, get}

fn shop!() -> Result[Response, HttpError] $ Http:
    _ := get!("https://bücher.example/")         # .Err(HttpError.InvalidUrl(url))
    get!("https://xn--bcher-kva.example/")       # the same host in punycode
```

1. r[std-http.url.valid] A request URL is valid when it is an absolute `http` or `https` URL by RFC 3986, with an ASCII host.
2. r[std-http.url.non-ascii] A URL whose host holds a non-ASCII character is invalid, so `send!` returns `.Err(HttpError.InvalidUrl(url))` for it.
3. r[std-http.url.grant] A grant matches a request by its URL host, in lowercase ASCII.
4. r[std-http.url.no-normalize] A provider maps the host no other way: no IDNA mapping and no lookalike normalization.

> **Why.** An ASCII-only host leaves one spelling for each host, so a
> grant and the request it covers always compare the same text.

## Http Errors

Every failure of a request is one error enum:

```text
pub enum HttpError:
    NotGranted(host: string)
    InvalidUrl(url: string)
    Dns(host: string)
    Connect(message: string)
    Tls(message: string)
    Timeout
    TooManyRedirects(url: string)
    Other(message: string)
    Status(response: Response)
    InvalidHeader(name: string)
    Body(error: BodyError)
```

1. r[std-http.error.decl-message-errors] `std.http` declares `HttpError` as above. Code imports it, as in `use std.http.HttpError`.
2. r[std-http.error.traits] `HttpError` implements `Eq`, `Debug`, `Display`, and `std.error.Error`.

| Rule | Variant | Means |
| --- | --- | --- |
| r[std-http.error.not-granted] Not granted | `NotGranted(host)` | the program's capability grant does not cover the request's host, by [Partial Deny](../cli/command-line.md#partial-deny) |
| r[std-http.error.invalid-url-target] Invalid URL | `InvalidUrl(url)` | Invalid HTTP target, including a URL rejected by `Url::parse` |
| r[std-http.error.dns] DNS | `Dns(host)` | the host name did not resolve |
| r[std-http.error.connect] Connect | `Connect(message)` | no connection was made, or it failed before the whole response arrived |
| r[std-http.error.tls] TLS | `Tls(message)` | the TLS handshake or a certificate check failed |
| r[std-http.error.timeout] Timeout | `Timeout` | the request took longer than its timeout |
| r[std-http.error.redirect-argument-limit] Redirects | `TooManyRedirects(url)` | the response exceeded a positive `send!` redirect limit; `url` is the last target |
| r[std-http.error.other] Other | `Other(message)` | any other failure, with the provider's message |
| r[std-http.error.status-response-value] Status | `Status(response)` | `Response.ok` found a status from 400 to 599, retaining the whole response |

3. r[std-http.error.not-granted.plain] The `Display` text of `NotGranted(host)` is `http access to HOST is not granted`, with the host for `HOST`.
4. r[std-http.error.status.safe-response-text] `Status(response)` displays `HTTP status STATUS for URL`, with the response status and its URL before any query or fragment.
5. r[std-http.error.status.no-response-secrets] The status error's `Display` text contains no response body or header values.
6. r[std-http.error.transient-borrowed] `HttpError.is_transient(&self) -> bool` returns true for `Timeout` and `Connect` errors.
7. r[std-http.error.transient-status-codes] A `Status` error is transient for 408, 429, 500, 502, 503, and 504.
8. r[std-http.error.permanent-kinds] `is_transient` returns false for every other error.
9. r[std-http.error.invalid-url-display] `InvalidUrl(url)` displays `invalid URL: URL` unless the input contains user-info.
10. r[std-http.error.invalid-url-user-info] User-info errors instead omit credentials from Display and recommend the Authorization header, including errors converted from `UrlError`.
11. r[std-http.error.invalid-header-display] `InvalidHeader(name)` displays `invalid header NAME`, using its header name.
12. r[std-http.error.body-display] `Body(error)` displays the body error's Display text.
13. r[std-http.error.from-body] `From[BodyError] for HttpError` wraps its input in `HttpError.Body`.

## Sending

`Http` sends one request and completes with the whole response, with status and body available separately:

```text
use std.http.{Http, HttpError, get}

fn latest_release!(repo: string) -> Result[string, HttpError] $ Http:
    response := get!("https://api.github.com/repos/${repo}/releases/latest")?
    if response.status != .Ok:
        return .Err(.Other("status ${response.status.code()}"))
    .Ok(response.text()?)
```

1. r[std-http.trait.decl-borrowed-request] `std.http` declares `Http` with `fn send!(&mut self, request: &Request, redirects: usize) -> Result[Response, HttpError]`. Code imports it, as in `use std.http.Http`.
2. r[std-http.send.borrowed-request] `send!(request, redirects)` sends the borrowed request and completes with the whole response.
3. r[std-http.send.status] Every status, 404 and 500 included, is an `.Ok` response, and the caller reads it from `status`.
4. r[std-http.send.redirect-argument] `send!` follows at most its `redirects` argument redirects.
5. r[std-http.send.provider-timeout] A host request exceeding the provider's default timeout completes with `.Err(HttpError.Timeout)`.
6. r[std-http.send.timeout.whole] A timeout covers the whole request, every redirect included.
7. r[std-http.send.cancel] Cancelling the suspension of `send!` aborts the request, by [`req.cancel.external-abort`](../lang/11-requirements-and-suspension.md#r-req.cancel.external-abort).
8. r[std-http.send.cancel.close] Aborting a request closes its connection.
9. r[std-http.send.mut-ref.reference-values] `send!` takes `&mut self`, so `Http` is a mutable requirement trait, `$.use(Http)` is a `&mut dyn Http` by [`req.ref.use.reference-values`](../lang/11-requirements-and-suspension.md#r-req.ref.use.reference-values), and a provider may record what it sends.
10. r[std-http.bodies.read-once-buffer] A body buffers a whole byte sequence behind read-once state; `std.http` has no streaming transport body.
11. r[std-http.helper.get.parsed-response] `get!(url: string) -> Result[Response, HttpError] $ Http` parses the URL, sends a GET, and checks `ok()`.
12. r[std-http.helper.get-post] The free request helpers are `get!` and `post!`; free `send!`, `get_text!`, and `post_json!` are not exported.

13. r[std-http.helper.post-json-checked] `post![T < Serialize](url: string, value: T) -> Result[Response, HttpError] $ Http` parses the URL, sends JSON, and checks `ok()`.
14. r[std-http.send.header-validation] Sending rejects invalid header names and CR or LF in header values as `HttpError.InvalidHeader(name)` before transport.
15. r[std-http.send.cookie-validation] Sending rejects invalid typed cookie components as `HttpError.InvalidHeader(name)` before transport.
16. r[std-http.send.mutated-url] Sending revalidates a URL's public fields, rejecting invalid HTTP targets as `HttpError.InvalidUrl(url)`.

> **Why.** A status is data about a request that worked, as with `fetch`
> and Go, and as a non-zero exit status is for `Process`. Header pairs
> keep a repeated header, which a map would lose.

## Status And JSON

`Response.ok()` checks 400–599 while preserving the whole response in the
error; the free `get!` calls it automatically:

```text
use std.error.Error
use std.http.{Http, HttpError, Request, Url, get}
use std.serde.{Deserialize, Serialize}

@derive(Serialize)
struct NewUser:
    name: string

@derive(Deserialize)
struct Created:
    id: i64

fn notes!() -> Result[string, HttpError] $ Http:
    .Ok(get!("https://example.com/notes.txt")?.text()?)   # a 404 is .Err(HttpError.Status(response))

fn create!(name: string) -> Result[Created, *dyn Error] $ Http:
    request := Request::post(Url::parse("https://example.com/users")?).set_json(NewUser { name: name })
    .Ok(request.send!()?.ok()?.json::[Created]()?)              # a bad body is a BodyError.Json
```

1. r[std-http.response.ok.borrowed-retained] `Response.ok(&self) -> Result[Response, HttpError]` copies the whole response into `Err(Status(response))` for codes 400–599, or `Ok(response)` otherwise.
2. r[std-http.response.json-read-once] `Response.json[T < Deserialize](&self) -> Result[T, BodyError]` delegates to `body.json[T]` without checking status.
3. r[std-http.response.body-errors-propagate] A function that sends and decodes propagates both errors with `?` into an error type covering both, such as `*dyn Error` ([`expr.try.convert`](../lang/05-expressions.md#r-expr.try.convert)).

> **Why.** Builder and provider `send!` calls keep every status available
> ([`std-http.send.status`](#r-std-http.send.status)). `Response.ok()` checks
> explicitly, as reqwest's `error_for_status` does; the free `get!` checks by default.
> Retaining the response lets a caller decode an API's error body.

## Scripted Http

`ScriptedHttp` is the deterministic `Http` provider. It consumes replies queued
for each URL, and records every request:

```text
use std.http.{Http, Response, ScriptedHttp, get}
fn status!(url: string) -> u16 $ Http:
    match get!(url):
        .Ok(response) => response.status.code()
        .Err(_) => 0

tests:
    use std.testing.assert_equal

    it("reads the scripted status"):
        http := ScriptedHttp::new()
        http.reply("https://example.com/", .Ok(Response::status(.NoContent)))
        $.with(Http=http):
            assert_equal(status!("https://example.com/"), 204, reason="scripted")
        assert_equal(http.received().len(), 1, reason="one request")
```

1. r[std-http.scripted.decl] `std.http` declares `ScriptedHttp`, which implements `Http` and `Debug`, with private fields. Code imports it, as in `use std.http.ScriptedHttp`.
2. r[std-http.scripted.new-empty] `ScriptedHttp::new() -> *mut ScriptedHttp` returns a new provider with no replies and no received requests.
3. r[std-http.scripted.reply-captured] `reply(&mut self, url: string, result: Result[Response, HttpError]) -> void` appends a result for that URL.
4. r[std-http.scripted.key-url-display] Replies match the complete request URL Display text exactly, whatever the request method.
5. r[std-http.scripted.queue-order] Sends consume queued results in insertion order, independently for each URL.
6. r[std-http.scripted.repeat-last] After a URL's queued results have been consumed, further sends repeat its last result.
7. r[std-http.scripted.error-values] A queued error retains its value, except that `Status(response)` gets a fresh response body for each delivery.
8. r[std-http.scripted.raw-status] Every status of a queued successful response is returned as `.Ok`, without a status check.
9. r[std-http.scripted.missing-reply] A URL without a reply returns `.Err(HttpError.Connect("ScriptedHttp has no response for URL"))`, substituting its full URL.
10. r[std-http.scripted.received-bodies] `received(&self) -> List[Request]` returns validated requests in order with independent unread body snapshots, including unanswered requests.
11. r[std-http.scripted.no-host] A `ScriptedHttp` opens no connection.
12. r[std-http.scripted.reply-body-snapshot] Registering a response copies its body payload into the reply queue without marking the caller's body used.
13. r[std-http.scripted.delivery-body-fresh] Every scripted response delivery, including a repeated last reply, receives a fresh unread `Body`.
14. r[std-http.scripted.reply-used-panic] Registering a used body panics with `ScriptedHttp.reply: the response body was already read`. Panic: `explicit-panic`.
15. r[std-http.scripted.status-body-snapshot] Those registration and delivery rules also apply to the response held by a queued `HttpError.Status`.

> **Why.** An unknown URL is an error that names it, not a 404. So a
> route that a test forgot never hides behind a valid status.

## Playground

The playground runs a program in a browser. Its `Http` provider calls the
browser's HTTP facilities:

```text
use std.http.{Http, HttpError, get}

fn ping!(url: string) -> string $ Http:
    match get!(url):
        .Ok(response) => "status ${response.status.code()}"
        .Err(.NotGranted(host)) => "the playground grants no ${host}"  # another origin
        .Err(.Connect(_)) => "the browser refused the request"         # a network failure or CORS
        .Err(error) => "${error}"
```

1. r[std-http.playground.origin] The playground binds `Http` with a grant that covers only its own origin, as the browser's same-origin rule does.
2. r[std-http.playground.other-origin] A request to another origin returns `.Err(HttpError.NotGranted(host))`.
3. r[std-http.playground.refused] A request that the browser refuses, for a network failure or by CORS, returns `.Err(HttpError.Connect(message))`.
4. r[std-http.playground.unbound] The playground binds no `Process`, `Sys`, or `Net` provider.
5. r[std-http.playground.zero-redirects-error] A granted browser request with `redirects: 0` returns `.Err(HttpError.Other("the browser host cannot stop redirects"))` before sending.

> **Why.** A browser can't tell a CORS refusal from a network failure, so
> neither can the provider. The playground needs no grant table: the
> browser's own rule decides.

### Waiting In The Playground

A [`block_on`](../lang/11-requirements-and-suspension.md#r-req.drive.block-on.signature)
call in the playground may have to wait for the host, as for a response
to `get!`. How it waits depends on the browser:

| Browser | A `block_on` that waits on the host |
| --- | --- |
| has JavaScript Promise Integration | waits for the host operation, as on any other host |
| lacks it | completes an HTTP request as a synchronous same-origin request, and panics for any other host operation |

1. r[std-http.playground.block-on.jspi] In a browser with JavaScript Promise Integration, a `block_on` call in the playground waits for each host operation it needs.
2. r[std-http.playground.block-on.sync] In a browser without it, an `Http` request that a `block_on` call waits on is made as a synchronous same-origin request.
3. r[std-http.playground.block-on.other] In such a browser, a `block_on` call that must wait on any other host operation panics, with a message that names the operation. Panic: `host-contract`.

```hd
use std.http.{Http, get}
use std.task.block_on

fn status_of(url: string) -> string $ Http:
    match block_on(get(url)):   # without Promise Integration: a synchronous request
        .Completed(.Ok(response)) => "status ${response.status.code()}"
        .Completed(.Err(error)) => "${error}"
        .Cancelled => "cancelled"
        .Abandoned => "cut short"
```

> **Why.** A browser thread cannot block, so only Promise Integration
> lets Wasm wait for a Promise. Without it, a synchronous request is the
> one wait a browser still offers, and only for HTTP.

See also: [Capability Grants](../cli/command-line.md#capability-grants),
[Process](process.md), [Time](time.md#duration).


## Timeout Conversion

1. r[std-http.error.from-timed-out] `HttpError` implements `std.convert.From[std.task.TimedOut]`, converting every deadline error to `.Timeout`.
