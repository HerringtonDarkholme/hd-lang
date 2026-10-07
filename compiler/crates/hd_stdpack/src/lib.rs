#![forbid(unsafe_code)]
//! `hd_stdpack`: the build-time tool that checks `lib/std` and writes the
//! std pack `hd_driver` embeds (design-overview.md §2.1). The pack needs
//! std's folder interfaces, which the subset pipeline cannot build yet;
//! `build_pack` runs the architecture driver and refuses until every
//! stage through `ModuleFinish` passes.

use hd_base::{NotImplemented, Stage, StageResult};
use hd_driver::architecture::analyze_package;
use hd_project::SourceSet;

/// The std pack: interface blobs by folder, in path order.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct StdPack {
    pub folders: Vec<(String, Vec<u8>)>,
}

pub fn build_pack(std: &dyn SourceSet) -> StageResult<StdPack> {
    let r = analyze_package("std", std);
    for s in [
        Stage::Parse,
        Stage::FolderIface,
        Stage::HeaderCheck,
        Stage::ModulePrep,
        Stage::Body,
        Stage::ModuleFinish,
    ] {
        let t = r.tally(s);
        if t.not_implemented + t.blocked > 0 {
            return Err(NotImplemented::new(
                s,
                format!(
                    "std pack: {} of std's units not through {}",
                    t.not_implemented + t.blocked,
                    s.name()
                ),
            ));
        }
    }
    Err(NotImplemented::new(Stage::PackageResult, "std pack writer"))
}

#[cfg(test)]
mod tests {
    use super::build_pack;
    use hd_project::MemorySources;

    #[test]
    fn refuses_until_std_checks() {
        let mut s = MemorySources::default();
        s.insert(
            "option.hd",
            "@derive(Eq)\npub enum Option[T]:\n    Some(T)\n    None\n",
        );
        assert!(build_pack(&s).is_err());
    }
}
