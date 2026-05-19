# Assessment Model

Shore records reviewer decisions as `ReviewAssessment` events: `Accepted`,
`AcceptedWithFollowUp`, `NeedsChanges`, and `NeedsClarification`.

State-change outcomes such as `Deferred`, `SplitOut`, `Overridden`, and `Superseded` are recorded
as review observations with `state-change:*` tags.

This document is a stub. Full payload reference, supersession rules, and CLI surface notes are added
in a follow-up.

## Legacy disposition events

Earlier versions of Shore wrote `review_disposition_recorded` events with eight variants. Shore is
pre-V1 and does not preserve those events on disk. Once legacy disposition support is removed,
loading a `.shore/events/` directory that contains legacy disposition events will fail with a typed
error pointing at this section.

**Migration:** delete the local `.shore/` directory and re-capture any in-progress reviews. There is
no automatic migration tool.
