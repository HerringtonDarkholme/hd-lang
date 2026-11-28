# Call An HTTP API

**Scenario.** Fetch a JSON resource, treat "not found" as an ordinary
answer, post a JSON body with a header, and test all of it without a
network.

**Derive the payloads.** `@derive(Serialize, Deserialize)` on each body
type; `response.json::[T]()` decodes one and `set_json(value)` writes one.
Don't walk a `Json` tree by hand.

## The program

```hd
use std.http.{BodyError, Http, HttpError, Request, Response, ScriptedHttp, get}
use std.url.{Url, UrlError}
use std.encoding.json.{decode, encode}
use std.encoding.{Deserialize, Serialize}
use std.testing.Arbitrary

@derive(Serialize, Deserialize, Arbitrary, Eq, Debug)
struct Repo:
    name: string
    stars: u32 = 0                  # a default: the field may be missing in the JSON

@derive(Serialize)
struct NewIssue:
    title: string

@error
pub enum ApiError:
    @error("request failed")
    Transport(@from error: HttpError)
    @error("bad address")
    Address(@from error: UrlError)
    @error("unexpected response body: $error")
    Body(@from error: BodyError)

# GET: `get!` returns `.Err(.Status(response))` for a 4xx or 5xx status.
fn repo!(name: string) -> Result[Repo?, ApiError] $ Http:
    response := match get!("https://api.example.com/repos/$name"):
        .Ok(response) => response
        .Err(.Status(response)) if response.status.code() == 404 => return .Ok(.None)
        .Err(error) => return .Err(.Transport(error))
    .Ok(.Some(response.json::[Repo]()?))     # decode the body into the derived type

# POST: build a `Request`, then `send!`. `send!` does not check the status.
fn open_issue!(repo: string, title: string, token: string) -> Result[u16, ApiError] $ Http:
    url := Url::parse("https://api.example.com/repos/$repo/issues")?
    request := Request::post(url)
        .set_json(NewIssue { title: title })
        .set_header(.Authorization, "Bearer $token")
    response := request.send!()?        # an HttpError becomes ApiError.Transport
    .Ok(response.status.code())

pub fn main!() -> Result[void, ApiError] $ Console + Http:
    match repo!("hd-lang")?:
        .Some(repo) => println("${repo.name}: ${repo.stars} stars")
        .None => println("no such repository")
    .Ok(())

tests:
    use std.testing.it_prop

    it("decodes a repository and maps 404 to none"):
        http := ScriptedHttp::new()
        http.reply("https://api.example.com/repos/hd", .Ok(Response::status(.Ok).set_json(Repo { name: "hd", stars: 7 })))
        http.reply("https://api.example.com/repos/gone", .Ok(Response::status(.NotFound)))
        $.with(Http=http):
            assert_equal(repo!("hd").ok(), .Some(.Some(Repo { name: "hd", stars: 7 })))
            assert_equal(repo!("gone").ok(), .Some(.None))
        assert_equal(http.received().len(), 2)

    it("reports a body that is not a repository"):
        http := ScriptedHttp::new()
        http.reply("https://api.example.com/repos/odd", .Ok(Response::status(.Ok).set_text("[]")))
        $.with(Http=http):
            error := repo!("odd").err().expect("an error")
            assert_equal("$error", "unexpected response body: invalid JSON body: expected object at $")

    it("posts JSON with a header"):
        http := ScriptedHttp::new()
        http.reply("https://api.example.com/repos/hd/issues", .Ok(Response::status(.Created)))
        $.with(Http=http):
            assert_equal(open_issue!("hd", "bug", "t0ken").ok(), .Some(201))
        sent := http.received()[0]
        assert_equal(sent.header(.Authorization), .Some("Bearer t0ken"))
        assert_equal(sent.text().ok(), .Some("{\"title\":\"bug\"}"))

    it_prop("every repo survives encode and decode", prop=fn!(repo: Repo):
        assert_equal(decode::[Repo](encode(repo)).ok(), .Some(repo))
    )
```

## The pieces

- **Capability**: `$ Http`, imported from `std.http`. Grant the hosts in
  `hd.toml`: `Http = ["api.example.com"]`
  ([capabilities](../design/capabilities.md)).
- **GET**: `get!(url)` takes the address as text and checks the status: a
  400 to 599 answer is `.Err(HttpError.Status(response))`, with the
  response inside.
- **Other requests**: `Request::get(url)` or `Request::post(url)` with a
  parsed `Url`, then setters that return the request: `set_json(value)`,
  `set_text(text)`, `set_form(value)`, `set_header(name, value)`,
  `set_query_as(value)`. `request.send!()` returns the response whatever
  its status; `response.ok()` turns a failing status into an error.
- **Reading a response**: `response.status` (`.code()`, `.is_success()`),
  `response.json::[T]()`, `response.text()`, `response.header(name)`.
  A body is read once; a second read is `BodyError.Used`.
- **Query strings and forms** use the same derived types:
  `request.set_query_as(value)`, `request.set_form(value)`, and
  `request.query_as::[T]()` or `request.form::[T]()` to read one.
- **One error type**: a `@from` variant each for `HttpError`, `UrlError`
  and `BodyError` ([errors](../design/errors.md)). Check a decoded
  payload's rules with `@derive(Validate)` and `.validated()`
  ([configuration](configuration.md)).
- **Header names** are an enum: `.Authorization`, `.ContentType`,
  `.UserAgent`, `.Accept`, or `.Other("x-custom")`.
- **Retries**: `error.is_transient()` on an `HttpError` says whether a
  retry may help. Pass it to `retry(..).when(..)` rather than writing a
  loop ([concurrency](../design/concurrency.md#policies-retry-timeout-fallback)).
- **Testing**: `ScriptedHttp::new()`, then `reply(url, result)` for each
  full address. `received()` lists the requests that were sent.
