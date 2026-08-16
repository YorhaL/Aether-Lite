#!/usr/bin/env bash
# First-time Docker Compose installer for Aether Lite.

set -euo pipefail
umask 077

SCRIPT_DIR="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd -P)"
ENV_FILE="${SCRIPT_DIR}/.env"
ENV_TEMPLATE="${SCRIPT_DIR}/.env.example"

MODE=""
START_SERVICES=true
WAIT_TIMEOUT_SECS=180

usage() {
    cat <<'EOF'
Usage: ./install.sh [options]

Options:
  --mode MODE          single-node or postgres
  --no-start           generate .env without starting containers
  --timeout SECONDS    startup health wait timeout (default: 180)
  -h, --help           show this help

The installer creates .env only once. It never overwrites an existing .env.
For non-interactive use, provide ADMIN_PASSWORD in the environment:

  ADMIN_PASSWORD='replace-with-a-strong-password' \
    ./install.sh --mode single-node
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
        --no-start)
            START_SERVICES=false
            shift
            ;;
        --timeout)
            [[ $# -ge 2 ]] || die "--timeout requires a value"
            WAIT_TIMEOUT_SECS="$2"
            shift 2
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

choose_mode() {
    if [[ -n "${MODE}" ]]; then
        return
    fi
    [[ -t 0 ]] || die "--mode is required for non-interactive installation"

    echo "Select deployment mode:"
    echo "  1) SQLite single node (recommended)"
    echo "  2) PostgreSQL + Redis"
    read -r -p "Choice [1]: " choice
    case "${choice:-1}" in
        1) MODE="single-node" ;;
        2) MODE="postgres" ;;
        *) die "invalid deployment mode choice" ;;
    esac
}

choose_mode
case "${MODE}" in
    single-node)
        COMPOSE_FILE="docker-compose.single-node.yml"
        ;;
    postgres)
        COMPOSE_FILE="docker-compose.yml"
        ;;
    *)
        die "unsupported mode: ${MODE}; expected single-node or postgres"
        ;;
esac

[[ "${WAIT_TIMEOUT_SECS}" =~ ^[1-9][0-9]*$ ]] || die "--timeout must be a positive integer"
[[ -f "${ENV_TEMPLATE}" ]] || die "environment template not found: ${ENV_TEMPLATE}"
[[ -f "${SCRIPT_DIR}/${COMPOSE_FILE}" ]] || die "compose file not found: ${COMPOSE_FILE}"
[[ ! -e "${ENV_FILE}" ]] || die ".env already exists; use update.sh for an existing deployment"
command -v openssl >/dev/null 2>&1 || die "openssl is required"
if [[ "${START_SERVICES}" == "true" ]]; then
    command -v docker >/dev/null 2>&1 || die "docker is required"
    docker compose version >/dev/null 2>&1 || die "Docker Compose Plugin is required"
    docker info >/dev/null 2>&1 || die "Docker is not running"
fi

random_secret() {
    openssl rand -base64 "$1" | tr '+/' '-_' | tr -d '=\n'
}

set_env() {
    local key="$1"
    local value="$2"
    local line
    local replaced=false
    local temp_file
    temp_file="$(mktemp "${ENV_FILE}.tmp.XXXXXX")"

    while IFS= read -r line || [[ -n "${line}" ]]; do
        if [[ "${line}" =~ ^[[:space:]]*#?[[:space:]]*${key}= ]]; then
            if [[ "${replaced}" == "false" ]]; then
                printf '%s=%s\n' "${key}" "${value}" >> "${temp_file}"
                replaced=true
            fi
        else
            printf '%s\n' "${line}" >> "${temp_file}"
        fi
    done < "${ENV_FILE}"

    if [[ "${replaced}" == "false" ]]; then
        printf '\n%s=%s\n' "${key}" "${value}" >> "${temp_file}"
    fi
    mv -- "${temp_file}" "${ENV_FILE}"
}

validate_admin_password() {
    local password="$1"
    [[ "${password}" != *$'\n'* && "${password}" != *$'\r'* ]] \
        || die "administrator password must be a single line"
    [[ "${password}" != *"'"* ]] \
        || die "administrator password must not contain a single quote"
    [[ ${#password} -ge 8 ]] \
        || die "administrator password must contain at least 8 characters"
    [[ "${password}" =~ [[:alpha:]] ]] \
        || die "administrator password must contain at least one letter"
    [[ "${password}" =~ [[:digit:]] ]] \
        || die "administrator password must contain at least one digit"
}

read_admin_password() {
    local first
    local second

    if [[ -n "${ADMIN_PASSWORD:-}" ]]; then
        validate_admin_password "${ADMIN_PASSWORD}"
        printf '%s' "${ADMIN_PASSWORD}"
        return
    fi
    [[ -t 0 ]] || die "ADMIN_PASSWORD is required for non-interactive installation"

    while true; do
        read -r -s -p "Administrator password: " first
        echo
        validate_admin_password "${first}"
        read -r -s -p "Confirm administrator password: " second
        echo
        if [[ "${first}" == "${second}" ]]; then
            printf '%s' "${first}"
            return
        fi
        echo "Passwords do not match; try again." >&2
    done
}

admin_password="$(read_admin_password)"

cp -- "${ENV_TEMPLATE}" "${ENV_FILE}"
set_env "APP_IMAGE" "ghcr.io/yorhal/aether-lite:latest"
set_env "JWT_SECRET_KEY" "$(random_secret 32)"
set_env "ENCRYPTION_KEY" "$(random_secret 32)"
set_env "ADMIN_PASSWORD" "'${admin_password}'"
set_env "DB_PASSWORD" "$(random_secret 24)"
set_env "REDIS_PASSWORD" "$(random_secret 24)"

echo ">>> Created ${ENV_FILE}"
echo ">>> Deployment mode: ${MODE}"

if [[ "${START_SERVICES}" != "true" ]]; then
    echo ">>> Configuration is ready. Containers were not started."
    exit 0
fi

COMPOSE=(
    docker compose
    --env-file "${ENV_FILE}"
    --project-directory "${SCRIPT_DIR}"
    -f "${SCRIPT_DIR}/${COMPOSE_FILE}"
)

echo ">>> Pulling images..."
"${COMPOSE[@]}" pull

echo ">>> Starting services..."
if ! "${COMPOSE[@]}" up -d --wait --wait-timeout "${WAIT_TIMEOUT_SECS}"; then
    echo ">>> Installation failed. Current status:" >&2
    "${COMPOSE[@]}" ps >&2 || true
    "${COMPOSE[@]}" logs --tail 100 app >&2 || true
    exit 1
fi

"${COMPOSE[@]}" ps
echo ">>> Aether Lite is available at http://127.0.0.1:8084"
