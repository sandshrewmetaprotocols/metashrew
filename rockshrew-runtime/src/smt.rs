use tiny_keccak::{Keccak, Hasher};
use primitive_types::H256;

#[derive(Clone)]
pub enum Node {
    Leaf(LeafNode),
    Branch(BranchNode),
}

impl Node {
    pub fn hash(&self) -> H256 {
        match self {
            Node::Leaf(leaf) => leaf.hash,
            Node::Branch(branch) => branch.hash,
        }
    }
}

#[derive(Clone)]
pub struct LeafNode {
    pub hash: H256,
}

#[derive(Clone)]
pub struct BranchNode {
    pub hash: H256,
    pub left: Box<Node>,
    pub right: Box<Node>,
}

#[derive(Clone)]
pub struct SparseMerkleTree {
    pub root: Box<Node>,
}

impl SparseMerkleTree {
    pub fn new() -> Self {
        let mut hasher = Keccak::v256();
        let mut output = [0u8; 32];
        hasher.update(&[]);
        hasher.finalize(&mut output);
        let empty_hash = H256::from(output);
        SparseMerkleTree {
            root: Box::new(Node::Leaf(LeafNode { hash: empty_hash })),
        }
    }

    pub fn update(&mut self, key: H256, value: H256) {
        self.root = Self::update_recursive(&self.root, key, value, 0);
    }

    pub fn root(&self) -> H256 {
        self.root.hash()
    }

    fn update_recursive(node: &Box<Node>, key: H256, value: H256, depth: usize) -> Box<Node> {
        if depth == 256 {
            let mut hasher = Keccak::v256();
            let mut output = [0u8; 32];
            hasher.update(value.as_bytes());
            hasher.finalize(&mut output);
            return Box::new(Node::Leaf(LeafNode { hash: H256::from(output) }));
        }

        match &**node {
            Node::Leaf(leaf) => {
                if leaf.hash == H256::zero() {
                    let new_node = Self::update_recursive(&Box::new(Node::Leaf(LeafNode { hash: H256::zero() })), key, value, depth + 1);
                    let mut branch = BranchNode {
                        hash: H256::zero(),
                        left: Box::new(Node::Leaf(LeafNode { hash: H256::zero() })),
                        right: Box::new(Node::Leaf(LeafNode { hash: H256::zero() })),
                    };
                    if key.bit(depth) {
                        branch.right = new_node;
                    } else {
                        branch.left = new_node;
                    }
                    let mut hasher = Keccak::v256();
                    let mut output = [0u8; 32];
                    hasher.update(branch.left.hash().as_bytes());
                    hasher.update(branch.right.hash().as_bytes());
                    hasher.finalize(&mut output);
                    branch.hash = H256::from(output);
                    Box::new(Node::Branch(branch))
                } else {
                    // If the leaf is not empty, it means we have a collision.
                    // In a simple SMT, we can just replace the leaf.
                    // For a more advanced SMT, we would need to create a new branch.
                    let new_node = Self::update_recursive(&Box::new(Node::Leaf(LeafNode { hash: H256::zero() })), key, value, depth + 1);
                    let mut branch = BranchNode {
                        hash: H256::zero(),
                        left: Box::new(Node::Leaf(LeafNode { hash: H256::zero() })),
                        right: Box::new(Node::Leaf(LeafNode { hash: H256::zero() })),
                    };
                    if key.bit(depth) {
                        branch.right = new_node;
                    } else {
                        branch.left = new_node;
                    }
                    let mut hasher = Keccak::v256();
                    let mut output = [0u8; 32];
                    hasher.update(branch.left.hash().as_bytes());
                    hasher.update(branch.right.hash().as_bytes());
                    hasher.finalize(&mut output);
                    branch.hash = H256::from(output);
                    Box::new(Node::Branch(branch))
                }
            },
            Node::Branch(branch) => {
                let mut new_branch = BranchNode {
                    hash: branch.hash,
                    left: branch.left.clone(),
                    right: branch.right.clone(),
                };
                if key.bit(depth) {
                    new_branch.right = Self::update_recursive(&branch.right, key, value, depth + 1);
                } else {
                    new_branch.left = Self::update_recursive(&branch.left, key, value, depth + 1);
                }
                let mut hasher = Keccak::v256();
                let mut output = [0u8; 32];
                hasher.update(new_branch.left.hash().as_bytes());
                hasher.update(new_branch.right.hash().as_bytes());
                hasher.finalize(&mut output);
                new_branch.hash = H256::from(output);
                Box::new(Node::Branch(new_branch))
            }
        }
    }
}

trait Bit {
    fn bit(&self, i: usize) -> bool;
}

impl Bit for H256 {
    fn bit(&self, i: usize) -> bool {
        let byte_index = i / 8;
        let bit_index = i % 8;
        (self.as_bytes()[byte_index] >> bit_index) & 1 == 1
    }
}