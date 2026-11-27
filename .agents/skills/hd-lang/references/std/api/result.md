# `std.result`

Generated from `hd doc std.result`. Run `hd doc std.result.NAME` in a package for one item's full text.

## `Result`

- `pub fn map[U](self, transform: fn(T) -> U) -> Result[U, E]`: `.Ok` of `transform(value)` for an `.Ok`, and the error unchanged for an `.Err`.
- `pub fn map_err[F](self, transform: fn(E) -> F) -> Result[T, F]`: `.Err` of `transform(error)` for an `.Err`, and the value unchanged for an `.Ok`.
- `pub fn and_then[U](self, transform: fn(T) -> Result[U, E]) -> Result[U, E]`: For an `.Ok`, `transform(value)`; for an `.Err`, the error unchanged.
- `pub fn ok(self) -> T?`: The value of an `.Ok` in `.Some`, and `.None` for an `.Err`.
- `pub fn err(self) -> E?`: The error of an `.Err` in `.Some`, and `.None` for an `.Ok`.
- `pub fn as_ref(&self) -> Result[&T, &E]`: Borrows either payload in place without copying it or moving the result.
- `pub fn as_mut(&mut self) -> Result[&mut T, &mut E]`: Borrows either payload mutably in place without moving the result.
- `pub fn is_ok(&self) -> bool`: True for an `.Ok`.
- `pub fn is_err(&self) -> bool`: True for an `.Err`.
- `pub fn unwrap_or(self, fallback: T) -> T`: The value of an `.Ok`, and `fallback` for an `.Err`.
- `pub fn expect(self, message: string) -> T`: The value of an `.Ok`; for an `.Err` it panics with `message: error`.
- `pub fn unwrap_or_else[F < ?Copy & FnMut(E) -> T $ R, $ R](self, mut fallback: F) -> T $ R`: The successful payload, or one call of `fallback` with the error.
- `pub fn or_else[E2 < ?Copy, F < ?Copy & FnMut(E) -> Result[T, E2] $ R, $ R]( self, mut fallback: F, ) -> Result[T, E2] $ R`: Keeps an `.Ok`, or replaces an `.Err` through `fallback`.
- `pub fn unwrap(self) -> T`: The successful payload; an error panics with its debug text.
- `pub fn context(self, message: string) -> Result[T, *dyn Error]`: For an `.Err`, wraps the error with `message` as its context; an `.Ok` is unchanged.
