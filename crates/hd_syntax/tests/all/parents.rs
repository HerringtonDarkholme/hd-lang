//! `GreenTree::parent`: each node's parent from links built once, the
//! root's none.

use hd_syntax::parse;

#[test]
fn a_typed_block_preserves_its_node_and_children() {
    let parsed = parse(include_bytes!("parents.hd"));
    let mut blocks = 0;
    for node in parsed.tree.root().descendants() {
        if let Some(block) = hd_syntax::Block::cast(node) {
            let raw = std::hint::black_box(hd_syntax::Block::node)(block);
            assert_eq!(raw.index(), node.index());
            assert_eq!(raw.children().count(), node.children().count());
            blocks += 1;
        }
    }
    assert!(blocks > 0);
}

#[test]
fn each_node_walks_up_to_the_root_through_its_parent() {
    let parsed = parse(include_bytes!("parents.hd"));
    let tree = &parsed.tree;
    let root = tree.root().index();
    assert_eq!(tree.parent(root), None);
    for node in tree.root().descendants() {
        let up = tree
            .parent(node.index())
            .expect("a node below the root has a parent");
        // The parent holds the node, and the node is one of its children.
        let parent = tree.node(up);
        assert!(
            parent.index().raw() < node.index().raw() && node.end().raw() <= parent.end().raw()
        );
        assert!(parent.children().any(|c| c.index() == node.index()));
        // The walk up ends at the root.
        let mut at = node.index();
        while let Some(next) = tree.parent(at) {
            at = next;
        }
        assert_eq!(at, root);
    }
}
