# URL References

`std.url` parses and resolves URI references independently of their transport.

## Parsed URLs

```hd
use std.url.{Url, UrlError}

fn decoded_path(text: string) -> Result[string, UrlError]:
    Url::parse(text).map(fn(url): url.path)
```

1. r[std-url.parsed-url.declaration] `std.url.Url` is a value with these public fields and private query and authority metadata.

| Field | Type | Meaning |
| --- | --- | --- |
| `scheme` | `string?` | Scheme when present |
| `host` | `string?` | Authority host when present, including IP-literal brackets |
| `port` | `u16?` | Explicit port when present |
| `path` | `string` | Percent-decoded path |
| `raw_path` | `string` | Encoded path, preserving encoded separators |
| `query` | `List[(string, string)]` | Decoded query pairs in order |
| `fragment` | `string?` | Decoded fragment when present |

2. r[std-url.parsed-url.parse] `Url::parse(text: string) -> Result[Url, UrlError]` parses absolute and relative RFC 3986 URI references.
3. r[std-url.parsed-url.relative] A reference without a scheme or authority has `None` for the corresponding field.
4. r[std-url.parsed-url.invalid] Malformed syntax, non-ASCII hosts or invalid ports return `UrlError.Invalid` with the original text.
5. r[std-url.parsed-url.user-info] Parsing preserves authority user-info for Display; sending an HTTP request still rejects user-info.
6. r[std-url.parsed-url.controls] A literal ASCII control character, space or DEL in the input makes the URL invalid.
7. r[std-url.parsed-url.scheme-host-case] Parsing lowercases ASCII letters in the scheme and host.
8. r[std-url.parsed-url.port] A nonempty explicit port must be decimal text within the `u16` range; an absent or empty port becomes `None`.
9. r[std-url.parsed-url.path] Parsing percent-decodes the path as UTF-8 without interpreting encoded `/` or `?` as component separators.
10. r[std-url.parsed-url.query] Parsing splits nonempty query parts at `&` and their first `=`, retaining pair order and repeated names.
11. r[std-url.parsed-url.query-empty-value] A query part without `=` has an empty value.
12. r[std-url.parsed-url.query-decode] Query names and values percent-decode UTF-8 and interpret `+` as space.
13. r[std-url.parsed-url.fragment] Parsing percent-decodes the fragment as UTF-8, retaining an explicit empty fragment as `Some("")`.
14. r[std-url.parsed-url.invalid-escape] Invalid percent escapes or invalid decoded UTF-8 in the path, query or fragment return `UrlError.InvalidEncoding(text, offset)`, naming the original input byte offset.
15. r[std-url.parsed-url.raw-path] Display retains `raw_path` when it is a valid encoded path whose decoded value equals `path`.
16. r[std-url.parsed-url.changed-path] Otherwise Display percent-encodes the public path as UTF-8, preserving RFC 3986 path characters and escaping literal `%` and `?`.
17. r[std-url.parsed-url.raw-path-unicode] Literal non-ASCII path bytes are percent-encoded without changing existing valid escapes.
18. r[std-url.parsed-url.display-query] Display writes nonempty query pairs as encoded `name=value` parts joined by `&`, encoding each component's UTF-8 bytes separately.
19. r[std-url.parsed-url.display-fragment] Display writes a present fragment after `#`, percent-encoding its UTF-8 bytes while retaining permitted fragment characters.
20. r[std-url.parsed-url.empty-query] Parsing an explicit empty query retains a private presence bit, so an empty public list can still display a trailing `?`.
21. r[std-url.parsed-url.nonempty-query] A nonempty public query list overrides that presence bit in Display.
22. r[std-url.parsed-url.param] `param(&self, name: string) -> string?` returns the first value whose name equals `name`, or `None` when absent.
23. r[std-url.parsed-url.add-param] `add_param[T < Display](&mut self, name: string, value: T) -> void` appends the name and the value's Display text as decoded data.
24. r[std-url.parsed-url.add-param-empty-query] `add_param` clears the earlier empty-query presence bit.
25. r[std-url.parsed-url.resolve] `resolve(&self, reference: string) -> Result[Url, UrlError]` resolves a reference by RFC 3986 section 5, preserving encoded path separators and query presence.
26. r[std-url.parsed-url.traits] `Url` implements `Display`, structural `Eq` and `Debug`.

27. r[std-url.parsed-url.path-plus] A literal `+` in a path remains `+`.
28. r[std-url.parsed-url.no-default-scheme] Parsing does not supply a scheme or authority for a relative reference.
29. r[std-url.parsed-url.display-reference] Display emits a present scheme followed by `:`, a present authority preceded by `//`, then path, query and fragment.

> **Why.** URI references are independent of HTTP, and decoding must retain encoded separator identity through `raw_path`.

## Typed Queries

Typed query values use the [shared query and form codec](http.md#typed-query-and-form-values) and its `std.http.FormError`.

1. r[std-url.form-codec.query-reader] `Url.query_as[T < Deserialize](&self) -> Result[T, FormError]` decodes its query pairs into `T`.
2. r[std-url.form-codec.query-writer] `Url.set_query_as[T < Serialize](&mut self, value: T) -> void` replaces the query using the same serde codec.
3. r[std-url.form-codec.query-write-order] Writing retains member declaration order and list or tuple item order as decoded query pairs.
4. r[std-url.form-codec.query-write-absence] Writing an absent optional value emits no pair.
5. r[std-url.form-codec.query-write-empty] Every query write clears the explicit empty-query presence bit.
6. r[std-url.form-codec.query-write-error] A failed query write retains a private `FormError`, which later `query_as` returns directly.
7. r[std-url.form-codec.query-replacement] A successful `set_query_as` replaces the query and clears its pending preparation error.
8. r[std-url.form-codec.query-append-error] `add_param` does not clear a pending query preparation error.
9. r[std-url.form-codec.query-resolve-inherit] Resolution retains a pending query error exactly when RFC 3986 inherits the base query, including empty and fragment-only references.
10. r[std-url.form-codec.query-resolve-replace] A reference replacing the query clears its pending error, including an explicit query, new path, authority or scheme.

## URL Errors

1. r[std-url.url-error.declaration] `UrlError` is an error enum with `Invalid(text: string)`, `RelativeWithoutBase(text: string)` and `InvalidEncoding(text: string, offset: usize)`.
2. r[std-url.url-error.display-invalid-text] `UrlError.Invalid(text)` normally displays `invalid URL: TEXT`.
3. r[std-url.url-error.display-relative] `UrlError.RelativeWithoutBase(text)` displays `relative URL without a base: TEXT`.
4. r[std-url.url-error.traits] `UrlError` implements structural `Eq` and `Debug`.
5. r[std-url.url-error.display-user-info] For user-info input, the Invalid error omits credentials from Display and recommends using the Authorization header.
6. r[std-url.url-error.encoding-display] `InvalidEncoding(text, offset)` displays `invalid URL encoding at byte OFFSET`.

