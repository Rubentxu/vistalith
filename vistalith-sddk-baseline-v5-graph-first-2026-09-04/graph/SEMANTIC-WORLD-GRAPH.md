# Semantic World Graph (SWG)

## Goal

Make relationships across software engineering first-class and queryable.

The graph is not only a visual model. It is Vistalith's shared semantic workspace.

## Node families

### Engineering
- Project
- Requirement
- WorkItem
- Decision
- ADR
- Risk
- Incident
- Experiment

### Architecture
- System
- Container
- Component
- Interface
- DataStore
- DeploymentNode

### Code
- Repository
- Module
- Package
- File
- Symbol
- Type
- Function
- Endpoint
- Schema

### Verification
- Test
- TestSuite
- VerificationCapability
- Evidence
- Artifact
- UATScenario
- HumanCheck

### Runtime/work
- Workflow
- WorkflowRun
- WorkflowNode
- Agent
- Delegation
- ToolCall
- Approval

### Agentic interaction
- Conversation
- Thread
- Turn
- Message
- ModelCall
- Provider
- Model
- MCPServer
- Tool

### Visual thinking
- Idea
- Note
- Question
- Hypothesis
- Option
- SketchElement
- VisualProposal

## Edge families

- contains
- depends_on
- calls
- implements
- exposes
- satisfies
- verifies
- tested_by
- decided_by
- motivated_by
- produced_by
- executed_by
- delegated_to
- affects
- blocks
- derives_from
- supersedes
- contradicts
- supports
- provides_evidence_for
- visualizes
- mentions
- proposes_change_to
- rejected_in_favor_of
- revisits
- observed_in
- used_model
- used_tool

## Every graph fact carries

- semantic identity;
- source;
- source revision;
- authority class;
- provenance;
- optional confidence;
- timestamps/event cursor;
- optional validity interval.

## Graph invariants

1. Renderer node IDs never become semantic IDs.
2. Derived relationships always have provenance.
3. Advisory facts are visually distinguishable.
4. SDDK-owned nodes cannot be authoritatively mutated through a graph patch.
5. Cross-domain edges may be advisory until promoted/validated.
6. Graph snapshots are revisioned.
