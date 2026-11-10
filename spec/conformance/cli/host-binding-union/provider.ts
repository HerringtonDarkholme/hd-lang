type Small = { tag: "_0"; fields: { 0: number } } | { tag: "_1"; fields: { 0: string } };
type Wide =
  | { tag: "_0"; fields: { 0: bigint } }
  | { tag: "_1"; fields: { 0: { x: number; y: number } } }
  | { tag: "_2"; fields: { 0: string[] } };

// Each case crosses as its tag and its one payload.
export default {
  get(flip: boolean): Small {
    return flip ? { tag: "_1", fields: { 0: "a" } } : { tag: "_0", fields: { 0: 42 } };
  },
  render(value: Wide): string {
    switch (value.tag) {
      case "_0":
        return `i64 ${value.fields[0]}`;
      case "_1":
        return `point ${value.fields[0].x},${value.fields[0].y}`;
      case "_2":
        return `list ${value.fields[0].join("+")}`;
    }
  },
};
