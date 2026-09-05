use std::fmt;

use serde::{Deserialize, Serialize};

/// Identity namespace: the top-level authority domain a subject belongs to.
///
/// Renderer identifiers never appear here (SPEC-001: renderer IDs are not
/// semantic IDs). Unknown namespaces remain representable via [`Namespace::Other`]
/// so the vocabulary can grow without breaking the type.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Namespace {
    /// SDDK-owned truth (planning, workflows, decisions, evidence).
    Sddk,
    /// Architecture models (C4-style systems, containers, components).
    Arch,
    /// Code-level subjects (repositories, symbols, endpoints).
    Code,
    /// Verification subjects (tests, evidence, UAT scenarios).
    Verification,
    /// Runtime work subjects (workflow runs, tool calls, approvals).
    Work,
    /// Agentic interaction subjects (conversations, models, MCP servers).
    Agentic,
    /// Visual-thinking subjects (ideas, hypotheses, proposals, sketches).
    Visual,
    /// Vistalith-internal bookkeeping subjects.
    Vistalith,
    /// A namespace outside the known vocabulary.
    Other(String),
}

impl Namespace {
    pub fn as_str(&self) -> &str {
        match self {
            Namespace::Sddk => "sddk",
            Namespace::Arch => "arch",
            Namespace::Code => "code",
            Namespace::Verification => "verification",
            Namespace::Work => "work",
            Namespace::Agentic => "agentic",
            Namespace::Visual => "visual",
            Namespace::Vistalith => "vistalith",
            Namespace::Other(raw) => raw,
        }
    }

    pub fn parse(raw: &str) -> Result<Self, crate::DomainError> {
        match raw {
            "sddk" => Ok(Namespace::Sddk),
            "arch" => Ok(Namespace::Arch),
            "code" => Ok(Namespace::Code),
            "verification" => Ok(Namespace::Verification),
            "work" => Ok(Namespace::Work),
            "agentic" => Ok(Namespace::Agentic),
            "visual" => Ok(Namespace::Visual),
            "vistalith" => Ok(Namespace::Vistalith),
            other => {
                validate_token(other, "namespace")?;
                Ok(Namespace::Other(other.to_owned()))
            }
        }
    }
}

impl fmt::Display for Namespace {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl Serialize for Namespace {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(self.as_str())
    }
}

impl<'de> Deserialize<'de> for Namespace {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let raw = String::deserialize(deserializer)?;
        Namespace::parse(&raw).map_err(serde::de::Error::custom)
    }
}

/// Validates lowercase alphanumeric tokens used for unknown namespaces/kinds.
fn validate_token(raw: &str, what: &str) -> Result<(), crate::DomainError> {
    let valid = !raw.is_empty()
        && raw
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit());
    if valid {
        Ok(())
    } else if what == "namespace" {
        Err(crate::DomainError::InvalidNamespace(raw.to_owned()))
    } else {
        Err(crate::DomainError::InvalidKind(raw.to_owned()))
    }
}

/// Typed kind of a semantic subject: the node families from
/// `graph/SEMANTIC-WORLD-GRAPH.md`, plus a validated catch-all so new kinds
/// can be used before the vocabulary is extended.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum SubjectKind {
    // Engineering
    Project,
    Requirement,
    WorkItem,
    Decision,
    Adr,
    Risk,
    Incident,
    Experiment,
    // Architecture
    System,
    Container,
    Component,
    Interface,
    DataStore,
    DeploymentNode,
    // Code
    Repository,
    Module,
    Package,
    File,
    Symbol,
    Type,
    Function,
    Endpoint,
    Schema,
    // Verification
    Test,
    TestSuite,
    VerificationCapability,
    Evidence,
    Artifact,
    UatScenario,
    HumanCheck,
    // Runtime / work
    Workflow,
    WorkflowRun,
    WorkflowNode,
    Agent,
    Delegation,
    ToolCall,
    Approval,
    // Agentic interaction
    Conversation,
    Thread,
    Turn,
    Message,
    Frame,
    ModelCall,
    Provider,
    Model,
    McpServer,
    Tool,
    // Visual thinking
    Idea,
    Note,
    Question,
    Hypothesis,
    Option,
    SketchElement,
    VisualProposal,
    /// Output of a reactive behavior: advisory, never authoritative (SPEC-003).
    Advisory,
    /// A governed SDDK proposal recorded from a Vistalith intent (SPK-012).
    Proposal,
    /// A kind outside the shipped vocabulary (lowercase alphanumeric).
    Other(String),
}

macro_rules! kind_impl {
    ($enum_name:ident, $($variant:ident => $kebab:literal),+ $(,)?) => {
        impl $enum_name {
            pub fn as_str(&self) -> &str {
                match self {
                    $($enum_name::$variant => $kebab,)+
                    $enum_name::Other(raw) => raw,
                }
            }

            pub fn parse(raw: &str) -> Result<Self, crate::DomainError> {
                match raw {
                    $($kebab => Ok($enum_name::$variant),)+
                    _ => {
                        validate_kebab(raw)?;
                        Ok($enum_name::Other(raw.to_owned()))
                    }
                }
            }
        }

        impl fmt::Display for $enum_name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str(self.as_str())
            }
        }

        impl Serialize for $enum_name {
            fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
                serializer.serialize_str(self.as_str())
            }
        }

        impl<'de> Deserialize<'de> for $enum_name {
            fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
                let raw = String::deserialize(deserializer)?;
                $enum_name::parse(&raw).map_err(serde::de::Error::custom)
            }
        }
    };
}

fn validate_kebab(raw: &str) -> Result<(), crate::DomainError> {
    let valid = !raw.is_empty()
        && raw.starts_with(|c: char| c.is_ascii_lowercase())
        && raw
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-');
    if valid {
        Ok(())
    } else {
        Err(crate::DomainError::InvalidKind(raw.to_owned()))
    }
}

kind_impl! {
    SubjectKind,
    Project => "project",
    Requirement => "requirement",
    WorkItem => "work-item",
    Decision => "decision",
    Adr => "adr",
    Risk => "risk",
    Incident => "incident",
    Experiment => "experiment",
    System => "system",
    Container => "container",
    Component => "component",
    Interface => "interface",
    DataStore => "data-store",
    DeploymentNode => "deployment-node",
    Repository => "repository",
    Module => "module",
    Package => "package",
    File => "file",
    Symbol => "symbol",
    Type => "type",
    Function => "function",
    Endpoint => "endpoint",
    Schema => "schema",
    Test => "test",
    TestSuite => "test-suite",
    VerificationCapability => "verification-capability",
    Evidence => "evidence",
    Artifact => "artifact",
    UatScenario => "uat-scenario",
    HumanCheck => "human-check",
    Workflow => "workflow",
    WorkflowRun => "workflow-run",
    WorkflowNode => "workflow-node",
    Agent => "agent",
    Delegation => "delegation",
    ToolCall => "tool-call",
    Approval => "approval",
    Conversation => "conversation",
    Thread => "thread",
    Frame => "frame",
    Turn => "turn",
    Message => "message",
    ModelCall => "model-call",
    Provider => "provider",
    Model => "model",
    McpServer => "mcp-server",
    Tool => "tool",
    Idea => "idea",
    Note => "note",
    Question => "question",
    Hypothesis => "hypothesis",
    Option => "option",
    SketchElement => "sketch-element",
    VisualProposal => "visual-proposal",
    Advisory => "advisory",
    Proposal => "proposal",
}

/// Stable, revision-aware semantic identity (ADR-011).
///
/// Semantic identity is `(namespace, kind, id)`; `revision` records which
/// revision of the underlying source the reference points at and deliberately
/// does not take part in equality, hashing, ordering or [`fmt::Display`]:
/// the same subject at a different revision is still the same subject.
#[derive(Debug, Clone)]
pub struct SubjectRef {
    namespace: Namespace,
    kind: SubjectKind,
    id: String,
    revision: Option<String>,
}

impl SubjectRef {
    pub fn new(
        namespace: Namespace,
        kind: SubjectKind,
        id: impl Into<String>,
    ) -> Result<Self, crate::DomainError> {
        let id = id.into();
        if id.is_empty() || id.contains(':') || id.contains('@') {
            return Err(crate::DomainError::InvalidSubjectId(id));
        }
        Ok(SubjectRef {
            namespace,
            kind,
            id,
            revision: None,
        })
    }

    /// Same identity, pointing at a specific source revision.
    pub fn at_revision(mut self, revision: impl Into<String>) -> Self {
        self.revision = Some(revision.into());
        self
    }

    pub fn namespace(&self) -> &Namespace {
        &self.namespace
    }

    pub fn kind(&self) -> &SubjectKind {
        &self.kind
    }

    pub fn id(&self) -> &str {
        &self.id
    }

    pub fn revision(&self) -> Option<&str> {
        self.revision.as_deref()
    }

    /// Parses `namespace:kind:id` with an optional trailing `@revision`.
    ///
    /// Examples from `visual/SUBJECT-REF.md`:
    /// `sddk:work-item:TEST-MODEL-001`, `arch:container:payment-service`.
    pub fn parse(raw: &str) -> Result<Self, crate::DomainError> {
        let (identity, revision) = match raw.rsplit_once('@') {
            Some((identity, rev)) if !rev.is_empty() => (identity, Some(rev.to_owned())),
            _ => (raw, None),
        };
        let mut parts = identity.splitn(3, ':');
        let namespace = parts
            .next()
            .ok_or_else(|| crate::DomainError::InvalidSubjectRef(raw.to_owned()))
            .and_then(Namespace::parse)?;
        let kind = parts
            .next()
            .ok_or_else(|| crate::DomainError::InvalidSubjectRef(raw.to_owned()))
            .and_then(SubjectKind::parse)?;
        let id = parts
            .next()
            .ok_or_else(|| crate::DomainError::InvalidSubjectRef(raw.to_owned()))?;
        SubjectRef::new(namespace, kind, id).map(|r| match revision {
            Some(rev) => r.at_revision(rev),
            None => r,
        })
    }

    /// Observation handle for an SDDK project: links Vistalith state to SDDK
    /// identity without duplicating SDDK authority (EVENT-SOURCED-GRAPH.md).
    pub fn observed_sddk_project(project: &sddk_domain::ProjectId) -> Self {
        // Identity strings from sddk-domain are already validated there.
        SubjectRef::new(Namespace::Sddk, SubjectKind::Project, project.as_str())
            .expect("sddk ProjectId is a valid subject id")
    }

    /// Observation handle for an SDDK cycle.
    pub fn observed_sddk_cycle(cycle: &sddk_domain::CycleId) -> Self {
        SubjectRef::new(Namespace::Sddk, SubjectKind::Workflow, cycle.as_str())
            .expect("sddk CycleId is a valid subject id")
    }
}

impl PartialEq for SubjectRef {
    fn eq(&self, other: &Self) -> bool {
        self.namespace == other.namespace && self.kind == other.kind && self.id == other.id
    }
}

impl Eq for SubjectRef {}

impl std::hash::Hash for SubjectRef {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.namespace.hash(state);
        self.kind.hash(state);
        self.id.hash(state);
    }
}

/// Ordering follows identity only (`namespace, kind, id`), consistently with
/// [`PartialEq`] and [`std::hash::Hash`]: revision never participates, so
/// ordered containers stay deterministic and revision-stable.
impl Ord for SubjectRef {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        (
            self.namespace.as_str(),
            self.kind.as_str(),
            self.id.as_str(),
        )
            .cmp(&(
                other.namespace.as_str(),
                other.kind.as_str(),
                other.id.as_str(),
            ))
    }
}

impl PartialOrd for SubjectRef {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl fmt::Display for SubjectRef {
    /// Renders the identity (`ns:kind:id`), never the revision.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}:{}:{}", self.namespace, self.kind, self.id)
    }
}

impl Serialize for SubjectRef {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        #[derive(Serialize)]
        struct Wire<'a> {
            namespace: &'a Namespace,
            kind: &'a SubjectKind,
            id: &'a str,
            #[serde(skip_serializing_if = "Option::is_none")]
            revision: Option<&'a str>,
        }
        Wire {
            namespace: &self.namespace,
            kind: &self.kind,
            id: &self.id,
            revision: self.revision.as_deref(),
        }
        .serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for SubjectRef {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        #[derive(Deserialize)]
        struct Wire {
            namespace: Namespace,
            kind: SubjectKind,
            id: String,
            #[serde(default)]
            revision: Option<String>,
        }
        let wire = Wire::deserialize(deserializer)?;
        let r = SubjectRef::new(wire.namespace, wire.kind, wire.id)
            .map_err(serde::de::Error::custom)?;
        Ok(match wire.revision {
            Some(rev) => r.at_revision(rev),
            None => r,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_baseline_examples() {
        let r = SubjectRef::parse("sddk:work-item:TEST-MODEL-001").unwrap();
        assert_eq!(r.namespace(), &Namespace::Sddk);
        assert_eq!(r.kind(), &SubjectKind::WorkItem);
        assert_eq!(r.id(), "TEST-MODEL-001");
        assert_eq!(r.to_string(), "sddk:work-item:TEST-MODEL-001");

        let r = SubjectRef::parse("arch:container:payment-service").unwrap();
        assert_eq!(r.namespace(), &Namespace::Arch);
        assert_eq!(r.kind(), &SubjectKind::Container);
    }

    #[test]
    fn roundtrips_with_revision() {
        let r = SubjectRef::parse("code:symbol:vistalith_graph@abc123").unwrap();
        assert_eq!(r.revision(), Some("abc123"));
        assert_eq!(r.to_string(), "code:symbol:vistalith_graph");
    }

    #[test]
    fn identity_ignores_revision() {
        let a = SubjectRef::parse("arch:system:vistalith").unwrap();
        let b = a.clone().at_revision("r7");
        assert_eq!(a, b);
        assert_ne!(a.revision(), b.revision());
    }

    #[test]
    fn rejects_bad_references() {
        assert!(SubjectRef::parse("sddk:work-item").is_err());
        assert!(SubjectRef::parse("sddk:WorkItem:x").is_err());
        assert!(SubjectRef::parse("sddk:work-item:with:colons").is_err());
        assert!(SubjectRef::parse("sddk:work-item:@rev").is_err());
        assert!(SubjectRef::parse("Bad Namespace:work-item:x").is_err());
    }

    #[test]
    fn unknown_kinds_and_namespaces_roundtrip_through_serde() {
        let r = SubjectRef::new(
            Namespace::parse("governance").unwrap(),
            SubjectKind::parse("capability").unwrap(),
            "cap-001",
        )
        .unwrap();
        let json = serde_json::to_string(&r).unwrap();
        assert_eq!(
            json,
            r#"{"namespace":"governance","kind":"capability","id":"cap-001"}"#
        );
        let back: SubjectRef = serde_json::from_str(&json).unwrap();
        assert_eq!(back, r);

        // Known vocabulary keeps its typed variant through serde.
        let known = SubjectRef::parse("sddk:work-item:X").unwrap();
        let back: SubjectRef =
            serde_json::from_str(&serde_json::to_string(&known).unwrap()).unwrap();
        assert_eq!(back, known);
        assert!(matches!(back.kind, SubjectKind::WorkItem));
    }

    #[test]
    fn maps_sddk_identities() {
        let project = sddk_domain::ProjectId::new("vistalith").unwrap();
        let r = SubjectRef::observed_sddk_project(&project);
        assert_eq!(r.to_string(), "sddk:project:vistalith");

        let cycle = sddk_domain::CycleId::from_parts(&project, "m1").unwrap();
        let r = SubjectRef::observed_sddk_cycle(&cycle);
        assert_eq!(r.namespace(), &Namespace::Sddk);
        assert_eq!(r.kind, SubjectKind::Workflow);
    }
}
