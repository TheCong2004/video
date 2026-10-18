// SPDX-License-Identifier: AGPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 Concat contributors

//! Generation dependency graph, capability preflight, verified artifact import, and timeline assembly.
//! Inspired by Velorn's flowAiSchema and auto-import pipeline, integrated natively with Concat.

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::{HashMap, HashSet, VecDeque};
use std::fmt;

/// Node types in the generation pipeline.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum GenerationNodeKind {
    /// Text prompt generation or assist node.
    Prompt,
    /// Text-to-image or image-to-image generation node.
    ImageGen,
    /// Video generation node (e.g. text-to-video or image-to-video).
    VideoGen,
    /// Background music or voice generation node.
    MusicGen,
    /// Video upscaling or enhancement node.
    Upscale,
}

/// A node in the generation graph.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GenerationNode {
    /// Unique node identifier.
    pub id: String,
    /// The node type.
    pub kind: GenerationNodeKind,
    /// Human-visible label for the node.
    pub label: String,
    /// Optional prompt payload.
    pub prompt: Option<String>,
    /// Upstream dependency node IDs.
    pub dependencies: Vec<String>,
    /// Target model identifier.
    pub model_id: Option<String>,
}

/// A directed acyclic dependency graph for generation.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct GenerationGraph {
    /// All nodes indexed by ID.
    pub nodes: HashMap<String, GenerationNode>,
}

/// Errors occurring during graph validation or topological resolution.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GraphError {
    /// The requested node was not found.
    NotFound(String),
    /// Graph contains a circular dependency cycle.
    CycleDetected,
    /// Node references an unknown dependency.
    MissingDependency(String, String),
    /// Required capability not supported by the provider.
    MissingCapability(String),
    /// Invalid node parameters.
    InvalidNode(String),
}

impl fmt::Display for GraphError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NotFound(id) => write!(f, "Node not found: {id}"),
            Self::CycleDetected => write!(f, "Cycle detected in generation graph"),
            Self::MissingDependency(id, dep) => {
                write!(f, "Missing dependency: {id} depends on {dep}")
            }
            Self::MissingCapability(cap) => write!(f, "Missing required model capability: {cap}"),
            Self::InvalidNode(msg) => write!(f, "Invalid node configuration: {msg}"),
        }
    }
}

impl std::error::Error for GraphError {}

impl GenerationGraph {
    /// Creates an empty generation graph.
    pub fn new() -> Self {
        Self {
            nodes: HashMap::new(),
        }
    }

    /// Adds a node to the graph.
    pub fn add_node(&mut self, node: GenerationNode) -> Result<(), GraphError> {
        if node.id.trim().is_empty() {
            return Err(GraphError::InvalidNode("Node ID cannot be empty".into()));
        }
        self.nodes.insert(node.id.clone(), node);
        Ok(())
    }

    /// Performs topological sort to determine execution order.
    /// Returns Err(CycleDetected) if the graph contains cycles.
    pub fn execution_order(&self) -> Result<Vec<String>, GraphError> {
        let mut in_degree: HashMap<&str, usize> = HashMap::new();
        let mut adj: HashMap<&str, Vec<&str>> = HashMap::new();

        for (id, node) in &self.nodes {
            in_degree.entry(id.as_str()).or_insert(0);
            for dep in &node.dependencies {
                if !self.nodes.contains_key(dep) {
                    return Err(GraphError::MissingDependency(id.clone(), dep.clone()));
                }
                adj.entry(dep.as_str()).or_default().push(id.as_str());
                *in_degree.entry(id.as_str()).or_insert(0) += 1;
            }
        }

        let mut queue: VecDeque<&str> = in_degree
            .iter()
            .filter(|&(_, deg)| *deg == 0)
            .map(|(&id, _)| id)
            .collect();

        let mut order = Vec::new();
        while let Some(id) = queue.pop_front() {
            order.push(id.to_string());
            if let Some(neighbors) = adj.get(id) {
                for &next in neighbors {
                    if let Some(deg) = in_degree.get_mut(next) {
                        *deg -= 1;
                        if *deg == 0 {
                            queue.push_back(next);
                        }
                    }
                }
            }
        }

        if order.len() != self.nodes.len() {
            return Err(GraphError::CycleDetected);
        }

        Ok(order)
    }

    /// Validates that all required capabilities are supported by advertised capabilities.
    pub fn validate_capabilities(
        &self,
        available_capabilities: &HashSet<String>,
    ) -> Result<(), GraphError> {
        for node in self.nodes.values() {
            let req = match node.kind {
                GenerationNodeKind::Prompt => continue,
                GenerationNodeKind::ImageGen => "image_generation",
                GenerationNodeKind::VideoGen => "video_generation",
                GenerationNodeKind::MusicGen => "music_generation",
                GenerationNodeKind::Upscale => "upscale",
            };
            if !available_capabilities.contains(req) {
                return Err(GraphError::MissingCapability(req.into()));
            }
        }
        Ok(())
    }
}

/// A verified generation artifact with SHA-256 provenance.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VerifiedArtifact {
    /// Generator node ID.
    pub node_id: String,
    /// Destination filename.
    pub filename: String,
    /// Verified SHA-256 hex digest.
    pub sha256: String,
    /// Byte length of the artifact.
    pub size_bytes: u64,
    /// Detected MIME type (e.g. video/mp4, image/png).
    pub mime_type: String,
}

/// Artifact verification error.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ArtifactError {
    /// SHA-256 digest did not match expected hash.
    ChecksumMismatch {
        /// Expected SHA-256.
        expected: String,
        /// Actual calculated SHA-256.
        actual: String,
    },
    /// Data buffer was empty.
    EmptyPayload,
    /// Artifact was already imported.
    Duplicate,
}

impl fmt::Display for ArtifactError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::ChecksumMismatch { expected, actual } => {
                write!(
                    f,
                    "Checksum mismatch: expected {expected}, calculated {actual}"
                )
            }
            Self::EmptyPayload => write!(f, "Empty or invalid file payload"),
            Self::Duplicate => write!(f, "Duplicate artifact ignored"),
        }
    }
}

impl std::error::Error for ArtifactError {}

/// Verifies bytes against expected SHA-256.
pub fn verify_artifact(
    node_id: &str,
    filename: &str,
    data: &[u8],
    expected_sha256: &str,
    mime_type: &str,
) -> Result<VerifiedArtifact, ArtifactError> {
    if data.is_empty() {
        return Err(ArtifactError::EmptyPayload);
    }
    let mut hasher = Sha256::new();
    hasher.update(data);
    let actual_sha256 = format!("{:x}", hasher.finalize());

    if !actual_sha256.eq_ignore_ascii_case(expected_sha256) {
        return Err(ArtifactError::ChecksumMismatch {
            expected: expected_sha256.to_owned(),
            actual: actual_sha256,
        });
    }

    Ok(VerifiedArtifact {
        node_id: node_id.to_owned(),
        filename: filename.to_owned(),
        sha256: actual_sha256,
        size_bytes: data.len() as u64,
        mime_type: mime_type.to_owned(),
    })
}

/// Registry of imported artifacts to prevent duplicate imports.
#[derive(Debug, Clone, Default)]
pub struct ArtifactRegistry {
    seen_hashes: HashSet<String>,
}

impl ArtifactRegistry {
    /// Creates an empty registry.
    pub fn new() -> Self {
        Self {
            seen_hashes: HashSet::new(),
        }
    }

    /// Attempts to register an artifact. Returns Err(ArtifactError::Duplicate) if already registered.
    pub fn register(&mut self, artifact: &VerifiedArtifact) -> Result<(), ArtifactError> {
        if !self.seen_hashes.insert(artifact.sha256.clone()) {
            return Err(ArtifactError::Duplicate);
        }
        Ok(())
    }

    /// Checks if a given SHA-256 hash is already registered.
    pub fn is_registered(&self, sha256: &str) -> bool {
        self.seen_hashes.contains(sha256)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn topological_execution_order_resolves_linear_and_branching_dependencies() {
        let mut graph = GenerationGraph::new();
        graph
            .add_node(GenerationNode {
                id: "prompt1".into(),
                kind: GenerationNodeKind::Prompt,
                label: "Scene 1 Prompt".into(),
                prompt: Some("A futuristic cyberpunk city".into()),
                dependencies: vec![],
                model_id: None,
            })
            .unwrap();
        graph
            .add_node(GenerationNode {
                id: "img1".into(),
                kind: GenerationNodeKind::ImageGen,
                label: "Scene 1 Base Image".into(),
                prompt: None,
                dependencies: vec!["prompt1".into()],
                model_id: Some("flux-pro".into()),
            })
            .unwrap();
        graph
            .add_node(GenerationNode {
                id: "vid1".into(),
                kind: GenerationNodeKind::VideoGen,
                label: "Scene 1 Animation".into(),
                prompt: None,
                dependencies: vec!["img1".into()],
                model_id: Some("kling-v1".into()),
            })
            .unwrap();

        let order = graph.execution_order().unwrap();
        assert_eq!(order, vec!["prompt1", "img1", "vid1"]);
    }

    #[test]
    fn circular_dependencies_are_refused() {
        let mut graph = GenerationGraph::new();
        graph
            .add_node(GenerationNode {
                id: "nodeA".into(),
                kind: GenerationNodeKind::ImageGen,
                label: "Node A".into(),
                prompt: None,
                dependencies: vec!["nodeB".into()],
                model_id: None,
            })
            .unwrap();
        graph
            .add_node(GenerationNode {
                id: "nodeB".into(),
                kind: GenerationNodeKind::ImageGen,
                label: "Node B".into(),
                prompt: None,
                dependencies: vec!["nodeA".into()],
                model_id: None,
            })
            .unwrap();

        assert_eq!(graph.execution_order(), Err(GraphError::CycleDetected));
    }

    #[test]
    fn capability_validation_refuses_missing_services() {
        let mut graph = GenerationGraph::new();
        graph
            .add_node(GenerationNode {
                id: "img".into(),
                kind: GenerationNodeKind::ImageGen,
                label: "Image".into(),
                prompt: None,
                dependencies: vec![],
                model_id: None,
            })
            .unwrap();

        let mut available = HashSet::new();
        available.insert("video_generation".to_string());
        assert_eq!(
            graph.validate_capabilities(&available),
            Err(GraphError::MissingCapability("image_generation".into()))
        );

        available.insert("image_generation".to_string());
        assert!(graph.validate_capabilities(&available).is_ok());
    }

    #[test]
    fn verified_artifact_checksum_and_deduplication() {
        let dummy_data = b"synthetic generated video frames payload";
        let mut hasher = Sha256::new();
        hasher.update(dummy_data);
        let valid_sha256 = format!("{:x}", hasher.finalize());

        // Mismatched checksum is refused
        assert!(matches!(
            verify_artifact(
                "vid1",
                "scene1.mp4",
                dummy_data,
                "bad_hash_0000",
                "video/mp4"
            ),
            Err(ArtifactError::ChecksumMismatch { .. })
        ));

        // Matching checksum succeeds
        let artifact =
            verify_artifact("vid1", "scene1.mp4", dummy_data, &valid_sha256, "video/mp4").unwrap();
        assert_eq!(artifact.size_bytes, dummy_data.len() as u64);

        // Deduplication in registry
        let mut registry = ArtifactRegistry::new();
        assert!(!registry.is_registered(&valid_sha256));
        assert!(registry.register(&artifact).is_ok());
        assert!(registry.is_registered(&valid_sha256));
        assert_eq!(registry.register(&artifact), Err(ArtifactError::Duplicate));
    }
}
