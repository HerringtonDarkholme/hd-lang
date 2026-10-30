// Native fetch transport for a lease-owned std.http.Http handle. This is
// runtime-independent: the caller supplies the public URL/grant helpers.
const ok = (value) => ({ ok: true, value });
const error = (tag, fields = {}) => ({ ok: false, error: { tag, fields } });
const redirected = new Set([301, 302, 303, 307, 308]);
const maxDecodedBytes = 64 * 1024 * 1024;
const bodyHeaders = new Set([
  "content-encoding",
  "content-language",
  "content-location",
  "content-type",
  "content-length",
  "transfer-encoding",
]);
const credentials = new Set(["authorization", "proxy-authorization", "cookie", "host"]);
const tlsCodes = new Set([
  "CERT_HAS_EXPIRED",
  "CERT_NOT_YET_VALID",
  "DEPTH_ZERO_SELF_SIGNED_CERT",
  "SELF_SIGNED_CERT_IN_CHAIN",
  "UNABLE_TO_GET_ISSUER_CERT",
  "UNABLE_TO_GET_ISSUER_CERT_LOCALLY",
  "UNABLE_TO_VERIFY_LEAF_SIGNATURE",
]);

function failure(cause, host, timedOut) {
  if (timedOut) {
    return error("Timeout");
  }
  const code = cause?.cause?.code ?? cause?.code ?? "";
  if (["ENOTFOUND", "EAI_AGAIN", "EAI_NODATA", "EAI_FAIL"].includes(code)) {
    return error("Dns", { host });
  }
  const message = String(cause?.cause?.message ?? cause?.message ?? cause);
  if (
    tlsCodes.has(code) ||
    /^ERR_(TLS|SSL)/.test(code) ||
    (code === "EPROTO" && /\b(tls|ssl)/i.test(message))
  ) {
    return error("Tls", { message });
  }
  return error(code ? "Connect" : "Other", { message });
}

// Fetch preserves Set-Cookie through its multi-value API. Other header
// pairs are exactly those exposed by the platform (std-http.headers.combined).
export function responseHeaders(headers) {
  const cookies =
    typeof headers.getSetCookie === "function"
      ? headers.getSetCookie()
      : typeof headers.getAll === "function"
        ? headers.getAll("Set-Cookie")
        : headers.has("set-cookie")
          ? null
          : [];
  if (cookies === null) {
    throw new Error("the platform does not expose separate Set-Cookie values");
  }
  const pairs = [];
  let emittedCookies = false;
  for (const [name, value] of headers) {
    if (name.toLowerCase() === "set-cookie") {
      if (!emittedCookies) {
        if (!cookies.length) {
          throw new Error("the platform omitted Set-Cookie values");
        }
        for (const cookie of cookies) {
          pairs.push([name, cookie]);
        }
        emittedCookies = true;
      }
    } else {
      pairs.push([name, value]);
    }
  }
  if (!emittedCookies) {
    for (const cookie of cookies) {
      pairs.push(["set-cookie", cookie]);
    }
  }
  return pairs;
}

function decodedFetchResponse(response) {
  return response.arrayBuffer().then((buffer) => {
    const body = new Uint8Array(buffer);
    if (body.length > maxDecodedBytes) {
      return error("OutputTooLarge");
    }
    const headers = responseHeaders(response.headers);
    const staleEncoding = headers.some(([name]) => name.toLowerCase() === "content-encoding");
    return ok({
      status: response.status,
      headers: staleEncoding
        ? headers.filter(
            ([name]) => !["content-encoding", "content-length"].includes(name.toLowerCase()),
          )
        : headers,
      body,
    });
  });
}

// Fetch exposes no resolver, TLS roots or identity, or phase timer, so the
// first transport setting a derived provider changed is refused before any
// work, in the order every host reports them (std-http.settings.host-support).
export function changedSetting(request) {
  const settings = [
    ["resolver", request.dns_servers != null],
    ["tls.roots", (request.tls_roots?.length ?? 0) > 0],
    ["tls.identity", request.tls_identity != null],
    ["connect_timeout", request.connect_timeout != null],
    ["tls_handshake_timeout", request.tls_handshake_timeout != null],
    ["response_header_timeout", request.response_header_timeout != null],
    ["body_read_timeout", request.body_read_timeout != null],
  ];
  return settings.find(([, changed]) => changed)?.[0] ?? null;
}

export function createHttpClient(
  lease,
  { grant, isAsciiUrl, writtenHost, fetch: fetcher = globalThis.fetch, timeoutMs = 30000 },
) {
  if (!grant || typeof grant.coversHost !== "function") {
    throw new TypeError("an explicit resolved grant is required");
  }
  return {
    send(request, context) {
      return lease.operation(context, (resolve) => {
        const controller = new AbortController();
        let timedOut = false;
        const timer = setTimeout(() => {
          timedOut = true;
          controller.abort();
        }, timeoutMs);
        const run = async () => {
          const unsupported = changedSetting(request);
          if (unsupported !== null) {
            return error("UnsupportedSetting", { name: unsupported });
          }
          let method =
            request.method.tag === "Other"
              ? request.method.fields.name
              : request.method.tag.toUpperCase();
          let { url, body } = request;
          let written = url,
            host = "";
          let headers = request.headers.map(([name, value]) => [name, value]);
          try {
            for (let count = 0; ; count++) {
              let parsed;
              try {
                if (!isAsciiUrl(url) || url.includes("\\") || !/^https?:\/\/[^/?#]+/i.test(url)) {
                  return error("InvalidUrl", { url });
                }
                parsed = new URL(url);
                if (!["http:", "https:"].includes(parsed.protocol)) {
                  return error("InvalidUrl", { url });
                }
              } catch {
                return error("InvalidUrl", { url });
              }
              host = writtenHost(written);
              const port = parsed.port
                ? Number(parsed.port)
                : parsed.protocol === "https:"
                  ? 443
                  : 80;
              if (!grant.coversHost("Http", host, port)) {
                return error("NotGranted", { host });
              }
              const response = await fetcher(url, {
                method,
                headers,
                body: body.length ? body : undefined,
                redirect: "manual",
                signal: controller.signal,
              });
              const location = response.headers.get("location");
              if (
                request.redirects === 0 ||
                !redirected.has(response.status) ||
                location === null
              ) {
                const decoded = await decodedFetchResponse(response);
                if (!decoded.ok) {
                  return decoded;
                }
                return ok({ ...decoded.value, url });
              }
              const decodedRedirect = await decodedFetchResponse(response);
              if (!decodedRedirect.ok) {
                return decodedRedirect;
              }
              // WHATWG URL accepts slashless schemes and extra authority
              // slashes; reject those before it can change the written host.
              const hasScheme = /^[a-z][a-z0-9+.-]*:/i.test(location);
              const networkPath = location.startsWith("//");
              const rawTarget = networkPath ? parsed.protocol + location : location;
              if (
                !isAsciiUrl(rawTarget) ||
                location.includes("\\") ||
                (hasScheme && !/^https?:\/\/[^/?#]+/i.test(location)) ||
                (networkPath && !/^\/\/[^/?#]+/.test(location))
              ) {
                return error("InvalidUrl", { url: location });
              }
              let next;
              try {
                next = new URL(location, parsed);
              } catch {
                return error("InvalidUrl", { url: location });
              }
              if (count === request.redirects) {
                return error("TooManyRedirects", { url: next.href });
              }
              if (next.origin !== parsed.origin) {
                headers = headers.filter(([name]) => !credentials.has(name.toLowerCase()));
              }
              if (
                (response.status === 303 && !["GET", "HEAD"].includes(method)) ||
                ([301, 302].includes(response.status) && method === "POST")
              ) {
                method = "GET";
                body = new Uint8Array(0);
                headers = headers.filter(([name]) => !bodyHeaders.has(name.toLowerCase()));
              }
              if (/^([a-z][a-z0-9+.-]*:)?\/\//i.test(location)) {
                written = location.startsWith("//") ? parsed.protocol + location : location;
              }
              url = next.href;
            }
          } catch (cause) {
            return failure(cause, host, timedOut);
          }
        };
        run().then(
          (value) => {
            clearTimeout(timer);
            resolve(value);
          },
          (cause) => {
            clearTimeout(timer);
            resolve(failure(cause, "", timedOut));
          },
        );
        return () => {
          clearTimeout(timer);
          controller.abort();
        };
      });
    },
  };
}
