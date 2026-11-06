# Specification: central storage reset

Observable obligations over the [entities](entities.md). Requirements: [../requirements/index.md](../requirements/index.md).

| Chunk | Reader question | Status |
| --- | --- | --- |
| [entities.md](entities.md) | What is a Message, a Machine, the Fleet, and when are two of them the same? | written |
| [central-layout.md](central-layout.md) | What shape must central have? (R1–R6) | written, D1 |
| [mail.md](mail.md) | How must a Message travel, and what does the sender see? (R7–R15) | written, D1 |
| remote-sessions | What does the caller see when creating a session on another Machine? | outcome in [U10](../requirements/remote-sessions.md#u10); obligations written when D2 starts |
| local-storage | Which names and files must each Router's storage have? | outcome in [U11–U13](../requirements/local-storage.md); obligations written when D3 starts |
| automations | How do wakes, notifications and schedules behave? | outcome in [U14–U17](../requirements/automations.md); obligations written when D4a starts |

Later chunks are written when their deliverable starts, against the code as it is then.
