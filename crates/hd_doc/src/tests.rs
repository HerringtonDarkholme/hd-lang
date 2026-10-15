use std::sync::Arc;

use hd_cache::MemoryStore;
use hd_driver::{Executor, Goal, Host, NoClock, Packages, build_packages};
use hd_project::MemorySources;
use hd_sched::SerialOrder;

use super::{DocFile, Module, Named, Package, lookup, page_stem, render, summary};

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

type Ifaces = Vec<(String, Arc<[u8]>)>;
type Scopes = Vec<(String, Vec<(String, Named)>)>;

fn ifaces(sources: &[(&str, &str)]) -> Ifaces {
    built(sources).0
}

/// The interfaces and module scopes of a package named `shop`.
fn built(sources: &[(&str, &str)]) -> (Ifaces, Scopes) {
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
    let packages = Packages {
        doc_scopes: true,
        ..Packages::default()
    };
    let out = build_packages(&host, "shop", &packages, &Goal::Analyze);
    assert!(!out.diags.has_errors(), "{}", out.render());
    (out.ifaces, out.scopes)
}

/// The standard library's modules, from the embedded `lib/std`.
fn std_modules() -> Vec<(String, &'static str)> {
    hd_driver::STD_FILES
        .iter()
        .filter_map(|(path, text)| Some((path.strip_suffix(".hd")?.replace('/', "."), *text)))
        .collect()
}

fn std_of<'a>(names: &'a [(String, &'static str)]) -> Vec<Module<'a>> {
    names
        .iter()
        .map(|(path, text)| Module { path, text })
        .collect()
}

fn file<'a>(files: &'a [DocFile], path: &str) -> &'a str {
    &files
        .iter()
        .find(|f| f.path == path)
        .unwrap_or_else(|| panic!("no {path}"))
        .text
}

fn site(private: bool) -> Vec<DocFile> {
    let (ifaces, scopes) = built(&[("src/lib.hd", LIB), ("src/text.hd", TEXT)]);
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
    let std = std_modules();
    let std = std_of(&std);
    render(&Package {
        name: "shop",
        root: "shop",
        modules: &modules,
        ifaces: &ifaces,
        private,
        scopes: &scopes,
        deps: &[],
        std: &std,
    })
    .files
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

/// `hd doc NAME` over the same package as `site`.
fn name(private: bool, name: &str) -> Result<String, Vec<String>> {
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
    let std = std_modules();
    let std = std_of(&std);
    lookup(
        &Package {
            name: "shop",
            root: "shop",
            modules: &modules,
            ifaces: &ifaces,
            private,
            scopes: &[],
            deps: &[],
            std: &std,
        },
        name,
    )
}

#[test]
fn name_prints_an_item_section() {
    assert_eq!(
        name(false, "text.slugify").unwrap(),
        "## `slugify`\n\n```hd\npub fn slugify(title: string) -> string\n```\n\nTurns a title into a slug.\n"
    );
    // An item's section holds its members, and stops at the next item.
    let line = name(false, "pkg.Line").unwrap();
    assert!(line.starts_with("## `Line`\n"), "{line}");
    assert!(line.contains("### `Line.new`"), "{line}");
    assert!(!line.contains("## `Show`"), "{line}");
    // A member's section stops at the next heading of its level.
    let new = name(false, "pkg.Line.new").unwrap();
    assert!(new.starts_with("### `Line.new`\n"), "{new}");
    assert!(new.contains("Makes a line."), "{new}");
    assert!(
        !new.contains("## `Show`") && !new.contains("<a id"),
        "{new}"
    );
}

#[test]
fn name_prints_a_whole_module_page() {
    let files = site(false);
    assert_eq!(name(false, "pkg").unwrap(), file(&files, "pkg.md"));
    assert_eq!(name(false, "text").unwrap(), file(&files, "text.md"));
}

#[test]
fn name_hides_private_items_without_private() {
    let missing = name(false, "pkg.helper").unwrap_err();
    assert!(missing.contains(&"pkg.total".to_owned()), "{missing:?}");
    assert!(!missing.contains(&"pkg.helper".to_owned()), "{missing:?}");
    let shown = name(true, "pkg.helper").unwrap();
    assert!(shown.starts_with("## `helper`\n"), "{shown}");
    assert!(name(false, "pkg.Line.secret").is_err());
    assert!(name(true, "pkg.Line.secret").is_ok());
}

#[test]
fn name_suggests_in_each_space() {
    assert_eq!(name(false, "shop").unwrap_err(), ["pkg", "text"]);
    let items = name(false, "text.slug").unwrap_err();
    assert_eq!(items, ["text.slugify"]);
    let std_items = name(false, "std.text.nope").unwrap_err();
    assert!(!std_items.is_empty() && std_items.iter().all(|n| n.starts_with("std.text.")));
    let std_modules = name(false, "std.tex").unwrap_err();
    assert!(
        std_modules.contains(&"std.text".to_owned()),
        "{std_modules:?}"
    );
}

#[test]
fn name_prints_a_std_entry() {
    let split = name(false, "std.text.split").unwrap();
    assert!(
        split.starts_with(
            "```hd\npub fn split(self, separator: string) -> List[string]\n```\n\nSplits the string around each occurrence of `separator`, in order.\n\n"
        ),
        "{split}"
    );
    assert!(
        split.ends_with(
            "Reference: [std.text.string.split](spec/std/text.md#r-std-text.string.split)\n"
        ),
        "{split}"
    );
    assert_eq!(split, name(false, "std.text.string.split").unwrap());
    // `pad_start` had no summary in the table scraped from the reference.
    let pad = name(false, "std.text.pad_start").unwrap();
    assert!(pad.contains("Pads the string on the left"), "{pad}");
    assert!(pad.contains("fill: char = ' '"), "{pad}");
}

#[test]
fn name_prints_a_std_module_page() {
    let page = name(false, "std.text").unwrap();
    assert!(page.starts_with("# Module `std.text`\n"), "{page}");
    assert!(page.contains("- [`join`](#join): "), "{page}");
    assert!(page.contains("### `string.split`"), "{page}");
    assert!(
        page.ends_with("Reference: [std.text](spec/std/text.md)\n"),
        "{page}"
    );
    assert!(!page.contains("](../../spec/"), "{page}");
    assert_eq!(
        name(false, "std.text.join")
            .unwrap()
            .matches("```hd")
            .count(),
        1
    );
}

const LINK_LIB: &str = "\
## The shop. See [`total`], [`Cart`], [`Cart.add`], [`std.text.split`], [`nothing`].

use pkg.cart.{Cart}

## Sums a [`helper`] result, not `[`plain`]` or [`a link`](x.md).
pub fn total() -> i32:
    helper()

fn helper() -> i32: 1
";

const LINK_CART: &str = "\
## Carts, summed by [`total`].

use pkg.{total}

## A cart.
pub data Cart:
    pub n: i32

impl Cart:
    ## Adds one to a [`Cart`] with [`Cart.n`].
    pub fn add(self) -> Cart: self
";

const LINK_DEEP: &str = "\
## Deep. See [`total`], [`Cart.add`], [`cart.Cart`] and [`pkg.cart`].

use pkg.{total}
use pkg.cart
use pkg.cart.{Cart}
";

fn link_files(private: bool) -> super::Rendered {
    let sources = [
        ("src/lib.hd", LINK_LIB),
        ("src/cart.hd", LINK_CART),
        ("src/shop/deep.hd", LINK_DEEP),
    ];
    let (ifaces, scopes) = built(&sources);
    let modules = [
        Module {
            path: "",
            text: LINK_LIB,
        },
        Module {
            path: "cart",
            text: LINK_CART,
        },
        Module {
            path: "shop.deep",
            text: LINK_DEEP,
        },
    ];
    let std = std_modules();
    let std = std_of(&std);
    render(&Package {
        name: "shop",
        root: "shop",
        modules: &modules,
        ifaces: &ifaces,
        private,
        scopes: &scopes,
        deps: &[],
        std: &std,
    })
}

#[test]
fn a_link_to_an_item_of_the_same_module_is_its_anchor() {
    let out = link_files(false);
    let pkg = file(&out.files, "pkg.md");
    assert!(
        pkg.contains(
            "See [`total`](#total), [`Cart`](cart.md#Cart), [`Cart.add`](cart.md#Cart.add)"
        ),
        "{pkg}"
    );
    let html = file(&out.files, "pkg.html");
    assert!(
        html.contains("<a href=\"#total\"><code>total</code></a>"),
        "{html}"
    );
    assert!(
        html.contains("<a href=\"cart.html#Cart.add\"><code>Cart.add</code></a>"),
        "{html}"
    );
}

#[test]
fn a_name_from_a_use_links_to_the_page_of_its_module() {
    let out = link_files(false);
    let cart = file(&out.files, "cart.md");
    assert!(
        cart.contains("summed by [`total`](pkg.md#total)."),
        "{cart}"
    );
    assert!(
        cart.contains("Adds one to a [`Cart`](#Cart) with [`Cart.n`](#Cart)."),
        "{cart}"
    );
    let html = file(&out.files, "cart.html");
    assert!(
        html.contains("<a href=\"pkg.html#total\"><code>total</code></a>"),
        "{html}"
    );
    // The entry page and the lists link with the same paths.
    let summary = file(&out.files, "llms.txt");
    assert!(
        summary.contains("- [cart](cart.md): Carts, summed by [`total`](pkg.md#total)."),
        "{summary}"
    );
}

#[test]
fn links_from_a_nested_module_page_climb_out() {
    let out = link_files(false);
    let deep = file(&out.files, "shop/deep.md");
    assert!(
        deep.contains(
            "See [`total`](../pkg.md#total), [`Cart.add`](../cart.md#Cart.add), [`cart.Cart`](../cart.md#Cart) and [`pkg.cart`](../cart.md)."
        ),
        "{deep}"
    );
    let html = file(&out.files, "shop/deep.html");
    assert!(
        html.contains("<a href=\"../cart.html#Cart.add\">"),
        "{html}"
    );
    // `llms-full.txt` is read from the output directory.
    let full = file(&out.files, "llms-full.txt");
    assert!(full.contains("[`total`](pkg.md#total)"), "{full}");
    assert!(
        full.contains("Adds one to a [`Cart`](cart.md#Cart)"),
        "{full}"
    );
    assert!(!full.contains("](../"), "{full}");
}

#[test]
fn a_std_name_links_to_the_reference() {
    let out = link_files(false);
    let pkg = file(&out.files, "pkg.md");
    assert!(
        pkg.contains("[`std.text.split`](spec/std/text.md#r-std-text.string.split)"),
        "{pkg}"
    );
    let html = file(&out.files, "pkg.html");
    assert!(
        html.contains("<a href=\"spec/std/text.md#r-std-text.string.split\">"),
        "{html}"
    );
}

#[test]
fn a_broken_or_private_name_is_plain_code_and_one_warning() {
    let out = link_files(false);
    let pkg = file(&out.files, "pkg.md");
    assert!(
        pkg.contains("`nothing`") && !pkg.contains("[`nothing`]"),
        "{pkg}"
    );
    assert!(pkg.contains("Sums a `helper` result"), "{pkg}");
    // Markdown with a target, and a link that is not a name, are untouched.
    assert!(
        pkg.contains("not `[`plain`]` or [`a link`](x.md)."),
        "{pkg}"
    );
    let broken: Vec<(&str, &str, bool)> = out
        .broken
        .iter()
        .map(|b| (b.module.as_str(), b.name.as_str(), b.private))
        .collect();
    assert_eq!(
        broken,
        [("shop", "nothing", false), ("shop", "helper", true)],
        "{broken:?}"
    );
    assert!(out.files.iter().any(|f| f.path == "pkg.html"));
}

#[test]
fn a_private_name_links_when_private_items_are_documented() {
    let out = link_files(true);
    let pkg = file(&out.files, "pkg.md");
    assert!(pkg.contains("Sums a [`helper`](#helper) result"), "{pkg}");
    let names: Vec<&str> = out.broken.iter().map(|b| b.name.as_str()).collect();
    assert_eq!(names, ["nothing"]);
}
