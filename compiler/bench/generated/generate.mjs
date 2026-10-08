import { mkdir, writeFile } from "node:fs/promises"
import { resolve } from "node:path"

const root = resolve(process.argv[2] ?? new URL("out/package", import.meta.url).pathname)
const folders = ["lexer/core", "math/kernel", "model/records", "dispatch/traits"]
const sources = new Map()

const main = []
for (let index = 0; index < folders.length; index += 1) {
    const modulePath = folders[index].replaceAll("/", ".")
    main.push(`use pkg.${modulePath}.unit.{run${index}}`)
}
main.push("")
main.push("fn main():")
for (let index = 0; index < folders.length; index += 1) {
    main.push(`    println(run${index}(1))`)
}
sources.set("main.hd", main)

for (let folderIndex = 0; folderIndex < folders.length; folderIndex += 1) {
    const lines = [
        `pub fn run${folderIndex}(x: i32) -> i32:`,
        `    return x + ${folderIndex * 10}`,
        "",
    ]
    sources.set(`${folders[folderIndex]}/unit.hd`, lines)
}

let functionIndex = 0
const paths = [...sources.keys()].slice(1)
while ([...sources.values()].reduce((sum, lines) => sum + lines.length, 0) + 3 <= 10000) {
    const path = paths[functionIndex % paths.length]
    const lines = sources.get(path)
    lines.push(`fn work${functionIndex}(x: i32) -> i32:`)
    lines.push(`    return x + ${functionIndex % 97}`)
    lines.push("")
    functionIndex += 1
}

const padding = sources.get(paths.at(-1))
while ([...sources.values()].reduce((sum, lines) => sum + lines.length, 0) < 10000) {
    padding.push("# padding")
}

for (const [relativePath, lines] of sources) {
    const path = resolve(root, relativePath)
    await mkdir(resolve(path, ".."), { recursive: true })
    await writeFile(path, `${lines.join("\n")}\n`)
}

// A package, so that `hd run` works on it.
await writeFile(resolve(root, "hd.toml"), '[package]\nname = "generated"\nversion = "0.1.0"\n')

console.log(`generated 10000 lines (${functionIndex} worker functions) in ${root}`)
