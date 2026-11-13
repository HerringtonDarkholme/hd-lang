# Vendored TOML parser

Parser-only ESM files from [smol-toml 1.9.1](https://github.com/squirrelchat/smol-toml/tree/v1.9.1), under BSD-3-Clause. The SDK deploy tools use these files without an npm installation or a network request. They are not imported by the deployed HTTP runtime.

`SOURCE.json` pins the release archive integrity and original source hashes. Files retain their upstream license headers and are formatted with the repository oxfmt. Two side-effect expression statements are spelled as equivalent `if` statements to pass strict lint. Inline character-label comments in `util.js` are omitted because oxfmt otherwise moves a comment across parentheses on every pass. `package.json` only selects ESM loading. The serializer and package entry point are omitted.
