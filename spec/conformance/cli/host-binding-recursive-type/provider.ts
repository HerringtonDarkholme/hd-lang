type Tree =
  | { tag: "Leaf"; fields: Record<string, never> }
  | { tag: "Node"; fields: { kids: Tree[] } };

// Wraps the tree in 40 more levels; the last level shares one subtree
// twice, which crosses as two copies (module.boundary.sharing).
export default {
  render(tree: Tree): Tree {
    for (let level = 0; level < 39; level++) {
      tree = { tag: "Node", fields: { kids: [tree] } };
    }
    return { tag: "Node", fields: { kids: [tree, tree] } };
  },
};
