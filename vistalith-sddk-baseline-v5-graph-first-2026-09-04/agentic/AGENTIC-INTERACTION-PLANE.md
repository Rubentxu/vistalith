# Agentic Interaction Plane

Chat is the main human door into the same semantic world shown visually.

```text
User
 ↓
Thread / Turn
 ↓
Semantic context selection
 ↓
Context View over SWG + direct SDDK state
 ↓
Model Profile resolution
 ↓
LLM
 ↓
streamed typed Items
 ↓
tools / agents / proposals
 ↓
trace + graph updates
```

## Typed item model

- UserMessage
- AssistantMessage
- ToolCall/Result
- AgentDelegation/Contribution
- ApprovalRequest/Resolution
- ContextCompaction
- VisualProposal
- SemanticChangeProposal
- FileChange
- ModelUsage
- Warning/Error

Items are not flattened into chat prose.
