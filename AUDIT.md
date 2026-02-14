# Reddwarf Production Readiness Audit

**Last updated:** 2026-02-14
**Baseline commit:** `58171c7` (Add periodic reconciliation, node health checker, and graceful pod termination)

---

## 1. Zone Runtime (`reddwarf-runtime`)

| Requirement | Status | Notes |
|---|---|---|
| Pod spec to zonecfg | DONE | `zone/config.rs`, `controller.rs:pod_to_zone_config()` |
| Zone lifecycle (zoneadm) | DONE | `illumos.rs` — create, install, boot, halt, uninstall, delete |
| Container to Zone mapping | DONE | Naming, sanitization, 64-char truncation |
| CPU limits to capped-cpu | DONE | Aggregates across containers, limits preferred over requests |
| Memory limits to capped-memory | DONE | Aggregates across containers, illumos G/M/K suffixes |
| Network to Crossbow VNIC | DONE | `dladm create-etherstub`, `create-vnic`, per-pod VNIC+IP |
| Volumes to ZFS datasets | DONE | Create, destroy, clone, quota, snapshot support |
| Image pull / clone | PARTIAL | ZFS clone works; LX tarball `-s` works. Missing: no image pull/registry, no `.zar` archive, no golden image bootstrap |
| Health probes (zlogin) | DONE | exec-in-zone via `zlogin`, liveness/readiness/startup probes with exec/HTTP/TCP actions, probe tracker state machine integrated into reconcile loop. v1 limitation: probes run at reconcile cadence, not per-probe `periodSeconds` |

## 2. Reconciliation / Controller Loop

| Requirement | Status | Notes |
|---|---|---|
| Event bus / watch | DONE | tokio broadcast channel, SSE watch API, multi-subscriber |
| Pod controller | DONE | Event-driven + full reconcile on lag, provision/deprovision |
| Node controller (NotReady) | DONE | `node_health.rs` — checks every 15s, marks stale (>40s) nodes NotReady with reason NodeStatusUnknown |
| Continuous reconciliation | DONE | `controller.rs` — periodic `reconcile_all()` every 30s via `tokio::time::interval` in select! loop |
| Graceful termination | DONE | DELETE sets `deletion_timestamp` + phase=Terminating; controller drives shutdown state machine; POST `.../finalize` for actual removal |

## 3. Pod Status Tracking

| Requirement | Status | Notes |
|---|---|---|
| Zone state to pod phase | DONE | 8 zone states mapped to pod phases |
| Status subresource (`/status`) | DONE | PUT endpoint, spec/status separation, fires MODIFIED events |
| ShuttingDown mapping | DONE | Fixed in `58171c7` — maps to "Terminating" |

## 4. Node Agent / Heartbeat

| Requirement | Status | Notes |
|---|---|---|
| Self-registration | DONE | Creates Node resource with allocatable CPU/memory |
| Periodic heartbeat | DONE | 10-second interval, Ready condition |
| Report zone states | NOT DONE | Heartbeat doesn't query actual zone states |
| Dynamic resource reporting | DONE | `sysinfo.rs` — detects CPU/memory via `sys-info`, capacity vs allocatable split with configurable reservations (`--system-reserved-cpu`, `--system-reserved-memory`, `--max-pods`). Done in `d3eb0b2` |

## 5. Main Binary

| Requirement | Status | Notes |
|---|---|---|
| API + scheduler + runtime wired | DONE | All 4 components spawned as tokio tasks |
| CLI via clap | DONE | `serve` and `agent` subcommands |
| Graceful shutdown | DONE | SIGINT + CancellationToken + 5s timeout |
| TLS (rustls) | DONE | Auto-generated self-signed CA + server cert, or user-provided PEM. Added in `cb6ca8c` |
| SMF service manifest | DONE | SMF manifest + method script in `smf/`. Added in `cb6ca8c` |

## 6. Networking

| Requirement | Status | Notes |
|---|---|---|
| Etherstub creation | DONE | `dladm create-etherstub` |
| VNIC per zone | DONE | `dladm create-vnic -l etherstub` |
| ipadm IP assignment | PARTIAL | IP set in zonecfg `allowed-address` but no explicit `ipadm create-addr` call |
| IPAM | DONE | Sequential alloc, idempotent, persistent, pool exhaustion handling |
| Service ClusterIP / NAT | NOT DONE | Services stored at API level but no backend controller, no ipnat rules, no proxy, no DNS |

## 7. Scheduler

| Requirement | Status | Notes |
|---|---|---|
| Versioned bind_pod() | DONE | Fixed in `c50ecb2` — creates versioned commits |
| Zone brand constraints | DONE | `ZoneBrandMatch` filter checks `reddwarf.io/zone-brand` annotation vs `reddwarf.io/zone-brands` node label. Done in `4c7f50a` |
| Actual resource usage | NOT DONE | Only compares requests vs static allocatable — no runtime metrics |

---

## Priority Order

### Critical (blocks production)
- [x] TLS — done in `cb6ca8c`
- [x] SMF manifest — done in `cb6ca8c`

### High (limits reliability)
- [x] Node health checker — done in `58171c7`
- [x] Periodic reconciliation — done in `58171c7`
- [x] Graceful pod termination — done in `58171c7`

### Medium (limits functionality)
- [ ] Service networking — no ClusterIP, no NAT/proxy, no DNS
- [x] Health probes — exec/HTTP/TCP liveness/readiness/startup probes via zlogin
- [ ] Image management — no pull/registry, no `.zar` support, no golden image bootstrap
- [x] Dynamic node resources — done in `d3eb0b2`

### Low (nice to have)
- [x] Zone brand scheduling filter — done in `4c7f50a`
- [x] ShuttingDown to Terminating mapping fix — done in `58171c7`
- [ ] bhyve brand — type exists but no implementation
