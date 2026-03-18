use anyhow::Result;
use crate::ai::{embedder, vector_store::VectorStore};

pub async fn load() -> Result<VectorStore> {
    let chunks  = context_chunks();
    let vectors = embed_chunks(&chunks).await?;
    let store   = build_store(chunks, vectors);

    println!("[setup] store listo — {} chunks cargados", store.len());
    Ok(store)
}

fn context_chunks() -> Vec<(&'static str, &'static str)> {
    vec![
        // ── Arquitectura ──────────────────────────────────────────────────────
        ("arch-001",
         "La arquitectura de microservicios divide una aplicación en servicios pequeños e independientes, \
          cada uno con su propia base de datos y desplegado por separado. \
          Se comunican via REST, gRPC o mensajería asíncrona (Kafka, RabbitMQ). \
          Ventajas: escalabilidad independiente, deploys frecuentes, fault isolation. \
          Desventajas: complejidad operacional, consistencia eventual, latencia de red."),

        ("arch-002",
         "El patrón CQRS (Command Query Responsibility Segregation) separa las operaciones de lectura \
          y escritura en modelos distintos. Los commands modifican estado, las queries solo leen. \
          Se combina frecuentemente con Event Sourcing para mantener el historial completo de cambios. \
          Útil cuando las necesidades de lectura y escritura tienen cargas muy distintas."),

        ("arch-003",
         "Domain-Driven Design (DDD) organiza el código alrededor del dominio de negocio. \
          Conceptos clave: Bounded Context (límite explícito de un dominio), \
          Aggregate (raíz que garantiza consistencia), \
          Value Object (inmutable, sin identidad), Entity (tiene identidad propia), \
          Domain Event (algo que ocurrió en el dominio)."),

        ("arch-004",
         "El patrón Saga maneja transacciones distribuidas entre microservicios. \
          Coreografía: cada servicio reacciona a eventos y emite los suyos. \
          Orquestación: un servicio central coordina el flujo. \
          Ante un fallo se ejecutan compensating transactions para revertir los pasos anteriores."),

        // ── Bases de datos ────────────────────────────────────────────────────
        ("db-001",
         "Los índices en bases de datos relacionales aceleran las queries pero tienen costo en escrituras. \
          B-Tree index: óptimo para rangos y equality. \
          Hash index: solo equality, más rápido para ese caso. \
          Composite index: el orden de las columnas importa, debe coincidir con el orden del WHERE/ORDER BY. \
          Un query sin índice hace full table scan — crítico en tablas grandes."),

        ("db-002",
         "Las propiedades ACID garantizan integridad en bases de datos relacionales: \
          Atomicity (todo o nada), Consistency (el estado siempre es válido), \
          Isolation (las transacciones no se interfieren), Durability (los cambios persisten). \
          Las bases NoSQL suelen relajar estas propiedades a cambio de escalabilidad horizontal (BASE: Basically Available, Soft state, Eventually consistent)."),

        ("db-003",
         "PostgreSQL features relevantes para backend: \
          JSONB para datos semiestructurados con índices GIN, \
          CTEs (WITH queries) para queries complejas y recursivas, \
          Window functions para analíticas sin GROUP BY, \
          Partitioning para tablas muy grandes, \
          LISTEN/NOTIFY para pub-sub liviano entre servicios."),

        // ── APIs ──────────────────────────────────────────────────────────────
        ("api-001",
         "REST best practices: usar sustantivos en los endpoints (/orders, no /getOrders), \
          HTTP verbs semánticamente correctos (GET idempotente, POST crea, PUT reemplaza, PATCH modifica parcialmente), \
          status codes correctos (201 Created, 400 Bad Request, 409 Conflict, 422 Unprocessable Entity), \
          versionado en la URL (/v1/) o en headers, \
          paginación con cursor o offset+limit."),

        ("api-002",
         "gRPC usa Protocol Buffers como formato de serialización — más eficiente que JSON. \
          Soporta streaming bidireccional, ideal para comunicación inter-servicios. \
          Genera código cliente/servidor desde el .proto. \
          Desventaja: menos legible que REST, requiere tooling específico. \
          Útil cuando la latencia y el throughput son críticos entre servicios internos."),

        // ── Concurrencia ──────────────────────────────────────────────────────
        ("concurrency-001",
         "Los problemas clásicos de concurrencia son: \
          Race condition (dos threads acceden al mismo dato sin sincronización), \
          Deadlock (dos threads se esperan mutuamente), \
          Starvation (un thread nunca obtiene el recurso). \
          En Rust, el ownership system previene race conditions en compile time. \
          En otros lenguajes se usan Mutex, RWLock, canales o estructuras lock-free."),

        ("concurrency-002",
         "El modelo async/await permite concurrencia sin threads del OS. \
          Un runtime (Tokio en Rust, asyncio en Python, libuv en Node) multiplexa tasks en un thread pool. \
          Ideal para I/O-bound workloads (HTTP, DB, files). \
          Para CPU-bound hay que spawnear threads separados para no bloquear el runtime. \
          El costo es la complejidad: futuros, lifetimes, Send/Sync bounds."),

        // ── Infraestructura ───────────────────────────────────────────────────
        ("infra-001",
         "Docker empaqueta una aplicación con todas sus dependencias en una imagen reproducible. \
          Docker Compose orquesta múltiples contenedores localmente (app, db, cache, broker). \
          En producción se usa Kubernetes para orquestación, scaling automático, \
          health checks, rolling deploys y service discovery. \
          Las imágenes deben ser lo más pequeñas posible: multi-stage builds, base alpine."),

        ("infra-002",
         "CI/CD automatiza el camino del código al deploy. \
          CI: cada push corre tests, linting y build. \
          CD: si CI pasa, se despliega automáticamente a staging o producción. \
          Estrategias de deploy: Blue/Green (dos entornos, se switchea el tráfico), \
          Canary (se manda un porcentaje del tráfico a la nueva versión), \
          Rolling (se reemplazan las instancias de a una)."),

        // ── Performance ───────────────────────────────────────────────────────
        ("perf-001",
         "Caching reduce latencia y carga en la DB. \
          Cache-aside: la app primero busca en cache, si hay miss busca en DB y popula el cache. \
          Write-through: se escribe en cache y DB simultáneamente. \
          TTL define cuánto tiempo vive un valor en cache. \
          Redis es el estándar: soporta strings, hashes, sorted sets, pub-sub y persistencia opcional."),

        ("perf-002",
         "El N+1 problem ocurre cuando se hace una query para obtener N registros \
          y luego N queries más para obtener sus relaciones. \
          Se soluciona con eager loading (JOIN o includes), \
          DataLoader pattern (batching de queries), \
          o desnormalización estratégica. \
          Es uno de los problemas de performance más comunes en backends con ORM."),
    ]
}

async fn embed_chunks(chunks: &[(&str, &str)]) -> Result<Vec<Vec<f32>>> {
    println!("[setup] vectorizando {} chunks…", chunks.len());

    let texts: Vec<&str> = chunks.iter().map(|(_, text)| *text).collect();
    embedder::embed_batch(&texts).await
}

fn build_store(chunks: Vec<(&str, &str)>, vectors: Vec<Vec<f32>>) -> VectorStore {
    let mut store = VectorStore::new();

    for ((id, payload), vector) in chunks.into_iter().zip(vectors) {
        store.upsert(id, vector, payload);
    }

    store
}