# How To: Call An HTTP API

**Scenario.** Fetch a JSON resource, treat "not found" as an ordinary
answer, post a JSON body with a header, and test all of it without a
network.

## The program

```hd
use std.http.{Http, HttpError, Request, Response, ScriptedHttp, Url, get}
use std.encoding.{Deserialize, Serialize}

@derive(Serialize, Deserialize, Eq, Debug)
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
    @error("bad address: $text")
    Address(text: string)
    @error("unexpected response body")
    Body

# GET: `get!` returns `.Err(.Status(response))` for a 4xx or 5xx status.
fn repo!(name: string) -> Result[Repo?, ApiError] $ Http:
    response := match get!("https://api.example.com/repos/$name"):
        .Ok(response) => response
        .Err(.Status(response)) if response.status.code() == 404 => return .Ok(.None)
        .Err(error) => return .Err(.Transport(error))
    match response.json::[Repo]():
        .Ok(repo) => .Ok(.Some(repo))
        .Err(_) => .Err(.Body)

# POST: build a `Request`, then `send!`. `send!` does not check the status.
fn open_issue!(repo: string, title: string, token: string) -> Result[u16, ApiError] $ Http:
    address := "https://api.example.com/repos/$repo/issues"
    url := match Url::parse(address):
        .Ok(url) => url
        .Err(_) => return .Err(.Address(address))
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
    it("decodes a repository and maps 404 to none"):
        http := ScriptedHttp::new()
        http.reply("https://api.example.com/repos/hd", .Ok(Response::status(.Ok).set_json(Repo { name: "hd", stars: 7 })))
        http.reply("https://api.example.com/repos/gone", .Ok(Response::status(.NotFound)))
        $.with(Http=http):
            assert_equal(repo!("hd").ok(), .Some(.Some(Repo { name: "hd", stars: 7 })))
            assert_equal(repo!("gone").ok(), .Some(.None))
        assert_equal(http.received().len(), 2)

    it("posts JSON with a header"):
        http := ScriptedHttp::new()
        http.reply("https://api.example.com/repos/hd/issues", .Ok(Response::status(.Created)))
        $.with(Http=http):
            assert_equal(open_issue!("hd", "bug", "t0ken").ok(), .Some(201))
        sent := http.received()[0]
        assert_equal(sent.header(.Authorization), .Some("Bearer t0ken"))
        assert_equal(sent.text().ok(), .Some("{\"title\":\"bug\"}"))
```

## The pieces

- **Capability**: `$ Http`, imported from `std.http`. In a package, grant
  the hosts in `hd.toml`: `Http = ["api.example.com"]`, or `Http = true`.
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
- **Header names** are an enum: `.Authorization`, `.ContentType`,
  `.UserAgent`, `.Accept`, or `.Other("x-custom")`.
- **Errors**: `error.is_transient()` on an `HttpError` says whether a retry
  may help ([how-to/concurrent-work.md](concurrent-work.md) has retries).
- **Testing**: `ScriptedHttp::new()`, then `reply(url, result)` for each
  full address. `received()` lists the requests that were sent.
