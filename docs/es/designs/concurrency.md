# Diseño de Concurrencia

## Planteamiento del Problema

Los sistemas VCS tradicionales serializan las escrituras a través de un único bloqueo o cola de fusión.
Esto funciona para flujos de trabajo a escala humana (10-100 commits/día) pero se rompe
con agentes IA que producen miles de modificaciones de archivos por minuto.

```mermaid
graph LR
    subgraph Problema
        A["100 agentes IA × 10 escrituras/seg = 1000 escrituras/seg"]
    end
    subgraph Tradicional
        B["Git/SVN: bloqueo único → cola<br/>~100 escrituras/seg de rendimiento"]
    end
    subgraph Noa
        C["noa: registros de solo anexión<br/>~10,000+ escrituras/seg de rendimiento"]
    end
```

## Arquitectura

### Capa 1: AgentLog (Ruta de Escritura)

Cada espacio de trabajo tiene un archivo JSONL dedicado bajo `.noa/agent-logs/`.

```mermaid
graph LR
    ws1["espacio de trabajo 'agent-001'"] --> f1["agent-logs/agent-001.log"]
    ws2["espacio de trabajo 'agent-002'"] --> f2["agent-logs/agent-002.log"]
```

Las escrituras usan la bandera `O_APPEND`, que proporciona:
- **Atomicidad**: El kernel garantiza atomicidad de escritura completa para anexiones
- **Ordenación**: Las escrituras se serializan por archivo (por espacio de trabajo)
- **Sin bloqueo**: No se requiere fcntl/flock entre archivos diferentes

```rust
pub trait AgentLog: Send + Sync {
    async fn append(&self, workspace: &str, entry: &LogEntry) -> Result<()>;
    async fn read_all(&self, workspace: &str) -> Result<Vec<LogEntry>>;
}
```

### Capa 2: Almacén de Instantáneas (Ruta de Lectura)

Las instantáneas se almacenan en redb con MVCC (control de concurrencia multiversión):
- Las escrituras se serializan a través de la transacción de escritor único de redb
- Las lecturas nunca bloquean las escrituras (aislamiento de instantánea)
- Múltiples lectores pueden acceder simultáneamente

### Capa 3: Consolidación (Ruta de Fusión)

El `Consolidator` lee todos los registros de agentes a través de los espacios de trabajo, ordena por
marca de tiempo y produce una cadena de instantáneas unificada:

```mermaid
graph TD
    subgraph Entrada
        L1["agent-001.log: [write A@t1, write B@t3]"]
        L2["agent-002.log: [write C@t2, write D@t4]"]
    end
    subgraph Consolidado
        C1["write A@t1 → write C@t2 → write B@t3 → write D@t4"]
    end
    L1 --> C1
    L2 --> C1
```

Esto se ejecuta de forma asíncrona y no bloquea las escrituras de los agentes.

## Garantías de Concurrencia

| Garantía | Mecanismo |
|-----------|-----------|
| Sin pérdida de datos | O_APPEND + fsync por escritura |
| Ordenación por espacio de trabajo | Archivo único por espacio de trabajo |
| Ordenación entre espacios de trabajo | Marcas de tiempo en microsegundos |
| Consistencia de lectura | Aislamiento de instantánea MVCC de redb |
| Seguridad de cabeza de espacio de trabajo | Actualizaciones CAS (comparar e intercambiar) |

## Análisis de Escalabilidad

### Proceso Único (Embebido)

| Agentes | 1-100 (mismo proceso) |
| Rendimiento | ~10,000 escrituras/seg |
| Cuello de botella | E/S de disco (fsync por escritura) |

### Multi-Proceso (noa-server)

| Agentes | 100-1000 (procesos separados) |
| Rendimiento | ~5,000 escrituras/seg |
| Cuello de botella | serialización de escritura del lado del servidor |

El servidor mantiene una única conexión de base de datos y serializa las escrituras.
Los registros de agentes permanecen por archivo para ingesta paralela.

### Distribuido (Backend MinIO)

| Agentes | 1000+ |
| Rendimiento | Límite de tasa PUT de S3 (~3,500/seg por prefijo) |
| Cuello de botella | red + límites de tasa de S3 |

## Comparación con Alternativas

### Git + Bloqueo de Archivos

```mermaid
graph LR
    A["Problema: Bloqueos advisory, sin aplicación forzosa"]
    B["Contención: Alta (actualización de ref única por push)"]
    C["Resolución: Requiere fusión manual"]
```

### SVN + svn:needs-lock

```mermaid
graph LR
    A["Problema: Bloqueos a nivel de archivo bloquean a todos los demás escritores"]
    B["Contención: Muy alta (commits serializados)"]
    C["Resolución: Espera de bloqueo → timeout → fallo"]
```

### Transformación Operacional (OT)

```mermaid
graph LR
    A["Problema: Algoritmo complejo, difícil de implementar correctamente"]
    B["Contención: Baja (transformación en memoria)"]
    C["Resolución: Automática, pero requiere servidor centralizado"]
```

### CRDT (Tipos de Datos Replicados Sin Conflicto)

```mermaid
graph LR
    A["Problema: Gran sobrecarga de metadatos, consistencia eventual"]
    B["Contención: Ninguna"]
    C["Resolución: Automática, pero puede producir resultados inesperados"]
```

### Enfoque de noa

```mermaid
graph LR
    A["Problema: Las escrituras de agentes son efímeras y pueden regenerarse"]
    B["Enfoque: Registros de solo anexión + consolidación asíncrona"]
    C["Contención: Ninguna para escrituras, serializada para instantáneas"]
    D["Resolución: upstream-wins por defecto + reaplicación por el agente"]
```

## Estrategia de fsync

Cada escritura del registro de agente sigue este patrón:

```rust
file.write_all(data)?;   // anexar al archivo
file.flush()?;           // vaciar búfer de espacio de usuario
file.sync_data()?;       // fsync — garantizar durabilidad en disco
```

En Linux, `sync_data()` omite la sincronización de metadatos (fdatasync), reduciendo la latencia
en ~30% comparado con fsync completo.

## Futuro: Agrupación de Registro de Escritura Anticipada

Actual: un fsync por escritura.
Planificado: agrupar múltiples escrituras en un solo fsync:

```rust
// El agente almacena escrituras en memoria
agent.buffer(write_a);
agent.buffer(write_b);
agent.buffer(write_c);
agent.flush(); // un solo fsync para las tres
```

Mejora de rendimiento esperada: 3-5x para escrituras en ráfagas.
