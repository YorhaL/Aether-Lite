#!/usr/bin/env bash
# Pull and recreate the Aether Lite application container.

set -euo pipefail

SCRIPT_DIR="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd -P)"

MODE="postgres"
COMPOSE_DIR="${SCRIPT_DIR}"
WAIT_TIMEOUT_SECS=120
PREPARE_ONLY=false
FORCE_RECREATE=false
SHOW_LOGS=false

usage() {
    cat <<'EOF'
Usage: ./update.sh [options]

Options:
  --mode MODE          postgres or single-node (default: postgres)
  --compose-dir DIR    deployment directory (default: script directory)
  --prepare            pull the image without recreating the container
  --force-recreate     recreate the app even when the image did not change
  --timeout SECONDS    health wait timeout (default: 120)
  --logs               follow app logs after a successful update
  -h, --help           show this help

Examples:
  ./update.sh
  ./update.sh --mode single-node
  ./update.sh --force-recreate --logs
EOF
}

die() {
    echo "ERROR: $*" >&2
    exit 1
}

while [[ $# -gt 0 ]]; do
    case "$1" in
        --mode)
            [[ $# -ge 2 ]] || die "--mode requires a value"
            MODE="$2"
            shift 2
            ;;
        --compose-dir)
            [[ $# -ge 2 ]] || die "--compose-dir requires a value"
            COMPOSE_DIR="$2"
            shift 2
            ;;
        --prepare)
            PREPARE_ONLY=true
            shift
            ;;
        --force-recreate)
            FORCE_RECREATE=true
            shift
            ;;
        --timeout)
            [[ $# -ge 2 ]] || die "--timeout requires a value"
            WAIT_TIMEOUT_SECS="$2"
            shift 2
            ;;
        --logs)
            SHOW_LOGS=true
            shift
            ;;
        -h|--help)
            usage
            exit 0
            ;;
        *)
            die "unknown argument: $1"
            ;;
    esac
done

case "${MODE}" in
    postgres)
        COMPOSE_FILE="docker-compose.yml"
        ;;
    single-node)
        COMPOSE_FILE="docker-compose.single-node.yml"
        ;;
    *)
        die "unsupported mode: ${MODE}; expected postgres or single-node"
        ;;
esac

[[ "${WAIT_TIMEOUT_SECS}" =~ ^[1-9][0-9]*$ ]] || die "--timeout must be a positive integer"
command -v docker >/dev/null 2>&1 || die "docker is required"
docker compose version >/dev/null 2>&1 || die "Docker Compose Plugin is required"
docker info >/dev/null 2>&1 || die "Docker is not running"

COMPOSE_DIR="$(cd -- "${COMPOSE_DIR}" && pwd -P)"
ENV_FILE="${COMPOSE_DIR}/.env"
COMPOSE_PATH="${COMPOSE_DIR}/${COMPOSE_FILE}"

[[ -f "${ENV_FILE}" ]] || die "environment file not found: ${ENV_FILE}"
[[ -f "${COMPOSE_PATH}" ]] || die "compose file not found: ${COMPOSE_PATH}"

COMPOSE=(
    docker compose
    --env-file "${ENV_FILE}"
    --project-directory "${COMPOSE_DIR}"
    -f "${COMPOSE_PATH}"
)

echo ">>> Deployment mode: ${MODE}"
echo ">>> Compose directory: ${COMPOSE_DIR}"
echo ">>> Pulling app image..."
"${COMPOSE[@]}" pull app

if [[ "${PREPARE_ONLY}" == "true" ]]; then
    echo ">>> Image is ready. The running container was not changed."
    exit 0
fi

UP_ARGS=(up -d --wait --wait-timeout "${WAIT_TIMEOUT_SECS}")
if [[ "${FORCE_RECREATE}" == "true" ]]; then
    UP_ARGS+=(--force-recreate)
fi
UP_ARGS+=(app)

echo ">>> Recreating app container..."
if ! "${COMPOSE[@]}" "${UP_ARGS[@]}"; then
    echo ">>> Update failed. Current status:" >&2
    "${COMPOSE[@]}" ps >&2 || true
    "${COMPOSE[@]}" logs --tail 100 app >&2 || true
    exit 1
fi

"${COMPOSE[@]}" ps
echo ">>> Update completed."

if [[ "${SHOW_LOGS}" == "true" ]]; then
    "${COMPOSE[@]}" logs -f app
fi
