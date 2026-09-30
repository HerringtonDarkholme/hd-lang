# hd_nvim

Filetype detection and syntax highlighting for hd in Neovim 0.10 or later.
It uses only Neovim's built-in features: `vim.filetype.add` and a regex
syntax file. It needs no tree-sitter, no plugin library, and no `hd` CLI.

## Files

| File | Purpose |
| --- | --- |
| `ftdetect/hd.lua` | Sets filetype `hd` for `*.hd` files. |
| `syntax/hd.vim` | Highlights hd source. |
| `test/run.sh` | Headless test; needs only `nvim`. |

## Install

Neovim reads `ftdetect/` only at startup, when it loads plugins. An
`rtp:append` that runs later, such as after lazy.nvim's `setup`, never
loads the detection, so `*.hd` files get no filetype. Use one of these:

1. Append early in `init.lua`, so it runs before Neovim loads plugins at
   startup. With lazy.nvim, use option 3 instead:

   ```lua
   vim.opt.rtp:append("/path/to/hd-lang/editors/hd_nvim")
   ```

2. Or, if the append must run later, load the detection yourself right
   after it:

   ```lua
   vim.opt.rtp:append("/path/to/hd-lang/editors/hd_nvim")
   vim.cmd("runtime! ftdetect/hd.lua")
   ```

3. With lazy.nvim, add a local plugin spec that is not lazy-loaded:

   ```lua
   { dir = "/path/to/hd-lang/editors/hd_nvim", lazy = false }
   ```

Or copy it into your config:

```sh
cp -r /path/to/hd-lang/editors/hd_nvim/ftdetect /path/to/hd-lang/editors/hd_nvim/syntax ~/.config/nvim/
```

## What It Highlights

- Reserved words, copied from `KEYWORDS` in `src/lexer.ts`, and the
  contextual words `use`, `as`, `reified`, `by`, and `$.use` / `$.with`.
- `true`, `false`, and numbers with `_` separators and a suffix, as in
  `1_500ms`, plus `0x`, `0o`, and `0b` literals.
- Strings with `$name` and `${expr}` interpolation, `"""` multiline
  strings, prefixed strings such as `r"\d+"`, and character literals.
- `#` comments and `##` doc comments.
- Decorators (`@derive`), the requirement row `$`, the `!` of a
  suspension call, `|>`, `::`, capitalized type names, and `.Variant`.

When the lexer's reserved words change, update the list at the top of
`syntax/hd.vim`.

## Test

```sh
editors/hd_nvim/test/run.sh
```

It opens `test/sample.hd` headless and checks the filetype and the
highlight group at each construct. It exits non-zero on a failure.
