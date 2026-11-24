# Dependency map

This document summarizes task-level dependencies for the complete plugin program. The first release slice and follow-up ownership are defined in [`delivery-split.md`](delivery-split.md); each OpenSpec task checklist remains the source of truth for its assigned task status.

For planning ranges and model suggestions, see [effort-estimates.md](effort-estimates.md).


## Delivery-change dependencies

```mermaid
flowchart LR
  MVP[Provider/catalog MVP]
  ACT[Game actions and configuration]
  RUN[Custom launch and runtimes]
  DATA[Shared data and persistence]
  MVP --> ACT
  MVP --> DATA
  ACT --> RUN
  ACT -. "folder approval for 13.2" .-> DATA
  RUN -. "runtime identity for 17.3" .-> DATA
```

See [`delivery-split.md`](delivery-split.md) for endpoint boundaries and source-task mapping. Most shared-data work can proceed alongside launch/runtime work after the MVP. Task 13.2 waits for folder approvals; task 17.3 waits for runtime identity.

The checklist declares only a small number of task-level dependencies. The diagram groups all task IDs by section and shows the most relevant cross-task paths. **Solid edges** are stated in the change artifacts. **Dotted edges** are planning inferences from task scope or completion notes; they are not new requirements or blockers unless the task artifacts are updated to say so.

```mermaid
flowchart LR
  subgraph foundation["Foundation"]
    S1["1 · Licensing and official baseline<br/>1.1–1.4"]
    S2["2 · Public API and SDK<br/>2.1–2.7"]
    S3["3 · Packages, trust, registry<br/>3.1–3.10"]
    S4["4 · Permissions and identity<br/>4.1–4.6"]
  end

  subgraph services["Services and actions"]
    S5["5 · Library adapters<br/>5.1–5.5"]
    S6["6 · Storage, files, network<br/>6.1–6.7"]
    S7["7 · Actions and jobs<br/>7.1–7.11"]
  end

  subgraph management["Lifecycle and management"]
    S8["8 · Provider/configuration lifecycle<br/>8.1–8.8"]
    S9["9 · Updates and recovery<br/>9.1–9.4"]
    S10["10 · Settings integration<br/>10.1–10.10"]
  end

  subgraph data["SDK, data, runtime"]
    S11["11 · Standalone SDKs/examples<br/>11.1–11.9"]
    S12["12 · Shared data and consent<br/>12.1–12.7"]
    S13["13 · Save/location inspection<br/>13.1–13.7"]
    S14["14 · Runtime/game identity<br/>14.1–14.5"]
    S15["15 · Diagnostic/gameplay data<br/>15.1–15.4"]
    S16["16 · Persistence and cleanup<br/>16.1–16.5"]
    S17["17 · Administration/interoperability<br/>17.1–17.4"]
  end

  subgraph acceptance["Acceptance and later decisions"]
    S18["18 · Cross-component acceptance<br/>18.1–18.10"]
    S19["19 · Accounting, public pool, discovery<br/>19.1–19.5"]
  end

  S4 -->|"explicit: 4.1 → 8.8"| S8
  S6 -->|"explicit: 6.3 → 8.8"| S8
  S6 -->|"explicit: 6.7 → 10.9"| S10
  S6 -->|"explicit: 6.7 → 14.1"| S14

  S9 -.->|"inferred: review before activation/UI"| S10
  S14 -.->|"inferred: runtime handles"| S7
  S14 -.->|"inferred: runtime launch acceptance"| S18
  S11 -.->|"inferred: SDKs/examples"| S17
  S12 -.->|"inferred: shared-data acceptance"| S16
  S15 -.->|"inferred: data examples/acceptance"| S17
  S16 -.->|"inferred: persistence controls/acceptance"| S17
  S17 -.->|"inferred: integration evidence"| S18

  classDef foundation fill:#e9eefb,stroke:#586fa8,color:#172033;
  classDef service fill:#e8f5ef,stroke:#448165,color:#172033;
  classDef management fill:#fff4df,stroke:#a87924,color:#172033;
  classDef dataNode fill:#f3eafb,stroke:#8059a3,color:#172033;
  classDef accept fill:#fce9e9,stroke:#a44d4d,color:#172033;
  class S1,S2,S3,S4 foundation;
  class S5,S6,S7 service;
  class S8,S9,S10 management;
  class S11,S12,S13,S14,S15,S16,S17 dataNode;
  class S18,S19 accept;
```

The section diagram groups tasks rather than asserting that every task in one section blocks every task in a connected section. These are the task-level edges represented in the map:

```mermaid
flowchart LR
  t62["6.2"] --> t66["6.6"]
  t66 -->|"completes file-handle coverage"| t54["5.4"]
  t62 --> t67["6.7"]
  t41["4.1"] --> t88["8.8"]
  t63["6.3"] --> t88
  t67 --> t109["10.9"]
  t67 --> t141["14.1"]
  t1010["10.10"] -. "completion note" .-> t72["7.2"]

  t83["8.3"] -.-> t85["8.5"]
  t84["8.4"] -.-> t85
  t84 -.-> t86["8.6"]
  t83 -.-> t87["8.7"]
  t84 -.-> t87
  t92["9.2"] -.-> t93["9.3"]
  t92 -.-> t101["10.1"]
  t92 -.-> t102["10.2"]
  t142["14.2"] -.-> t78["7.8"]
  t142 -.-> t185["18.5"]
  t142 -.-> t188["18.8"]

  classDef explicit fill:#e8f5ef,stroke:#448165,color:#172033;
  classDef inferred fill:#fff4df,stroke:#a87924,color:#172033;
  class t62,t66,t54,t67,t41,t88,t63,t109,t141,t1010,t72 explicit;
  class t83,t84,t85,t86,t87,t92,t93,t101,t102,t142,t78,t185,t188 inferred;
```

## Task-level relationships

### Explicit in the task artifacts

- **6.2 → 6.6**: artwork accepts the approved file handles introduced by 6.2. Task 6.6 says it completes the file-handle portion of 5.4.
- **6.2 → 6.7**: declared save/prefix locations are suggestions only and use the folder-approval flow from 6.2.
- **4.1 + 6.3 → 8.8**: 8.8 connects approved HTTPS origins to persisted per-account grants.
- **6.7 → 10.9**: installation/folder suggestions feed the settings approval flow.
- **6.7 → 14.1**: suggested library roots follow the same closed-placeholder and approval rules.
- **10.10 → 7.2**: the completion note for 7.2 records that custom/standard launch confirmation uses the on-request intent flow from 10.10.

### Inferred from the requirements and completion notes

- **8.3 + 8.4 → 8.5, 8.6, 8.7**: session/window lifecycle and `ui.configure` are foundations for fencing windows, selecting external versus declarative configuration, and validating lifecycle fixtures/documentation.
- **9.2 → 9.3, 10.1, 10.2**: update review and approval precede staged activation/rollback and the UI's update state/actions.
- **14.2 → 7.8, 18.5, 18.8**: approved runtime resources are needed to complete custom launch runtime handles and runtime-wrapper/overlay acceptance.
- **11.x → 17.3–17.4 → 18.x**: standalone SDK distributions and examples feed interoperability and cross-component checks.
- **12.x, 15.x, 16.x → 17.x → 18.x**: shared data, produced data and persistence feed administration/examples and combined acceptance scenarios.
- **Implementation sections → 18.x**: section 18 is an acceptance sweep over implemented contracts; each 18.x task should be tied to the relevant feature tasks when its exact scope is reviewed.

These inferred paths are navigation aids. Before treating one as a blocking dependency, record it in `tasks.md` and review the affected tasks for ordering and scope.
