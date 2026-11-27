# Io

Status: standard library specification draft.

This chapter defines `std.io`, which `lib/std` writes in ordinary hd
over the language tier:

- the trait `Read`, which every byte source read in chunks implements;
- the error enum `IoError`;
- `Cursor`, the `Read` source over bytes in memory;
- `BufReader`, which reads a source a buffer at a time and hands out
  lines, and its line cursor `Lines`.

A file ([Reading Files In Chunks](fs.md#reading-files-in-chunks)) and standard input
([Standard Input Bytes](console.md#standard-input-bytes)) implement `Read`.

## Reading In Chunks

`Read` reads a source a chunk at a time, so a program holds one chunk, not
the whole input, and can stop early:

```text
pub trait Read:
    fn read!(&mut self, max: usize) -> Result[List[u8], IoError]
```

```text
use std.io.{IoError, Read}

fn count_bytes![R < Read](source: &mut R) -> Result[usize, IoError]:
    let total: usize = 0
    while true:
        chunk := source.read!(65536)?
        if chunk.len() == 0:
            break
        total = total + chunk.len()
    .Ok(total)
```

1. r[std-io.read.decl] `std.io` declares the trait `Read` with the method above. Code imports it, as in `use std.io.Read`.
2. r[std-io.read.at-most] `read!(max)` returns the next bytes of the source, at most `max` of them. It may return fewer than `max` before the end.
3. r[std-io.read.end] An empty list means the end of input. Before the end, a `read!` with `max` above 0 returns at least one byte, waiting until the source has one.
4. r[std-io.read.zero] `read!(0)` returns an empty list at once and reads nothing.
5. r[std-io.read.suspends] Reading is I/O, so `read!` is a bang call. A read that waits for its source suspends the calling task, and other tasks run.
6. r[std-io.read.one-call] A host source makes one host call per `read!`, and holds no more of its input in the program than the chunk it returns.

> **Why.** Go's `io.Reader` and Rust's `Read` fill a caller's buffer and
> report a count. hd has no borrowed byte buffers at the host boundary,
> so the chunk is the result, as Deno's `read` and Python's `read(n)`
> return it.

## Io Errors

```text
pub enum IoError:
    Closed
    NotGranted(subject: string)
    InvalidUtf8
    Other(message: string)
```

1. r[std-io.error.decl] `std.io` declares `IoError` as above. It implements `Debug`, `Eq`, and `Display`. Code imports it, as in `use std.io.IoError`.
2. r[std-io.error.other] `Other` holds a host read failure with the host's message.
3. r[std-io.error.invalid-utf8] `InvalidUtf8` reports bytes that a read of text, such as [`BufReader.read_line!`](#buffered-reading), cannot decode as UTF-8.
4. r[std-io.error.closed] `Closed` reports a read of a source that was disposed, such as a [`File`](fs.md#reading-files-in-chunks).
5. r[std-io.error.not-granted] `NotGranted(subject)` reports a source that the run's grants do not cover. Its `Display` text is `access to SUBJECT is not granted`.

## Cursor

`Cursor` reads bytes held in memory, so a test feeds a reader without a
host:

```text
use std.io.{Cursor, Read}

tests:
    use std.testing.assert_equal

    it("reads two bytes, then the rest"):
        let mut source = Cursor::new([1, 2, 3])
        assert_equal(source.read!(2), .Ok([1, 2]))
        assert_equal(source.read!(2), .Ok([3]))
        assert_equal(source.read!(2), .Ok([]))
```

1. r[std-io.cursor.decl] `std.io` declares `Cursor`, with private fields, which implements `Read` and `Debug`. `Cursor::new(bytes: List[u8]) -> Cursor` returns a cursor at the first byte.
2. r[std-io.cursor.read] Each `read!(max)` returns the next `max` bytes, or all that are left when fewer, then an empty list.
3. r[std-io.cursor.position] `position(&self) -> usize` returns how many bytes have been read.

## Buffered Reading

`BufReader` reads its source a buffer at a time, so a line costs no host
call of its own:

```text
use std.console.{ConsoleInput, stdin}
use std.io.BufReader

fn count_lines!() -> usize $ ConsoleInput:
    let mut lines = BufReader::new(stdin()).lines()
    let count: usize = 0
    while true:
        match lines.next!():
            .Some(.Ok(_)) => count = count + 1
            _ => break
    count
```

```text
pub struct BufReader[R < Read]
pub struct Lines[R < Read]
```

1. r[std-io.buf-reader.decl] `std.io` declares `BufReader[R < Read]`, with private fields, which implements `Read`. `BufReader::new(inner: R)` returns a reader over `inner` with an 8 KiB buffer.
2. r[std-io.buf-reader.capacity] `BufReader::with_capacity(capacity: usize, inner: R)` reads `inner` at most `capacity` bytes at a time. A `capacity` of 0 reads one byte at a time.
3. r[std-io.buf-reader.read-line] `read_line!(&mut self) -> Result[string?, IoError]` returns the next line with its line ending exactly as read, `\n` or `\r\n`, or `.None` at the end of input. A last line with no ending is returned as it is.
4. r[std-io.buf-reader.invalid-utf8] A line that is not UTF-8 is `.Err(IoError.InvalidUtf8)`. Its bytes are read, so the next `read_line!` reads the line after it.
5. r[std-io.buf-reader.lines] `lines(self) -> Lines[R]` returns a cursor whose `next!(&mut self) -> Result[string, IoError]?` returns what `read_line!` reads: `.Some(.Ok(line))`, `.Some(.Err(error))`, or `.None` at the end.
6. r[std-io.buf-reader.read] `read!(max)` returns buffered bytes first. With an empty buffer, a `max` of at least the capacity reads `inner` directly, and a smaller one refills the buffer.
7. r[std-io.buf-reader.into-inner] `into_inner(self) -> R` returns the source. Bytes still in the buffer are dropped.
8. r[std-io.buf-reader.cost] A line costs O(its length): each byte is read from `inner` once, scanned once, and copied once.
9. r[std-io.buf-reader.dispose] A `BufReader` does not dispose its source. The binding that opened the source disposes it, as `using! file = open!(path)?` does, and a reader made from `file` holds a copy of the same handle.

> **Why.** Line endings stay, as Rust's `read_line` and Go's
> `ReadString('\n')` keep them, so a filter writes back exactly the bytes
> it read. `Lines` is a cursor with a bang `next!`, not an `Iterator`,
> because reading suspends. As Go's `bufio.Reader` has no `Close`, one
> binding owns each handle's disposal, so a wrapper never closes a source
> that the code which opened it still uses.

See also: [Reading Files In Chunks](fs.md#reading-files-in-chunks),
[Standard Input Bytes](console.md#standard-input-bytes).
