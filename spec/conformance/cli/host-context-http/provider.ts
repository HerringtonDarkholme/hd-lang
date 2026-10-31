import assert from "node:assert/strict";

interface HttpContext {
  http: {
    coversHost(host: string, port: number): boolean;
    isAsciiUrl(text: string): boolean;
    writtenHost(text: string): string;
  };
}

export default {
  inspect(context: HttpContext) {
    const policy = context.http;
    assert.equal(Object.isFrozen(policy), true);
    assert.equal(Reflect.set(context, "http", {}), false);
    assert.equal(Reflect.defineProperty(context, "http", { value: {} }), false);
    assert.equal(Reflect.deleteProperty(context, "http"), false);
    assert.equal(context.http, policy);
    assert.equal(
      Reflect.set(policy, "coversHost", () => true),
      false,
    );
    assert.equal(policy.isAsciiUrl("https://%65vil.test/"), false);
    assert.equal(policy.isAsciiUrl("https://bücher.test/"), false);
    assert.equal(policy.isAsciiUrl("https://allowed.test/path"), true);
    assert.equal(policy.writtenHost("HTTP://127.1:80/path"), "127.1");
    return [
      policy.coversHost("allowed.test", 443),
      policy.coversHost("allowed.test", 80),
      policy.coversHost("denied.test", 443),
    ].join(",");
  },
};
