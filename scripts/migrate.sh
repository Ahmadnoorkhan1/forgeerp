#!/bin/bash
# Migration runner script for SQLx migrations
#
# This script provides convenient commands for running migrations in different environments.
# It supports both local development (with .env file) and CI environments.

set -e

# Colors for output
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
NC='\033[0m' # No Color

# Function to print colored output
info() {
    echo -e "${GREEN}[INFO]${NC} $1"
}

warn() {
    echo -e "${YELLOW}[WARN]${NC} $1"
}

error() {
    echo -e "${RED}[ERROR]${NC} $1"
}

# Check if DATABASE_URL is set, otherwise try to construct from .env
if [ -z "$DATABASE_URL" ]; then
    if [ -f ".env" ]; then
        source .env
        export DATABASE_URL="${DATABASE_URL:-postgres://${POSTGRES_USER:-forgeerp}:${POSTGRES_PASSWORD:-forgeerp}@${POSTGRES_HOST:-localhost}:${POSTGRES_PORT:-5432}/${POSTGRES_DB:-forgeerp}}"
        info "Using DATABASE_URL from .env file"
    else
        error "DATABASE_URL not set and .env file not found"
        exit 1
    fi
fi

# Check if sqlx-cli is installed
if ! command -v sqlx &> /dev/null; then
    error "sqlx-cli not found. Install it with: cargo install sqlx-cli --no-default-features --features postgres"
    exit 1
fi

# Set migrations directory
MIGRATIONS_DIR="${MIGRATIONS_DIR:-docker/migrations}"

# Parse command
COMMAND=${1:-info}

case "$COMMAND" in
    run)
        info "Running pending migrations..."
        sqlx migrate --source "$MIGRATIONS_DIR" run
        info "Migrations completed successfully"
        ;;
    revert)
        warn "Reverting last migration..."
        sqlx migrate --source "$MIGRATIONS_DIR" revert
        info "Migration reverted successfully"
        ;;
    info|status)
        info "Migration status:"
        sqlx migrate --source "$MIGRATIONS_DIR" info
        ;;
    add)
        if [ -z "$2" ]; then
            error "Migration name required. Usage: $0 add <migration_name>"
            exit 1
        fi
        info "Creating new migration: $2"
        sqlx migrate --source "$MIGRATIONS_DIR" add "$2"
        ;;
    validate)
        info "Validating migration files..."
        sqlx migrate --source "$MIGRATIONS_DIR" info
        info "All migrations are valid"
        ;;
    *)
        echo "Usage: $0 {run|revert|info|add|validate}"
        echo ""
        echo "Commands:"
        echo "  run       - Apply all pending migrations"
        echo "  revert    - Revert the last applied migration"
        echo "  info      - Show migration status (default)"
        echo "  add NAME  - Create a new migration file"
        echo "  validate  - Validate migration files"
        exit 1
        ;;
esac

