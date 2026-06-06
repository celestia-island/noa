# Elección del Backend Git: gix (gitoxide) vs git2 (libgit2)

## Estado: Análisis

**Actual**: `git2 = "0.19"` (binding C a libgit2)
**Propuesto**: `gix = "0.84"` (implementación git en Rust puro)

## Resumen

gix (gitoxide) es una implementación madura de Git en Rust puro con suficiente
cobertura de funcionalidades para reemplazar a git2 en el puente git de noa. La migración
elimina una dependencia C (libgit2), reduce la fricción de compilación cruzada,
y proporciona APIs idiomáticas de Rust.

## Matriz de Comparación

| Criterio | git2 (libgit2) | gix (gitoxide) |
|-----------|---------------|----------------|
| **Lenguaje** | C (bindings Rust mediante crate git2) | Rust puro |
| **Madurez** | 14 años, probado en producción | 5 años, desarrollo activo (0.84) |
| **Compilación** | ~15s (recompilación), requiere CMake + libgit2-dev | ~8s (recompilación), solo cargo |
| **Compilación cruzada** | Problemática (necesita cadena de herramientas C cruzada) | Trivial (cargo cross-compile) |
| **Estilo de API** | Similar a C, bloques unsafe, lifetimes manuales | Rústico-idiomático, seguro en préstamos, patrones builder |
| **Manejo de objetos** | git2::Blob, Tree, Commit mediante ODB | gix::objs::BlobRef, TreeRef, CommitRef |
| **Recorrido de árboles** | Iterador manual con .to_object() | breadthfirst/virtual_roots con delegado |
| **Push/pull remoto** | git2::Remote (fetch, push) | gix::remote (connect, fetch, push) |
| **Pack/pack-index** | Integrado | Completo (crate propio: gix-pack) |
| **Refs** | git2::Reference (lectura/escritura) | gix::refs (soporte completo de transacciones) |
| **Configuración** | Limitada (a nivel de repositorio) | Configuración en capas (sistema, usuario, repositorio) |
| **SHA-1/256** | Solo SHA-1 | SHA-1 + SHA-256 (experimental) |
| **Seguridad de memoria** | Riesgo por bugs C de libgit2 | Garantías de Rust |
| **Auditabilidad** | Necesidad de auditar el código base C de libgit2 | Solo Rust, cargo-audit |
| **Comunidad** | Enorme (todas las principales herramientas VCS) | Creciente (gitoxide, crates-index-diff, etc.) |

## Requisitos del Puente Git de noa

Uso actual en `src/git/`:

```rust
// import.rs:
//   - Repository::open()           → gix::open()
//   - repo.head().target()         → gix.head().project_id()
//   - repo.find_commit(oid)        → gix.find_object().try_into_commit()
//   - commit.tree()                → gix.find_object(commit.tree()).try_into_tree()
//   - tree.iter()                  → gix::objs::TreeRefIter
//   - entry.to_object(repo)        → gix.find_object(entry.oid())
//   - obj.kind() === Blob          → obj.kind == ObjectKind::Blob
//   - blob.content()               → blob.data

// translate.rs:
//   - Manipulación pura a nivel de bytes (sin dependencia git externa)

// export.rs:
//   - Actualmente todo!() — push usaría gix::remote::connect()
//   - Generación de archivos pack mediante gix-pack (si es necesario)
```

Las 6 llamadas API actuales tienen equivalentes directos en gix.

## Cobertura de Funcionalidades de gix para noa

| Funcionalidad necesaria | Soporte git2 | Soporte gix | Notas |
|---------------|-------------|-------------|-------|
| Abrir repositorio | ✅ | ✅ | `gix::open()` o `gix::ThreadSafeRepository::open()` |
| Leer referencia HEAD | ✅ | ✅ | `gix.head_ref()` / `gix.head()` |
| Encontrar commit por OID | ✅ | ✅ | `gix.find_object(id)?.try_into_commit()` |
| Leer árbol desde commit | ✅ | ✅ | `gix.find_object(commit.tree())?.try_into_tree()` |
| Iterar entradas de árbol | ✅ | ✅ | `tree.iter()` retorna `TreeRefIter` |
| Leer contenido de blob | ✅ | ✅ | `blob.data` en `BlobRef` |
| Fetch desde remoto | ✅ | ✅ | `gix::remote::connect()`  |
| Push a remoto | ✅ | ✅ | `gix::remote::connect()`  |
| Clone | ✅ | ✅ | `gix::prepare_clone()` |
| Generación de archivos pack | ✅ | ✅ | crate `gix-pack` |
| Soporte SHA-256 | ❌ | ✅ (experimental) | Relevante para instantáneas SHA-256 |
| Soporte asíncrono | ❌ | ✅ (opt-in) | Bueno para integración con tokio |

## Viabilidad

Todas las operaciones git actuales y planificadas tienen equivalentes en gix. El mapeo
de API es directo:

```rust
// git2 (actual)
let repo = git2::Repository::open(path)?;
let head = repo.head()?;
let commit = repo.find_commit(head.target().unwrap())?;
let tree = commit.tree()?;

// gix (propuesto)
let repo = gix::open(path)?;
let head = repo.head_ref()?.expect("HEAD not found");
let head_id = head.id().detach();
let commit = repo.find_object(head_id)?.try_into_commit()
    .map_err(|_| NoaError::Remote("not a commit".into()))?;
let tree = repo.find_object(commit.tree())?.try_into_tree()
    .map_err(|_| NoaError::Remote("not a tree".into()))?;
```

## Plan de Migración

### Fase 1: Reemplazar import.rs (operaciones de solo lectura)
- Reemplazar git2::Repository con gix::ThreadSafeRepository
- Reimplementar el recorrido de árboles
- Ejecutar las pruebas existentes de importación git

### Fase 2: Reemplazar translate.rs
- No se necesitan cambios (manipulación pura de bytes, sin dependencia C)

### Fase 3: Implementar export.rs mediante gix
- Usar gix::remote para push
- Usar gix::prepare_clone para clone
- Usar gix-pack para generación de packfiles (si es necesario para el lado del servidor)

### Fase 4: Eliminar git2 de Cargo.toml
- Eliminar la dependencia del sistema libgit2
- Verificar compilación cruzada (x86_64 → aarch64, → wasm en el futuro)

## Evaluación de Riesgos

| Riesgo | Probabilidad | Impacto | Mitigación |
|------|-----------|--------|------------|
| Ruptura de API de gix (0.x) | Media | Bajo | Fijar versión, adaptarse a cambios de API |
| Funcionalidades avanzadas faltantes | Baja | Medio | gix tiene push/fetch remoto desde 0.50+ |
| Regresión de rendimiento | Baja | Bajo | gix suele ser más rápido (sin sobrecarga de FFI C) |
| Riesgo de adopción comunitaria | Bajo | Bajo | gix es la biblioteca git de facto en Rust |
| Errores de interoperabilidad SHA-256 | Medio | Bajo | Protegido por feature gate, omitir mediante translate.rs puro |

## Recomendación

**Migrar a gix.** Los beneficios (cero dependencias C, seguridad en Rust puro,
compilación cruzada más sencilla, soporte SHA-256) superan los riesgos (estabilidad
de API 0.x, comunidad más pequeña). La migración es de bajo riesgo porque:

1. El uso actual de git2 es mínimo (6 llamadas API en import.rs)
2. translate.rs no requiere cambios
3. export.rs no está implementado (terreno virgen para gix)
4. gix es la biblioteca git estándar de Rust (usada por el índice de crates.io)

## Dependencias Después de la Migración

```diff
- git2 = "0.19"           # binding C libgit2
+ gix = { version = "0.84", features = ["basic", "index", "pack"] }
```

Sin nuevas dependencias del sistema. Solo `cargo build`.
