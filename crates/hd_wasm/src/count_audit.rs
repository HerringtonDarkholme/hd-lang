//! Checked-only dynamic count sites, inserted after count optimization.
//! The guest holds integers; source names and lines stay in custom sections.
use crate::{
    CountAudit, Runtime,
    module::{Func, HEAP, Ins, Sym, Val},
    op::Op as I,
};
use hd_base::wire::{Reader, Writer};

pub(crate) fn instrument(
    f: &mut Func,
    r: &Runtime,
    hooks: CountAudit,
    next: &mut u32,
    sites: &mut Writer,
) {
    // The observer must see the validated count, rather than observing the
    // checked runtime's own validation loads as user count observations.
    if f.origin_item.as_deref().is_some_and(|p| {
        p.starts_with("std/rt.checked")
            || p.starts_with("std/rt.alloc")
            || p.starts_with("std/rt.memory")
    }) {
        return;
    }
    if f.raw.is_some() || f.body.is_empty() {
        return;
    }
    let generated = f.origin_item.is_none();
    let body = std::mem::take(&mut f.body);
    let origins = f.origins.as_mut().map(std::mem::take);
    let outer = f.origin_at;
    let saved = f.local(Val::I32);
    let value = f.local(Val::I32);
    let wide = f.local(Val::I64);
    let address = f.local(Val::I32);
    let source = f.local(Val::I32);
    let length = f.local(Val::I32);
    for (position, ins) in body.into_iter().enumerate() {
        f.origin_at = origins.as_ref().and_then(|o| o[position]);
        let count = match ins {
            Ins::Call(callee) => crate::views::count_action(&f.symbol(callee), Some(r)).is_some(),
            _ => false,
        };
        // Keep the source caller active across ordinary/indirect calls too:
        // hashing, view construction and cold-frame glue can count internally.
        if count || !generated && matches!(ins, Ins::Call(_) | Ins::CallIndirect(_)) {
            let site = *next;
            *next += 1;
            sites.u32(site);
            sites.str(f.name.strip_suffix("#0").unwrap_or(&f.name));
            sites.str(f.origin_item.as_deref().unwrap_or(""));
            sites.u32(
                f.source_origin(f.origin_at)
                    .or(f.origin_root)
                    .unwrap_or(u32::MAX),
            );
            sites.str(
                &f.origin_at
                    .and_then(|o| f.count_origin(o))
                    .map_or_else(|| "source call".to_owned(), |n| n.reason.to_string()),
            );
            f.u32(site);
            f.call(Sym::Inst(if generated {
                hooks.generated
            } else {
                hooks.mark
            }));
            f.set(saved);
            f.push(ins);
            f.get(saved);
            f.call(Sym::Inst(hooks.mark));
            f.op(I::Drop);
            continue;
        }
        let load = match ins {
            Ins::Op(I::I32Load(m) | I::F32Load(m)) => Some((m, 4)),
            Ins::Op(I::I32Load8U(m) | I::I32Load8S(m)) => Some((m, 1)),
            Ins::Op(I::I32Load16U(m) | I::I32Load16S(m)) => Some((m, 2)),
            Ins::Op(I::I64Load(m) | I::F64Load(m)) => Some((m, 8)),
            _ => None,
        };
        if let Some((m, width)) = load
            && m.memory_index == HEAP
            && u32::try_from(m.offset).is_ok()
        {
            f.u32(u32::try_from(m.offset).expect("memory32 offset"));
            f.u32(width);
            f.call(Sym::Inst(hooks.header));
        }
        match ins {
            // Treat raw integer bits conservatively as possible addresses.
            // This can hide a candidate but cannot flag a real escape.
            Ins::Op(I::I32Store(m) | I::I64Store(m))
                if m.memory_index == HEAP && u32::try_from(m.offset).is_ok() =>
            {
                let is_wide = matches!(ins, Ins::Op(I::I64Store(_)));
                f.set(if is_wide { wide } else { value });
                f.op(I::LocalTee(address));
                f.get(if is_wide { wide } else { value });
                f.push(ins);
                f.get(address);
                f.u32(u32::try_from(m.offset).expect("memory32 offset"));
                f.op(I::I32Add);
                f.u32(if is_wide { 8 } else { 4 });
                f.call(Sym::Inst(hooks.escape_range));
                continue;
            }
            Ins::Op(I::MemoryCopy { dst_mem: HEAP, .. }) => {
                f.set(length);
                f.set(source);
                f.op(I::LocalTee(address));
                f.get(source);
                f.get(length);
                f.push(ins);
                f.get(address);
                f.get(length);
                f.call(Sym::Inst(hooks.escape_range));
                continue;
            }
            Ins::GlobalSet(g)
                if matches!(
                    f.symbols.global(g),
                    crate::module::Global::Storage(_, _, Val::I32)
                ) =>
            {
                f.call(Sym::Inst(hooks.observe));
            }
            _ => {}
        }
        f.push(ins);
    }
    f.origin_at = outer;
    f.rebuild_callees();
}

/// Resolve the private site's source anchors against this build's sources.
/// This happens after linking, so cached machine code never freezes line numbers.
pub fn resolve(wasm: &mut Vec<u8>, line: impl Fn(&str, u32) -> Option<(String, u32)>) {
    fn leb(bytes: &mut &[u8]) -> Option<usize> {
        let mut n = 0usize;
        for shift in (0..35).step_by(7) {
            let (&b, rest) = bytes.split_first()?;
            *bytes = rest;
            n |= usize::from(b & 127) << shift;
            if b < 128 {
                return Some(n);
            }
        }
        None
    }
    let Some(mut bytes) = wasm.get(8..) else {
        return;
    };
    let mut data = None;
    while let Some((&id, rest)) = bytes.split_first() {
        bytes = rest;
        let Some(len) = leb(&mut bytes) else {
            return;
        };
        let Some((mut section, rest)) = bytes.split_at_checked(len) else {
            return;
        };
        bytes = rest;
        if id == 0 {
            let Some(len) = leb(&mut section) else {
                return;
            };
            let Some((name, section)) = section.split_at_checked(len) else {
                return;
            };
            if name == b"hd.count.anchors" {
                data = Some(section);
            }
        }
    }
    let Some(data) = data else {
        return;
    };
    let mut source = Reader::new(data);
    if source.u8() != 1 {
        return;
    }
    let mut out = Writer::default();
    out.u8(1);
    while !source.at_end() && source.ok() {
        let site = source.u32();
        let function = source.str();
        let item = source.str();
        let origin = source.u32();
        let reason = source.str();
        let (file, number) = line(item, origin).unwrap_or_else(|| ("<generated>".to_owned(), 0));
        out.u32(site);
        out.str(function);
        out.str(&file);
        out.u32(number);
        out.str(reason);
    }
    if !source.ok() {
        return;
    }
    let mut section = wasm_encoder::Module::new();
    section.section(&wasm_encoder::CustomSection {
        name: "hd.count.sites".into(),
        data: out.bytes.into(),
    });
    wasm.extend_from_slice(&section.finish()[8..]);
}
