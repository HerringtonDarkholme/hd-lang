-- Headless test for hd_nvim. Run through test/run.sh, or directly:
--   nvim --headless -u NONE -i NONE -n -l editors/hd_nvim/test/run.lua
local here = vim.fn.fnamemodify(debug.getinfo(1, "S").source:sub(2), ":p:h")
local plugin = vim.fn.fnamemodify(here, ":h")

vim.opt.runtimepath:prepend(plugin)
vim.cmd("filetype on")
vim.cmd("syntax on")
vim.cmd.edit(vim.fn.fnameescape(here .. "/sample.hd"))

local failures, passes = 0, 0

local function check(label, ok, detail)
  if ok then
    passes = passes + 1
    io.stdout:write("ok    " .. label .. "\n")
  else
    failures = failures + 1
    io.stdout:write("FAIL  " .. label .. (detail and (": " .. detail) or "") .. "\n")
  end
end

-- The syntax group at the first occurrence of `needle` on line `lnum`,
-- `offset` bytes into it.
local function group_at(lnum, needle, offset)
  local line = vim.fn.getline(lnum)
  local start = line:find(needle, 1, true)
  if not start then
    return nil, ("%q not found on line %d"):format(needle, lnum)
  end
  local col = start + (offset or 0)
  return vim.fn.synIDattr(vim.fn.synID(lnum, col, 1), "name")
end

local function expect(lnum, needle, offset, group)
  local got, err = group_at(lnum, needle, offset)
  local label = ("line %d %q+%d is %s"):format(lnum, needle, offset, group)
  check(label, got == group, err or ("got " .. tostring(got)))
end

check("filetype is hd", vim.bo.filetype == "hd", "got " .. vim.bo.filetype)
check("b:current_syntax is hd", vim.b.current_syntax == "hd", "got " .. tostring(vim.b.current_syntax))

-- Keywords.
expect(10, "fn", 0, "hdKeyword")
expect(11, "if", 0, "hdConditional")
expect(3, "data", 0, "hdStructure")
expect(16, "pub", 0, "hdStorage")
expect(22, "true", 0, "hdBoolean")

-- Strings and interpolation.
expect(20, '"Ada"', 1, "hdString")
expect(22, "Hello", 0, "hdString")
expect(22, "$name", 0, "hdInterpName")
expect(22, "${", 0, "hdInterpDelim")
expect(22, "adjust", 0, "hdFunctionCall")
expect(22, "\\n", 0, "hdEscape")
expect(24, "second", 0, "hdTripleString")
expect(21, 'r"', 0, "hdStringPrefix")
expect(21, "\\d", 0, "hdRawString")

-- Comments.
expect(25, "# say", 0, "hdComment")
expect(1, "##", 0, "hdDocComment")

-- Numbers with a separator and a suffix, and a radix literal.
expect(17, "1_500ms", 0, "hdNumber")
expect(17, "1_500ms", 5, "hdNumber")
expect(18, "0xff", 0, "hdNumber")

-- Decorators.
expect(2, "@derive", 0, "hdDecorator")
expect(2, "@derive", 3, "hdDecorator")

-- Types, variants, calls, the requirement row.
expect(3, "Counter", 0, "hdType")
expect(19, ".Fast", 1, "hdVariant")
expect(10, "adjust", 0, "hdFunction")
expect(16, "$ Console", 0, "hdRequirement")
expect(25, "use", 0, "hdProvider")
expect(25, "!(", 0, "hdBang")

-- Each group links to the expected standard highlight group.
local function link(group)
  return vim.api.nvim_get_hl(0, { name = group, link = true }).link
end
for group, target in pairs({
  hdKeyword = "Keyword",
  -- Structure and StorageClass link to Type by default, which made
  -- `pub trait Add` one color; declaration keywords stay Keyword.
  hdStructure = "Keyword",
  hdStorage = "Keyword",
  hdType = "Type",
  hdString = "String",
  hdInterpName = "Identifier",
  hdComment = "Comment",
  hdDocComment = "SpecialComment",
  hdNumber = "Number",
  hdDecorator = "PreProc",
}) do
  check(group .. " links to " .. target, link(group) == target, "got " .. tostring(link(group)))
end

io.stdout:write(("%d passed, %d failed\n"):format(passes, failures))
os.exit(failures == 0 and 0 or 1)
