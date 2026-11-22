---
name: rust-dependencies
description: Use when adding, removing, upgrading, pinning, forking, tracking, auditing, or vetting Rust dependencies, build scripts, native libraries, toolchains, or dependency policy.
---

# Rust dependencies

Do not add a dependency without explicit permission. Treat each dependency change as a risk and compatibility decision. Before adding one, establish why the standard library and existing dependencies are insufficient; inspect maintenance, releases, transitive code, build scripts, unsafe or native code, licenses, targets, advisories, vetting, public API exposure, and replacement strategy. Rejecting every dependency is also a risk decision; reimplementation carries its own maintenance and security burden, so make the trade explicit and choose deliberately.

Track the exact dependency closure of shipped artifacts together with source revision, dirty state, toolchain, target, features, profile, source origins and checksums, native components, artifact digest, and deployment or release identity where applicable. Upgrading the main branch does not remediate an older deployed artifact.

Connect advisories to affected artifacts and deployments, fixed versions, relevance, owner, remediation state, and expiration of accepted risk. Use vetting records for exact versions or ranges and make exemptions time-bounded.

Keep stale releases, abandoned packages, old toolchains, pinned revisions, forks, and repeated exemptions visible. Keep the minimum supported Rust version intentional and tested, and do not let a developer-local toolchain determine release behavior. Automate routine updates only to the extent that tests, compatibility checks, and policy make automatic handling safe. Isolate frequently breaking or publicly exposed dependencies behind a narrow internal boundary when that reduces coupling.

