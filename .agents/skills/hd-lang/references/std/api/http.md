# `std.http`

Generated from `hd doc std.http`. Run `hd doc std.http.NAME` in a package for one item's full text.

## `HttpStatus`

```text
pub enum HttpStatus
```

A standard HTTP status or another numeric code.

- `pub fn code(&self) -> u16`
- `pub fn from_code(code: u16) -> HttpStatus`
- `pub fn is_success(&self) -> bool`

## `HeaderName`

```text
pub enum HeaderName
```

A header name; equality and hashing use its lowercase wire name.

- `pub fn wire_name(&self) -> string`

## `HeaderError`

```text
pub enum HeaderError
```

## `Body`

```text
pub struct Body
```

A shared read-once message body. Copying the value shares consumption.

- `pub fn from_bytes(bytes: List[u8]) -> Body`
- `pub fn bytes(&self) -> Result[List[u8], BodyError]`
- `pub fn text(&self) -> Result[string, BodyError]`
- `pub fn json[T < Deserialize](&self) -> Result[T, BodyError]`
- `pub fn form[T < Deserialize](&self) -> Result[T, BodyError]`

## `BodyError`

```text
pub enum BodyError
```

A typed message-body read failure.

## `FormError`

```text
pub enum FormError
```

A typed query or form conversion failure.

## `UrlError`

```text
pub enum UrlError
```

## `Headers`

```text
pub struct Headers
```

Ordered header pairs. Repeated names retain their individual values.

- `pub fn new() -> Headers`
- `pub fn get(&self, name: HeaderName) -> string?`: Joins repeated values, except Set-Cookie, whose first line is returned.
- `pub fn set(&mut self, name: HeaderName, value: string) -> void`: Replaces all values for this name. Wire validation happens at send.
- `pub fn remove(&mut self, name: HeaderName) -> void`
- `pub fn entries(&self) -> List[(HeaderName, string)]`
- `pub fn request_cookie(&self, name: string) -> string?`: The first value for this request cookie name.
- `pub fn request_cookies(&self) -> List[(string, string)]`: Cookie header pairs in received order, including repeated names.
- `pub fn set_request_cookie(&mut self, name: string, value: string) -> void`: Merges request cookies into one Cookie field, replacing this name.
- `pub fn response_cookies(&self) -> List[Cookie]`: Parses all Set-Cookie lines without combining them.
- `pub fn set_response_cookie(&mut self, cookie: Cookie) -> void`: Replaces earlier Set-Cookie lines for exactly this cookie name.

## `MultipartPart`

```text
pub struct MultipartPart
```

One named part of a buffered multipart/form-data message.

## `Multipart`

```text
pub struct Multipart
```

Ordered buffered multipart/form-data parts.

- `pub fn new() -> Multipart`
- `pub fn add(mut self, part: MultipartPart) -> Multipart`: Adds an arbitrary part without changing its headers or body.
- `pub fn text(mut self, name: string, value: string) -> Multipart`: Adds one UTF-8 text field.
- `pub fn file( mut self, name: string, filename: string, content_type: string, bytes: List[u8], ) -> Multipart`: Adds one in-memory file field. The caller supplies its media type.

## `MultipartError`

```text
pub enum MultipartError
```

Why a buffered multipart message could not be built or read.

## `SameSite`

```text
pub enum SameSite
```

The SameSite policy of a response cookie.

## `Cookie`

```text
pub struct Cookie
```

A response cookie. Outgoing defaults are secure; parsed defaults reflect the wire.

- `pub fn expired(name: string) -> Cookie`

## `Url`

```text
pub struct Url
```

Parsed absolute HTTP URL. Public components are values, not references.

- `pub fn parse(text: string) -> Result[Url, UrlError]`
- `pub fn resolve(&self, reference: string) -> Result[Url, UrlError]`
- `pub fn query_as[T < Deserialize](&self) -> Result[T, FormError]`
- `pub fn set_query_as[T < Serialize](&mut self, value: T) -> void`
- `pub fn param(&self, name: string) -> string?`
- `pub fn add_param[T < Display](&mut self, name: string, value: T) -> void`

## `Method`

```text
pub enum Method
```

An HTTP method: a standard one, or `Other(name)`.

## `Request`

```text
pub struct Request
```

A request used for both incoming and outgoing messages.

- `pub fn new(method: Method, url: Url) -> Request`
- `pub fn get(url: Url) -> Request`
- `pub fn post(url: Url) -> Request`
- `pub fn set_json[T < Serialize](mut self, value: T) -> Request`
- `pub fn set_text(mut self, text: string) -> Request`
- `pub fn set_bytes(mut self, bytes: List[u8]) -> Request`
- `pub fn set_form[T < Serialize](mut self, value: T) -> Request`
- `pub fn set_multipart(mut self, form: Multipart) -> Result[Request, MultipartError]`: Encodes ordered multipart parts once, replacing this request's body and Content-Type.
- `pub fn set_query_as[T < Serialize](mut self, value: T) -> Request`
- `pub fn set_header(mut self, name: HeaderName, value: string) -> Request`
- `pub fn set_cookie(mut self, name: string, value: string) -> Request`
- `pub fn json[T < Deserialize](&self) -> Result[T, BodyError]`
- `pub fn text(&self) -> Result[string, BodyError]`
- `pub fn bytes(&self) -> Result[List[u8], BodyError]`
- `pub fn form[T < Deserialize](&self) -> Result[T, BodyError]`
- `pub fn multipart(&self) -> Result[Multipart, MultipartError]`: Claims this request body and parses its multipart/form-data parts.
- `pub fn query_as[T < Deserialize](&self) -> Result[T, FormError]`
- `pub fn header(&self, name: HeaderName) -> string?`
- `pub fn cookie(&self, name: string) -> string?`
- `pub fn send!(&self, redirects: usize = 10) -> Result[Response, HttpError] $ Http`

## `CookieJar`

```text
pub struct CookieJar
```

Plain cookie data; time is supplied explicitly rather than read implicitly.

- `pub fn new() -> CookieJar`
- `pub fn cookies_for(&self, url: &Url, now: Timestamp) -> List[(string, string)]`
- `pub fn store(&mut self, url: &Url, response: &Response, now: Timestamp) -> void`
- `pub fn clear_expired(&mut self, now: Timestamp) -> void`

## `Client`

```text
pub struct Client
```

Sends through Http with one serializable jar and one Clock read per send.

- `pub fn new() -> Client`
- `pub fn with_jar(jar: CookieJar) -> Client`
- `pub fn send!( &mut self, request: &Request, redirects: usize = 10, ) -> Result[Response, HttpError] $ Http + Clock`

## `Response`

```text
pub struct Response
```

A response used for both incoming and outgoing messages.

- `pub fn status(code: HttpStatus) -> Response`
- `pub fn set_json[T < Serialize](mut self, value: T) -> Response`
- `pub fn set_text(mut self, text: string) -> Response`
- `pub fn set_bytes(mut self, bytes: List[u8]) -> Response`
- `pub fn set_header(mut self, name: HeaderName, value: string) -> Response`
- `pub fn set_cookie(mut self, cookie: Cookie) -> Response`
- `pub fn json[T < Deserialize](&self) -> Result[T, BodyError]`
- `pub fn text(&self) -> Result[string, BodyError]`
- `pub fn bytes(&self) -> Result[List[u8], BodyError]`
- `pub fn header(&self, name: HeaderName) -> string?`
- `pub fn cookies(&self) -> List[Cookie]`
- `pub fn ok(&self) -> Result[Response, HttpError]`

## `HttpError`

```text
pub enum HttpError
```

Why a request failed, including transport, content-decoding and response-size failures.

- `pub fn is_transient(&self) -> bool`: Whether a timeout, connection failure or retryable status may succeed on retry.

## `DnsResolver`

```text
pub struct DnsResolver
```

A validated list of literal DNS server addresses for a derived host provider.

## `dns_servers`

```text
pub fn dns_servers(servers: List[string]) -> Result[DnsResolver, HttpConfigError]
```

Validates each server as a literal IPv4 or IPv6 address.

## `Certificate`

```text
pub struct Certificate
```

One PEM certificate that extends the host's trusted roots.

- `pub fn from_pem(pem: string) -> Result[Certificate, HttpConfigError]`: Checks that the text is exactly one PEM `CERTIFICATE` block.

## `Identity`

```text
pub struct Identity
```

A PEM client certificate chain and its PEM private key, for mutual TLS.

- `pub fn from_pem( certificate_chain: string, private_key: string, ) -> Result[Identity, HttpConfigError]`: Checks a PEM certificate chain and one PEM private key.

## `HttpConfigError`

```text
pub enum HttpConfigError
```

Why a transport setting could not be constructed.

## `Http`

```text
pub trait Http
```

The host capability to send an HTTP request and complete with the whole response.

- `fn resolver(&self, _resolver: DnsResolver) -> *mut dyn Http`: A new provider that resolves host names with these DNS servers.
- `fn tls(&self, roots: List[Certificate] = [], identity: Identity? = .None) -> *mut dyn Http`: A new provider whose TLS adds these trusted roots and presents this client identity.
- `fn connect_timeout(&self, _limit: Duration) -> *mut dyn Http`: A new provider that bounds DNS lookup and TCP connection on each redirect hop.
- `fn tls_handshake_timeout(&self, _limit: Duration) -> *mut dyn Http`: A new provider that bounds the TLS handshake on each HTTPS redirect hop.
- `fn response_header_timeout(&self, _limit: Duration) -> *mut dyn Http`: A new provider that bounds the wait for complete response headers on each redirect hop.
- `fn body_read_timeout(&self, _limit: Duration) -> *mut dyn Http`: A new provider that bounds reading the whole response body on each redirect hop.

## `request_from_wire`

```text
pub fn request_from_wire(request: WireRequest) -> Result[Request, HttpError]
```

Host adapter: builds a public incoming message from its byte DTO.

## `response_to_wire`

```text
pub fn response_to_wire(response: &Response) -> Result[WireResponse, HttpError]
```

Host adapter: validates fields, then claims the outgoing body once.

## `get`

```text
pub fn get!(url: string) -> Result[Response, HttpError] $ Http
```

Sends a GET and checks its response for a 400–599 failure.

## `post`

```text
pub fn post![T < Serialize](url: string, value: T) -> Result[Response, HttpError] $ Http
```

## `ScriptedHttp`

```text
pub struct ScriptedHttp
```

A deterministic Http provider with queued replies and a log of received requests.

- `pub fn new() -> *mut ScriptedHttp`: Returns a new provider with no replies or received requests.
- `pub fn reply(&mut self, url: string, result: Result[Response, HttpError]) -> void`: Appends a result to this URL's reply queue.
- `pub fn received(&self) -> List[Request]`: The received requests in order, including requests without a reply.

## `WireIdentity`

```text
pub struct WireIdentity
```

A wire request's PEM certificate chain and PEM private key.

## `WireRequest`

```text
pub struct WireRequest
```

The request bytes and transport settings exchanged with a host.

## `WireResponse`

```text
pub struct WireResponse
```

The response bytes exchanged with a host.

## `WireHttpError`

```text
pub enum WireHttpError
```

A host transport failure, including its typed payload.

## `WireHttp`

```text
pub trait WireHttp
```

The host-supplied HTTP transport over wire request and response values.
