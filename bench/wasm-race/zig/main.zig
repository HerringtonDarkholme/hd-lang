const std = @import("std");
const Io = std.Io;
const flate = std.compress.flate;

const Record = struct {
    id: i64,
    name: []const u8,
    email: []const u8,
    score: f64,
    tags: []const []const u8,
};

fn nowNs(io: Io) i96 {
    return Io.Clock.awake.now(io).nanoseconds;
}

fn fillU32LCG(dst: []u32, seed: u32) void {
    var state: u32 = seed;
    for (dst) |*v| {
        state = state *% 1664525 +% 1013904223;
        v.* = state;
    }
}

fn benchJson(allocator: std.mem.Allocator, io: Io, n: usize) !struct { u64, u64 } {
    var names = try allocator.alloc([]u8, n);
    defer { for (names) |s| allocator.free(s); allocator.free(names); }
    var emails = try allocator.alloc([]u8, n);
    defer { for (emails) |s| allocator.free(s); allocator.free(emails); }
    const tags_static = [_][]const u8{ "a", "b", "c" };
    var recs = try allocator.alloc(Record, n);
    defer allocator.free(recs);
    for (0..n) |i| {
        names[i] = try std.fmt.allocPrint(allocator, "user_{d}", .{i});
        emails[i] = try std.fmt.allocPrint(allocator, "user_{d}@example.com", .{i});
        recs[i] = .{ .id = @intCast(i), .name = names[i], .email = emails[i], .score = @as(f64, @floatFromInt(i % 1000)) + 0.5, .tags = tags_static[0..] };
    }
    const t0 = nowNs(io);
    var out: std.Io.Writer.Allocating = .init(allocator);
    defer out.deinit();
    try std.json.Stringify.value(recs, .{}, &out.writer);
    const raw = out.written();
    const parsed = try std.json.parseFromSlice([]Record, allocator, raw, .{ .allocate = .alloc_always });
    defer parsed.deinit();
    const t1 = nowNs(io);
    var sum: u64 = 0;
    for (parsed.value) |r| { sum += @intCast(r.id); sum += r.name.len; sum += r.email.len; sum += r.tags.len; }
    sum += raw.len;
    return .{ @intCast(t1 - t0), sum };
}

fn benchCsv(allocator: std.mem.Allocator, io: Io, n: usize) !struct { u64, u64 } {
    const t0 = nowNs(io);
    var list: std.ArrayList(u8) = .empty;
    defer list.deinit(allocator);
    try list.ensureTotalCapacity(allocator, n * 24);
    var i: usize = 0;
    while (i < n) : (i += 1) try list.print(allocator, "{d},user_{d},{d}\n", .{ i, i, i % 1000 });
    const s = list.items;
    var sum: u64 = 0;
    var start: usize = 0;
    var idx: usize = 0;
    while (idx < s.len) : (idx += 1) {
        if (s[idx] == '\n') {
            const line = s[start..idx];
            if (std.mem.lastIndexOfScalar(u8, line, ',')) |c| sum += try std.fmt.parseInt(u64, line[c + 1 ..], 10);
            start = idx + 1;
        }
    }
    const t1 = nowNs(io);
    return .{ @intCast(t1 - t0), sum + s.len };
}

fn benchSieve(allocator: std.mem.Allocator, io: Io, limit: usize) !struct { u64, u64 } {
    const t0 = nowNs(io);
    var prime = try allocator.alloc(bool, limit + 1);
    defer allocator.free(prime);
    @memset(prime, true);
    if (limit >= 0) prime[0] = false;
    if (limit >= 1) prime[1] = false;
    var p: usize = 2;
    while (p * p <= limit) : (p += 1) {
        if (prime[p]) {
            var m = p * p;
            while (m <= limit) : (m += p) prime[m] = false;
        }
    }
    var count: u64 = 0;
    var i: usize = 2;
    while (i <= limit) : (i += 1) { if (prime[i]) count += 1; }
    const t1 = nowNs(io);
    return .{ @intCast(t1 - t0), count };
}

fn benchSha(allocator: std.mem.Allocator, io: Io, size: usize, iters: usize) !struct { u64, u64 } {
    const buf = try allocator.alloc(u8, size);
    defer allocator.free(buf);
    for (buf, 0..) |*b, bi| b.* = @truncate(bi);
    var last: [32]u8 = undefined;
    const t0 = nowNs(io);
    var i: usize = 0;
    while (i < iters) : (i += 1) std.crypto.hash.sha2.Sha256.hash(buf, &last, .{});
    const t1 = nowNs(io);
    return .{ @intCast(t1 - t0), std.mem.readInt(u64, last[0..8], .big) };
}

fn benchSort(allocator: std.mem.Allocator, io: Io, n: usize) !struct { u64, u64 } {
    const a = try allocator.alloc(u32, n);
    defer allocator.free(a);
    fillU32LCG(a, 0xC0FFEE);
    const t0 = nowNs(io);
    std.mem.sort(u32, a, {}, std.sort.asc(u32));
    const t1 = nowNs(io);
    var sum: u64 = @as(u64, a[0]) + @as(u64, a[n - 1]);
    var i: usize = 0;
    while (i < n) : (i += 1024) sum ^= a[i];
    return .{ @intCast(t1 - t0), sum };
}

fn benchHashmap(allocator: std.mem.Allocator, io: Io, n: usize) !struct { u64, u64 } {
    const keys = try allocator.alloc(u32, n);
    defer allocator.free(keys);
    fillU32LCG(keys, 0xBADC0DE);
    const t0 = nowNs(io);
    var map = std.AutoHashMap(u32, u32).init(allocator);
    defer map.deinit();
    try map.ensureTotalCapacity(@intCast(n));
    for (keys, 0..) |k, i| try map.put(k, @intCast(i));
    var sum: u64 = 0;
    for (keys) |k| sum += map.get(k).?;
    var misses: u64 = 0;
    for (keys) |k| { if (map.get(k ^ 0xFFFFFFFF) == null) misses += 1; }
    const t1 = nowNs(io);
    return .{ @intCast(t1 - t0), sum + misses };
}

fn benchFind(allocator: std.mem.Allocator, io: Io, size: usize, stride: usize) !struct { u64, u64 } {
    const needle = "needle!!";
    const buf = try allocator.alloc(u8, size);
    defer allocator.free(buf);
    for (buf, 0..) |*b, i| b.* = @truncate(i);
    var i: usize = 0;
    while (i + needle.len <= size) : (i += stride) @memcpy(buf[i .. i + needle.len], needle);
    const t0 = nowNs(io);
    var count: u64 = 0;
    var pos: usize = 0;
    while (pos + needle.len <= buf.len) {
        if (std.mem.indexOf(u8, buf[pos..], needle)) |idx| { count += 1; pos += idx + 1; } else break;
    }
    const t1 = nowNs(io);
    return .{ @intCast(t1 - t0), count };
}

fn benchBase64(allocator: std.mem.Allocator, io: Io, size: usize) !struct { u64, u64 } {
    const buf = try allocator.alloc(u8, size);
    defer allocator.free(buf);
    for (buf, 0..) |*b, i| b.* = @truncate(i);
    const t0 = nowNs(io);
    const enc_len = std.base64.standard.Encoder.calcSize(buf.len);
    const enc_buf = try allocator.alloc(u8, enc_len);
    defer allocator.free(enc_buf);
    const enc = std.base64.standard.Encoder.encode(enc_buf, buf);
    const dec_len = try std.base64.standard.Decoder.calcSizeForSlice(enc);
    const dec = try allocator.alloc(u8, dec_len);
    defer allocator.free(dec);
    try std.base64.standard.Decoder.decode(dec, enc);
    const t1 = nowNs(io);
    var sum: u64 = enc.len + dec.len;
    if (dec.len >= 8) sum ^= std.mem.readInt(u64, dec[0..8], .big);
    return .{ @intCast(t1 - t0), sum };
}

const TNode = struct {
    left: ?*TNode,
    right: ?*TNode,
    val: i32,
};

fn buildTree(allocator: std.mem.Allocator, d: i32) !*TNode {
    const n = try allocator.create(TNode);
    if (d == 0) {
        n.* = .{ .left = null, .right = null, .val = 1 };
        return n;
    }
    n.* = .{ .left = try buildTree(allocator, d - 1), .right = try buildTree(allocator, d - 1), .val = d };
    return n;
}

fn freeTree(allocator: std.mem.Allocator, n: ?*TNode) void {
    if (n) |node| {
        freeTree(allocator, node.left);
        freeTree(allocator, node.right);
        allocator.destroy(node);
    }
}

fn walkTree(n: ?*TNode) i64 {
    const node = n orelse return 0;
    return @as(i64, node.val) + walkTree(node.left) + walkTree(node.right);
}

fn benchAlloc(allocator: std.mem.Allocator, io: Io, depth: i32, rounds: usize) !struct { u64, u64 } {
    const t0 = nowNs(io);
    var sum: u64 = 0;
    var r: usize = 0;
    while (r < rounds) : (r += 1) {
        const root = try buildTree(allocator, depth);
        sum += @intCast(walkTree(root));
        freeTree(allocator, root);
    }
    const t1 = nowNs(io);
    return .{ @intCast(t1 - t0), sum };
}


const BTNode = struct {
    left: ?*BTNode,
    right: ?*BTNode,
};

fn bottomUpTree(allocator: std.mem.Allocator, d: i32) !*BTNode {
    const n = try allocator.create(BTNode);
    if (d == 0) {
        n.* = .{ .left = null, .right = null };
        return n;
    }
    n.* = .{ .left = try bottomUpTree(allocator, d - 1), .right = try bottomUpTree(allocator, d - 1) };
    return n;
}

fn freeBT(allocator: std.mem.Allocator, n: ?*BTNode) void {
    if (n) |node| {
        freeBT(allocator, node.left);
        freeBT(allocator, node.right);
        allocator.destroy(node);
    }
}

fn itemCheck(t: ?*BTNode) i32 {
    const node = t orelse return 0;
    if (node.left == null) return 1;
    return 1 + itemCheck(node.left) + itemCheck(node.right);
}

fn benchBTree(allocator: std.mem.Allocator, io: Io, max_depth: i32) !struct { u64, u64 } {
    const t0 = nowNs(io);
    var sum: u64 = 0;
    const stretch = try bottomUpTree(allocator, max_depth + 1);
    sum += @intCast(itemCheck(stretch));
    freeBT(allocator, stretch);
    const long_lived = try bottomUpTree(allocator, max_depth);
    var depth: i32 = 4;
    while (depth <= max_depth) : (depth += 2) {
        const iterations: usize = @as(usize, 1) << @intCast(max_depth - depth + 4);
        var i: usize = 0;
        while (i < iterations) : (i += 1) {
            const tr = try bottomUpTree(allocator, depth);
            sum += @intCast(itemCheck(tr));
            freeBT(allocator, tr);
        }
    }
    sum += @intCast(itemCheck(long_lived));
    freeBT(allocator, long_lived);
    const t1 = nowNs(io);
    return .{ @intCast(t1 - t0), sum };
}

fn benchBTreeStretch(allocator: std.mem.Allocator, io: Io, depth: i32) !struct { u64, u64 } {
    const t0 = nowNs(io);
    const tr = try bottomUpTree(allocator, depth);
    const sum: u64 = @intCast(itemCheck(tr));
    freeBT(allocator, tr);
    const t1 = nowNs(io);
    return .{ @intCast(t1 - t0), sum };
}

fn buildGraph(allocator: std.mem.Allocator, n: usize) ![]i32 {
    const edges = try allocator.alloc(i32, n * 3);
    var i: usize = 0;
    while (i < n) : (i += 1) {
        edges[i * 3 + 0] = @intCast((i * 7 + 1) % n);
        edges[i * 3 + 1] = @intCast((i * 7 + 3) % n);
        edges[i * 3 + 2] = @intCast((i * 11 + 5) % n);
    }
    return edges;
}

fn benchGraphDFS(allocator: std.mem.Allocator, io: Io, n: usize) !struct { u64, u64 } {
    const t0 = nowNs(io);
    const edges = try buildGraph(allocator, n);
    defer allocator.free(edges);
    var seen = try allocator.alloc(bool, n);
    defer allocator.free(seen);
    @memset(seen, false);
    var stack: std.ArrayList(usize) = .empty;
    defer stack.deinit(allocator);
    try stack.append(allocator, 0);
    seen[0] = true;
    var count: u64 = 0;
    var id_sum: u64 = 0;
    while (stack.items.len > 0) {
        const u = stack.pop().?;
        count += 1;
        id_sum += u;
        const base = u * 3;
        var k: usize = 0;
        while (k < 3) : (k += 1) {
            const v: usize = @intCast(edges[base + k]);
            if (!seen[v]) {
                seen[v] = true;
                try stack.append(allocator, v);
            }
        }
    }
    const t1 = nowNs(io);
    return .{ @intCast(t1 - t0), id_sum + count };
}

fn benchGraphBFS(allocator: std.mem.Allocator, io: Io, n: usize) !struct { u64, u64 } {
    const t0 = nowNs(io);
    const edges = try buildGraph(allocator, n);
    defer allocator.free(edges);
    var seen = try allocator.alloc(bool, n);
    defer allocator.free(seen);
    @memset(seen, false);
    var dist = try allocator.alloc(i32, n);
    defer allocator.free(dist);
    @memset(dist, -1);
    var q: std.ArrayList(usize) = .empty;
    defer q.deinit(allocator);
    try q.append(allocator, 0);
    seen[0] = true;
    dist[0] = 0;
    var count: u64 = 0;
    var id_sum: u64 = 0;
    var dist_sum: u64 = 0;
    var head: usize = 0;
    while (head < q.items.len) : (head += 1) {
        const u = q.items[head];
        count += 1;
        id_sum += u;
        dist_sum += @intCast(dist[u]);
        const base = u * 3;
        var k: usize = 0;
        while (k < 3) : (k += 1) {
            const v: usize = @intCast(edges[base + k]);
            if (!seen[v]) {
                seen[v] = true;
                dist[v] = dist[u] + 1;
                try q.append(allocator, v);
            }
        }
    }
    const t1 = nowNs(io);
    return .{ @intCast(t1 - t0), dist_sum + id_sum + count };
}

fn fib(n: i32) i64 {
    if (n < 2) return n;
    return fib(n - 1) + fib(n - 2);
}

fn benchFib(io: Io, n: i32) struct { u64, u64 } {
    const t0 = nowNs(io);
    const v = fib(n);
    const t1 = nowNs(io);
    return .{ @intCast(t1 - t0), @intCast(v) };
}

fn benchMatmul(allocator: std.mem.Allocator, io: Io, n: usize) !struct { u64, u64 } {
    const a = try allocator.alloc(f64, n * n);
    defer allocator.free(a);
    const b = try allocator.alloc(f64, n * n);
    defer allocator.free(b);
    const c = try allocator.alloc(f64, n * n);
    defer allocator.free(c);
    @memset(c, 0);
    for (0..n * n) |i| {
        a[i] = @as(f64, @floatFromInt((i * 3 + 1) % 97)) / 97.0;
        b[i] = @as(f64, @floatFromInt((i * 5 + 2) % 89)) / 89.0;
    }
    const t0 = nowNs(io);
    var i: usize = 0;
    while (i < n) : (i += 1) {
        var k: usize = 0;
        while (k < n) : (k += 1) {
            const aik = a[i * n + k];
            var j: usize = 0;
            while (j < n) : (j += 1) c[i * n + j] += aik * b[k * n + j];
        }
    }
    const t1 = nowNs(io);
    var sum: f64 = 0;
    var d: usize = 0;
    while (d < n) : (d += 1) sum += c[d * n + d];
    return .{ @intCast(t1 - t0), @bitCast(sum) };
}

fn benchBinenc(allocator: std.mem.Allocator, io: Io, n: usize) !struct { u64, u64 } {
    const t0 = nowNs(io);
    var list: std.ArrayList(u8) = .empty;
    defer list.deinit(allocator);
    try list.ensureTotalCapacity(allocator, n * 40);
    var i: usize = 0;
    while (i < n) : (i += 1) {
        var tmp: [8]u8 = undefined;
        std.mem.writeInt(u64, &tmp, i, .little);
        try list.appendSlice(allocator, &tmp);
        var name_buf: [32]u8 = undefined;
        const name = try std.fmt.bufPrint(&name_buf, "user_{d}", .{i});
        var nl: [2]u8 = undefined;
        std.mem.writeInt(u16, &nl, @intCast(name.len), .little);
        try list.appendSlice(allocator, &nl);
        try list.appendSlice(allocator, name);
        const bits: u64 = @bitCast(@as(f64, @floatFromInt(i % 1000)) + 0.5);
        std.mem.writeInt(u64, &tmp, bits, .little);
        try list.appendSlice(allocator, &tmp);
        try list.append(allocator, 3);
        for ([_][]const u8{ "a", "b", "c" }) |t| {
            try list.append(allocator, @intCast(t.len));
            try list.appendSlice(allocator, t);
        }
    }
    const buf = list.items;
    var sum: u64 = 0;
    var pos: usize = 0;
    while (pos < buf.len) {
        const id = std.mem.readInt(u64, buf[pos..][0..8], .little);
        pos += 8;
        const nl = std.mem.readInt(u16, buf[pos..][0..2], .little);
        pos += 2 + nl;
        pos += 8;
        const tc = buf[pos];
        pos += 1;
        var t: u8 = 0;
        while (t < tc) : (t += 1) {
            const l = buf[pos];
            pos += 1 + l;
        }
        sum += id;
    }
    const t1 = nowNs(io);
    sum += buf.len;
    if (buf.len >= 8) sum ^= std.mem.readInt(u64, buf[0..8], .little);
    return .{ @intCast(t1 - t0), sum };
}

fn benchZlib(allocator: std.mem.Allocator, io: Io, size: usize) !struct { u64, u64 } {
    const src = try allocator.alloc(u8, size);
    defer allocator.free(src);
    for (src, 0..) |*b, i| b.* = @truncate((i % 251) + (i / 64) % 7);
    const t0 = nowNs(io);
    var out: std.Io.Writer.Allocating = .init(allocator);
    defer out.deinit();
    try out.ensureTotalCapacity(1024);
    var window: [flate.max_window_len]u8 = undefined;
    var c = try flate.Compress.init(&out.writer, &window, .zlib, flate.Compress.Options.default);
    try c.writer.writeAll(src);
    try c.writer.flush();
    try c.finish();
    const compressed = out.written();
    var in_reader: std.Io.Reader = .fixed(compressed);
    var d_buf: [flate.max_window_len]u8 = undefined;
    var d = flate.Decompress.init(&in_reader, .zlib, &d_buf);
    var decoded: std.Io.Writer.Allocating = .init(allocator);
    defer decoded.deinit();
    try decoded.ensureTotalCapacity(1024);
    _ = try d.reader.streamRemaining(&decoded.writer);
    const dec = decoded.written();
    const t1 = nowNs(io);
    var sum: u64 = dec.len;
    if (dec.len >= 8) sum ^= std.mem.readInt(u64, dec[0..8], .big);
    return .{ @intCast(t1 - t0), sum };
}

fn benchUtf8(allocator: std.mem.Allocator, io: Io, repeats: usize) !struct { u64, u64 } {
    const chunk = "Hello世界🙂MoonBit综合性能ABC";
    var list: std.ArrayList(u8) = .empty;
    defer list.deinit(allocator);
    var r: usize = 0;
    while (r < repeats) : (r += 1) try list.appendSlice(allocator, chunk);
    const s = list.items;
    const t0 = nowNs(io);
    var sum: u64 = 0;
    var it = std.unicode.Utf8View.initUnchecked(s).iterator();
    while (it.nextCodepoint()) |cp| sum += cp;
    // roundtrip via validating decode count
    var it2 = std.unicode.Utf8View.initUnchecked(s).iterator();
    var rune_count: u64 = 0;
    while (it2.nextCodepoint()) |cp| {
        sum += cp;
        rune_count += 1;
    }
    sum += s.len + rune_count;
    const t1 = nowNs(io);
    return .{ @intCast(t1 - t0), sum };
}

fn benchAllocArena(allocator: std.mem.Allocator, io: Io, depth: i32, rounds: usize) !struct { u64, u64 } {
    const t0 = nowNs(io);
    var sum: u64 = 0;
    var r: usize = 0;
    var arena = std.heap.ArenaAllocator.init(allocator);
    defer arena.deinit();
    while (r < rounds) : (r += 1) {
        const root = try buildTree(arena.allocator(), depth);
        sum += @intCast(walkTree(root));
        _ = arena.reset(.retain_capacity);
    }
    const t1 = nowNs(io);
    return .{ @intCast(t1 - t0), sum };
}

fn benchBTreeArena(allocator: std.mem.Allocator, io: Io, max_depth: i32) !struct { u64, u64 } {
    const t0 = nowNs(io);
    var sum: u64 = 0;
    var arena = std.heap.ArenaAllocator.init(allocator);
    defer arena.deinit();
    var long_arena = std.heap.ArenaAllocator.init(allocator);
    defer long_arena.deinit();
    const stretch = try bottomUpTree(arena.allocator(), max_depth + 1);
    sum += @intCast(itemCheck(stretch));
    _ = arena.reset(.retain_capacity);
    const long_lived = try bottomUpTree(long_arena.allocator(), max_depth);
    var depth: i32 = 4;
    while (depth <= max_depth) : (depth += 2) {
        const iterations: usize = @as(usize, 1) << @intCast(max_depth - depth + 4);
        var i: usize = 0;
        while (i < iterations) : (i += 1) {
            const tr = try bottomUpTree(arena.allocator(), depth);
            sum += @intCast(itemCheck(tr));
            _ = arena.reset(.retain_capacity);
        }
    }
    sum += @intCast(itemCheck(long_lived));
    const t1 = nowNs(io);
    return .{ @intCast(t1 - t0), sum };
}

pub fn main(init: std.process.Init) !void {
    const allocator = init.gpa;
    const io = init.io;
    const args = try init.minimal.args.toSlice(init.arena.allocator());
    if (args.len < 2) {
        std.debug.print("usage: zig-bench <workload> [args...]\n", .{});
        return error.InvalidArgs;
    }
    const workload = args[1];
    var ns: u64 = 0;
    var sum: u64 = 0;
    if (std.mem.eql(u8, workload, "json")) {
        const n = if (args.len > 2) try std.fmt.parseInt(usize, args[2], 10) else 10000;
        const r = try benchJson(allocator, io, n); ns = r[0]; sum = r[1];
    } else if (std.mem.eql(u8, workload, "csv")) {
        const n = if (args.len > 2) try std.fmt.parseInt(usize, args[2], 10) else 500000;
        const r = try benchCsv(allocator, io, n); ns = r[0]; sum = r[1];
    } else if (std.mem.eql(u8, workload, "sieve")) {
        const n = if (args.len > 2) try std.fmt.parseInt(usize, args[2], 10) else 10000000;
        const r = try benchSieve(allocator, io, n); ns = r[0]; sum = r[1];
    } else if (std.mem.eql(u8, workload, "sha256")) {
        const size = if (args.len > 2) try std.fmt.parseInt(usize, args[2], 10) else 1 << 20;
        const iters = if (args.len > 3) try std.fmt.parseInt(usize, args[3], 10) else 2000;
        const r = try benchSha(allocator, io, size, iters); ns = r[0]; sum = r[1];
    } else if (std.mem.eql(u8, workload, "sort")) {
        const n = if (args.len > 2) try std.fmt.parseInt(usize, args[2], 10) else 1000000;
        const r = try benchSort(allocator, io, n); ns = r[0]; sum = r[1];
    } else if (std.mem.eql(u8, workload, "hashmap")) {
        const n = if (args.len > 2) try std.fmt.parseInt(usize, args[2], 10) else 500000;
        const r = try benchHashmap(allocator, io, n); ns = r[0]; sum = r[1];
    } else if (std.mem.eql(u8, workload, "alloc-arena")) {
        const d: i32 = if (args.len > 2) try std.fmt.parseInt(i32, args[2], 10) else 16;
        const c: usize = if (args.len > 3) try std.fmt.parseInt(usize, args[3], 10) else 20;
        const r = try benchAllocArena(allocator, io, d, c); ns = r[0]; sum = r[1];
    } else if (std.mem.eql(u8, workload, "btree-arena")) {
        const n: i32 = if (args.len > 2) try std.fmt.parseInt(i32, args[2], 10) else 15;
        const r = try benchBTreeArena(allocator, io, n); ns = r[0]; sum = r[1];
    } else if (std.mem.eql(u8, workload, "find")) {
        const size = if (args.len > 2) try std.fmt.parseInt(usize, args[2], 10) else 8 << 20;
        const stride = if (args.len > 3) try std.fmt.parseInt(usize, args[3], 10) else 1024;
        const r = try benchFind(allocator, io, size, stride); ns = r[0]; sum = r[1];
    } else if (std.mem.eql(u8, workload, "base64")) {
        const size = if (args.len > 2) try std.fmt.parseInt(usize, args[2], 10) else 10 << 20;
        const r = try benchBase64(allocator, io, size); ns = r[0]; sum = r[1];
    } else if (std.mem.eql(u8, workload, "alloc")) {
        const depth: i32 = if (args.len > 2) try std.fmt.parseInt(i32, args[2], 10) else 16;
        const rounds = if (args.len > 3) try std.fmt.parseInt(usize, args[3], 10) else 20;
        const r = try benchAlloc(allocator, io, depth, rounds); ns = r[0]; sum = r[1];
    } else if (std.mem.eql(u8, workload, "fib")) {
        const n: i32 = if (args.len > 2) try std.fmt.parseInt(i32, args[2], 10) else 37;
        const r = benchFib(io, n); ns = r[0]; sum = r[1];
    } else if (std.mem.eql(u8, workload, "matmul")) {
        const n = if (args.len > 2) try std.fmt.parseInt(usize, args[2], 10) else 256;
        const r = try benchMatmul(allocator, io, n); ns = r[0]; sum = r[1];
    } else if (std.mem.eql(u8, workload, "binenc")) {
        const n = if (args.len > 2) try std.fmt.parseInt(usize, args[2], 10) else 100000;
        const r = try benchBinenc(allocator, io, n); ns = r[0]; sum = r[1];
    } else if (std.mem.eql(u8, workload, "zlib")) {
        const size = if (args.len > 2) try std.fmt.parseInt(usize, args[2], 10) else 2 << 20;
        const r = try benchZlib(allocator, io, size); ns = r[0]; sum = r[1];
    } else if (std.mem.eql(u8, workload, "utf8")) {
        const n = if (args.len > 2) try std.fmt.parseInt(usize, args[2], 10) else 100000;
        const r = try benchUtf8(allocator, io, n); ns = r[0]; sum = r[1];
    } else if (std.mem.eql(u8, workload, "btree")) {
        const n: i32 = if (args.len > 2) try std.fmt.parseInt(i32, args[2], 10) else 15;
        const r = try benchBTree(allocator, io, n); ns = r[0]; sum = r[1];
    } else if (std.mem.eql(u8, workload, "btree-stretch")) {
        const n: i32 = if (args.len > 2) try std.fmt.parseInt(i32, args[2], 10) else 18;
        const r = try benchBTreeStretch(allocator, io, n); ns = r[0]; sum = r[1];
    } else if (std.mem.eql(u8, workload, "graph-dfs")) {
        const n = if (args.len > 2) try std.fmt.parseInt(usize, args[2], 10) else 200000;
        const r = try benchGraphDFS(allocator, io, n); ns = r[0]; sum = r[1];
    } else if (std.mem.eql(u8, workload, "graph-bfs")) {
        const n = if (args.len > 2) try std.fmt.parseInt(usize, args[2], 10) else 200000;
        const r = try benchGraphBFS(allocator, io, n); ns = r[0]; sum = r[1];
    } else return error.UnknownWorkload;

    var buf: [256]u8 = undefined;
    const msg = try std.fmt.bufPrint(&buf, "RESULT lang=zig workload={s} ns={d} checksum={d}\n", .{ workload, ns, sum });
    var stdout_buffer: [256]u8 = undefined;
    var stdout_writer = std.Io.File.stdout().writer(io, &stdout_buffer);
    try stdout_writer.interface.writeAll(msg);
    try stdout_writer.interface.flush();
}

