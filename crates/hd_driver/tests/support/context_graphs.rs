use crate::contexts::UnitSet;
/// Query each dependency context in its graph order through the same lazy
/// fold the driver uses, without forcing the old readers' slice copies.
#[must_use]
pub fn unit_contexts(graph: &hd_project::ModuleGraph) -> Vec<UnitSet> {
    let contexts = crate::contexts::Contexts::new(graph.uses.len());
    graph
        .order
        .iter()
        .map(|&u| contexts.closure(graph, [u]))
        .collect()
}
