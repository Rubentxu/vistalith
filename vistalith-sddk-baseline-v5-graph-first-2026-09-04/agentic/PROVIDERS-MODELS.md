# Providers and Models

## Provider
- identity;
- endpoint;
- authentication method;
- model discovery;
- streaming;
- structured output;
- tool use;
- modalities;
- health/rate limit;
- locality;
- pricing metadata.

## Model
- provider/model canonical ID;
- aliases;
- context window;
- output limit;
- tool/structured-output support;
- vision/audio/etc.;
- reasoning capabilities;
- effort levels;
- service tiers;
- lifecycle;
- cost metadata.

## ModelProfile

Prefer semantic profiles:
- fast;
- balanced;
- deep;
- planning;
- implementation;
- review;
- judge;
- vision;
- long-context;
- local.

Resolution:

```text
AgentRole
 → ModelProfile
 → capability filter
 → policy
 → health
 → cost/latency objective
 → selected model
```

Every reroute is traced.
