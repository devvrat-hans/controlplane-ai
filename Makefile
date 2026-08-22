.PHONY: help build check test fmt clippy infra-up infra-down migrate seed frontend-dev demo verify clean

# Detect docker compose variant (v2 plugin or v1 standalone)
ifeq ($(shell docker compose version 2>/dev/null),)
  DC := docker-compose
else
  DC := docker compose
endif

help: ## Show available targets
	@grep -E '^[a-zA-Z_-]+:.*?## .*$$' $(MAKEFILE_LIST) | sort | awk 'BEGIN {FS = ":.*?## "}; {printf "\033[36m%-20s\033[0m %s\n", $$1, $$2}'

# --- Rust -------------------------------------------------------------------

build: ## Build all Rust services
	cargo build

check: ## Type-check all Rust crates
	cargo check

test: ## Run all Rust tests
	cargo test

fmt: ## Format all Rust code
	cargo fmt

clippy: ## Run clippy lints
	cargo clippy -- -D warnings

# --- Infrastructure ----------------------------------------------------------

infra-up: ## Start PostgreSQL + NATS (Docker)
	$(DC) -f infra/docker-compose.yml up -d

infra-down: ## Stop infrastructure containers
	$(DC) -f infra/docker-compose.yml down

infra-reset: ## Stop containers and remove volumes
	$(DC) -f infra/docker-compose.yml down -v

migrate: ## Run database migrations
	bash infra/migrate.sh

seed: migrate ## Run migrations + seed demo data
	@echo "Demo data seeded via migration 009."

# --- Frontend ----------------------------------------------------------------

frontend-install: ## Install frontend dependencies
	cd frontend && pnpm install

frontend-dev: ## Start frontend dev server
	cd frontend && pnpm dev

frontend-build: ## Build frontend for production
	cd frontend && pnpm build

frontend-lint: ## Lint frontend with Biome
	cd frontend && pnpm dlx @biomejs/biome check .

# --- Full demo ---------------------------------------------------------------

demo: infra-up migrate build ## Full demo: infra + migrate + build + run
	@echo "Starting ControlPlane gateway..."
	cargo run -p controlplane-gateway

# --- CI / Verify -------------------------------------------------------------

verify: fmt clippy test frontend-build ## Full CI sweep: fmt, clippy, test, frontend build
	@echo "All checks passed."

# --- Cleanup -----------------------------------------------------------------

clean: ## Remove build artifacts
	cargo clean
	rm -rf frontend/.next frontend/node_modules
