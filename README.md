# Reddwarf: Rust-Based Single-Binary Kubernetes Control Plane

A pure Rust implementation of a Kubernetes control plane with DAG-based resource versioning.

## Project Status

**Current Phase**: Phase 4 Complete (API Server) ✅

### Completed Phases

#### Phase 1: Foundation & Core Types ✅
- ✅ Workspace structure created
- ✅ Core Kubernetes types and traits (Pod, Node, Service, Namespace)
- ✅ Error handling with miette diagnostics
- ✅ ResourceKey and GroupVersionKind types
- ✅ JSON/YAML serialization helpers
- ✅ 9 tests passing

#### Phase 2: Storage Layer with redb ✅
- ✅ KVStore trait abstraction
- ✅ redb backend implementation (100% pure Rust)
- ✅ Key encoding for resources
- ✅ Transaction support
- ✅ Prefix scanning and indexing
- ✅ 9 tests passing

#### Phase 3: Versioning Layer ✅
- ✅ VersionStore for DAG-based versioning
- ✅ Commit operations (create, get, list)
- ✅ Conflict detection between concurrent modifications
- ✅ DAG traversal for history
- ✅ Common ancestor finding
- ✅ 7 tests passing

#### Phase 4: API Server ✅
- ✅ Axum-based REST API server
- ✅ HTTP verb handlers (GET, POST, PUT, PATCH, DELETE)
- ✅ Pod, Node, Service, Namespace endpoints
- ✅ LIST operations with prefix filtering
- ✅ Resource validation
- ✅ Kubernetes-compatible error responses
- ✅ Health check endpoints (/healthz, /livez, /readyz)
- ✅ 7 tests passing

### Total: 32 tests passing ✅

## Architecture

```
reddwarf/
├── crates/
│   ├── reddwarf-core/          # ✅ Core K8s types & traits
│   ├── reddwarf-storage/       # ✅ redb storage backend
│   ├── reddwarf-versioning/    # ✅ DAG-based versioning
│   ├── reddwarf-apiserver/     # ✅ Axum REST API server
│   ├── reddwarf-scheduler/     # 🔄 Pod scheduler (pending)
│   └── reddwarf/               # 🔄 Main binary (pending)
└── tests/                      # 🔄 Integration tests (pending)
```

## Building

```bash
# Build all crates
cargo build --workspace

# Run all tests
cargo test --workspace

# Run clippy
cargo clippy --workspace -- -D warnings

# Build release binary
cargo build --release
```

## Next Phases

### Phase 5: Basic Scheduler (Week 6)
- Pod scheduling to nodes
- Resource-based filtering
- Simple scoring algorithm

### Phase 6: Main Binary Integration (Week 7)
- Single binary combining all components
- Configuration and CLI
- TLS support
- Graceful shutdown
- Observability (logging, metrics)

### Phase 7: Testing & Documentation (Week 8)
- Integration tests
- End-to-end tests with kubectl
- User documentation
- API documentation

## Key Features

- **Pure Rust**: 100% Rust implementation, no C++ dependencies
- **Portable**: Supports x86_64, ARM64, illumos
- **redb Storage**: Fast, ACID-compliant storage with MVCC
- **DAG Versioning**: Advanced resource versioning with conflict detection
- **Type-Safe**: Leverages Rust's type system for correctness
- **Rich Errors**: miette diagnostics for user-friendly error messages

## License

MIT OR Apache-2.0
