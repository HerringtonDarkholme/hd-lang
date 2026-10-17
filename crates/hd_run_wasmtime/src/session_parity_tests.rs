//! One REPL session on wasmtime and on Node (`f1c55020:future-work/compiler/repl-plan.md` job W4): the
//! JS host's session (`host/core.mjs`, `createSession`) runs the same
//! input modules with the same transcript, and each engine's journal
//! replays on the other byte for byte (`hd_run::journal`'s row format).
//!
//! The session runs here first. Its modules, texts, transcript and journal
//! go to a scratch directory, where `scripts/repl-parity.js` runs them on
//! Node, checks the transcript, rebuilds a Node session from the wasmtime
//! journal and writes the Node session's own journal. A new wasmtime
//! session is then rebuilt from that one. Each rebuild makes no host call,
//! prints nothing, records a journal equal to the one it replayed, and
//! goes on live: a later input reads the rebuilt bindings. The wasmtime
//! rebuild holds each run of inputs that joined the session in one module
//! (#228), so its journal names that module where Node's names each
//! input's own; it is equal to Node's but for those hashes.

use std::path::{Path, PathBuf};
use std::process::Command;

use super::{Again, Repl, build_prelude, text, with_session};
use hd_run::Outcome;

/// Bindings, a function, a value, a clock read, a sleep, a line longer
/// than a request's 8 inline bytes, a panic, and inputs that use earlier
/// bindings.
const INPUTS: [&str; 10] = [
    "use std.time.{now, sleep, ms}\n",
    "rate := +3\n",
    "fn price(n: i32) -> i32:\n    n * rate\n",
    "price(+2)\n",
    "started := now().unix_milliseconds()\n",
    "started > 0\n",
    "sleep!(ms(+1))\n",
    "println(\"five cost ${price(+5)}\")\n",
    "x := [+1][3]\n",
    "price(+5) + rate\n",
];

/// The input each engine runs after its rebuild.
const AFTER: &str = "println(price(+1))\n";

/// The scratch directory, removed on drop.
struct Scratch(PathBuf);

impl Scratch {
    fn new() -> Scratch {
        let dir = std::env::temp_dir().join(format!("hd-repl-parity-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("the scratch directory");
        Scratch(dir)
    }

    fn write(&self, name: &str, bytes: &[u8]) {
        std::fs::write(self.0.join(name), bytes).expect("writes");
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// Writes one input that ran as `N.hd`, `N.wasm`, `N.status`, `N.out`
/// and `N.err`: its text, module, status and output.
fn write_input(
    dir: &Scratch,
    number: u32,
    source: &str,
    wasm: &[u8],
    ran: (Outcome, String, String),
) {
    let (outcome, out, err) = ran;
    let Outcome::Exit(status) = outcome else {
        panic!("input {number} ended with {outcome:?}");
    };
    dir.write(&format!("{number}.hd"), source.as_bytes());
    dir.write(&format!("{number}.wasm"), wasm);
    dir.write(&format!("{number}.status"), status.to_string().as_bytes());
    dir.write(&format!("{number}.out"), out.as_bytes());
    dir.write(&format!("{number}.err"), err.as_bytes());
}

fn script() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../scripts/repl-parity.js")
}

#[test]
fn a_session_runs_and_replays_the_same_on_node() {
    let dir = Scratch::new();
    let mut numbers = Vec::new();
    let mut texts = std::collections::BTreeMap::new();
    with_session(|session, io| {
        let store = hd_cache::MemoryStore::default();
        let prelude = build_prelude(&store);
        assert!(prelude.diags.is_empty(), "{}", prelude.render());
        let wasm = prelude.wasm.expect("the prelude");
        let outcome = session.run_input(0, "", &wasm).expect("the prelude runs");
        write_input(&dir, 0, "", &wasm, (outcome, text(&io.out), text(&io.err)));
        numbers.push(0);
        texts.insert(0, String::new());
        let mut built = hd_driver::Session::start();
        built.add(prelude.session_added.expect("what the prelude adds"));
        let mut repl = Repl {
            session,
            built,
            files: hd_project::MemorySources::default(),
            next: 0,
            texts: std::collections::BTreeMap::from([(0, String::new())]),
            store,
        };
        for input in INPUTS {
            let built = repl.build(input);
            let (number, wasm) = (built.number, built.wasm.clone());
            let outcome = repl.run(built);
            write_input(
                &dir,
                number,
                input,
                &wasm,
                (outcome, text(&io.out), text(&io.err)),
            );
            numbers.push(number);
            texts.insert(number, input.to_owned());
        }
        dir.write(
            "wasmtime.journal",
            repl.session.journal().expect("recording"),
        );
        let after = repl.build(AFTER);
        let (number, wasm) = (after.number, after.wasm.clone());
        let outcome = repl.run(after);
        write_input(
            &dir,
            number,
            AFTER,
            &wasm,
            (outcome, text(&io.out), text(&io.err)),
        );
        dir.write("after", number.to_string().as_bytes());
    });
    let inputs: Vec<String> = numbers.iter().map(u32::to_string).collect();
    dir.write("inputs", inputs.join("\n").as_bytes());

    let node = Command::new("node")
        .arg(script())
        .arg(&dir.0)
        .output()
        .expect("node runs");
    assert!(
        node.status.success(),
        "{}{}",
        String::from_utf8_lossy(&node.stdout),
        String::from_utf8_lossy(&node.stderr)
    );
    let said = String::from_utf8_lossy(&node.stdout);
    assert!(said.starts_with("repl parity: 11 inputs"), "{said}");
    let journal = std::fs::read(dir.0.join("node.journal")).expect("the Node journal");

    with_session(|session, io| {
        let mut repl = Repl {
            session,
            built: hd_driver::Session::start(),
            files: hd_project::MemorySources::default(),
            next: 0,
            texts,
            store: hd_cache::MemoryStore::default(),
        };
        let rebuilt = rebuild_from(&mut repl, &journal);
        assert_eq!(rebuilt.kept, numbers, "{:?}", rebuilt.reason);
        assert_eq!(repl.session.host_calls(), 0, "a host call repeated");
        assert_eq!(text(&io.out), "");
        assert_eq!(text(&io.err), "");
        assert_eq!(
            but_modules(repl.session.journal().expect("recording")),
            but_modules(&journal),
            "the rebuilt journal differs from the Node journal it replayed"
        );
        repl.next = *numbers.last().expect("inputs");
        assert_eq!(repl.input(AFTER), Outcome::Exit(0), "{}", text(&io.err));
        assert_eq!(text(&io.out), "3\n");
    });
}

/// What `journal` records, with each input's module hash left out.
fn but_modules(journal: &[u8]) -> hd_run::replay::Recorded {
    let mut record = hd_run::replay::recorded(journal).expect("the journal reads");
    for input in &mut record.inputs {
        input.module = 0;
    }
    record
}

/// Rebuilds `repl`'s session from `journal` over its texts.
fn rebuild_from(repl: &mut Repl<'_>, journal: &[u8]) -> crate::Rebuilt {
    let mut again = Again {
        files: &mut repl.files,
        built: &mut repl.built,
        store: &repl.store,
        texts: &repl.texts,
        added: hd_base::HashMap::default(),
        building: std::time::Duration::ZERO,
    };
    repl.session
        .rebuild_from(journal, &mut again)
        .expect("the session rebuilds")
}
