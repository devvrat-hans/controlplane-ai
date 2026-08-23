# Demo Credentials

These accounts are pre-seeded in the database by migration `009_seed_demo_data.sql`.

## User Accounts

| Email | Password | Role | Permissions |
|-------|----------|------|-------------|
| `admin@controlplane.ai` | `admin123` | admin | Full access: policies, escalations, audit, settings |
| `reviewer@controlplane.ai` | `reviewer123` | reviewer | Can resolve escalations, view audit trail |
| `viewer@controlplane.ai` | `viewer123` | viewer | Read-only dashboard access |

## Demo Applications

| App ID | Name | Description |
|--------|------|-------------|
| `10000000-...-000000000001` | ChatBot-Prod | Production customer-facing chatbot |
| `10000000-...-000000000002` | Agent-Internal | Internal agentic workflow assistant |
| `10000000-...-000000000003` | RAG-Customer-Support | RAG-powered customer support system |

## Authentication Flow

1. **Login**: Client-side validation at `/login` with email + password
2. **Token**: Credentials stored in `sessionStorage` (demo mode)
3. **API calls**: Demo bypass — API allows anonymous access (no token required)
4. **Production**: Would use proper JWT auth with server-side validation

## JWT Secret

- Default: `dev-secret-change-me`
- Override via environment: `JWT_SECRET=your-production-secret`
- Algorithm: HS256
- Expiry: 24 hours

## Security Notes

- These credentials are for demonstration only
- Auth is client-side only in demo mode (API allows all requests)
- In production: use proper password hashing (argon2id), server-side JWT validation, refresh tokens
