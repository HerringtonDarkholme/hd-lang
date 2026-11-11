//! The examples of the specification chapters and of the docs, checked in
//! this process the way the fixtures are, on one shared store, so checking
//! N examples costs N checks and std is checked once (spec/STYLE.md
//! "Examples").
//!
//! - Every `hd` and `text` fence of a numbered, stdlib or CLI chapter has
//!   one row in `spec/conformance/examples.tsv`, and every `hd` fence of
//!   README.md, `website/guide/`, `website/tour/` and `docs/compiler/` one
//!   row in `docs/examples.tsv`, keyed by its position among the file's
//!   fences.
//! - A `complete` example checks as printed (`check --tests`); a tour
//!   page's program also runs and prints its output block.
//! - A `mixed` example reports errors on exactly its marked lines: each
//!   `# error: CODE` line reports `CODE`, a bare `# error` any code, and a
//!   marked line may report further codes, such as a cascade. A `mixed`
//!   example that marks no line is left to the fixtures.
//! - A `known-failure` example must still fail, so a fixed one is
//!   reclassified. Other classifications are not checked here.
//!
//! - The package sources under the suite's `packages/` and `trees/` parse.
//!
//! `HD_CONFORMANCE_ONLY=a,b` checks only the examples whose place contains
//! one of the parts (the inventories and package sources are still
//! checked) and prints each verdict.

use std::collections::BTreeMap;
use std::path::Path;
use std::time::Instant;

use hd_base::SliceSort as _;
use hd_driver::Output;
use hd_syntax::parse;

use super::{
    Case, Fixture, Phase, Store, Verdict, byte_line, caught, check_errors, check_fixture,
    load_tree, new_store, pool, root, runtime_case, suite_root, type_case, unit_fixture,
};

/// Clear of the suite's case indices and the runner's own tests, so an
/// example's program requests are its own.
const SERIAL: usize = 1 << 41;

/// A fenced block: the word after its info string, its body, and the line
/// its body starts on.
struct Block {
    word: String,
    body: String,
    line: usize,
}

/// The fenced blocks of `text` whose opening line `open` accepts, giving
/// the word after the info string. A block ends at the next line that
/// starts with three backticks.
fn blocks<'t>(text: &'t str, open: impl Fn(&'t str) -> Option<&'t str>) -> Vec<Block> {
    let mut out = Vec::new();
    let mut lines = text.lines().enumerate();
    while let Some((index, line)) = lines.next() {
        let Some(word) = open(line) else {
            continue;
        };
        let mut body = String::new();
        for (_, line) in lines.by_ref() {
            if line.starts_with("```") {
                out.push(Block {
                    word: word.trim().to_owned(),
                    body,
                    line: index + 2,
                });
                break;
            }
            body.push_str(line);
            body.push('\n');
        }
    }
    out
}

/// A chapter's examples: its `hd` and `text` fences.
fn chapter_blocks(text: &str) -> Vec<Block> {
    blocks(text, |line| {
        matches!(line, "```hd" | "```text").then_some("")
    })
}

/// A document's `hd` fences, whose info string may carry a word
/// (`hd example`, `hd src/pricing.hd`).
fn doc_blocks(text: &str) -> Vec<Block> {
    blocks(text, |line| {
        line.strip_prefix("```hd")
            .filter(|rest| rest.is_empty() || rest.starts_with([' ', '\t']))
    })
}

/// An inventory: document, then block number, to classification.
type Inventory = BTreeMap<String, BTreeMap<usize, String>>;

fn inventory(path: &Path) -> Inventory {
    let text = std::fs::read_to_string(path).expect("example inventory");
    let mut inventory = Inventory::new();
    for row in text.lines().skip(1).filter(|row| !row.is_empty()) {
        let mut fields = row.split('\t');
        let document = fields.next().unwrap_or_default();
        let block = fields.next().unwrap_or_default().parse().unwrap_or(0);
        let classification = fields.next().unwrap_or_default();
        inventory
            .entry(document.to_owned())
            .or_default()
            .insert(block, classification.to_owned());
    }
    inventory
}

/// A line's error marker's code: `# error: CODE`, or a bare `# error` for
/// the code `any`, then only spaces or tabs to the end of the line.
fn error_marker(line: &str) -> Option<&str> {
    line.match_indices("# error").find_map(|(at, marker)| {
        let rest = &line[at + marker.len()..];
        let (code, tail) = match rest.strip_prefix(": ") {
            Some(coded) => {
                let end = coded
                    .find(|c: char| !matches!(c, 'a'..='z' | '0'..='9' | '-'))
                    .unwrap_or(coded.len());
                (&coded[..end], &coded[end..])
            }
            None => ("any", rest),
        };
        let blank = tail.chars().all(|c| matches!(c, ' ' | '\t'));
        (blank && !code.is_empty()).then_some(code)
    })
}

/// A marked line of an error example, and its code, `any` for any code.
struct Mark {
    line: usize,
    code: String,
}

fn marks(body: &str) -> Vec<Mark> {
    body.lines()
        .enumerate()
        .filter_map(|(index, line)| {
            error_marker(line).map(|code| Mark {
                line: index + 1,
                code: code.to_owned(),
            })
        })
        .collect()
}

enum Expect {
    /// `complete`: checks as printed, and a tour program runs.
    Passes,
    /// `known-failure` that marks no line: does not check as printed.
    StillFails,
    /// `mixed`: reports errors on exactly its marked lines.
    Rejects(Vec<Mark>),
    /// `known-failure` that marks lines: does not report exactly them.
    StillMisses(Vec<Mark>),
}

struct Example {
    /// `DOCUMENT:LINE (block N, CLASSIFICATION)`, for a failure.
    place: String,
    /// The chapter, or the document's path, as the case's specification.
    document: String,
    text: String,
    phase: Phase,
    expect: Expect,
}

fn markdown(name: &str) -> bool {
    Path::new(name)
        .extension()
        .is_some_and(|extension| extension == "md")
}

/// The chapters whose examples are inventoried, under `spec/`: numbered,
/// stdlib, and CLI.
fn chapters(spec: &Path) -> Vec<String> {
    let mut names = Vec::new();
    for directory in ["lang", "std", "cli"] {
        let mut listed: Vec<String> = std::fs::read_dir(spec.join(directory))
            .expect("spec chapter directory")
            .flatten()
            .map(|entry| entry.file_name().to_string_lossy().into_owned())
            .filter(|name| {
                let numbered = name.as_bytes().get(..3).is_some_and(|prefix| {
                    prefix[0].is_ascii_digit() && prefix[1].is_ascii_digit() && prefix[2] == b'-'
                });
                markdown(name) && name != "README.md" && (directory != "lang" || numbered)
            })
            .collect();
        listed.sort_stable();
        names.extend(listed.into_iter().map(|name| format!("{directory}/{name}")));
    }
    names
}

/// The checked examples of the chapters, and the inventory's failures.
fn spec_examples(root: &Path) -> (Vec<Example>, Vec<String>) {
    let spec = root.join("spec");
    let inventory = inventory(&spec.join("conformance/examples.tsv"));
    let empty = BTreeMap::new();
    let mut examples = Vec::new();
    let mut failures = Vec::new();
    for chapter in chapters(&spec) {
        let text = std::fs::read_to_string(spec.join(&chapter)).expect("spec chapter");
        let found = chapter_blocks(&text);
        let indexed = inventory.get(&chapter).unwrap_or(&empty);
        if indexed.len() != found.len() {
            failures.push(format!(
                "spec/{chapter} has {} examples but {} inventory entries",
                found.len(),
                indexed.len()
            ));
        }
        for (index, block) in found.into_iter().enumerate() {
            let classification = indexed.get(&(index + 1)).map_or("", String::as_str);
            let marked = marks(&block.body);
            let expect = match classification {
                "mixed" if !marked.is_empty() => Expect::Rejects(marked),
                "known-failure" if !marked.is_empty() => Expect::StillMisses(marked),
                "known-failure" => Expect::StillFails,
                "complete" => Expect::Passes,
                _ => continue,
            };
            examples.push(Example {
                place: format!(
                    "spec/{chapter}:{} (block {}, {classification})",
                    block.line,
                    index + 1
                ),
                document: chapter.clone(),
                text: block.body,
                phase: Phase::Type,
                expect,
            });
        }
    }
    (examples, failures)
}

/// The documents `docs/examples.tsv` inventories, in order.
fn documents(root: &Path) -> Vec<String> {
    let mut documents = vec!["README.md".to_owned()];
    for directory in ["website/guide", "website/tour", "docs/compiler"] {
        let mut listed: Vec<String> = std::fs::read_dir(root.join(directory))
            .expect("doc directory")
            .flatten()
            .map(|entry| entry.file_name().to_string_lossy().into_owned())
            .filter(|name| markdown(name))
            .collect();
        listed.sort_stable();
        documents.extend(listed.into_iter().map(|name| format!("{directory}/{name}")));
    }
    documents
}

/// A tour page's program as a runtime case: an `# expect-stdout:` line for
/// each line of its output block, unless the program has tests.
fn tour_program(code: &str, page: &str) -> String {
    let mut out = String::new();
    if !code.lines().any(|line| line.starts_with("tests:")) {
        let output = blocks(page, |line| (line == "```output").then_some(""));
        if let Some(block) = output.first() {
            let body = block.body.strip_suffix('\n').unwrap_or(&block.body);
            for line in body.split('\n') {
                out.push_str("# expect-stdout: ");
                out.push_str(&line.replace('\\', "\\\\"));
                out.push('\n');
            }
        }
    }
    out.push_str(code);
    out
}

/// Whether `document` is a numbered tour page, `website/tour/NN-slug.md`.
fn tour_page(document: &str) -> bool {
    document.strip_prefix("website/tour/").is_some_and(|name| {
        let digits = name.bytes().take_while(u8::is_ascii_digit).count();
        digits > 0 && name.as_bytes().get(digits) == Some(&b'-')
    })
}

/// The checked blocks of the docs, and the inventory's failures.
fn doc_examples(root: &Path) -> (Vec<Example>, Vec<String>) {
    let inventory = inventory(&root.join("docs/examples.tsv"));
    let documents = documents(root);
    let mut failures: Vec<String> = inventory
        .keys()
        .filter(|document| !documents.contains(document))
        .map(|document| format!("{document} is inventoried but is not a checked document"))
        .collect();
    let empty = BTreeMap::new();
    let mut examples = Vec::new();
    for document in documents {
        let text = std::fs::read_to_string(root.join(&document)).expect("doc file");
        let found = doc_blocks(&text);
        let indexed = inventory.get(&document).unwrap_or(&empty);
        if indexed.len() != found.len() {
            failures.push(format!(
                "{document} has {} hd blocks but {} inventory entries",
                found.len(),
                indexed.len()
            ));
        }
        let tour = tour_page(&document);
        for (index, block) in found.into_iter().enumerate() {
            let classification = indexed.get(&(index + 1)).map_or("", String::as_str);
            let place = format!(
                "{document}:{} (block {}, {classification})",
                block.line,
                index + 1
            );
            let expect = match classification {
                "complete" => Expect::Passes,
                "known-failure" => Expect::StillFails,
                "fragment" => continue,
                _ => {
                    failures.push(format!("{place}: unknown classification"));
                    continue;
                }
            };
            // A tour page's program (the block with no word) runs, and
            // prints its output block.
            let program = tour && block.word.is_empty() && matches!(expect, Expect::Passes);
            examples.push(Example {
                place,
                document: document.clone(),
                text: if program {
                    tour_program(&block.body, &text)
                } else {
                    block.body
                },
                phase: if program { Phase::Runtime } else { Phase::Type },
                expect,
            });
        }
    }
    (examples, failures)
}

/// The errors of `output`, each as `; CODE at line N`, or `; CODE in FILE`
/// for an error outside the example.
fn describe(fixture: &Fixture, output: &Output) -> String {
    check_errors(output)
        .into_iter()
        .map(|index| {
            let span = output.diags.primary[index];
            let code = output.diags.code[index].as_str();
            match output.files.get(span.file.idx()) {
                Some(file) if *file == fixture.primary => {
                    format!("{code} at line {}", byte_line(&fixture.text, span.lo))
                }
                file => format!("{code} in {}", file.map_or("?", String::as_str)),
            }
        })
        .fold(String::new(), |out, error| format!("{out}; {error}"))
}

/// What an error example's check gets wrong about its marks: no error, a
/// marked line without its code, or an error on an unmarked line.
fn mark_problems(fixture: &Fixture, output: &Output, marks: &[Mark]) -> Vec<String> {
    let all = check_errors(output);
    if all.is_empty() {
        return vec!["expected rejection, got no error".to_owned()];
    }
    let located: Vec<(usize, &str)> = all
        .into_iter()
        .filter_map(|index| {
            let span = output.diags.primary[index];
            (output.files.get(span.file.idx()) == Some(&fixture.primary)).then(|| {
                (
                    byte_line(&fixture.text, span.lo),
                    output.diags.code[index].as_str(),
                )
            })
        })
        .collect();
    let show = |errors: &[&(usize, &str)]| {
        errors
            .iter()
            .map(|(line, code)| format!("{code} at line {line}"))
            .collect::<Vec<_>>()
            .join(", ")
    };
    let mut problems = Vec::new();
    for mark in marks {
        let reported: Vec<_> = located
            .iter()
            .filter(|(line, _)| *line == mark.line)
            .collect();
        if !reported
            .iter()
            .any(|(_, code)| mark.code == "any" || mark.code == *code)
        {
            let found = if reported.is_empty() {
                String::new()
            } else {
                format!(" (found {})", show(&reported))
            };
            problems.push(format!(
                "no error {} on line {}{found}",
                mark.code, mark.line
            ));
        }
    }
    let unmarked: Vec<_> = located
        .iter()
        .filter(|(line, _)| !marks.iter().any(|mark| mark.line == *line))
        .collect();
    if !unmarked.is_empty() {
        problems.push(format!("unmarked errors reported: {}", show(&unmarked)));
    }
    problems
}

/// The example's failure, or `None` when it holds to its classification.
fn judge(example: &Example, serial: usize, store: &Store) -> Option<String> {
    let fixture = unit_fixture(&example.text);
    let case = Case {
        path: example.place.clone(),
        phase: example.phase,
        expectation: "accept".to_owned(),
        specification: example.document.clone(),
    };
    let (marks, misses) = match &example.expect {
        Expect::Rejects(marks) => (marks, false),
        Expect::StillMisses(marks) => (marks, true),
        Expect::Passes | Expect::StillFails => {
            let verdict = caught(|| match example.phase {
                Phase::Runtime => runtime_case(&case, &fixture, store, SERIAL + serial),
                _ => type_case(&case, &fixture, store),
            });
            if let Verdict::Crash(message) = &verdict {
                return Some(format!("crashes the compiler: {message}"));
            }
            return match (&example.expect, verdict == Verdict::Pass) {
                (Expect::StillFails, true) => {
                    Some("now checks as printed; classify it complete".to_owned())
                }
                (Expect::Passes, false) => {
                    let errors = describe(&fixture, &check_fixture(&fixture, store));
                    let how = match &verdict {
                        Verdict::Fail(code) => format!("fail:{code}"),
                        other => format!("{other:?}"),
                    };
                    Some(format!("does not check as printed: {how}{errors}"))
                }
                _ => None,
            };
        }
    };
    let mut problems = Vec::new();
    let verdict = caught(|| {
        problems = mark_problems(&fixture, &check_fixture(&fixture, store), marks);
        Verdict::Pass
    });
    if let Verdict::Crash(message) = verdict {
        return Some(format!("crashes the compiler: {message}"));
    }
    match (misses, problems.is_empty()) {
        (false, false) => Some(format!(
            "does not reject exactly its marked lines: {}",
            problems.join("; ")
        )),
        (true, true) => Some("now rejects exactly its marked lines; classify it mixed".to_owned()),
        _ => None,
    }
}

#[test]
fn spec_and_doc_examples_check_as_printed() {
    let root = root();
    let (mut examples, mut failures) = spec_examples(&root);
    let (docs, doc_failures) = doc_examples(&root);
    examples.extend(docs);
    failures.extend(doc_failures);
    failures.extend(package_source_failures());
    let only = std::env::var("HD_CONFORMANCE_ONLY").ok();
    if let Some(only) = &only {
        let parts: Vec<&str> = only.split(',').filter(|part| !part.is_empty()).collect();
        examples.retain(|example| parts.iter().any(|part| example.place.contains(part)));
    }
    // The costliest first, so no slow example holds the run open: the
    // programs that run, then the longest text.
    let mut order: Vec<usize> = (0..examples.len()).collect();
    order.sort_stable_by_key(|&index| {
        let example = &examples[index];
        (
            example.phase != Phase::Runtime,
            std::cmp::Reverse(example.text.len()),
        )
    });
    let store = new_store();
    let started = Instant::now();
    let problems = pool(examples.len(), &order, |index| {
        judge(&examples[index], index, &store)
    });
    let elapsed = started.elapsed();
    for (example, problem) in examples.iter().zip(&problems) {
        match problem {
            Some(problem) => failures.push(format!("{} {problem}", example.place)),
            None if only.is_some() => println!("pass  {}", example.place),
            None => {}
        }
    }
    let complete = examples
        .iter()
        .filter(|example| matches!(example.expect, Expect::Passes))
        .count();
    println!(
        "examples: {} checked, {complete} complete, in {:.1} s",
        examples.len(),
        elapsed.as_secs_f64()
    );
    assert!(
        failures.is_empty(),
        "example check failures:\n{}",
        failures
            .iter()
            .map(|failure| format!("- {failure}"))
            .collect::<Vec<_>>()
            .join("\n")
    );
}

/// Package sources (`packages/` and `trees/` under the suite) are inputs
/// of the package-role and package-tree environments, not cases; the
/// parser must accept each.
fn package_source_failures() -> Vec<String> {
    let mut failures = Vec::new();
    for directory in ["packages", "trees"] {
        let mut sources = hd_project::MemorySources::default();
        let base = suite_root().join(directory);
        load_tree(&base, &base, &mut sources);
        for entry in hd_project::SourceSet::list(&sources) {
            let bytes = hd_project::SourceSet::read(&sources, &entry.path).expect("source");
            let text = String::from_utf8_lossy(&bytes);
            if let Some(diagnostic) = parse(text.as_bytes()).diagnostics.first() {
                failures.push(format!(
                    "spec/conformance/{directory}/{}:{} does not parse: {}",
                    entry.path,
                    byte_line(&text, diagnostic.primary.lo),
                    diagnostic.code.as_str()
                ));
            }
        }
    }
    failures
}
