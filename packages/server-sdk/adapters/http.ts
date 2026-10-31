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
const dnsCodes = new Set(["ENOTFOUND", "EAI_AGAIN", "EAI_NODATA", "EAI_FAIL"]);
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
  const detail = cause?.cause ?? cause;
  const code = detail?.code ?? "";
  if (dnsCodes.has(code)) {
    return error("Dns", { host });
  }
  const message = String(detail?.message ?? detail);
  const isTls = tlsCodes.has(code) || /^ERR_(TLS|SSL)/.test(code);
  if (isTls || (code === "EPROTO" && /\b(tls|ssl)/i.test(message))) {
    return error("Tls", { message });
  }
  return error(code ? "Connect" : "Other", { message });
}

// Fetch preserves Set-Cookie through its multi-value API. Other header
// pairs are exactly those exposed by the platform (std-http.headers.combined).
export function responseHeaders(headers) {
  let cookies;
  if (typeof headers.getSetCookie === "function") {
    cookies = headers.getSetCookie();
  } else if (typeof headers.getAll === "function") {
    cookies = headers.getAll("Set-Cookie");
  } else {
    cookies = headers.has("set-cookie") ? null : [];
  }
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

function parseRequestUrl(url, isAsciiUrl) {
  try {
    if (!isAsciiUrl(url) || url.includes("\\") || !/^https?:\/\/[^/?#]+/i.test(url)) {
      return null;
    }
    const parsed = new URL(url);
    return ["http:", "https:"].includes(parsed.protocol) ? parsed : null;
  } catch {
    return null;
  }
}

function requestPort(parsed) {
  if (parsed.port) {
    return Number(parsed.port);
  }
  return parsed.protocol === "https:" ? 443 : 80;
}

function isFinalResponse(request, response, location) {
  return request.redirects === 0 || !redirected.has(response.status) || location === null;
}

function parseRedirect(location, parsed, isAsciiUrl) {
  const hasScheme = /^[a-z][a-z0-9+.-]*:/i.test(location);
  const networkPath = location.startsWith("//");
  const rawTarget = networkPath ? parsed.protocol + location : location;
  if (
    !isAsciiUrl(rawTarget) ||
    location.includes("\\") ||
    (hasScheme && !/^https?:\/\/[^/?#]+/i.test(location)) ||
    (networkPath && !/^\/\/[^/?#]+/.test(location))
  ) {
    return null;
  }
  try {
    return new URL(location, parsed);
  } catch {
    return null;
  }
}

function redirectsToGet(status, method) {
  return (
    (status === 303 && !["GET", "HEAD"].includes(method)) ||
    ([301, 302].includes(status) && method === "POST")
  );
}

async function completedResponse(response, url) {
  const decoded = await decodedFetchResponse(response);
  return decoded.ok ? ok({ ...decoded.value, url }) : decoded;
}

function redirectHeaders(headers, parsed, next) {
  return next.origin === parsed.origin
    ? headers
    : headers.filter(([name]) => !credentials.has(name.toLowerCase()));
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
              const parsed = parseRequestUrl(url, isAsciiUrl);
              if (!parsed) {
                return error("InvalidUrl", { url });
              }
              host = writtenHost(written);
              const port = requestPort(parsed);
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
              if (isFinalResponse(request, response, location)) {
                return completedResponse(response, url);
              }
              const decodedRedirect = await decodedFetchResponse(response);
              if (!decodedRedirect.ok) {
                return decodedRedirect;
              }
              // WHATWG URL accepts slashless schemes and extra authority
              // slashes; reject those before it can change the written host.
              const next = parseRedirect(location, parsed, isAsciiUrl);
              if (!next) {
                return error("InvalidUrl", { url: location });
              }
              if (count === request.redirects) {
                return error("TooManyRedirects", { url: next.href });
              }
              headers = redirectHeaders(headers, parsed, next);
              if (redirectsToGet(response.status, method)) {
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
