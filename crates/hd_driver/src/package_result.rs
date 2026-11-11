//! The package result (#1510, `crates/hd_cache/DESIGN.md` §5.5): one
//! `depfiles` entry per warm `hd check` whose inputs are unchanged, keyed by
//! every package's listing (each file's path and stamp) and the run's
//! settings. A run whose key it holds takes the stored result instead of
//! decoding each set's last-run record and looking up each module's entries.
//!
//! The entry holds what a check's caller reads besides the file lists: the
//! diagnostics, the analysis findings of a lint run, the entry rows
//! (`Output::entry_capabilities`), and for `hd check FILE` the `use` edges
//! of the files its roots reach (`Output::uses`). A run with an internal
//! error, which may come from a damaged entry rather than the sources, is
//! not stored; a file listed without a stamp keeps the per-module path.
//!
//! Per-run cost, n files: the listing's one `stat` per file (as before), one
//! hash of each listed path and stamp, and one entry lookup; on a hit, the
//! entry's bytes, which grow with the diagnostics and the FILE closure,
//! never with n. Writing costs one entry of that size, on a run that missed.

use hd_base::SliceSort as _;
use hd_base::wire::{Reader, Writer};
use hd_base::{DefId, FileId, Hash128, Range32, Span, StableHasher};
use hd_cache::{CacheStore, EntryKind};
use hd_diag::buf::{CauseKind, FixEdit, FixSafety, RootKey};
use hd_diag::{Code, DiagBuf, Severity};
use hd_project::{ModuleTable, PackageIn};

use crate::analysis::Sink;
use crate::{CapabilityEntry, EntryCapability, Goal, Host, Packages};

/// What a stored package result gives back.
pub(crate) struct Stored {
    pub entry_capabilities: Vec<EntryCapability>,
    pub diags: DiagBuf,
    pub analysis_diags: DiagBuf,
    /// Per file, its `use` edges: those of the files a FILE run's roots
    /// reach, empty for every other file and for a whole-package run.
    pub uses: Vec<Vec<usize>>,
}

/// The key of a run's package result, or `None` when the run may not use
/// one: any goal but `hd check`'s (with or without `--tests`), a request
/// for more than diagnostics, entry rows and `use` edges, or a file listed
/// without a stamp.
#[allow(clippy::too_many_arguments)]
pub(crate) fn key(
    goal: &Goal,
    package: &str,
    packages: &Packages<'_>,
    host: &Host<'_>,
    analysis: Option<&Sink<'_>>,
    ins: &[PackageIn<'_>],
    table: &ModuleTable,
    toolchain: Hash128,
) -> Option<Hash128> {
    let plain = matches!(goal, Goal::Analyze | Goal::CheckTests)
        && package != "std"
        && host.render_tir.is_empty()
        // The sets host declarations are read from must be the packages'
        // own, whose files the key holds.
        && packages.host_declaration_sources.iter().all(|d| {
            d.identity()
                .is_some_and(|id| ins.iter().any(|p| p.sources.identity() == Some(id)))
        })
        && !packages.instruction_origins
        && !packages.single_file
        // `Output::scopes` is not stored: only `hd lint`'s broken-link pass
        // reads it, and the CLI stores that command's whole result.
        && !packages.doc_scopes
        && packages.session.is_none()
        && packages.reuse.is_none()
        && packages.inputs_from.is_none()
        && packages.failed_inputs.is_empty()
        && packages.body_fuel.is_none()
        && !packages.checked
        && !packages.verify_own;
    if !plain {
        return None;
    }
    let mut k = StableHasher::new("package-result-v2");
    k.hash(toolchain);
    k.u16(super::LAYOUT);
    k.str(package);
    k.u8(u8::from(matches!(goal, Goal::CheckTests)));
    // The consumer is compiled into the toolchain; what it asks for
    // decides which findings a run collects.
    k.u8(crate::analysis::result_tag(analysis));
    match &packages.check_roots {
        None => k.u8(0),
        Some(roots) => {
            k.u8(1);
            k.u32(u32::try_from(roots.len()).ok()?);
            for root in roots {
                k.str(root);
            }
        }
    }
    k.u8(u8::from(packages.entry_capabilities));
    k.str(packages.std_leaf.unwrap_or(""));
    k.str(&format!(
        "{:?} {:?} {:?} {:?}",
        packages.profile, packages.target, packages.level, packages.build_profile
    ));
    for d in &packages.deps {
        k.u8(u8::from(d.fetched));
    }
    // Host packages: what the run's host-contract key reads, and the
    // declarations the manifest holds.
    for h in &packages.host_packages {
        k.u16(h.package);
        k.str(&h.identity);
        k.str(h.versioned_source.as_deref().unwrap_or(""));
        k.str(&h.manifest);
        k.str(&format!("{:?}", h.declarations));
    }
    k.u32(u32::try_from(ins.len()).ok()?);
    for p in ins {
        k.str(p.name);
        k.str(p.identity);
        k.str(&format!("{:?}", p.scope));
        for list in [&p.requires, &p.dev_requires] {
            k.u32(u32::try_from(list.len()).ok()?);
            for (name, at) in list {
                k.str(name);
                k.u16(*at);
            }
        }
        k.u32(u32::try_from(p.entries.len()).ok()?);
        for e in &p.entries {
            k.str(e);
        }
        // A set without an identity has no last-run record either; the
        // embedded std's files are the toolchain's.
        match p.sources.identity() {
            Some(id) => k.hash(id),
            None if std::ptr::eq(p, ins.last()?) => k.str("std"),
            None => return None,
        }
        k.u8(u8::from(p.sources.partial()));
    }
    k.u32(u32::try_from(table.sources.len()).ok()?);
    for ((package, path), stamp) in table.sources.iter().zip(&table.stamps) {
        k.u16(*package);
        k.str(path);
        k.hash((*stamp)?);
    }
    Some(k.finish())
}

fn entry_key(toolchain: Hash128, key: Hash128) -> Hash128 {
    hd_cache::depfiles_key(toolchain, &format!("pkg:{:032x}", key.0))
}

/// The stored result under `key`, for a run of `files` files. A lint code
/// the diagnostic table does not name resolves through the run's analysis
/// consumer, as its per-module entries do.
pub(crate) fn read(
    store: &dyn CacheStore,
    toolchain: Hash128,
    key: Hash128,
    files: usize,
    analysis: Option<&Sink<'_>>,
) -> Option<Stored> {
    let at = entry_key(toolchain, key);
    let bytes = store.get(EntryKind::Depfiles, &at)?;
    let (h, sections) = hd_cache::decode_entry(&bytes).ok()?;
    if h.key != at || h.toolchain != toolchain || h.layout != super::LAYOUT {
        return None;
    }
    let mut r = Reader::new(sections.first()?.1);
    let entry_capabilities = (0..r.count())
        .map(|_| {
            let span = read_span(&mut r);
            let selectors = (0..r.count()).map(|_| r.str().to_owned()).collect();
            let entry = match r.u8() {
                0 => CapabilityEntry::Main,
                1 => CapabilityEntry::Task,
                _ => CapabilityEntry::Test(r.str().to_owned()),
            };
            EntryCapability {
                span,
                selectors,
                entry,
                written_row: r.u8() != 0,
            }
        })
        .collect();
    let code =
        |name: &str| Code::from_name(name).or_else(|| crate::analysis::code_of(analysis, name));
    let diags = read_diags(&mut r, &code)?;
    let analysis_diags = read_diags(&mut r, &code)?;
    let mut uses = vec![Vec::new(); files];
    for _ in 0..r.count() {
        let file = r.u32() as usize;
        let edges: Vec<usize> = (0..r.count()).map(|_| r.u32() as usize).collect();
        if edges.iter().any(|&e| e >= files) {
            return None;
        }
        *uses.get_mut(file)? = edges;
    }
    (r.ok() && r.at_end()).then_some(Stored {
        entry_capabilities,
        diags,
        analysis_diags,
        uses,
    })
}

/// Whether a run's result may be stored: an internal error may come from a
/// damaged cache entry rather than the sources, so it is reported again.
pub(crate) fn storable(diags: &DiagBuf) -> bool {
    (0..diags.len()).all(|i| !diags.is_internal(i))
}

/// Stores a run's result under `key`. With check roots, `uses` is written
/// for the files the roots reach.
pub(crate) fn write(
    store: &dyn CacheStore,
    toolchain: Hash128,
    key: Hash128,
    out: &crate::Output,
    roots: Option<&[String]>,
) {
    let mut w = Writer::default();
    w.len_of(&out.entry_capabilities);
    for row in &out.entry_capabilities {
        write_span(&mut w, row.span);
        w.len_of(&row.selectors);
        for s in &row.selectors {
            w.str(s);
        }
        match &row.entry {
            CapabilityEntry::Main => w.u8(0),
            CapabilityEntry::Task => w.u8(1),
            CapabilityEntry::Test(name) => {
                w.u8(2);
                w.str(name);
            }
        }
        w.u8(u8::from(row.written_row));
    }
    write_diags(&mut w, &out.diags);
    write_diags(&mut w, &out.analysis_diags);
    // The FILE closure as `hd check FILE` walks it: from each root's file
    // through the `use` edges.
    let mut reached = vec![false; out.files.len()];
    let mut stack: Vec<usize> = roots.map_or_else(Vec::new, |roots| {
        out.files
            .iter()
            .enumerate()
            .filter(|(_, f)| roots.contains(f))
            .map(|(i, _)| i)
            .collect()
    });
    let mut closure = Vec::new();
    while let Some(f) = stack.pop() {
        if f < reached.len() && !std::mem::replace(&mut reached[f], true) {
            closure.push(f);
            stack.extend(out.uses.get(f).into_iter().flatten().copied());
        }
    }
    closure.sort_stable();
    w.len_of(&closure);
    for f in closure {
        let edges = out.uses.get(f).map_or(&[][..], Vec::as_slice);
        w.u32(crate::u32_of(f));
        w.len_of(edges);
        for &e in edges {
            w.u32(crate::u32_of(e));
        }
    }
    let at = entry_key(toolchain, key);
    let bytes = hd_cache::encode_entry(
        EntryKind::Depfiles,
        super::LAYOUT,
        at,
        toolchain,
        0,
        &[(0, 0, &w.bytes)],
    );
    store.put(EntryKind::Depfiles, &at, &bytes);
}

fn write_span(w: &mut Writer, s: Span) {
    w.u32(s.file.raw());
    w.u32(s.lo);
    w.u32(s.hi);
}

fn read_span(r: &mut Reader<'_>) -> Span {
    Span {
        file: FileId::from_raw(r.u32()),
        lo: r.u32(),
        hi: r.u32(),
    }
}

fn write_range(w: &mut Writer, r: Range32) {
    w.u32(r.start);
    w.u32(r.len);
}

fn read_range(r: &mut Reader<'_>) -> Range32 {
    Range32 {
        start: r.u32(),
        len: r.u32(),
    }
}

/// A buffer's columns as they are, so the replay renders, orders and
/// deduplicates exactly as the run that stored it.
fn write_diags(w: &mut Writer, d: &DiagBuf) {
    w.str(&d.text);
    w.len_of(&d.code);
    for i in 0..d.code.len() {
        w.str(d.code[i].as_str());
        w.u8(u8::from(d.severity[i] == Severity::Warning));
        write_span(w, d.primary[i]);
        write_range(w, d.message[i]);
        write_range(w, d.labels[i]);
        write_range(w, d.fixes[i]);
        match d.root[i] {
            None => w.u8(u8::MAX),
            Some(root) => {
                w.u8(root.cause as u8);
                w.u32(root.item.raw());
                w.u32(root.value);
            }
        }
    }
    w.len_of(&d.label_span);
    for (span, msg) in d.label_span.iter().zip(&d.label_msg) {
        write_span(w, *span);
        write_range(w, *msg);
    }
    w.len_of(&d.fix_title);
    for f in 0..d.fix_title.len() {
        write_range(w, d.fix_title[f]);
        w.u8(u8::from(d.fix_safety[f] == FixSafety::Suggestion));
        write_range(w, d.fix_edits[f]);
    }
    w.len_of(&d.edits);
    for e in &d.edits {
        write_span(w, e.span);
        write_range(w, e.text);
    }
}

fn read_diags(r: &mut Reader<'_>, code: &dyn Fn(&str) -> Option<Code>) -> Option<DiagBuf> {
    let mut d = DiagBuf {
        text: r.str().to_owned(),
        ..DiagBuf::default()
    };
    for _ in 0..r.count() {
        d.code.push(code(r.str())?);
        d.severity.push(if r.u8() == 0 {
            Severity::Error
        } else {
            Severity::Warning
        });
        d.primary.push(read_span(r));
        d.message.push(read_range(r));
        d.labels.push(read_range(r));
        d.fixes.push(read_range(r));
        let cause = match r.u8() {
            u8::MAX => None,
            0 => Some(CauseKind::Type),
            1 => Some(CauseKind::Name),
            2 => Some(CauseKind::Bound),
            3 => Some(CauseKind::Row),
            4 => Some(CauseKind::Init),
            5 => Some(CauseKind::Syntax),
            _ => return None,
        };
        d.root.push(cause.map(|cause| RootKey {
            item: DefId::from_raw(r.u32()),
            cause,
            value: r.u32(),
        }));
    }
    for _ in 0..r.count() {
        d.label_span.push(read_span(r));
        d.label_msg.push(read_range(r));
    }
    for _ in 0..r.count() {
        d.fix_title.push(read_range(r));
        d.fix_safety.push(if r.u8() == 0 {
            FixSafety::Exact
        } else {
            FixSafety::Suggestion
        });
        d.fix_edits.push(read_range(r));
    }
    for _ in 0..r.count() {
        d.edits.push(FixEdit {
            span: read_span(r),
            text: read_range(r),
        });
    }
    // Every range indexes its pool, and every text range lies on character
    // boundaries, so a damaged entry is a miss, never a renderer's panic.
    let within = |x: Range32, n: usize| (x.start as usize).saturating_add(x.len as usize) <= n;
    let text = |x: Range32| {
        within(x, d.text.len())
            && d.text.is_char_boundary(x.start as usize)
            && d.text.is_char_boundary((x.start + x.len) as usize)
    };
    let ok = d.message.iter().all(|&m| text(m))
        && d.label_msg.iter().all(|&m| text(m))
        && d.fix_title.iter().all(|&t| text(t))
        && d.edits.iter().all(|e| text(e.text))
        && d.labels.iter().all(|&l| within(l, d.label_span.len()))
        && d.fixes.iter().all(|&f| within(f, d.fix_title.len()))
        && d.fix_edits.iter().all(|&e| within(e, d.edits.len()));
    (r.ok() && ok).then_some(d)
}
