# Primeros Pasos

## Prerrequisitos

- Rust 1.75+ (estable)
- Python 3.8+ (para scripts de compilación)
- Ejecutor de comandos `just`

## Instalación

```bash
git clone https://github.com/celestia-island/noa.git
cd noa
just init          # obtener dependencias
just build-dev     # compilación de desarrollo
```

El binario `noa` se encuentra en `target/debug/noa`.

## Inicio Rápido

```bash
# Inicializar un nuevo repositorio
noa init .

# Verificar estado
noa status
# En el espacio de trabajo: default

# Crear un espacio de trabajo
noa workspace create feature-1

# Cambiar a él
noa workspace switch feature-1

# Crear una instantánea
noa snapshot create -m "trabajo inicial"

# Ver historial
noa log

# Volver atrás y fusionar
noa workspace switch default
noa workspace merge feature-1

# Gestionar remotos
noa remote add origin https://github.com/example/repo.git
noa remote list
```

## Ejecutar Ejemplos

```bash
python3 examples/run_all.py
```

## Desarrollo

```bash
just fmt            # formatear código
just clippy         # lint
just test           # ejecutar pruebas
just check          # comprobación de tipos
```
