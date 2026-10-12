use std::sync::Arc;

use hd_cache::MemoryStore;
use hd_driver::{Executor, Goal, Host, NoClock, build};
use hd_project::MemorySources;
use hd_sched::SerialOrder;

use super::{DocFile, Module, Package, page_stem, render, summary};

const LIB: &str = "\
## Tools for the shop.

## A cart's line.
@derive(Eq)
pub data Line:
    ## The product.
    pub name: string
    price: i32

## Shows a line.
pub trait Show:
    ## The text.
    fn show(self) -> string

impl Show for Line:
    fn show(self) -> string:
        self.name

impl Line:
    ## Makes a line.
    ##
    ## ```hd
    ## use pkg.{Line}
    ##
    ## l := Line.new(\"tea\")
    ## ```
    pub fn new(name: string) -> Line:
        Line { name: name, price: 0 }

    fn secret(self) -> i32: self.price

## Adds the prices
## of a cart. Then rounds.
##
## ```hd
## use pkg.{total}
##
## x := total(1)  # error: type-mismatch
## ```
pub fn total(
    prices: List[i32],
) -> i32:
    let sum = +0
    for price in prices:
        sum = sum + price
    sum

pub type Price = i32

fn helper() -> i32: 1
";

const TEXT: &str = "\
## Text helpers.

## Turns a title into a slug.
pub fn slugify(title: string) -> string:
    title
";

fn ifaces(sources: &[(&str, &str)]) -> Vec<(String, Arc<[u8]>)> {
    let mut src = MemorySources::default();
    for (path, text) in sources {
        src.insert(path, text);
    }
    let store = MemoryStore::default();
    let host = Host {
        render_tir: &[],
        sources: &src,
        store: &store,
        clock: &NoClock,
        executor: Executor::Serial(SerialOrder::Priority),
    };
    let out = build(&host, "shop", &Goal::Analyze);
    assert!(!out.diags.has_errors(), "{}", out.render());
    out.ifaces
}

fn file<'a>(files: &'a [DocFile], path: &str) -> &'a str {
    &files
        .iter()
        .find(|f| f.path == path)
        .unwrap_or_else(|| panic!("no {path}"))
        .text
}

fn site(private: bool) -> Vec<DocFile> {
    let ifaces = ifaces(&[("src/lib.hd", LIB), ("src/text.hd", TEXT)]);
    let modules = [
        Module {
            path: "text",
            text: TEXT,
        },
        Module {
            path: "",
            text: LIB,
        },
    ];
    render(&Package {
        name: "shop",
        root: "shop",
        modules: &modules,
        ifaces: &ifaces,
        private,
    })
}

#[test]
fn stems_and_summaries() {
    assert_eq!(page_stem(""), "pkg");
    assert_eq!(page_stem("index"), "index/index");
    assert_eq!(page_stem("index.list"), "index/list");
    assert_eq!(page_stem("shop.cart"), "shop/cart");
    assert_eq!(
        summary("Adds the prices\nof a cart. Then."),
        "Adds the prices of a cart."
    );
    assert_eq!(summary("Version 1.2 is out"), "Version 1.2 is out");
    assert_eq!(summary(""), "");
}

#[test]
fn the_page_set() {
    let files = site(false);
    let paths: Vec<&str> = files.iter().map(|f| f.path.as_str()).collect();
    assert_eq!(
        paths,
        [
            "pkg.md",
            "pkg.html",
            "text.md",
            "text.html",
            "index.md",
            "index.html",
            "llms.txt",
            "llms-full.txt"
        ]
    );
    assert_eq!(
        file(&files, "llms.txt"),
        "# shop\n\n> Tools for the shop.\n\n## Modules\n- [pkg](pkg.md): Tools for the shop.\n- [text](text.md): Text helpers.\n"
    );
    assert_eq!(
        file(&files, "index.md"),
        "# shop\n\n- [`pkg`](pkg.md): Tools for the shop.\n- [`text`](text.md): Text helpers.\n\n"
    );
    let full = file(&files, "llms-full.txt");
    assert!(full.starts_with("# Module `pkg`\n"), "{full}");
    assert!(full.contains("\n# Module `text`\n"), "{full}");
}

#[test]
fn items_signatures_members_impls_and_examples() {
    let files = site(false);
    let pkg = file(&files, "pkg.md");
    let want = [
        "# Module `pkg`\n\nTools for the shop.\n\n",
        "- [`Line`](#Line): A cart's line.\n- [`Show`](#Show): Shows a line.\n- [`total`](#total): Adds the prices of a cart.\n- [`Price`](#Price)\n\n",
        "<a id=\"Line\"></a>\n## `Line`\n\n```hd\npub data Line:\n    pub name: string\n    # private fields omitted\n```\n\nA cart's line.\n\n",
        "- `impl Eq for Line` (derived)\n- `impl Show for Line`\n\n",
        "<a id=\"Line.name\"></a>\n### `Line.name`\n\n```hd\npub name: string\n```\n\nThe product.\n\n",
        "<a id=\"Line.new\"></a>\n### `Line.new`\n\n```hd\npub fn new(name: string) -> Line\n```\n\nMakes a line.\n\nExample `doc pkg.Line.new[0]`:\n```hd\nuse pkg.{Line}\n\nl := Line.new(\"tea\")\n```\n\n",
        "## `Show`\n\n```hd\npub trait Show:\n    fn show(self) -> string\n```\n",
        "```hd\npub fn total(\n    prices: List[i32],\n) -> i32\n```\n\nAdds the prices\nof a cart. Then rounds.\n\nExample `doc pkg.total[0]`, Does not compile (type-mismatch):\n```hd\nuse pkg.{total}\n\nx := total(1)  # error: type-mismatch\n```\n\n",
        "```hd\npub type Price = i32\n```\n",
    ];
    let mut at = 0;
    for w in want {
        let found = pkg[at..]
            .find(w)
            .unwrap_or_else(|| panic!("{w:?} after {at} in\n{pkg}"));
        at += found + w.len();
    }
    assert!(!pkg.contains("helper") && !pkg.contains("secret") && !pkg.contains("sum + price"));
    let html = file(&files, "pkg.html");
    assert!(
        html.contains("<h2 id=\"Line\"><code>Line</code></h2>"),
        "{html}"
    );
    assert!(
        html.contains("<h3 id=\"Line.new\"><code>Line.new</code></h3>"),
        "{html}"
    );
    assert!(html.contains("<code>doc pkg.Line.new[0]</code>"), "{html}");
}

#[test]
fn private_items_and_members() {
    let files = site(true);
    let pkg = file(&files, "pkg.md");
    assert!(
        pkg.contains("## `helper`\n\n```hd\nfn helper() -> i32\n```\n"),
        "{pkg}"
    );
    assert!(pkg.contains("    price: i32\n```"), "{pkg}");
    assert!(pkg.contains("### `Line.secret`"), "{pkg}");
    assert!(!pkg.contains("private fields omitted"), "{pkg}");
}
